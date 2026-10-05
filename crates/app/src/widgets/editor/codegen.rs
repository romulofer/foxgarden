use super::text_offset::{byte_to_char, char_to_byte};
use crate::widgets::modal::show_modal;
use fg_core::{FileKind, FileNode, Language};
use fg_i18n::t;
use syntax::{CodeGeneration, TypeFields, TypeMember};

/// Which accessors to generate — driven by the Tools menu's separate
/// "Generate Getters"/"Generate Setters" items and `Ctrl+Shift+G` (which
/// requests both).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessorKind {
    Getters,
    Setters,
    Both,
}

impl AccessorKind {
    fn request(self, fields: &[TypeMember]) -> CodeGeneration<'_> {
        CodeGeneration::Accessors {
            fields,
            getters: self != Self::Setters,
            setters: self != Self::Getters,
        }
    }
}

/// `kind`'s accessors for every field in `fields`, as `language`'s
/// extension writes them — one block, each field's accessors separated by
/// a blank line. Empty when there is nothing to generate: no fields, a
/// setter-only request over read-only fields, or no extension generating
/// code for `language`.
pub fn generate_accessors(language: Language, fields: &[TypeMember], indent_unit: &str, kind: AccessorKind) -> String {
    syntax::generate_code(language, kind.request(fields), indent_unit).unwrap_or_default()
}

/// A type code can be generated into: its name, the fields to offer, and
/// where the generated block goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationTarget {
    pub name: String,
    pub fields: Vec<TypeMember>,
    /// Just before the type body's closing delimiter.
    pub insertion_byte: usize,
}

/// The types in `types` that have a body to generate into.
pub fn generation_targets(types: Vec<TypeFields>) -> Vec<GenerationTarget> {
    types
        .into_iter()
        .filter_map(|ty| {
            Some(GenerationTarget {
                insertion_byte: ty.declaration.insertion_byte?,
                name: ty.declaration.name,
                fields: ty.fields,
            })
        })
        .collect()
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
/// closing `}`, per `GenerationTarget::insertion_byte`), prefixed with a blank
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
/// not one accessor pair per field — see `generate_method`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenerateMethodKind {
    Constructor,
    ToString,
    EqualsAndHashCode,
}

/// `kind`'s method(s) over `fields`, as `language`'s extension writes them —
/// the whole-method-body counterpart to `generate_accessors`. Every kind
/// generates something even with zero fields; empty only when no extension
/// generates code for `language`.
pub fn generate_method(
    language: Language,
    class_name: &str,
    fields: &[TypeMember],
    indent_unit: &str,
    kind: GenerateMethodKind,
) -> String {
    let request = match kind {
        GenerateMethodKind::Constructor => CodeGeneration::Constructor {
            type_name: class_name,
            fields,
        },
        GenerateMethodKind::ToString => CodeGeneration::ToString {
            type_name: class_name,
            fields,
        },
        GenerateMethodKind::EqualsAndHashCode => CodeGeneration::EqualsAndHashCode {
            type_name: class_name,
            fields,
        },
    };
    syntax::generate_code(language, request, indent_unit).unwrap_or_default()
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
    language: Language,
    kind: GenerateMethodKind,
    classes: Vec<GenerationTarget>,
    selected_class: usize,
    checked: Vec<bool>,
}

impl GenerateMethodDialog {
    /// Panics if `classes` is empty — same contract as
    /// `GenerateAccessorsDialog::new`.
    pub fn new(language: Language, classes: Vec<GenerationTarget>, kind: GenerateMethodKind) -> Self {
        let checked = vec![true; classes[0].fields.len()];
        Self {
            language,
            kind,
            classes,
            selected_class: 0,
            checked,
        }
    }

    pub fn classes(&self) -> &[GenerationTarget] {
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
    let selected_fields: Vec<TypeMember> = class
        .fields
        .iter()
        .zip(dialog.checked.iter())
        .filter(|&(_, &checked)| checked)
        .map(|(field, _)| field.clone())
        .collect();

    let generated = generate_method(dialog.language, &class.name, &selected_fields, indent_unit, dialog.kind);
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
            ui.label(t().codegen.generate_for);
            for (index, class) in dialog.classes().iter().enumerate() {
                if ui
                    .radio(index == dialog.selected_class(), class.name.as_str())
                    .clicked()
                {
                    new_selection = Some(index);
                }
            }
            ui.separator();
        }

