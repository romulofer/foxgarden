use fg_core::{Diagnostic, Severity};
use tree_sitter::{Node, Tree};

/// Descends only into subtrees that contain an error (`has_error`), so a
/// valid file costs one check at the root rather than a visit to every node
/// on every reparse. An `ERROR` node's own squiggle already covers anything
/// inside it, so its children are not reported again on top of it.
fn walk_errors(node: Node, out: &mut Vec<Diagnostic>) {
    if !node.has_error() {
        return;
    }
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
        return;
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
