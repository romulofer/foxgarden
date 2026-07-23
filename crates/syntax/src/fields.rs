use tree_sitter::{Node, Tree};

/// One field found by `java_fields_in_enclosing_class`, with what's needed
/// to generate a getter/setter for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    pub name: String,
    pub java_type: String,
    /// `final` fields can't be reassigned, so callers should generate only
    /// a getter for these, not a setter.
    pub is_final: bool,
}

/// The smallest descendant of `node` (including `node` itself) whose kind
/// is `kind` and whose byte range contains `byte` — used to find "the
/// class the cursor is currently inside," walking from the tree's root.
fn smallest_containing<'tree>(node: Node<'tree>, byte: usize, kind: &str) -> Option<Node<'tree>> {
    if !(node.start_byte() <= byte && byte <= node.end_byte()) {
        return None;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = smallest_containing(child, byte, kind) {
            return Some(found);
        }
    }
    (node.kind() == kind).then_some(node)
}

/// Every field declared directly in the body of the Java class enclosing
/// `cursor_byte`, in source order — the source data for generating
/// getters/setters at the cursor. Static fields are skipped (accessors for
/// them are a much rarer, more deliberate choice than for instance state,
/// and this keeps the generated set to what's usually wanted). A
/// multi-variable declaration (`int x, y;`) yields one `FieldInfo` per
/// variable. Returns an empty `Vec` if the cursor isn't inside any class
/// body.
pub fn java_fields_in_enclosing_class(tree: &Tree, source: &str, cursor_byte: usize) -> Vec<FieldInfo> {
    let Some(class_node) = smallest_containing(tree.root_node(), cursor_byte, "class_declaration") else {
        return Vec::new();
    };
    let Some(body) = class_node.child_by_field_name("body") else {
        return Vec::new();
    };

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
        if modifiers_text.contains("static") {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IncrementalParser;
    use fg_core::Language;

    fn fields_at(source: &str, cursor_byte: usize) -> Vec<FieldInfo> {
        let mut parser = IncrementalParser::new(Language::Java);
        let tree = parser.parse(source);
        java_fields_in_enclosing_class(tree, source, cursor_byte)
    }

    #[test]
    fn finds_instance_fields_in_source_order() {
        let source = "public class Point {\n    private int x;\n    private int y;\n}\n";
        let fields = fields_at(source, source.len() - 2);

        assert_eq!(
            fields,
            vec![
                FieldInfo { name: "x".to_string(), java_type: "int".to_string(), is_final: false },
                FieldInfo { name: "y".to_string(), java_type: "int".to_string(), is_final: false },
            ]
        );
    }

    #[test]
    fn marks_final_fields() {
        let source = "class Foo {\n    private final String name;\n}\n";
        let fields = fields_at(source, source.len() - 2);

        assert_eq!(fields, vec![FieldInfo { name: "name".to_string(), java_type: "String".to_string(), is_final: true }]);
    }

    #[test]
    fn skips_static_fields() {
        let source = "class Foo {\n    private static int counter;\n    private int id;\n}\n";
        let fields = fields_at(source, source.len() - 2);

        assert_eq!(fields, vec![FieldInfo { name: "id".to_string(), java_type: "int".to_string(), is_final: false }]);
    }

    #[test]
    fn handles_a_multi_variable_declaration() {
        let source = "class Foo {\n    private int x, y;\n}\n";
        let fields = fields_at(source, source.len() - 2);

        assert_eq!(
            fields,
            vec![
                FieldInfo { name: "x".to_string(), java_type: "int".to_string(), is_final: false },
                FieldInfo { name: "y".to_string(), java_type: "int".to_string(), is_final: false },
            ]
        );
    }

    #[test]
    fn cursor_outside_any_class_returns_empty() {
        let fields = fields_at("// just a comment\n", 5);
        assert!(fields.is_empty());
    }

    #[test]
    fn only_considers_the_innermost_enclosing_class() {
        // Cursor inside `Inner`'s body — its own field should be found,
        // not `Outer`'s.
        let source = "class Outer {\n    private int outerField;\n    class Inner {\n        private int innerField;\n    }\n}\n";
        let cursor = source.find("innerField;").unwrap();
        let fields = fields_at(source, cursor);

        assert_eq!(fields, vec![FieldInfo { name: "innerField".to_string(), java_type: "int".to_string(), is_final: false }]);
    }
}
