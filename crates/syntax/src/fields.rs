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
#[path = "fields_test.rs"]
mod fields_test;
