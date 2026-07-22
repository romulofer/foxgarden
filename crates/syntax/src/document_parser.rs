use fg_core::Language;
use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::language::ts_language;

/// Owns an incremental tree-sitter parse for a single document.
pub struct IncrementalParser {
    parser: Parser,
    language: Language,
    tree: Option<Tree>,
}

impl IncrementalParser {
    pub fn new(language: Language) -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&ts_language(language))
            .expect("bundled grammar must load");
        Self {
            parser,
            language,
            tree: None,
        }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// Parses `source` from scratch, discarding any previous tree.
    pub fn parse(&mut self, source: &str) -> &Tree {
        self.tree = self.parser.parse(source, None);
        self.tree.as_ref().expect("parse must succeed")
    }

    /// Applies `edit` to the previous tree, then reparses `source`
    /// incrementally against it (tree-sitter's `edit()` + `parse()` reusing
    /// the old tree, per SPEC.md §5.4).
    pub fn reparse(&mut self, source: &str, edit: InputEdit) -> &Tree {
        if let Some(tree) = self.tree.as_mut() {
            tree.edit(&edit);
        }
        self.tree = self.parser.parse(source, self.tree.as_ref());
        self.tree.as_ref().expect("parse must succeed")
    }
}

/// Converts a byte offset in `text` into a tree-sitter `Point` (row/column),
/// for building `InputEdit`s from a plain byte-offset edit description.
pub fn byte_to_point(text: &str, byte: usize) -> Point {
    let mut row = 0;
    let mut last_newline_end = 0;
    for (i, b) in text.as_bytes()[..byte].iter().enumerate() {
        if *b == b'\n' {
            row += 1;
            last_newline_end = i + 1;
        }
    }
    Point {
        row,
        column: byte - last_newline_end,
    }
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let max = a.len().min(b.len());
    let mut i = 0;
    while i < max && a[i] == b[i] {
        i += 1;
    }
    i
}

fn common_suffix_len(a: &str, b: &str, prefix_len: usize) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let max = (a.len() - prefix_len).min(b.len() - prefix_len);
    let mut i = 0;
    while i < max && a[a.len() - 1 - i] == b[b.len() - 1 - i] {
        i += 1;
    }
    i
}

/// Computes the `InputEdit` describing how `old` was changed into `new`, by
/// finding the common prefix/suffix between the two full-text snapshots.
/// Used to feed a plain-`String`-backed text widget's edits into
/// `IncrementalParser::reparse` without the widget needing to track edits
/// itself.
pub fn diff_edit(old: &str, new: &str) -> InputEdit {
    let mut prefix_len = common_prefix_len(old, new);
    // Keep the split on a UTF-8 char boundary in both strings.
    while prefix_len > 0 && (!old.is_char_boundary(prefix_len) || !new.is_char_boundary(prefix_len)) {
        prefix_len -= 1;
    }

    let mut suffix_len = common_suffix_len(old, new, prefix_len);
    while suffix_len > 0
        && (!old.is_char_boundary(old.len() - suffix_len) || !new.is_char_boundary(new.len() - suffix_len))
    {
        suffix_len -= 1;
    }

    let start_byte = prefix_len;
    let old_end_byte = old.len() - suffix_len;
    let new_end_byte = new.len() - suffix_len;

    InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte,
        start_position: byte_to_point(old, start_byte),
        old_end_position: byte_to_point(old, old_end_byte),
        new_end_position: byte_to_point(new, new_end_byte),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_to_point_tracks_rows_and_columns() {
        let text = "abc\ndef\nghi";
        assert_eq!(byte_to_point(text, 0), Point { row: 0, column: 0 });
        assert_eq!(byte_to_point(text, 3), Point { row: 0, column: 3 });
        assert_eq!(byte_to_point(text, 4), Point { row: 1, column: 0 });
        assert_eq!(byte_to_point(text, 9), Point { row: 2, column: 1 });
    }

    #[test]
    fn diff_edit_detects_pure_insertion() {
        let old = "class Hello {}";
        let new = "class Hello { int x; }";
        let edit = diff_edit(old, new);
        assert_eq!(edit.start_byte, 13);
        assert_eq!(edit.old_end_byte, 13);
        assert_eq!(edit.new_end_byte, 21);
        assert_eq!(&new[edit.start_byte..edit.new_end_byte], " int x; ");
    }

    #[test]
    fn diff_edit_detects_pure_deletion() {
        let old = "class Hello { int x; }";
        let new = "class Hello {}";
        let edit = diff_edit(old, new);
        assert_eq!(edit.start_byte, 13);
        assert_eq!(edit.old_end_byte, 21);
        assert_eq!(edit.new_end_byte, 13);
        assert_eq!(&old[edit.start_byte..edit.old_end_byte], " int x; ");
    }

    #[test]
    fn diff_edit_detects_replacement() {
        let old = "let x = 1;";
        let new = "let x = 999;";
        let edit = diff_edit(old, new);
        assert_eq!(&old[edit.start_byte..edit.old_end_byte], "1");
        assert_eq!(&new[edit.start_byte..edit.new_end_byte], "999");
    }

    #[test]
    fn diff_edit_handles_multibyte_boundary() {
        let old = "// caf\u{e9} shop\nlet x = 1;";
        let new = "// caf\u{e9} shop\nlet x = 42;";
        let edit = diff_edit(old, new);
        assert!(old.is_char_boundary(edit.start_byte));
        assert!(old.is_char_boundary(edit.old_end_byte));
        assert!(new.is_char_boundary(edit.new_end_byte));
        assert_eq!(&old[edit.start_byte..edit.old_end_byte], "1");
        assert_eq!(&new[edit.start_byte..edit.new_end_byte], "42");
    }
}
