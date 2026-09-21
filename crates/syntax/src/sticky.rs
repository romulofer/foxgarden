use fg_core::Language;
use tree_sitter::Tree;

use crate::node_kinds::scope_kinds;

/// Start byte of each scope-defining node (a class/method/etc. declaration —
/// see `node_kinds::scope_kinds`) that encloses `byte_offset`, **outermost
/// first**. This is the data behind sticky scroll: the topmost visible line's
/// byte offset goes in, and the declaration headers that should pin to the
/// top of the viewport come out, ordered so they stack outer-to-inner
/// top-to-bottom.
///
/// Structurally the same upward climb `methods::enclosing_class` does, just
/// generalized from a single kind to the per-language scope set and
/// collecting the whole ancestor chain instead of stopping at the first hit.
/// Returns **byte offsets**, not line indices — the caller already owns the
/// byte↔line↔row mapping it needs for the gutter, and keeping this
/// line-agnostic makes it testable without any notion of rendering.
///
/// Empty when `byte_offset` is outside the tree, when nothing scope-defining
/// encloses it (top-level code), or when the language has no scope vocabulary
/// (anything but Java today).
pub fn enclosing_scope_starts(tree: &Tree, byte_offset: usize, language: Language) -> Vec<usize> {
    let kinds = scope_kinds(language);
    if kinds.is_empty() {
        return Vec::new();
    }

    let mut starts = Vec::new();
    let mut node = tree
        .root_node()
        .named_descendant_for_byte_range(byte_offset, byte_offset);
    while let Some(current) = node {
        if kinds.contains(&current.kind()) {
            starts.push(current.start_byte());
        }
        node = current.parent();
    }

    // Collected innermost-first on the way up; sticky scroll wants outermost
    // first so the headers stack in nesting order down the top of the view.
    starts.reverse();
    starts
}

#[cfg(test)]
#[path = "sticky_test.rs"]
mod sticky_test;
