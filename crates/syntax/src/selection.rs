use std::ops::Range;

use tree_sitter::Tree;

/// Finds the smallest named syntax node whose byte range contains
/// `start..end` — one step of "expand selection to the next enclosing
/// syntax node" (`Ctrl+W`). If the smallest containing node's range is
/// already exactly `start..end` (the common case: that's usually how the
/// previous expand landed, or the caller passed an AST-aligned range to
/// begin with), climbs to its nearest strictly-larger ancestor instead, so
/// this always grows the selection rather than being a no-op on a range
/// that already exactly matches a node. Only ever returns a *named* node's
/// range — anonymous nodes (bare punctuation/keyword tokens) aren't a
/// meaningful selection unit on their own, so a walk that lands on one
/// keeps climbing to the nearest named ancestor.
///
/// `start == end` (an empty/collapsed selection) is a valid query — it
/// returns the smallest named node touching that point, the same "expand
/// from the cursor" starting case `Ctrl+D`'s word lookup handles at the
/// text level rather than the syntax-tree level.
///
/// Returns `None` only when there's no containing node at all (`start`/
/// `end` outside the tree's range) or the walk reaches the root with
/// nothing left to climb to (an already-whole-file selection).
pub fn expand_selection(tree: &Tree, start: usize, end: usize) -> Option<Range<usize>> {
    let mut node = tree.root_node().named_descendant_for_byte_range(start, end)?;
    while node.byte_range() == (start..end) {
        node = node.parent()?;
    }
    while !node.is_named() {
        node = node.parent()?;
    }
    Some(node.byte_range())
}

#[cfg(test)]
#[path = "selection_test.rs"]
mod selection_test;
