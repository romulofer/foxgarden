use super::*;
use crate::IncrementalParser;
use fg_core::Language;

fn classes_in(source: &str) -> Vec<ClassFields> {
    let mut parser = IncrementalParser::new(Language::Java).expect("a bundled grammar must load");
    let tree = parser.parse(source);
    java_classes_with_fields(tree, source)
}

#[test]
fn finds_instance_fields_in_source_order() {
    let source = "public class Point {\n    private int x;\n    private int y;\n}\n";
    let classes = classes_in(source);

    assert_eq!(classes.len(), 1);
    assert_eq!(
        classes[0].fields,
        vec![
            FieldInfo {
                name: "x".to_string(),
                java_type: "int".to_string(),
                is_final: false
            },
            FieldInfo {
                name: "y".to_string(),
                java_type: "int".to_string(),
                is_final: false
            },
        ]
    );
}

#[test]
fn marks_final_fields() {
    let source = "class Foo {\n    private final String name;\n}\n";
    let classes = classes_in(source);

    assert_eq!(
        classes[0].fields,
        vec![FieldInfo {
            name: "name".to_string(),
            java_type: "String".to_string(),
            is_final: true
        }]
    );
}

#[test]
fn skips_static_fields() {
    let source = "class Foo {\n    private static int counter;\n    private int id;\n}\n";
    let classes = classes_in(source);

    assert_eq!(
        classes[0].fields,
        vec![FieldInfo {
            name: "id".to_string(),
            java_type: "int".to_string(),
            is_final: false
        }]
    );
}

#[test]
fn handles_a_multi_variable_declaration() {
    let source = "class Foo {\n    private int x, y;\n}\n";
    let classes = classes_in(source);

    assert_eq!(
        classes[0].fields,
        vec![
            FieldInfo {
                name: "x".to_string(),
                java_type: "int".to_string(),
                is_final: false
            },
            FieldInfo {
                name: "y".to_string(),
                java_type: "int".to_string(),
                is_final: false
            },
        ]
    );
}

#[test]
fn a_class_with_no_fields_is_excluded() {
    let classes = classes_in("class Empty {\n}\n");
    assert!(classes.is_empty());
}

#[test]
fn a_file_with_no_classes_returns_empty() {
    let classes = classes_in("// just a comment\n");
    assert!(classes.is_empty());
}

#[test]
fn multiple_top_level_classes_are_all_returned_in_source_order() {
    let source = "class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n";
    let classes = classes_in(source);

    assert_eq!(classes.len(), 2);
    assert_eq!(classes[0].name, "Foo");
    assert_eq!(classes[1].name, "Bar");
}

#[test]
fn a_nested_class_is_listed_separately_from_its_enclosing_class() {
    let source =
        "class Outer {\n    private int outerField;\n    class Inner {\n        private int innerField;\n    }\n}\n";
    let classes = classes_in(source);

    assert_eq!(classes.len(), 2);
    assert_eq!(classes[0].name, "Outer");
    assert_eq!(
        classes[0].fields,
        vec![FieldInfo {
            name: "outerField".to_string(),
            java_type: "int".to_string(),
            is_final: false
        }]
    );
    assert_eq!(classes[1].name, "Inner");
    assert_eq!(
        classes[1].fields,
        vec![FieldInfo {
            name: "innerField".to_string(),
            java_type: "int".to_string(),
            is_final: false
        }]
    );
}

#[test]
fn insertion_byte_points_just_before_the_closing_brace() {
    let source = "class Foo {\n    private int x;\n}\n";
    let classes = classes_in(source);

    assert_eq!(&source[classes[0].insertion_byte..classes[0].insertion_byte + 1], "}");
}

fn tree_of(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Java).expect("a bundled grammar must load");
    parser.parse(source).clone()
}

#[test]
fn fields_in_type_finds_a_named_types_fields() {
    let source = "class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n";
    let tree = tree_of(source);
    let fields = fields_in_type(&tree, source, "Bar", false);
    assert_eq!(
        fields,
        vec![FieldInfo {
            name: "y".to_string(),
            java_type: "int".to_string(),
            is_final: false
        }]
    );
}

#[test]
fn fields_in_type_skips_static_fields_by_default() {
    let source = "class Foo {\n    private static int counter;\n    private int id;\n}\n";
    let tree = tree_of(source);
    let fields = fields_in_type(&tree, source, "Foo", false);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "id");
}

#[test]
fn fields_in_type_includes_static_fields_when_asked() {
    let source = "class Foo {\n    private static int counter;\n    private int id;\n}\n";
    let tree = tree_of(source);
    let fields = fields_in_type(&tree, source, "Foo", true);
    assert_eq!(fields.len(), 2);
    assert!(fields.iter().any(|f| f.name == "counter"));
}

#[test]
fn fields_in_type_returns_empty_for_an_unknown_type() {
    let source = "class Foo {\n    private int x;\n}\n";
    let tree = tree_of(source);
    assert_eq!(fields_in_type(&tree, source, "NoSuchType", false), vec![]);
}
