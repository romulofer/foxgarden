use super::text_offset::{byte_to_char, char_to_byte};
use crate::widgets::modal::show_modal;
use fg_core::{FileKind, FileNode};
use syntax::{ClassFields, FieldInfo, MethodSignature};

/// Which accessors to generate — driven by the Tools menu's separate
/// "Generate Getters"/"Generate Setters" items and `Ctrl+Shift+G` (which
/// requests both).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessorKind {
    Getters,
    Setters,
    Both,
}

/// Uppercases the first character of `name` for the `getX`/`setX` accessor
/// method name suffix, leaving the rest as-is (so e.g. `userId` becomes
/// `UserId`, matching standard Java bean-accessor naming).
fn capitalized(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn getter_for(field: &FieldInfo, indent_unit: &str) -> String {
    let cap = capitalized(&field.name);
    let ty = &field.java_type;
    let name = &field.name;
    format!(
        "{indent_unit}public {ty} get{cap}() {{\n\
         {indent_unit}{indent_unit}return this.{name};\n\
         {indent_unit}}}\n"
    )
}

/// `None` for a `final` field — it can't be reassigned, so a setter for it
/// wouldn't compile.
fn setter_for(field: &FieldInfo, indent_unit: &str) -> Option<String> {
    if field.is_final {
        return None;
    }
    let cap = capitalized(&field.name);
    let ty = &field.java_type;
    let name = &field.name;
    Some(format!(
        "{indent_unit}public void set{cap}({ty} {name}) {{\n\
         {indent_unit}{indent_unit}this.{name} = {name};\n\
         {indent_unit}}}\n"
    ))
}

/// Generates the accessors `kind` asks for, for `field`, each line
/// indented with `indent_unit`. `AccessorKind::Both` puts the getter
/// first, a blank line, then the setter (skipped for a `final` field,
/// which can't be reassigned) — standard Java accessor shape, `this.` on
/// the getter's return and the setter's assignment to disambiguate the
/// field from the setter's identically-named parameter. Empty for
/// `AccessorKind::Setters` on a `final` field — there's nothing to
/// generate.
fn accessors_for(field: &FieldInfo, indent_unit: &str, kind: AccessorKind) -> String {
    match kind {
        AccessorKind::Getters => getter_for(field, indent_unit),
        AccessorKind::Setters => setter_for(field, indent_unit).unwrap_or_default(),
        AccessorKind::Both => {
            let mut out = getter_for(field, indent_unit);
            if let Some(setter) = setter_for(field, indent_unit) {
                out.push('\n');
                out.push_str(&setter);
            }
            out
        }
    }
}

/// Generates `kind`'s accessors for every field in `fields`, each field's
/// block separated by a blank line — ready to insert as one standalone
/// chunk of source. Empty if `fields` is empty, or if `kind` is
/// `AccessorKind::Setters` and every field is `final`.
pub fn generate_accessors(fields: &[FieldInfo], indent_unit: &str, kind: AccessorKind) -> String {
    fields
        .iter()
        .map(|field| accessors_for(field, indent_unit, kind))
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Inserts `generated` at `cursor_char` in `text`, returning the new text
/// and the cursor position right after the inserted block — matching how
/// most editors leave the cursor after a code-generation insertion rather
/// than jumping back to where it started.
pub fn insert_generated(text: &str, cursor_char: usize, generated: &str) -> (String, usize) {
    let byte = char_to_byte(text, cursor_char);
    let new_text = format!("{}{generated}{}", &text[..byte], &text[byte..]);
    let new_cursor = cursor_char + generated.chars().count();
    (new_text, new_cursor)
}

/// Inserts `generated` right before `insertion_byte` (a class body's
/// closing `}`, per `ClassFields::insertion_byte`), prefixed with a blank
/// line so it doesn't run into whatever line already precedes the brace.
/// Delegates to `insert_generated` once the byte offset (from walking the
/// syntax tree) is converted to the char offset it expects.
pub fn insert_at_class_end(text: &str, insertion_byte: usize, generated: &str) -> (String, usize) {
    let insertion_char = byte_to_char(text, insertion_byte);
    insert_generated(text, insertion_char, &format!("\n{generated}"))
}

/// Which whole-method-body template to generate — driven by the Tools
/// menu's "Generate Constructor"/"Generate toString"/"Generate equals() and
/// hashCode()" items. A sibling to `AccessorKind`, not a variant of it:
/// these build one full method body from every selected field at once,
/// not one accessor pair per field, so they don't share
/// `generate_accessors`'s per-field iteration shape — see
/// `generate_method`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenerateMethodKind {
    Constructor,
    ToString,
    EqualsAndHashCode,
}

/// A constructor assigning every field from a same-named parameter —
/// `this.x = x;` per field, the same disambiguation `setter_for` already
/// uses. Still valid Java with zero fields (an empty, parameterless
/// constructor), so this doesn't special-case that.
fn constructor_for(class_name: &str, fields: &[FieldInfo], indent_unit: &str) -> String {
    let params = fields.iter().map(|f| format!("{} {}", f.java_type, f.name)).collect::<Vec<_>>().join(", ");
    let assignments: String = fields
        .iter()
        .map(|f| format!("{indent_unit}{indent_unit}this.{name} = {name};\n", name = f.name))
        .collect();
    format!("{indent_unit}public {class_name}({params}) {{\n{assignments}{indent_unit}}}\n")
}

/// `@Override public String toString()`, string-concatenation form (no
/// import needed, unlike `String.format`/text blocks) — `"ClassName{x=" +
/// x + ", y=" + y + "}"`.
fn to_string_for(class_name: &str, fields: &[FieldInfo], indent_unit: &str) -> String {
    let body = if fields.is_empty() {
        format!("\"{class_name}{{}}\"")
    } else {
        let parts = fields
            .iter()
            .map(|f| format!("\"{name}=\" + {name}", name = f.name))
            .collect::<Vec<_>>()
            .join(" + \", \" + ");
        format!("\"{class_name}{{\" + {parts} + \"}}\"")
    };
    format!(
        "{indent_unit}@Override\n\
         {indent_unit}public String toString() {{\n\
         {indent_unit}{indent_unit}return {body};\n\
         {indent_unit}}}\n"
    )
}

/// `@Override public boolean equals(Object o)` + `@Override public int
/// hashCode()`, generated together (standard IDE behavior — the two must
/// stay consistent with each other, so there's no separate "just equals"/
/// "just hashCode" option the way getters/setters have). Uses
/// `java.util.Objects.equals`/`.hash` (fully qualified, deliberately —
/// correct for primitives via autoboxing same as for objects, and avoids
/// needing to check for or insert an `import java.util.Objects;` line the
/// way a bare `Objects.equals(...)` call would need). A zero-field class
/// still generates validly: `equals` reduces to comparing only class
/// identity, `hashCode` to `Objects.hash()` (a constant).
fn equals_and_hash_code_for(class_name: &str, fields: &[FieldInfo], indent_unit: &str) -> String {
    let comparison = if fields.is_empty() {
        format!("{indent_unit}{indent_unit}return true;\n")
    } else {
        let conditions = fields
            .iter()
            .map(|f| format!("java.util.Objects.equals({name}, that.{name})", name = f.name))
            .collect::<Vec<_>>()
            .join(" && ");
        format!("{indent_unit}{indent_unit}return {conditions};\n")
    };
    let equals = format!(
        "{indent_unit}@Override\n\
         {indent_unit}public boolean equals(Object o) {{\n\
         {indent_unit}{indent_unit}if (this == o) return true;\n\
         {indent_unit}{indent_unit}if (o == null || getClass() != o.getClass()) return false;\n\
         {indent_unit}{indent_unit}{class_name} that = ({class_name}) o;\n\
         {comparison}\
         {indent_unit}}}\n"
    );

    let hash_args = fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ");
    let hash_code = format!(
        "{indent_unit}@Override\n\
         {indent_unit}public int hashCode() {{\n\
         {indent_unit}{indent_unit}return java.util.Objects.hash({hash_args});\n\
         {indent_unit}}}\n"
    );

    format!("{equals}\n{hash_code}")
}

/// Generates `kind`'s method(s) for every field in `fields`, ready to
/// insert as one standalone chunk of source — the whole-method-body
/// counterpart to `generate_accessors`.
pub fn generate_method(class_name: &str, fields: &[FieldInfo], indent_unit: &str, kind: GenerateMethodKind) -> String {
    match kind {
        GenerateMethodKind::Constructor => constructor_for(class_name, fields, indent_unit),
        GenerateMethodKind::ToString => to_string_for(class_name, fields, indent_unit),
        GenerateMethodKind::EqualsAndHashCode => equals_and_hash_code_for(class_name, fields, indent_unit),
    }
}

/// State for the "Generate Constructor"/"Generate toString"/"Generate
/// equals() and hashCode()" picker — the whole-method-body counterpart to
/// `GenerateAccessorsDialog`, shown under the identical circumstance (more
/// than one eligible class in the file) and shaped identically (class
/// picker, then field checkboxes), just generating from `GenerateMethodKind`
/// instead of `AccessorKind`. Kept as its own small, parallel type rather
/// than folded into `GenerateAccessorsDialog` — the two pickers happen to
/// look alike, but genericizing one struct over "which enum of generation
/// kinds" to share it buys little given there are only two, and costs a
/// type parameter threaded through every method and caller.
pub struct GenerateMethodDialog {
    kind: GenerateMethodKind,
    classes: Vec<ClassFields>,
    selected_class: usize,
    checked: Vec<bool>,
}

impl GenerateMethodDialog {
    /// Panics if `classes` is empty — same contract as
    /// `GenerateAccessorsDialog::new`.
    pub fn new(classes: Vec<ClassFields>, kind: GenerateMethodKind) -> Self {
        let checked = vec![true; classes[0].fields.len()];
        Self { kind, classes, selected_class: 0, checked }
    }

    pub fn classes(&self) -> &[ClassFields] {
        &self.classes
    }

    pub fn selected_class(&self) -> usize {
        self.selected_class
    }

    pub fn checked(&self) -> &[bool] {
        &self.checked
    }

    pub fn select_class(&mut self, index: usize) {
        if let Some(class) = self.classes.get(index) {
            self.selected_class = index;
            self.checked = vec![true; class.fields.len()];
        }
    }

    pub fn set_checked(&mut self, field_index: usize, value: bool) {
        if let Some(slot) = self.checked.get_mut(field_index) {
            *slot = value;
        }
    }
}

/// Generates `dialog`'s method(s) for whichever of the selected class's
/// fields are checked, and inserts them at that class's end. Unlike
/// `apply_dialog`, this never produces "nothing" — every `GenerateMethodKind`
/// generates *something* even with zero fields checked (see
/// `constructor_for`/`to_string_for`/`equals_and_hash_code_for`'s own doc
/// comments), so there's no `Option` here.
pub fn apply_method_dialog(dialog: &GenerateMethodDialog, text: &str, indent_unit: &str) -> (String, usize) {
    let class = &dialog.classes[dialog.selected_class];
    let selected_fields: Vec<FieldInfo> = class
        .fields
        .iter()
        .zip(dialog.checked.iter())
        .filter(|&(_, &checked)| checked)
        .map(|(field, _)| field.clone())
        .collect();

    let generated = generate_method(&class.name, &selected_fields, indent_unit, dialog.kind);
    insert_at_class_end(text, class.insertion_byte, &generated)
}

/// Renders `generate_method_dialog`'s class/field picker, if it's open —
/// the whole-method-body counterpart to `show_generate_accessors_dialog`,
/// same shape (intents collected into locals first, applied to
/// `*generate_method_dialog` only after `show_modal` returns, for the same
/// overlapping-mutable-borrow reason that function's doc comment explains).
/// Always `Some(Ok(...))` on "Generate" (never `Some(Err(...))`) since
/// `apply_method_dialog` never produces nothing to insert.
pub fn show_generate_method_dialog(
    ui: &egui::Ui,
    generate_method_dialog: &mut Option<GenerateMethodDialog>,
    text: &str,
    indent_unit: &str,
) -> Option<(String, usize)> {
    let is_open = generate_method_dialog.is_some();

    let mut new_selection = None;
    let mut toggled = None;
    let mut generate_clicked = false;
    let mut cancel_clicked = false;

    let modal_outcome = show_modal(ui, "generate_method_dialog", is_open.then_some(()), |ui, _| {
        let dialog = generate_method_dialog.as_ref().expect("guarded by is_open above");

        if dialog.classes().len() > 1 {
            ui.label("Generate for:");
            for (index, class) in dialog.classes().iter().enumerate() {
                if ui.radio(index == dialog.selected_class(), class.name.as_str()).clicked() {
                    new_selection = Some(index);
                }
            }
            ui.separator();
        }

        let class = &dialog.classes()[dialog.selected_class()];
        for (index, field) in class.fields.iter().enumerate() {
            let label = format!("{} : {}", field.name, field.java_type);
            let mut checked = dialog.checked()[index];
            if ui.checkbox(&mut checked, label).changed() {
                toggled = Some((index, checked));
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            generate_clicked = ui.button("Generate").clicked();
            cancel_clicked = ui.button("Cancel").clicked();
        });
    });

    let escape_pressed = modal_outcome.is_some_and(|(_, escape_pressed)| escape_pressed);

    let dialog = generate_method_dialog.as_mut()?;
    if let Some(index) = new_selection {
        dialog.select_class(index);
    }
    if let Some((index, checked)) = toggled {
        dialog.set_checked(index, checked);
    }

    if generate_clicked {
        let result = apply_method_dialog(dialog, text, indent_unit);
        *generate_method_dialog = None;
        Some(result)
    } else {
        if cancel_clicked || escape_pressed {
            *generate_method_dialog = None;
        }
        None
    }
}

/// State for the "Generate Getters/Setters" picker shown when a file has
/// more than one class with eligible fields — lets the user pick which
/// class, then which of its fields, before generating. Not used when a
/// file has exactly one eligible class: that case generates immediately
/// for every field, no picker needed.
pub struct GenerateAccessorsDialog {
    kind: AccessorKind,
    classes: Vec<ClassFields>,
    selected_class: usize,
    /// Index-aligned with `classes[selected_class].fields`; unchecked
    /// fields are skipped when generating.
    checked: Vec<bool>,
}

impl GenerateAccessorsDialog {
    /// Starts on the first class, every field checked. Panics if `classes`
    /// is empty — callers only build this dialog once they already know
    /// there's more than one eligible class (see `widget::show`), so an
    /// empty list here would be a caller bug, not a state to handle
    /// gracefully.
    pub fn new(classes: Vec<ClassFields>, kind: AccessorKind) -> Self {
        let checked = vec![true; classes[0].fields.len()];
        Self { kind, classes, selected_class: 0, checked }
    }

    pub fn classes(&self) -> &[ClassFields] {
        &self.classes
    }

    pub fn selected_class(&self) -> usize {
        self.selected_class
    }

    pub fn checked(&self) -> &[bool] {
        &self.checked
    }

    /// Switches the picker to `index`'s class, resetting every one of its
    /// fields back to checked — a field selection from the previously
    /// viewed class wouldn't even line up with the new one's field list.
    pub fn select_class(&mut self, index: usize) {
        if let Some(class) = self.classes.get(index) {
            self.selected_class = index;
            self.checked = vec![true; class.fields.len()];
        }
    }

    pub fn set_checked(&mut self, field_index: usize, value: bool) {
        if let Some(slot) = self.checked.get_mut(field_index) {
            *slot = value;
        }
    }
}

/// Generates `dialog`'s kind of accessors for whichever of the selected
/// class's fields are checked, and inserts them at that class's end.
/// `None` if nothing ends up generated — every field unchecked, or (for a
/// `Setters`-only dialog) every checked field is `final`.
pub fn apply_dialog(dialog: &GenerateAccessorsDialog, text: &str, indent_unit: &str) -> Option<(String, usize)> {
    let class = &dialog.classes[dialog.selected_class];
    let selected_fields: Vec<FieldInfo> = class
        .fields
        .iter()
        .zip(dialog.checked.iter())
        .filter(|&(_, &checked)| checked)
        .map(|(field, _)| field.clone())
        .collect();

    let generated = generate_accessors(&selected_fields, indent_unit, dialog.kind);
    if generated.is_empty() {
        return None;
    }
    Some(insert_at_class_end(text, class.insertion_byte, &generated))
}

/// Renders `generate_dialog`'s class/field picker, if it's open.
/// `Some(Ok((new_text, new_cursor)))` once "Generate" produces something to
/// insert, `Some(Err(message))` once it produces nothing (nothing checked,
/// or every checked field turned out `final` under a `Setters`-only
/// dialog), `None` while the dialog stays closed, is untouched this frame,
/// or was just cancelled. Every intent from inside the modal (class pick,
/// checkbox toggle, which button was clicked) is collected into plain
/// locals first and only applied to `*generate_dialog` after `show_modal`
/// returns — the "Generate"/"Cancel" buttons need to clear
/// `*generate_dialog` itself, and doing that while a `&mut
/// GenerateAccessorsDialog` borrowed from it is still captured several
/// closures deep (the modal body, then `ui.horizontal`) would be two
/// overlapping mutable borrows of the same `Option`.
pub fn show_generate_accessors_dialog(
    ui: &egui::Ui,
    generate_dialog: &mut Option<GenerateAccessorsDialog>,
    text: &str,
    indent_unit: &str,
) -> Option<Result<(String, usize), String>> {
    let is_open = generate_dialog.is_some();

    let mut new_selection = None;
    let mut toggled = None;
    let mut generate_clicked = false;
    let mut cancel_clicked = false;

    let modal_outcome = show_modal(ui, "generate_accessors_dialog", is_open.then_some(()), |ui, _| {
        let dialog = generate_dialog.as_ref().expect("guarded by is_open above");

        if dialog.classes().len() > 1 {
            ui.label("Generate accessors for:");
            for (index, class) in dialog.classes().iter().enumerate() {
                if ui.radio(index == dialog.selected_class(), class.name.as_str()).clicked() {
                    new_selection = Some(index);
                }
            }
            ui.separator();
        }

        let class = &dialog.classes()[dialog.selected_class()];
        for (index, field) in class.fields.iter().enumerate() {
            let label = if field.is_final {
                format!("{} : {} (final)", field.name, field.java_type)
            } else {
                format!("{} : {}", field.name, field.java_type)
            };
            let mut checked = dialog.checked()[index];
            if ui.checkbox(&mut checked, label).changed() {
                toggled = Some((index, checked));
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            generate_clicked = ui.button("Generate").clicked();
            cancel_clicked = ui.button("Cancel").clicked();
        });
    });

    let escape_pressed = modal_outcome.is_some_and(|(_, escape_pressed)| escape_pressed);

    let dialog = generate_dialog.as_mut()?;
    if let Some(index) = new_selection {
        dialog.select_class(index);
    }
    if let Some((index, checked)) = toggled {
        dialog.set_checked(index, checked);
    }

    if generate_clicked {
        let result = apply_dialog(dialog, text, indent_unit)
            .ok_or_else(|| "Nothing to generate: no fields selected.".to_string());
        *generate_dialog = None;
        Some(result)
    } else if cancel_clicked || escape_pressed {
        *generate_dialog = None;
        None
    } else {
        None
    }
}

/// Finds a `.java` file anywhere in `node`'s subtree whose name (without
/// extension) is exactly `stem` — "Override Method"'s way of turning a
/// superclass's simple name (`syntax::superclass_name`) into a source file
/// to look its methods up in. Deliberately limited to files already in the
/// project's own (already-filtered, skip-list-applied) tree rather than a
/// fresh filesystem walk — a JDK/library supertype has no file in the
/// project at all, so this correctly returns `None` for one rather than
/// searching disk for it.
pub fn find_java_file_by_stem(node: &FileNode, stem: &str) -> Option<std::path::PathBuf> {
    match node.kind {
        FileKind::File => {
            let is_java = node.path.extension().and_then(|ext| ext.to_str()) == Some("java");
            let matches_stem = node.path.file_stem().and_then(|s| s.to_str()) == Some(stem);
            (is_java && matches_stem).then(|| node.path.clone())
        }
        FileKind::Dir => node.children.iter().find_map(|child| find_java_file_by_stem(child, stem)),
    }
}

/// The literal Java expression `Override Method`'s generated stub should
/// `return` for `java_type` — `None` for `void`, which needs no `return`
/// statement at all. Every non-`void` type needs *something*, since a
/// stub with a missing return statement wouldn't compile.
fn default_return_for(java_type: &str) -> Option<&'static str> {
    match java_type {
        "void" => None,
        "boolean" => Some("false"),
        "byte" | "short" | "int" | "long" => Some("0"),
        "float" => Some("0.0f"),
        "double" => Some("0.0"),
        "char" => Some("'\\0'"),
        _ => Some("null"),
    }
}

