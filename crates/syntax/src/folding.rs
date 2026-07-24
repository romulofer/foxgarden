use fg_core::Language;
use tree_sitter::{Node, Tree};

use crate::node_kinds::foldable_kinds;

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
/// given marker line. Foldable nodes are the brace/comment-delimited bodies in
/// `node_kinds::foldable_kinds` that span **more than one line** — a one-line
/// `{ ... }` body gets no marker, since there's nothing below its opening line
/// to hide.
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
    let kinds = foldable_kinds(language);
    if kinds.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    collect(tree.root_node(), source, kinds, &mut ranges);

    // Pre-order DFS visits a parent before its children, so for two nodes
    // sharing a marker line the outer one is pushed first — keep it, drop the
    // rest. `ranges` is already in source (marker-line-ascending) order, so a
    // single adjacent-dedup on `marker_line` suffices.
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

/// Turns a multi-line foldable node into its `FoldRange`: the marker sits on
/// the node's opening line, and the hidden span runs from the start of the
/// next line to the node's end. `None` if the opening line has no newline
/// after the node start (can't happen for a genuinely multi-line node, but
/// guarded rather than assumed) or if that would hide an empty range.
fn fold_range_for(node: Node, source: &str) -> Option<FoldRange> {
    let start = node.start_byte();
    let end = node.end_byte();
    // First newline at or after the node's start ends the marker line; the
    // hidden region begins one byte past it.
    let newline = source.get(start..end)?.find('\n')? + start;
    let hidden_start = newline + 1;
    if hidden_start >= end {
        return None;
    }
    Some(FoldRange { marker_line: node.start_position().row, start_byte: hidden_start, end_byte: end })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IncrementalParser;

    fn parsed(source: &str) -> Tree {
        let mut parser = IncrementalParser::new(Language::Java);
        parser.parse(source).clone()
    }

    fn hidden<'a>(source: &'a str, r: &FoldRange) -> &'a str {
        &source[r.start_byte..r.end_byte]
    }

    #[test]
    fn folds_a_class_body_and_its_multi_line_methods() {
        let source = "\
class Foo {
    void a() {
        step();
    }
    void b() {
        step();
    }
}
";
        let tree = parsed(source);
        let ranges = foldable_ranges(&tree, source, Language::Java);

        // One fold for the class body, one for each multi-line method body.
        assert_eq!(ranges.len(), 3, "class body + two method bodies");

        // Marker lines are the opening lines: `class Foo {` (0), `void a() {`
        // (1), `void b() {` (4).
        assert_eq!(ranges.iter().map(|r| r.marker_line).collect::<Vec<_>>(), vec![0, 1, 4]);

        // The class-body fold hides everything through the final `}`.
        assert!(hidden(source, &ranges[0]).contains("void a()"));
        assert!(hidden(source, &ranges[0]).trim_end().ends_with('}'));
        // A method-body fold hides just that method's statements.
        assert!(hidden(source, &ranges[1]).contains("step();"));
        assert!(!hidden(source, &ranges[1]).contains("void b()"));
    }

    #[test]
    fn a_one_line_body_is_not_foldable() {
        let source = "class Foo {\n    void a() { step(); }\n}\n";
        let tree = parsed(source);
        let ranges = foldable_ranges(&tree, source, Language::Java);

        // Only the (multi-line) class body folds; the single-line method body
        // has nothing below its opening line to hide.
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].marker_line, 0);
    }

    #[test]
    fn folds_a_multi_line_block_comment() {
        let source = "\
/*
 * docs
 */
class Foo {}
";
        let tree = parsed(source);
        let ranges = foldable_ranges(&tree, source, Language::Java);
        // The block comment folds; `class Foo {}` is one line, so it doesn't.
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].marker_line, 0);
        assert!(hidden(source, &ranges[0]).contains("docs"));
    }

    #[test]
    fn hidden_span_never_includes_the_marker_line() {
        let source = "class Foo {\n    int x;\n}\n";
        let tree = parsed(source);
        let ranges = foldable_ranges(&tree, source, Language::Java);
        assert_eq!(ranges.len(), 1);
        // Nothing before the first newline (the end of `class Foo {`) is
        // hidden — the marker line stays fully visible.
        assert!(!hidden(source, &ranges[0]).contains("class Foo"));
        assert!(hidden(source, &ranges[0]).contains("int x;"));
    }

    #[test]
    fn a_language_without_a_foldable_vocabulary_returns_empty() {
        let source = "class Foo {\n    void a() {\n        step();\n    }\n}\n";
        let tree = parsed(source);
        assert!(foldable_ranges(&tree, source, Language::Yaml).is_empty());
    }
}