        let class = &dialog.classes()[dialog.selected_class()];
        for (index, field) in class.fields.iter().enumerate() {
            let label = format!("{} : {}", field.name, field.type_text);
            let mut checked = dialog.checked()[index];
            if ui.checkbox(&mut checked, label).changed() {
                toggled = Some((index, checked));
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            generate_clicked = ui.button(t().codegen.generate).clicked();
            cancel_clicked = ui.button(t().common.cancel).clicked();
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
    language: Language,
    kind: AccessorKind,
    classes: Vec<GenerationTarget>,
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
    pub fn new(language: Language, classes: Vec<GenerationTarget>, kind: AccessorKind) -> Self {
        let checked = vec![true; classes[0].fields.len()];
        Self {
            language,
            kind,
            classes,
            selected_class: 0,
            checked,
        }
    }

    pub fn classes(&self) -> &[GenerationTarget] {
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
    let selected_fields: Vec<TypeMember> = class
        .fields
        .iter()
        .zip(dialog.checked.iter())
        .filter(|&(_, &checked)| checked)
        .map(|(field, _)| field.clone())
        .collect();

    let generated = generate_accessors(dialog.language, &selected_fields, indent_unit, dialog.kind);
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
            ui.label(t().codegen.generate_accessors_for);
            for (index, class) in dialog.classes().iter().enumerate() {
                if ui
                    .radio(index == dialog.selected_class(), class.name.as_str())
                    .clicked()
                {
                    new_selection = Some(index);
                }
            }
            ui.separator();
        }

        let class = &dialog.classes()[dialog.selected_class()];
        for (index, field) in class.fields.iter().enumerate() {
            let label = if field.read_only {
                format!("{} : {} ({})", field.name, field.type_text, t().codegen.read_only)
            } else {
                format!("{} : {}", field.name, field.type_text)
            };
            let mut checked = dialog.checked()[index];
            if ui.checkbox(&mut checked, label).changed() {
                toggled = Some((index, checked));
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            generate_clicked = ui.button(t().codegen.generate).clicked();
            cancel_clicked = ui.button(t().common.cancel).clicked();
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

/// Finds a source file carrying one of `extensions` (no leading dot)
/// anywhere in `node`'s subtree whose name (without extension) is exactly
/// `stem` — how a type's simple name (`syntax::supertype`,
/// `syntax::receiver_type`) becomes a file to read its members from, for
/// "Override Method" and dot-completion's cross-project member lookup
/// (`SPEC.md` §4) alike. Deliberately limited to files already in the
/// project's own (already-filtered, skip-list-applied) tree rather than a
/// fresh filesystem walk — a library or standard-library type has no file
/// in the project at all, so this correctly returns `None` for one rather
/// than searching disk for it.
pub fn find_source_file_by_stem(node: &FileNode, stem: &str, extensions: &[String]) -> Option<std::path::PathBuf> {
    match node.kind {
        FileKind::File => {
            let matches_ext = node
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.iter().any(|wanted| wanted == ext));
            let matches_stem = node.path.file_stem().and_then(|s| s.to_str()) == Some(stem);
            (matches_ext && matches_stem).then(|| node.path.clone())
        }
        FileKind::Dir => node
            .children
            .iter()
            .find_map(|child| find_source_file_by_stem(child, stem, extensions)),
    }
}

/// The project file `type_name` of `language` is declared in, by the
/// one-type-per-file naming convention: a file named after the type, with
/// one of the language's own extensions.
pub fn find_type_source(node: &FileNode, type_name: &str, language: Language) -> Option<std::path::PathBuf> {
    find_source_file_by_stem(node, type_name, &syntax::source_file_extensions(language))
}

/// State for the "Override Method" picker: every inherited, not-already-
/// overridden method found on the superclass, each with a checkbox.
/// Unlike `GenerateAccessorsDialog`/`GenerateMethodDialog`, there's no
/// class-ambiguity step first — the target class (and so `insertion_byte`)
/// is already fixed to wherever the cursor was when "Override Method" was
/// invoked (`syntax::enclosing_type`), before this dialog ever opens.
pub struct OverrideMethodDialog {
    language: Language,
    methods: Vec<TypeMember>,
    checked: Vec<bool>,
    insertion_byte: usize,
}

impl OverrideMethodDialog {
    /// Starts with every candidate checked, same convention as
    /// `GenerateAccessorsDialog`/`GenerateMethodDialog`'s field lists.
    pub fn new(language: Language, methods: Vec<TypeMember>, insertion_byte: usize) -> Self {
        let checked = vec![true; methods.len()];
        Self {
            language,
            methods,
            checked,
            insertion_byte,
        }
    }

    pub fn methods(&self) -> &[TypeMember] {
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
    let selected: Vec<TypeMember> = dialog
        .methods
        .iter()
        .zip(dialog.checked.iter())
        .filter(|&(_, &checked)| checked)
        .map(|(m, _)| m.clone())
        .collect();
    if selected.is_empty() {
        return None;
    }
    let generated = syntax::generate_code(dialog.language, CodeGeneration::Overrides { methods: &selected }, indent_unit)
        .filter(|generated| !generated.is_empty())?;
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

        ui.label(t().codegen.override_for);
        for (index, method) in dialog.methods().iter().enumerate() {
            let params = method
                .params
                .iter()
                .map(|(ty, name)| format!("{ty} {name}"))
                .collect::<Vec<_>>()
                .join(", ");
            let label = format!("{} {}({})", method.type_text, method.name, params);
            let mut checked = dialog.checked()[index];
            if ui.checkbox(&mut checked, label).changed() {
                toggled = Some((index, checked));
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            generate_clicked = ui.button(t().codegen.generate).clicked();
            cancel_clicked = ui.button(t().common.cancel).clicked();
        });
    });

    let escape_pressed = modal_outcome.is_some_and(|(_, escape_pressed)| escape_pressed);

    let dialog = override_method_dialog.as_mut()?;
    if let Some((index, checked)) = toggled {
        dialog.set_checked(index, checked);
    }

    if generate_clicked {
        let result = apply_override_dialog(dialog, text, indent_unit)
            .ok_or_else(|| "Nothing to generate: no methods selected.".to_string());
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
mod codegen_test;