/// `@Override` plus a stub body returning `default_return_for`'s value (or
/// no `return` at all, for `void`) — one inherited method turned into a
/// compilable override.
fn override_stub_for(method: &MethodSignature, indent_unit: &str) -> String {
    let params = method.params.iter().map(|(ty, name)| format!("{ty} {name}")).collect::<Vec<_>>().join(", ");
    let body = match default_return_for(&method.return_type) {
        Some(value) => format!("{indent_unit}{indent_unit}return {value};\n"),
        None => String::new(),
    };
    let ty = &method.return_type;
    let name = &method.name;
    format!("{indent_unit}@Override\n{indent_unit}public {ty} {name}({params}) {{\n{body}{indent_unit}}}\n")
}

/// State for the "Override Method" picker: every inherited, not-already-
/// overridden method found on the superclass, each with a checkbox.
/// Unlike `GenerateAccessorsDialog`/`GenerateMethodDialog`, there's no
/// class-ambiguity step first — the target class (and so `insertion_byte`)
/// is already fixed to wherever the cursor was when "Override Method" was
/// invoked (`syntax::enclosing_class`), before this dialog ever opens.
pub struct OverrideMethodDialog {
    methods: Vec<MethodSignature>,
    checked: Vec<bool>,
    insertion_byte: usize,
}

