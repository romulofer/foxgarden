use super::text_offset::{byte_to_char, char_to_byte};
use crate::widgets::modal::show_modal;
use syntax::{ClassFields, FieldInfo};

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
}
