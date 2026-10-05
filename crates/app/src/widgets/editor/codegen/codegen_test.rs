//! Unit tests for [`super`](codegen.rs). The generated text itself is the
//! extension's own (its templates are tested where they live); these cover
//! what stays here — picking classes and fields, routing a request to the
//! extension that owns the language, and splicing the result in.

use super::*;
use syntax::{MemberKind, TypeDeclaration};

fn field(name: &str, java_type: &str, is_final: bool) -> TypeMember {
    TypeMember {
        name: name.to_string(),
        kind: MemberKind::Field,
        type_text: java_type.to_string(),
        params: Vec::new(),
        read_only: is_final,
    }
}

fn method(name: &str, return_type: &str) -> TypeMember {
    TypeMember {
        name: name.to_string(),
        kind: MemberKind::Method,
        type_text: return_type.to_string(),
        params: Vec::new(),
        read_only: false,
    }
}

fn target(name: &str, fields: Vec<TypeMember>, insertion_byte: usize) -> GenerationTarget {
    GenerationTarget {
        name: name.to_string(),
        fields,
        insertion_byte,
    }
}

/// Generating goes through the installed extensions, so every test that
/// produces text needs them.
fn java() -> Language {
    test_support::install_grammars();
    Language::Java
}

#[test]
fn accessors_come_from_the_extension_that_owns_the_language() {
    let generated = generate_accessors(java(), &[field("name", "String", false)], "    ", AccessorKind::Both);
    assert!(generated.contains("getName()"));
    assert!(generated.contains("setName(String name)"));
}

#[test]
fn accessor_kinds_ask_for_the_right_halves() {
    let fields = [field("name", "String", false)];
    let getters = generate_accessors(java(), &fields, "    ", AccessorKind::Getters);
    assert!(getters.contains("getName") && !getters.contains("setName"));
    let setters = generate_accessors(java(), &fields, "    ", AccessorKind::Setters);
    assert!(!setters.contains("getName") && setters.contains("setName"));
}

#[test]
fn a_language_nobody_generates_for_produces_nothing() {
    test_support::install_grammars();
    let fields = [field("name", "String", false)];
    assert_eq!(generate_accessors(Language::Yaml, &fields, "    ", AccessorKind::Both), "");
    assert_eq!(
        generate_method(Language::Kotlin, "Foo", &fields, "    ", GenerateMethodKind::ToString),
        ""
    );
}

#[test]
fn generate_method_dispatches_to_the_right_template() {
    let fields = [field("x", "int", false)];
    assert!(generate_method(java(), "Foo", &fields, "    ", GenerateMethodKind::Constructor).contains("public Foo(int x)"));
    assert!(generate_method(java(), "Foo", &fields, "    ", GenerateMethodKind::ToString).contains("toString()"));
    assert!(generate_method(java(), "Foo", &fields, "    ", GenerateMethodKind::EqualsAndHashCode).contains("hashCode()"));
}

#[test]
fn generation_targets_drop_types_without_a_body() {
    let types = vec![
        TypeFields {
            declaration: TypeDeclaration {
                name: "WithBody".to_string(),
                insertion_byte: Some(10),
            },
            fields: vec![field("x", "int", false)],
        },
        TypeFields {
            declaration: TypeDeclaration {
                name: "Bodiless".to_string(),
                insertion_byte: None,
            },
            fields: vec![field("y", "int", false)],
        },
    ];
    assert_eq!(
        generation_targets(types),
        [target("WithBody", vec![field("x", "int", false)], 10)]
    );
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
    let (new_text, _) = insert_at_class_end(
        text,
        insertion_byte,
        "    public int getX() {\n        return this.x;\n    }\n",
    );
    assert_eq!(
        new_text,
        "class Foo {\n    private int x;\n\n    public int getX() {\n        return this.x;\n    }\n}\n"
    );
}

#[test]
fn apply_dialog_targets_whichever_class_is_selected() {
    let text = "class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n";
    let foo_end = text.find("}\nclass Bar").unwrap();
    let bar_end = text.rfind('}').unwrap();
    let classes = vec![
        target("Foo", vec![field("x", "int", false)], foo_end),
        target("Bar", vec![field("y", "int", false)], bar_end),
    ];
    let mut dialog = GenerateAccessorsDialog::new(java(), classes, AccessorKind::Getters);
    dialog.select_class(1);

    let (new_text, _) = apply_dialog(&dialog, text, "    ").expect("Bar has a field to generate for");
    assert!(new_text.contains("getY"));
    assert!(!new_text.contains("getX"));
}

#[test]
fn apply_dialog_skips_unchecked_fields() {
    let text = "class Foo {\n    private int x;\n    private int y;\n}\n";
    let insertion_byte = text.rfind('}').unwrap();
    let classes = vec![target(
        "Foo",
        vec![field("x", "int", false), field("y", "int", false)],
        insertion_byte,
    )];
    let mut dialog = GenerateAccessorsDialog::new(java(), classes, AccessorKind::Getters);
    dialog.set_checked(1, false);

    let (new_text, _) = apply_dialog(&dialog, text, "    ").expect("x is still checked");
    assert!(new_text.contains("getX"));
    assert!(!new_text.contains("getY"));
}

