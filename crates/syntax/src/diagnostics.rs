use fg_core::{Diagnostic, Severity};
use tree_sitter::{Node, Tree};

fn walk_errors(node: Node, out: &mut Vec<Diagnostic>) {
    if node.is_missing() {
        out.push(Diagnostic {
            range: node.byte_range(),
            severity: Severity::Error,
            message: format!("missing {}", node.kind()),
        });
        return;
    }

    if node.is_error() {
        out.push(Diagnostic {
            range: node.byte_range(),
            severity: Severity::Error,
            message: "syntax error".to_string(),
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_errors(child, out);
    }
}

/// Walks `tree` for `ERROR`/`MISSING` nodes, converting each into a
/// `Diagnostic` with the node's byte range, per SPEC.md §5.5.
pub fn syntax_errors(tree: &Tree) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    walk_errors(tree.root_node(), &mut out);
    out
}
