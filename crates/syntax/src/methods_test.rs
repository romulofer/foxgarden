use super::*;
use crate::IncrementalParser;
use fg_core::Language;

fn parsed(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Java);
    parser.parse(source).clone()
}

#[test]
fn enclosing_class_finds_the_class_the_cursor_sits_in() {
    let source = "class Foo {\n    void run() {\n    }\n}\n";
    let tree = parsed(source);
    let cursor = source.find("run").unwrap();

    let (name, insertion_byte) = enclosing_class(&tree, source, cursor).unwrap();
    assert_eq!(name, "Foo");
    assert_eq!(&source[insertion_byte..insertion_byte + 1], "}");
}

#[test]
fn enclosing_class_finds_the_innermost_class_for_nested_classes() {
    let source = "class Outer {\n    class Inner {\n        void run() {\n        }\n    }\n}\n";
    let tree = parsed(source);
    let cursor = source.find("run").unwrap();

    let (name, _) = enclosing_class(&tree, source, cursor).unwrap();
    assert_eq!(name, "Inner");
}

#[test]
fn enclosing_class_returns_none_outside_any_class() {
    let source = "// just a comment\n";
    let tree = parsed(source);
    assert_eq!(enclosing_class(&tree, source, 0), None);
}

#[test]
fn superclass_name_finds_the_extends_clause() {
    let source = "class Foo extends Bar {\n}\n";
    let tree = parsed(source);
    assert_eq!(superclass_name(&tree, source, "Foo"), Some("Bar".to_string()));
}

#[test]
fn superclass_name_strips_generic_arguments() {
    let source = "class Foo extends Bar<String> {\n}\n";
    let tree = parsed(source);
    assert_eq!(superclass_name(&tree, source, "Foo"), Some("Bar".to_string()));
}

#[test]
fn superclass_name_strips_package_qualification() {
    let source = "class Foo extends java.util.AbstractList {\n}\n";
    let tree = parsed(source);
    assert_eq!(superclass_name(&tree, source, "Foo"), Some("AbstractList".to_string()));
}

#[test]
fn superclass_name_falls_back_to_the_first_implemented_interface() {
    let source = "class Foo implements Runnable, java.io.Closeable {\n}\n";
    let tree = parsed(source);
    assert_eq!(superclass_name(&tree, source, "Foo"), Some("Runnable".to_string()));
}

#[test]
fn superclass_name_prefers_extends_over_implements() {
    let source = "class Foo extends Bar implements Runnable {\n}\n";
    let tree = parsed(source);
    assert_eq!(superclass_name(&tree, source, "Foo"), Some("Bar".to_string()));
}

#[test]
fn superclass_name_is_none_with_neither_clause() {
    let source = "class Foo {\n}\n";
    let tree = parsed(source);
    assert_eq!(superclass_name(&tree, source, "Foo"), None);
}

#[test]
fn methods_in_type_finds_overridable_methods() {
    let source =
        "class Bar {\n    public void run() {\n    }\n    public int compute(int x) {\n        return x;\n    }\n}\n";
    let tree = parsed(source);

    let methods = methods_in_type(&tree, source, "Bar");

    assert_eq!(methods.len(), 2);
    assert_eq!(methods[0].name, "run");
    assert_eq!(methods[0].return_type, "void");
    assert!(methods[0].params.is_empty());
    assert_eq!(methods[1].name, "compute");
    assert_eq!(methods[1].return_type, "int");
    assert_eq!(methods[1].params, vec![("int".to_string(), "x".to_string())]);
}

#[test]
fn methods_in_type_excludes_static_private_and_final_methods() {
    let source = "class Bar {\n    public static void a() {}\n    private void b() {}\n    public final void c() {}\n    public void d() {}\n}\n";
    let tree = parsed(source);

    let methods = methods_in_type(&tree, source, "Bar");

    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "d");
}

#[test]
fn methods_in_type_finds_interface_methods() {
    let source = "interface Bar {\n    void run();\n}\n";
    let tree = parsed(source);

    let methods = methods_in_type(&tree, source, "Bar");

    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "run");
}

#[test]
fn methods_in_type_returns_empty_for_an_unknown_type() {
    let source = "class Bar {\n    void run() {}\n}\n";
    let tree = parsed(source);
    assert_eq!(methods_in_type(&tree, source, "NoSuchType"), vec![]);
}

#[test]
fn all_methods_in_type_includes_static_private_and_final_methods() {
    let source = "class Bar {\n    public static void a() {}\n    private void b() {}\n    public final void c() {}\n    public void d() {}\n}\n";
    let tree = parsed(source);

    let methods = all_methods_in_type(&tree, source, "Bar");

    assert_eq!(methods.len(), 4);
    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c", "d"]);
}
