//! Unit tests for [`super`](codegen.rs), extracted verbatim from that
//! file's colocated `#[cfg(test)] mod tests` so the module file stays focused
//! on the code under test. Behavior-identical to the inline module it replaced.

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