impl OverrideMethodDialog {
    /// Starts with every candidate checked, same convention as
    /// `GenerateAccessorsDialog`/`GenerateMethodDialog`'s field lists.
    pub fn new(methods: Vec<MethodSignature>, insertion_byte: usize) -> Self {
        let checked = vec![true; methods.len()];
        Self { methods, checked, insertion_byte }
    }

    pub fn methods(&self) -> &[MethodSignature] {
        &self.methods
    }

    pub fn checked(&self) -> &[bool] {
        &self.checked
    }

    pub fn set_checked(&mut self, index: usize, value: bool) {
        if let Some(slot) = self.checked.get_mut(index) {
            *slot = value;
        }
    }
}

/// Generates `@Override` stubs for whichever of `dialog`'s methods are
/// checked, inserting them at its fixed target class's end. `None` if
/// nothing is checked — unlike `GenerateMethodDialog` (whose templates are
/// always valid Java even with zero fields), an override dialog with
/// nothing checked really does have nothing to generate.
pub fn apply_override_dialog(dialog: &OverrideMethodDialog, text: &str, indent_unit: &str) -> Option<(String, usize)> {
    let selected: Vec<&MethodSignature> =
        dialog.methods.iter().zip(dialog.checked.iter()).filter(|&(_, &checked)| checked).map(|(m, _)| m).collect();
    if selected.is_empty() {
        return None;
    }
    let generated = selected.iter().map(|m| override_stub_for(m, indent_unit)).collect::<Vec<_>>().join("\n");
    Some(insert_at_class_end(text, dialog.insertion_byte, &generated))
}

