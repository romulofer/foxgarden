use super::*;
use crate::IncrementalParser;
use fg_core::Language;

fn parsed(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Kotlin);
    parser.parse(source).clone()
}

#[test]
fn kotlin_enclosing_class_finds_the_class_the_cursor_sits_in() {
    let source = "class Foo {\n    fun run() {\n    }\n}\n";
    let tree = parsed(source);
    let cursor = source.find("run").unwrap();
    assert_eq!(kotlin_enclosing_class(&tree, source, cursor), Some("Foo".to_string()));
}

#[test]
fn kotlin_enclosing_class_returns_none_outside_any_class() {
    let source = "// just a comment\n";
    let tree = parsed(source);
    assert_eq!(kotlin_enclosing_class(&tree, source, 0), None);
}

#[test]
fn kotlin_superclass_name_finds_a_delegated_constructor_call() {
    let source = "class Foo : Bar() {\n}\n";
    let tree = parsed(source);
    assert_eq!(kotlin_superclass_name(&tree, source, "Foo"), Some("Bar".to_string()));
}

#[test]
fn kotlin_superclass_name_finds_a_plain_interface_type() {
    let source = "class Foo : Baz {\n}\n";
    let tree = parsed(source);
    assert_eq!(kotlin_superclass_name(&tree, source, "Foo"), Some("Baz".to_string()));
}

#[test]
fn kotlin_superclass_name_strips_generics_and_package_qualification() {
    let source = "class Foo : java.util.ArrayList<String>() {\n}\n";
    let tree = parsed(source);
    assert_eq!(
        kotlin_superclass_name(&tree, source, "Foo"),
        Some("ArrayList".to_string())
    );
}

#[test]
fn kotlin_superclass_name_is_none_with_no_delegation_specifiers() {
    let source = "class Foo {\n}\n";
    let tree = parsed(source);
    assert_eq!(kotlin_superclass_name(&tree, source, "Foo"), None);
}

#[test]
fn kotlin_properties_in_class_body_finds_explicit_and_inferred_typed_properties() {
    let source = "class Foo {\n    val x: Int = 0\n    var y = Bar()\n}\n";
    let tree = parsed(source);
    let class_node = find_class_node(tree.root_node(), source, "Foo").unwrap();
    let body = child_by_kind(class_node, "class_body").unwrap();

    let mut props = kotlin_properties_in_class_body(body, source);
    props.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        props,
        vec![
            FieldInfo {
                name: "x".to_string(),
                java_type: "Int".to_string(),
                is_final: true
            },
            FieldInfo {
                name: "y".to_string(),
                java_type: "Bar".to_string(),
                is_final: false
            },
        ]
    );
}

#[test]
fn kotlin_properties_in_class_body_skips_an_unresolvable_inferred_property() {
    let source = "class Foo {\n    val x = someFunction()\n}\n";
    let tree = parsed(source);
    let class_node = find_class_node(tree.root_node(), source, "Foo").unwrap();
    let body = child_by_kind(class_node, "class_body").unwrap();

    assert_eq!(kotlin_properties_in_class_body(body, source), vec![]);
}

#[test]
fn kotlin_properties_in_type_includes_constructor_promoted_properties() {
    let source = "class Foo(val x: Int, var y: String, z: Boolean) {\n    val w: Bar = Bar()\n}\n";
    let tree = parsed(source);

    let mut props = kotlin_properties_in_type(&tree, source, "Foo");
    props.sort_by(|a, b| a.name.cmp(&b.name));
    let names: Vec<&str> = props.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["w", "x", "y"]);
    assert!(props.iter().find(|f| f.name == "x").unwrap().is_final);
    assert!(!props.iter().find(|f| f.name == "y").unwrap().is_final);
}

#[test]
fn kotlin_properties_in_type_returns_empty_for_an_unknown_type() {
    let source = "class Foo {\n    val x: Int = 0\n}\n";
    let tree = parsed(source);
    assert_eq!(kotlin_properties_in_type(&tree, source, "NoSuchType"), vec![]);
}

#[test]
fn kotlin_functions_in_type_excludes_private_functions() {
    let source = "class Foo {\n    fun run() {\n    }\n    private fun hidden() {\n    }\n}\n";
    let tree = parsed(source);

    let functions = kotlin_functions_in_type(&tree, source, "Foo");
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "run");
}

#[test]
fn all_kotlin_functions_in_type_includes_private_functions() {
    let source = "class Foo {\n    fun run() {\n    }\n    private fun hidden() {\n    }\n}\n";
    let tree = parsed(source);

    let functions = all_kotlin_functions_in_type(&tree, source, "Foo");
    let names: Vec<&str> = functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["run", "hidden"]);
}

#[test]
fn kotlin_functions_in_type_reads_return_type_and_parameters() {
    let source = "class Foo {\n    fun compute(a: Int, b: String): Bar {\n        return Bar()\n    }\n}\n";
    let tree = parsed(source);

    let functions = kotlin_functions_in_type(&tree, source, "Foo");
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].return_type, "Bar");
    assert_eq!(
        functions[0].params,
        vec![
            ("Int".to_string(), "a".to_string()),
            ("String".to_string(), "b".to_string())
        ]
    );
}

#[test]
fn kotlin_functions_in_type_defaults_to_unit_with_no_explicit_return_type() {
    let source = "class Foo {\n    fun run() {\n    }\n}\n";
    let tree = parsed(source);

    let functions = kotlin_functions_in_type(&tree, source, "Foo");
    assert_eq!(functions[0].return_type, "Unit");
}
