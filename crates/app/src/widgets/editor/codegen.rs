use super::auto_edit::char_to_byte;
use syntax::FieldInfo;

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
}