#[test]
fn apply_dialog_is_none_when_every_checked_field_is_read_only_for_setters() {
    let text = "class Foo {\n    private final int id;\n}\n";
    let insertion_byte = text.rfind('}').unwrap();
    let classes = vec![target("Foo", vec![field("id", "int", true)], insertion_byte)];
    let dialog = GenerateAccessorsDialog::new(java(), classes, AccessorKind::Setters);

    assert_eq!(apply_dialog(&dialog, text, "    "), None);
}

#[test]
fn apply_dialog_is_none_when_nothing_is_checked() {
    let text = "class Foo {\n    private int x;\n}\n";
    let insertion_byte = text.rfind('}').unwrap();
    let classes = vec![target("Foo", vec![field("x", "int", false)], insertion_byte)];
    let mut dialog = GenerateAccessorsDialog::new(java(), classes, AccessorKind::Getters);
    dialog.set_checked(0, false);

    assert_eq!(apply_dialog(&dialog, text, "    "), None);
}

#[test]
fn apply_method_dialog_targets_whichever_class_is_selected() {
    let text = "class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n";
    let foo_end = text.find("}\nclass Bar").unwrap();
    let bar_end = text.rfind('}').unwrap();
    let classes = vec![
        target("Foo", vec![field("x", "int", false)], foo_end),
        target("Bar", vec![field("y", "int", false)], bar_end),
    ];
    let mut dialog = GenerateMethodDialog::new(java(), classes, GenerateMethodKind::ToString);
    dialog.select_class(1);

    let (new_text, _) = apply_method_dialog(&dialog, text, "    ");
    assert!(new_text.contains("\"y=\" + y"));
    assert!(!new_text.contains("\"x=\" + x"));
}

#[test]
fn apply_method_dialog_skips_unchecked_fields() {
    let text = "class Foo {\n    private int x;\n    private int y;\n}\n";
    let insertion_byte = text.rfind('}').unwrap();
    let classes = vec![target(
        "Foo",
        vec![field("x", "int", false), field("y", "int", false)],
        insertion_byte,
    )];
    let mut dialog = GenerateMethodDialog::new(java(), classes, GenerateMethodKind::Constructor);
    dialog.set_checked(1, false);

    let (new_text, _) = apply_method_dialog(&dialog, text, "    ");
    // Exactly the `x` parameter, not `y` too — checking for the full
    // signature (rather than e.g. `!new_text.contains("int y")`) since
    // the original `private int y;` field declaration is still
    // present in `new_text` regardless of what got generated.
    assert!(new_text.contains("public Foo(int x)"));
}

fn file(path: &str) -> FileNode {
    FileNode {
        path: std::path::PathBuf::from(path),
        name: std::path::PathBuf::from(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        kind: FileKind::File,
        children: Vec::new(),
    }
}

fn dir(path: &str, children: Vec<FileNode>) -> FileNode {
    FileNode {
        path: std::path::PathBuf::from(path),
        name: std::path::PathBuf::from(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        kind: FileKind::Dir,
        children,
    }
}

fn extensions(list: &[&str]) -> Vec<String> {
    list.iter().map(|ext| ext.to_string()).collect()
}

#[test]
fn find_source_file_by_stem_finds_a_nested_match() {
    let tree = dir(
        "/root",
        vec![file("/root/Main.java"), dir("/root/pkg", vec![file("/root/pkg/Base.java")])],
    );

    assert_eq!(
        find_source_file_by_stem(&tree, "Base", &extensions(&["java"])),
        Some(std::path::PathBuf::from("/root/pkg/Base.java"))
    );
}

#[test]
fn find_source_file_by_stem_ignores_other_extensions_with_the_same_stem() {
    let tree = dir("/root", vec![file("/root/Base.txt")]);
    assert_eq!(find_source_file_by_stem(&tree, "Base", &extensions(&["java"])), None);
}

#[test]
fn find_source_file_by_stem_returns_none_when_not_found() {
    let tree = dir("/root", vec![file("/root/Main.java")]);
    assert_eq!(find_source_file_by_stem(&tree, "NoSuchClass", &extensions(&["java"])), None);
}

#[test]
fn find_source_file_by_stem_accepts_any_of_several_extensions() {
    let tree = dir("/root", vec![file("/root/Main.kt"), file("/root/Base.kts")]);
    assert_eq!(
        find_source_file_by_stem(&tree, "Base", &extensions(&["kt", "kts"])),
        Some(std::path::PathBuf::from("/root/Base.kts"))
    );
}

#[test]
fn find_type_source_uses_the_languages_own_extensions() {
    test_support::install_grammars();
    let tree = dir("/root", vec![file("/root/Base.java"), file("/root/pkg/Other.kt")]);
    assert_eq!(
        find_type_source(&tree, "Base", Language::Java),
        Some(std::path::PathBuf::from("/root/Base.java"))
    );
    assert_eq!(find_type_source(&tree, "Base", Language::Kotlin), None);
}

#[test]
fn apply_override_dialog_generates_only_checked_methods() {
    let text = "class Foo extends Bar {\n}\n";
    let insertion_byte = text.rfind('}').unwrap();
    let methods = vec![method("run", "void"), method("stop", "void")];
    let mut dialog = OverrideMethodDialog::new(java(), methods, insertion_byte);
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
    let mut dialog = OverrideMethodDialog::new(java(), vec![method("run", "void")], insertion_byte);
    dialog.set_checked(0, false);

    assert_eq!(apply_override_dialog(&dialog, text, "    "), None);
}
