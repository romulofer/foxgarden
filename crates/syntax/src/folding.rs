use fg_core::Language;
use tree_sitter::{Node, Tree};

use crate::node_kinds::{foldable_kinds, import_kind};

/// A collapsible region of the buffer. When folded, `marker_line` stays
/// visible (with a fold marker on it) and the bytes `start_byte..end_byte`
/// are hidden. `start_byte` is the first byte *after* `marker_line`'s newline
/// — i.e. folding never hides any of the line the marker sits on, only the
/// lines below it through the end of the foldable node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRange {
    /// 0-based index of the line the `▾`/`▸` marker sits on (the line the
    /// foldable node opens on). Stays visible when folded.
    pub marker_line: usize,
    /// First hidden byte: the start of the line just below `marker_line`.
    pub start_byte: usize,
    /// One past the last hidden byte: the foldable node's end.
    pub end_byte: usize,
}

/// Every foldable region in `source`, in source order, outermost-first for any
/// given marker line. Two kinds of region: the brace/comment-delimited bodies
/// in `node_kinds::foldable_kinds` that span **more than one line** (a
/// one-line `{ ... }` body gets no marker, since there's nothing below its
/// opening line to hide), and consecutive-import blocks (`collect_import_blocks`
/// below) — a run of two or more `import` statements with no blank line
/// between any of them. A lone import, like a one-line body, gets no marker:
/// there's nothing meaningfully separate to hide.
///
/// Pure and independent of any folding UI: this is the same structural
/// "where are the collapsible regions" data a minimap or a collapse-all
/// command would also consume, which is why it's worth building and testing on
/// its own ahead of the widget work that renders it.
///
/// Nested regions are kept (a class body and the method bodies inside it are
/// separate `FoldRange`s), but two foldable nodes that open on the *same* line
/// collapse to a single range — the outermost — since one marker line can only
/// carry one fold.
pub fn foldable_ranges(tree: &Tree, source: &str, language: Language) -> Vec<FoldRange> {
    let mut ranges = Vec::new();

    let kinds = foldable_kinds(language);
    if !kinds.is_empty() {
        collect(tree.root_node(), source, kinds, &mut ranges);
    }
    if let Some(import_kind) = import_kind(language) {
        collect_import_blocks(tree.root_node(), source, import_kind, &mut ranges);
    }

    // Stable sort: for two ranges already pushed in outer-first order and
    // sharing a marker line (the pre-order DFS `collect` guarantees this for
    // nested node-kind folds; import blocks never share a marker line with
    // anything else), the outer one stays first — keep it, drop the rest via
    // the adjacent-dedup below.
    ranges.sort_by_key(|r| r.marker_line);
    ranges.dedup_by_key(|r| r.marker_line);
    ranges
}

fn collect(node: Node, source: &str, kinds: &[&str], out: &mut Vec<FoldRange>) {
    if kinds.contains(&node.kind())
        && node.start_position().row != node.end_position().row
        && let Some(range) = fold_range_for(node, source)
    {
        out.push(range);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, source, kinds, out);
    }
}

/// Turns a `marker_line..end_byte` span (starting at `start_byte`) into its
/// `FoldRange`: the hidden region runs from the start of the line just below
/// `marker_line` to `end_byte`. `None` if there's no newline between `start`
/// and `end` (can't happen for a genuinely multi-line span, but guarded
/// rather than assumed) or if that would hide an empty range. Shared by
/// `fold_range_for` (a single multi-line node) and `flush_import_run` (a run
/// of import statements, which isn't one AST node at all).
fn fold_range_from_span(marker_line: usize, start: usize, end: usize, source: &str) -> Option<FoldRange> {
    let newline = source.get(start..end)?.find('\n')? + start;
    let hidden_start = newline + 1;
    if hidden_start >= end {
        return None;
    }
    Some(FoldRange {
        marker_line,
        start_byte: hidden_start,
        end_byte: end,
    })
}

/// Turns a multi-line foldable node into its `FoldRange` — the marker sits on
/// the node's opening line.
fn fold_range_for(node: Node, source: &str) -> Option<FoldRange> {
    fold_range_from_span(node.start_position().row, node.start_byte(), node.end_byte(), source)
}

/// Whether `prev` and `next` (adjacent import statements) have a blank line
/// between them — two or more newlines in the gap between them, as opposed
/// to the single newline that just ends `prev`'s own line.
fn blank_line_between(prev: Node, next: Node, source: &str) -> bool {
    source
        .get(prev.end_byte()..next.start_byte())
        .is_some_and(|gap| gap.matches('\n').count() > 1)
}

/// Pushes one `FoldRange` for `run` — a maximal sequence of consecutive
/// import statements — if it has at least two entries; a lone import has
/// nothing meaningfully separate to hide, same "no marker" treatment a
/// one-line brace body gets.
fn flush_import_run(run: &[Node], source: &str, out: &mut Vec<FoldRange>) {
    if run.len() < 2 {
        return;
    }
    let first = run[0];
    let last = run[run.len() - 1];
    if let Some(range) = fold_range_from_span(first.start_position().row, first.start_byte(), last.end_byte(), source) {
        out.push(range);
    }
}

/// Groups `root`'s direct `import_kind` children into blocks: a maximal run
/// of imports with no blank line between any two of them is one block.
/// Import statements are always direct children of the file's root node in
/// both grammars (`node_kinds::import_kind`'s own doc comment), never
/// nested, so this only ever needs to look at `root`'s immediate children,
/// not a full recursive walk like `collect` above. A blank line between two
/// imports, or any other intervening node (a comment, a declaration), ends
/// the current run.
fn collect_import_blocks<'a>(root: Node<'a>, source: &str, import_kind: &str, out: &mut Vec<FoldRange>) {
    let mut cursor = root.walk();
    let mut run: Vec<Node<'a>> = Vec::new();

    for child in root.named_children(&mut cursor) {
        let is_import = child.kind() == import_kind;
        let breaks_run = !is_import || run.last().is_some_and(|&prev| blank_line_between(prev, child, source));

        if breaks_run {
            flush_import_run(&run, source, out);
            run.clear();
        }
        if is_import {
            run.push(child);
        }
    }
    flush_import_run(&run, source, out);
}

#[cfg(test)]
#[path = "folding_test.rs"]
mod folding_test;
