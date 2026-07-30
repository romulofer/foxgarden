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

    #[test]
    fn folds_a_run_of_two_or_more_consecutive_imports_but_not_a_lone_one() {
        // Mirrors the real-world shape reported: one lone import, a blank
        // line, then two separate multi-import blocks separated by another
        // blank line.
        let source = "\
package com.example;

import static org.springframework.http.MediaType.APPLICATION_PDF;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.http.HttpStatus;
import org.springframework.web.bind.annotation.RestController;

import br.ufsc.bridge.pec.backend.app.config.security.UserPrincipal;
import br.ufsc.bridge.pec.backend.module.Resources;

class Foo {}
";
        let tree = parsed(source);
        let ranges = foldable_ranges(&tree, source, Language::Java);

        // Only the two multi-import blocks fold — the lone
        // `import static ...` line gets no marker.
        assert_eq!(ranges.len(), 2);

        assert!(hidden(source, &ranges[0]).contains("HttpStatus"));
        assert!(hidden(source, &ranges[0]).contains("RestController"));
        assert!(!hidden(source, &ranges[0]).contains("Autowired"), "marker line itself stays visible");
        assert!(!hidden(source, &ranges[0]).contains("UserPrincipal"), "must not swallow the next block");

        assert!(hidden(source, &ranges[1]).contains("Resources"));
        assert!(!hidden(source, &ranges[1]).contains("UserPrincipal"), "marker line itself stays visible");
    }

    #[test]
    fn a_single_import_with_no_neighbors_is_not_foldable() {
        let source = "import java.util.List;\n\nclass Foo {}\n";
        let tree = parsed(source);
        assert!(foldable_ranges(&tree, source, Language::Java).is_empty());
    }

    #[test]
    fn exactly_two_consecutive_imports_is_the_minimum_foldable_block() {
        let source = "import java.util.List;\nimport java.util.Map;\n\nclass Foo {}\n";
        let tree = parsed(source);
        let ranges = foldable_ranges(&tree, source, Language::Java);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].marker_line, 0);
        assert!(hidden(source, &ranges[0]).contains("Map"));
    }

    #[test]
    fn a_comment_between_two_imports_breaks_the_run() {
        let source = "\
import java.util.List;
// why this next one exists
import java.util.Map;

class Foo {}
";
        let tree = parsed(source);
        // Neither import has an unbroken run of 2+ with the comment between
        // them, so nothing folds.
        assert!(foldable_ranges(&tree, source, Language::Java).is_empty());
    }

    #[test]
    fn kotlin_folds_a_run_of_consecutive_imports() {
        let mut parser = IncrementalParser::new(fg_core::Language::Kotlin);
        // `class Foo {}` is a single-line (empty) body, same "not foldable on
        // its own" convention the Java import-run tests use above — keeps
        // this test focused on import-block folding, not class-body folding
        // (covered separately by `kotlin_folds_a_class_body_a_method_body_
        // and_a_control_flow_block` below).
        let source = "\
package com.example

import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PostMapping

class Foo {}
";
        let tree = parser.parse(source).clone();
        let ranges = foldable_ranges(&tree, source, fg_core::Language::Kotlin);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].marker_line, 2);
        assert!(hidden(source, &ranges[0]).contains("PostMapping"));
    }

    #[test]
    fn kotlin_a_lone_import_is_not_foldable() {
        let mut parser = IncrementalParser::new(fg_core::Language::Kotlin);
        let source = "import org.springframework.web.bind.annotation.GetMapping\n\nclass Foo {}\n";
        let tree = parser.parse(source).clone();
        assert!(foldable_ranges(&tree, source, fg_core::Language::Kotlin).is_empty());
    }

    #[test]
    fn kotlin_folds_a_class_body_a_method_body_and_a_control_flow_block() {
        let mut parser = IncrementalParser::new(fg_core::Language::Kotlin);
        let source = "\
class Foo {
    fun bar(): Int {
        if (true) {
            return 1
        }
        return 0
    }
}
";
        let tree = parser.parse(source).clone();
        let ranges = foldable_ranges(&tree, source, fg_core::Language::Kotlin);

        // class body (0), method body (1), if-block (2) — three nested folds.
        assert_eq!(ranges.iter().map(|r| r.marker_line).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert!(hidden(source, &ranges[0]).contains("fun bar"));
        assert!(hidden(source, &ranges[1]).contains("if (true)"));
        assert!(hidden(source, &ranges[2]).contains("return 1"));
        assert!(!hidden(source, &ranges[2]).contains("return 0"));
    }

    #[test]
    fn kotlin_folds_an_enum_class_body() {
        let mut parser = IncrementalParser::new(fg_core::Language::Kotlin);
        let source = "enum class Color {\n    RED, GREEN\n}\n";
        let tree = parser.parse(source).clone();
        let ranges = foldable_ranges(&tree, source, fg_core::Language::Kotlin);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].marker_line, 0);
        assert!(hidden(source, &ranges[0]).contains("RED"));
    }

    #[test]
    fn kotlin_folds_a_multi_line_block_comment() {
        let mut parser = IncrementalParser::new(fg_core::Language::Kotlin);
        let source = "/*\n * docs\n */\nclass Foo {}\n";
        let tree = parser.parse(source).clone();
        let ranges = foldable_ranges(&tree, source, fg_core::Language::Kotlin);
        // The block comment folds; `class Foo {}` is one line, so it doesn't.
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].marker_line, 0);
        assert!(hidden(source, &ranges[0]).contains("docs"));
    }
}
