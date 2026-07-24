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
mod tests {
    use super::*;
    use crate::IncrementalParser;
    use fg_core::Language;

    fn parsed(source: &str) -> Tree {
        let mut parser = IncrementalParser::new(Language::Java);
        parser.parse(source).clone()
    }

    #[test]
    fn expands_from_an_identifier_to_its_enclosing_expression() {
        let source = "class Foo {\n    void run() {\n        foo();\n    }\n}\n";
        let tree = parsed(source);
        let call_start = source.find("foo()").unwrap();
        let ident_range = call_start..call_start + 3; // "foo"

        let expanded = expand_selection(&tree, ident_range.start, ident_range.end).unwrap();
        // The identifier's smallest containing named node other than
        // itself is the whole method-invocation expression "foo()".
        assert_eq!(&source[expanded.clone()], "foo()");
        assert!(expanded.start <= ident_range.start && expanded.end >= ident_range.end);
    }

    #[test]
    fn expanding_a_range_that_already_matches_a_node_climbs_to_its_parent() {
        let source = "class Foo {\n    void run() {\n        foo();\n    }\n}\n";
        let tree = parsed(source);
        let call_start = source.find("foo()").unwrap();
        let call_end = call_start + "foo()".len();

        // "foo()" is itself a full method-invocation node — expanding
        // *from* its exact range must climb past it to the enclosing
        // statement, not return the identical range back.
        let expanded = expand_selection(&tree, call_start, call_end).unwrap();
        assert_ne!(expanded, call_start..call_end);
        assert!(expanded.start <= call_start && expanded.end >= call_end);
    }

    #[test]
    fn repeated_expansion_strictly_grows_the_range_each_time() {
        let source = "class Foo {\n    void run() {\n        foo();\n    }\n}\n";
        let tree = parsed(source);
        let call_start = source.find("foo()").unwrap();

        let mut range = call_start..call_start + 3; // "foo"
        for _ in 0..4 {
            let next = expand_selection(&tree, range.start, range.end).unwrap();
            assert!(
                next.start <= range.start && next.end >= range.end && next != range,
                "expansion must strictly grow: {range:?} -> {next:?}"
            );
            range = next;
        }
    }

    #[test]
    fn expanding_the_whole_file_selection_returns_none() {
        let source = "class Foo {}\n";
        let tree = parsed(source);

        // The root node's own range already covers the whole source, and
        // it has no parent left to climb to.
        assert_eq!(expand_selection(&tree, 0, source.len()), None);
    }

    #[test]
    fn expanding_a_collapsed_cursor_finds_the_smallest_touching_named_node() {
        let source = "class Foo {\n    int x;\n}\n";
        let tree = parsed(source);
        let x_pos = source.find('x').unwrap();

        let expanded = expand_selection(&tree, x_pos, x_pos).unwrap();
        assert!(source[expanded].contains('x'));
    }
}