/// Renders `override_method_dialog`'s method picker, if it's open — same
/// shape (and the same reason for collecting intents into locals first) as
/// `show_generate_accessors_dialog`, minus that function's class-picker
/// step, since `OverrideMethodDialog` is never built with more than one
/// possible target class to begin with.
pub fn show_override_method_dialog(
    ui: &egui::Ui,
    override_method_dialog: &mut Option<OverrideMethodDialog>,
    text: &str,
    indent_unit: &str,
) -> Option<Result<(String, usize), String>> {
    let is_open = override_method_dialog.is_some();

    let mut toggled = None;
    let mut generate_clicked = false;
    let mut cancel_clicked = false;

    let modal_outcome = show_modal(ui, "override_method_dialog", is_open.then_some(()), |ui, _| {
        let dialog = override_method_dialog.as_ref().expect("guarded by is_open above");

        ui.label("Override:");
        for (index, method) in dialog.methods().iter().enumerate() {
            let params =
                method.params.iter().map(|(ty, name)| format!("{ty} {name}")).collect::<Vec<_>>().join(", ");
            let label = format!("{} {}({})", method.return_type, method.name, params);
            let mut checked = dialog.checked()[index];
            if ui.checkbox(&mut checked, label).changed() {
                toggled = Some((index, checked));
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            generate_clicked = ui.button("Generate").clicked();
            cancel_clicked = ui.button("Cancel").clicked();
        });
    });

    let escape_pressed = modal_outcome.is_some_and(|(_, escape_pressed)| escape_pressed);

    let dialog = override_method_dialog.as_mut()?;
    if let Some((index, checked)) = toggled {
        dialog.set_checked(index, checked);
    }

    if generate_clicked {
        let result =
            apply_override_dialog(dialog, text, indent_unit).ok_or_else(|| "Nothing to generate: no methods selected.".to_string());
        *override_method_dialog = None;
        Some(result)
    } else if cancel_clicked || escape_pressed {
        *override_method_dialog = None;
        None
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, java_type: &str, is_final: bool) -> FieldInfo {
        FieldInfo { name: name.to_string(), java_type: java_type.to_string(), is_final }
    }

    #[test]
    fn generates_getter_and_setter_for_a_mutable_field() {
        let generated = generate_accessors(&[field("name", "String", false)], "    ", AccessorKind::Both);
        assert_eq!(
            generated,
            "    public String getName() {\n\
             \x20\x20\x20\x20\x20\x20\x20\x20return this.name;\n\
             \x20\x20\x20\x20}\n\
             \n\
             \x20\x20\x20\x20public void setName(String name) {\n\
             \x20\x20\x20\x20\x20\x20\x20\x20this.name = name;\n\
             \x20\x20\x20\x20}\n"
        );
    }

    #[test]
    fn both_generates_only_a_getter_for_a_final_field() {
        let generated = generate_accessors(&[field("id", "int", true)], "  ", AccessorKind::Both);
        assert_eq!(generated, "  public int getId() {\n    return this.id;\n  }\n");
        assert!(!generated.contains("setId"));
    }

    #[test]
    fn getters_kind_generates_only_getters() {
        let generated = generate_accessors(&[field("name", "String", false)], "    ", AccessorKind::Getters);
        assert!(generated.contains("getName"));
        assert!(!generated.contains("setName"));
    }

    #[test]
    fn setters_kind_generates_only_setters() {
        let generated = generate_accessors(&[field("name", "String", false)], "    ", AccessorKind::Setters);
        assert!(!generated.contains("getName"));
        assert!(generated.contains("setName"));
    }

    #[test]
    fn setters_kind_on_an_all_final_class_produces_nothing() {
        let generated = generate_accessors(&[field("id", "int", true)], "    ", AccessorKind::Setters);
        assert_eq!(generated, "");
    }

    #[test]
    fn separates_multiple_fields_with_a_blank_line() {
        let generated = generate_accessors(
            &[field("x", "int", true), field("y", "int", true)],
            "  ",
            AccessorKind::Both,
        );
        assert_eq!(
            generated,
            "  public int getX() {\n    return this.x;\n  }\n\n  public int getY() {\n    return this.y;\n  }\n"
        );
    }

    #[test]
    fn separates_multiple_fields_with_a_blank_line_for_getters_only() {
        let generated = generate_accessors(
            &[field("x", "int", false), field("y", "int", false)],
            "  ",
            AccessorKind::Getters,
        );
        assert_eq!(
            generated,
            "  public int getX() {\n    return this.x;\n  }\n\n  public int getY() {\n    return this.y;\n  }\n"
        );
    }

    #[test]
    fn capitalizes_only_the_first_character() {
        let generated = generate_accessors(&[field("userId", "long", false)], "", AccessorKind::Both);
        assert!(generated.contains("getUserId"));
        assert!(generated.contains("setUserId"));
    }

    #[test]
    fn empty_fields_produces_empty_output() {
        assert_eq!(generate_accessors(&[], "    ", AccessorKind::Both), "");
    }

    #[test]
    fn insert_generated_places_text_at_the_cursor_and_moves_it_past() {
        let (text, cursor) = insert_generated("class Foo {\n}\n", 12, "    // generated\n");
        assert_eq!(text, "class Foo {\n    // generated\n}\n");
        assert_eq!(cursor, 12 + "    // generated\n".chars().count());
    }

    #[test]
    fn insert_at_class_end_lands_right_before_the_closing_brace() {
        let text = "class Foo {\n    private int x;\n}\n";
        let insertion_byte = text.rfind('}').unwrap();
        let (new_text, _) = insert_at_class_end(text, insertion_byte, "    public int getX() {\n        return this.x;\n    }\n");
        assert_eq!(
            new_text,
            "class Foo {\n    private int x;\n\n    public int getX() {\n        return this.x;\n    }\n}\n"
        );
    }

    fn class_fields(name: &str, fields: Vec<FieldInfo>, insertion_byte: usize) -> ClassFields {
        ClassFields { name: name.to_string(), fields, insertion_byte }
    }

    #[test]
    fn apply_dialog_targets_whichever_class_is_selected() {
        let text = "class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n";
        let foo_end = text.find("}\nclass Bar").unwrap();
        let bar_end = text.rfind('}').unwrap();
        let classes = vec![
            class_fields("Foo", vec![field("x", "int", false)], foo_end),
            class_fields("Bar", vec![field("y", "int", false)], bar_end),
        ];
        let mut dialog = GenerateAccessorsDialog::new(classes, AccessorKind::Getters);
        dialog.select_class(1);

        let (new_text, _) = apply_dialog(&dialog, text, "    ").expect("Bar has a field to generate for");
        assert!(new_text.contains("getY"));
        assert!(!new_text.contains("getX"));
    }

    #[test]
    fn apply_dialog_skips_unchecked_fields() {
        let text = "class Foo {\n    private int x;\n    private int y;\n}\n";
        let insertion_byte = text.rfind('}').unwrap();
        let classes = vec![class_fields("Foo", vec![field("x", "int", false), field("y", "int", false)], insertion_byte)];
        let mut dialog = GenerateAccessorsDialog::new(classes, AccessorKind::Getters);
        dialog.set_checked(1, false);

        let (new_text, _) = apply_dialog(&dialog, text, "    ").expect("x is still checked");
        assert!(new_text.contains("getX"));
        assert!(!new_text.contains("getY"));
    }

    #[test]
    fn apply_dialog_is_none_when_every_checked_field_is_final_for_setters() {
        let text = "class Foo {\n    private final int id;\n}\n";
        let insertion_byte = text.rfind('}').unwrap();
        let classes = vec![class_fields("Foo", vec![field("id", "int", true)], insertion_byte)];
        let dialog = GenerateAccessorsDialog::new(classes, AccessorKind::Setters);

        assert_eq!(apply_dialog(&dialog, text, "    "), None);
    }

    #[test]
    fn apply_dialog_is_none_when_nothing_is_checked() {
        let text = "class Foo {\n    private int x;\n}\n";
        let insertion_byte = text.rfind('}').unwrap();
        let classes = vec![class_fields("Foo", vec![field("x", "int", false)], insertion_byte)];
        let mut dialog = GenerateAccessorsDialog::new(classes, AccessorKind::Getters);
        dialog.set_checked(0, false);

        assert_eq!(apply_dialog(&dialog, text, "    "), None);
    }

    #[test]
    fn constructor_for_assigns_every_field_from_a_same_named_parameter() {
        let generated =
            constructor_for("Point", &[field("x", "int", false), field("y", "int", false)], "    ");
        assert_eq!(
            generated,
            "    public Point(int x, int y) {\n\
             \x20\x20\x20\x20\x20\x20\x20\x20this.x = x;\n\
             \x20\x20\x20\x20\x20\x20\x20\x20this.y = y;\n\
             \x20\x20\x20\x20}\n"
        );
    }

    #[test]
    fn constructor_for_with_no_fields_is_still_valid_java() {
        let generated = constructor_for("Empty", &[], "    ");
        assert_eq!(generated, "    public Empty() {\n    }\n");
    }

    #[test]
    fn to_string_for_concatenates_every_field() {
        let generated = to_string_for("Point", &[field("x", "int", false), field("y", "int", false)], "    ");
        assert!(generated.contains("@Override"));
        assert!(generated.contains("public String toString()"));
        assert!(generated.contains("\"Point{\" + \"x=\" + x + \", \" + \"y=\" + y + \"}\""));
    }

    #[test]
    fn to_string_for_with_no_fields_has_no_trailing_concatenation() {
        let generated = to_string_for("Empty", &[], "    ");
        assert!(generated.contains("\"Empty{}\""));
    }

    #[test]
    fn equals_and_hash_code_for_compares_every_field() {
        let generated =
            equals_and_hash_code_for("Point", &[field("x", "int", false), field("y", "int", false)], "    ");
        assert!(generated.contains("public boolean equals(Object o)"));
        assert!(generated.contains("Point that = (Point) o;"));
        assert!(generated.contains("java.util.Objects.equals(x, that.x) && java.util.Objects.equals(y, that.y)"));
        assert!(generated.contains("public int hashCode()"));
        assert!(generated.contains("java.util.Objects.hash(x, y)"));
    }

    #[test]
    fn equals_and_hash_code_for_with_no_fields_compares_only_class_identity() {
        let generated = equals_and_hash_code_for("Empty", &[], "    ");
        assert!(generated.contains("return true;"));
        assert!(generated.contains("java.util.Objects.hash()"));
    }

    #[test]
    fn generate_method_dispatches_to_the_right_template() {
        let fields = [field("x", "int", false)];
        assert!(generate_method("Foo", &fields, "    ", GenerateMethodKind::Constructor).contains("public Foo(int x)"));
        assert!(generate_method("Foo", &fields, "    ", GenerateMethodKind::ToString).contains("toString()"));
        assert!(generate_method("Foo", &fields, "    ", GenerateMethodKind::EqualsAndHashCode).contains("hashCode()"));
    }

    #[test]
    fn apply_method_dialog_targets_whichever_class_is_selected() {
        let text = "class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n";
        let foo_end = text.find("}\nclass Bar").unwrap();
        let bar_end = text.rfind('}').unwrap();
        let classes = vec![
            class_fields("Foo", vec![field("x", "int", false)], foo_end),
            class_fields("Bar", vec![field("y", "int", false)], bar_end),
        ];
        let mut dialog = GenerateMethodDialog::new(classes, GenerateMethodKind::ToString);
        dialog.select_class(1);

        let (new_text, _) = apply_method_dialog(&dialog, text, "    ");
        assert!(new_text.contains("\"y=\" + y"));
        assert!(!new_text.contains("\"x=\" + x"));
    }

    #[test]
    fn apply_method_dialog_skips_unchecked_fields() {
        let text = "class Foo {\n    private int x;\n    private int y;\n}\n";
        let insertion_byte = text.rfind('}').unwrap();
        let classes = vec![class_fields("Foo", vec![field("x", "int", false), field("y", "int", false)], insertion_byte)];
        let mut dialog = GenerateMethodDialog::new(classes, GenerateMethodKind::Constructor);
        dialog.set_checked(1, false);

        let (new_text, _) = apply_method_dialog(&dialog, text, "    ");
        // Exactly the `x` parameter, not `y` too — checking for the full
        // signature (rather than e.g. `!new_text.contains("int y")`) since
        // the original `private int y;` field declaration is still
        // present in `new_text` regardless of what got generated.
        assert!(new_text.contains("public Foo(int x)"));
    }

    fn method(name: &str, return_type: &str, params: Vec<(&str, &str)>) -> MethodSignature {
        MethodSignature {
            name: name.to_string(),
            return_type: return_type.to_string(),
            params: params.into_iter().map(|(t, n)| (t.to_string(), n.to_string())).collect(),
        }
    }

    fn java_file(path: &str) -> FileNode {
        FileNode {
            path: std::path::PathBuf::from(path),
            name: std::path::PathBuf::from(path).file_name().unwrap().to_string_lossy().into_owned(),
            kind: FileKind::File,
            children: Vec::new(),
        }
    }

    fn dir(path: &str, children: Vec<FileNode>) -> FileNode {
        FileNode {
            path: std::path::PathBuf::from(path),
            name: std::path::PathBuf::from(path).file_name().unwrap().to_string_lossy().into_owned(),
            kind: FileKind::Dir,
            children,
        }
    }

    #[test]
    fn find_java_file_by_stem_finds_a_nested_match() {
        let tree = dir(
            "/root",
            vec![java_file("/root/Main.java"), dir("/root/pkg", vec![java_file("/root/pkg/Base.java")])],
        );

        assert_eq!(find_java_file_by_stem(&tree, "Base"), Some(std::path::PathBuf::from("/root/pkg/Base.java")));
    }

    #[test]
    fn find_java_file_by_stem_ignores_non_java_files_with_the_same_stem() {
        let tree = dir("/root", vec![java_file("/root/Base.txt")]);
        assert_eq!(find_java_file_by_stem(&tree, "Base"), None);
    }

    #[test]
    fn find_java_file_by_stem_returns_none_when_not_found() {
        let tree = dir("/root", vec![java_file("/root/Main.java")]);
        assert_eq!(find_java_file_by_stem(&tree, "NoSuchClass"), None);
    }

    #[test]
    fn default_return_for_void_is_none() {
        assert_eq!(default_return_for("void"), None);
    }

    #[test]
    fn default_return_for_primitives_and_objects() {
        assert_eq!(default_return_for("boolean"), Some("false"));
        assert_eq!(default_return_for("int"), Some("0"));
        assert_eq!(default_return_for("double"), Some("0.0"));
        assert_eq!(default_return_for("String"), Some("null"));
    }

    #[test]
    fn override_stub_for_a_void_method_has_no_return_statement() {
        let stub = override_stub_for(&method("run", "void", vec![]), "    ");
        assert_eq!(stub, "    @Override\n    public void run() {\n    }\n");
    }

    #[test]
    fn override_stub_for_a_method_with_params_and_a_return_type() {
        let stub = override_stub_for(&method("compute", "int", vec![("int", "x")]), "    ");
        assert_eq!(
            stub,
            "    @Override\n    public int compute(int x) {\n        return 0;\n    }\n"
        );
    }

    #[test]
    fn apply_override_dialog_generates_only_checked_methods() {
        let text = "class Foo extends Bar {\n}\n";
        let insertion_byte = text.rfind('}').unwrap();
        let methods = vec![method("run", "void", vec![]), method("stop", "void", vec![])];
        let mut dialog = OverrideMethodDialog::new(methods, insertion_byte);
        dialog.set_checked(1, false);

        let (new_text, _) = apply_override_dialog(&dialog, text, "    ").unwrap();
        assert!(new_text.contains("public void run()"));
        assert!(!new_text.contains("public void stop()"));
        assert!(new_text.contains("@Override"));
    }

    #[test]
    fn apply_override_dialog_is_none_when_nothing_is_checked() {
        let text = "class Foo extends Bar {\n}\n";
        let insertion_byte = text.rfind('}').unwrap();
        let mut dialog = OverrideMethodDialog::new(vec![method("run", "void", vec![])], insertion_byte);
        dialog.set_checked(0, false);

        assert_eq!(apply_override_dialog(&dialog, text, "    "), None);
    }
}
