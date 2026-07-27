use tree_sitter::{Node, Tree};

/// One field found in a class body, with what's needed to generate a
/// getter/setter for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    pub name: String,
    pub java_type: String,
    /// `final` fields can't be reassigned, so callers should generate only
    /// a getter for these, not a setter.
    pub is_final: bool,
}

/// One Java class with at least one eligible field, found by
/// `java_classes_with_fields`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassFields {
    pub name: String,
    pub fields: Vec<FieldInfo>,
    /// Byte offset of the class body's closing `}` — where generated
    /// accessors get inserted, right before it.
    pub insertion_byte: usize,
}

/// Every field declared directly in `body` (a `class_declaration`'s body
/// node), in source order. Static fields are skipped unless
/// `include_static` is set — accessors (`java_classes_with_fields`'s use)
/// want them skipped (a much rarer, more deliberate choice than for
/// instance state), completion (`fields_in_type`'s use) wants them
/// included, since a class's constants are legitimately callable off
/// `this.`/`super.`. A multi-variable declaration (`int x, y;`) yields one
/// `FieldInfo` per variable.
pub(crate) fn fields_in_class_body(body: Node, source: &str, include_static: bool) -> Vec<FieldInfo> {
    let mut fields = Vec::new();
    let mut body_cursor = body.walk();
    for field_decl in body.children(&mut body_cursor) {
        if field_decl.kind() != "field_declaration" {
            continue;
        }
        let Some(type_node) = field_decl.child_by_field_name("type") else {
            continue;
        };
        // Modifiers ("public", "static", "final", ...) are anonymous
        // tokens inside this node rather than distinct named child types
        // (per tree-sitter-java's own node-types.json), so the simplest
        // reliable way to detect them is a text search over the span
        // between the declaration's start and its type — there's nothing
        // else that could appear there.
        let modifiers_text = &source[field_decl.start_byte()..type_node.start_byte()];
        if modifiers_text.contains("static") && !include_static {
            continue;
        }
        let is_final = modifiers_text.contains("final");
        let java_type = source[type_node.byte_range()].to_string();

        let mut declarator_cursor = field_decl.walk();
        for declarator in field_decl.children_by_field_name("declarator", &mut declarator_cursor) {
            let Some(name_node) = declarator.child_by_field_name("name") else {
                continue;
            };
            fields.push(FieldInfo {
                name: source[name_node.byte_range()].to_string(),
                java_type: java_type.clone(),
                is_final,
            });
        }
    }
    fields
}

/// Every class in the file with at least one eligible field, in source
/// order — including nested classes, each keeping only its own directly
/// declared fields (a nested class's fields aren't attributed to its
/// enclosing class, since they're a separate `class_declaration` with its
/// own body). The data source for the getters/setters picker: which
/// classes exist to generate accessors for, and where to insert them.
pub fn java_classes_with_fields(tree: &Tree, source: &str) -> Vec<ClassFields> {
    let mut classes = Vec::new();
    collect_classes_with_fields(tree.root_node(), source, &mut classes);
    classes
}

fn collect_classes_with_fields(node: Node, source: &str, out: &mut Vec<ClassFields>) {
    if node.kind() == "class_declaration"
        && let Some(name_node) = node.child_by_field_name("name")
        && let Some(body) = node.child_by_field_name("body")
    {
        let fields = fields_in_class_body(body, source, false);
        if !fields.is_empty() {
            out.push(ClassFields {
                name: source[name_node.byte_range()].to_string(),
                fields,
                insertion_byte: body.end_byte().saturating_sub(1),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_classes_with_fields(child, source, out);
    }
}

/// Every field declared directly in `type_name`'s class body, anywhere in
/// `tree` — analogous to `methods::methods_in_type` but for fields, and
/// completion's (`SPEC.md` §4) entry point for "what fields does this
/// type have," as opposed to `java_classes_with_fields`'s "every class
/// that has fields" whole-file listing. `include_static` is threaded
/// straight through to `fields_in_class_body`.
pub fn fields_in_type(tree: &Tree, source: &str, type_name: &str, include_static: bool) -> Vec<FieldInfo> {
    let mut out = Vec::new();
    collect_fields_in_type(tree.root_node(), source, type_name, include_static, &mut out);
    out
}

fn collect_fields_in_type(node: Node, source: &str, type_name: &str, include_static: bool, out: &mut Vec<FieldInfo>) {
    if node.kind() == "class_declaration"
        && node
            .child_by_field_name("name")
            .is_some_and(|n| &source[n.byte_range()] == type_name)
    {
        if let Some(body) = node.child_by_field_name("body") {
            out.extend(fields_in_class_body(body, source, include_static));
        }
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_fields_in_type(child, source, type_name, include_static, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IncrementalParser;
    use fg_core::Language;

    fn classes_in(source: &str) -> Vec<ClassFields> {
        let mut parser = IncrementalParser::new(Language::Java);
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
        let source = "class Outer {\n    private int outerField;\n    class Inner {\n        private int innerField;\n    }\n}\n";
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
        let mut parser = IncrementalParser::new(Language::Java);
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
}
