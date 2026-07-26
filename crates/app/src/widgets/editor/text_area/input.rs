//! The virtualized editor's **pure caret + edit model** (PLAN.md 2d/2e): the
//! motion and editing operations `egui::TextEdit` used to own privately,
//! re-expressed as `(text, caret) + action -> (text, caret)` functions with no
//! frame, no `egui::Context`, no input queue. This is the parity net — the
//! table tests over these functions are what prove the reimplemented cursor
//! behaviour matches what `TextEdit` gave before the swap (2h), and they run in
//! microseconds with none of the interactive plumbing.
//!
//! All offsets are **char** offsets (the currency the rest of the editor's
//! interception/codegen code already speaks), converted to bytes only at the
//! moment a `&str` is sliced.

/// A single caret and its selection anchor, in char offsets. Collapsed (no
/// selection) when `primary == anchor`. `primary` is the moving end (where the
/// caret visibly is); `anchor` is the fixed end a Shift-drag/Shift-arrow
/// extends from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caret {
    pub primary: usize,
    pub anchor: usize,
}

impl Caret {
    pub fn at(pos: usize) -> Self {
        Self {
            primary: pos,
            anchor: pos,
        }
    }

    pub fn is_collapsed(&self) -> bool {
        self.primary == self.anchor
    }

    /// The selection as a sorted `start..end` char range (empty when collapsed).
    pub fn range(&self) -> std::ops::Range<usize> {
        self.primary.min(self.anchor)..self.primary.max(self.anchor)
    }

    /// Places both ends at `pos` (collapses the selection).
    fn collapsed_at(pos: usize) -> Self {
        Self::at(pos)
    }

    /// Sets `primary` to `pos`, keeping `anchor` when `extend` (Shift held) or
    /// collapsing onto `pos` otherwise — the shared tail of every motion.
    fn moved_to(self, pos: usize, extend: bool) -> Self {
        Self {
            primary: pos,
            anchor: if extend { self.anchor } else { pos },
        }
    }
}

/// Line-start offsets (SPEC.md §7 / PLAN.md Phase 5): built once per actual
/// text change — not once per motion, see `shell.rs`'s `process_events`, the
/// sole caller of every function below that takes one — so `move_*`/
/// `replace_selection` stop re-scanning the whole buffer from char/byte 0 on
/// every single motion or edit.
///
/// Deliberately narrower than a full char->byte table: line *boundaries* are
/// O(log n) to find via binary search over `starts`, but converting a byte
/// offset that lands mid-line (`char_to_byte`) still walks that one line's
/// own bytes — bounded by line length, not file size, which is the actual win
/// for typical source files (many short lines, not one enormous one). This
/// keeps the module's existing `&str`-only, `Rope`-decoupled contract intact
/// (`LineIndex` is just another plain-data parameter, no `Rope`/`Document`
/// dependency) rather than reintroducing the coupling `text_area` was built
/// to avoid.
///
/// Scoped to `input.rs`'s own hot per-motion/per-keystroke functions —
/// `text_offset::char_to_byte`/`byte_to_char`, used throughout `widget.rs`'s
/// interception blocks, `context_menu.rs`, `codegen.rs`, `templates.rs`, and
/// `auto_edit.rs`, are each called on a specific, comparatively rare user
/// action (Ctrl+/, wrap-selection, code generation, …), not on every
/// keystroke or arrow-press the way everything here is — left as plain O(n)
/// scans rather than threading an index through that much wider, more
/// decentralized call-site set for a proportionally smaller win.
pub(super) struct LineIndex {
    /// `(char offset, byte offset)` of each line's start, index 0 first;
    /// always has at least one entry (line 0 starts at `(0, 0)`). Length is
    /// `last_line() + 1`.
    starts: Vec<(usize, usize)>,
    total_chars: usize,
}

impl LineIndex {
    pub(super) fn build(text: &str) -> Self {
        let mut starts = vec![(0usize, 0usize)];
        let mut total_chars = 0;
        for (char_idx, (byte_idx, c)) in text.char_indices().enumerate() {
            total_chars = char_idx + 1;
            if c == '\n' {
                starts.push((total_chars, byte_idx + 1));
            }
        }
        Self { starts, total_chars }
    }

    pub(super) fn char_len(&self) -> usize {
        self.total_chars
    }

    /// Index of the last line (== number of `\n`s).
    pub(super) fn last_line(&self) -> usize {
        self.starts.len() - 1
    }

    /// Which line `char_off` falls on — the greatest line whose own start is
    /// at-or-before `char_off`, found by binary search over `starts` rather
    /// than a linear scan from char 0. `char_off` must already be `<=
    /// total_chars` (every public method clamps before calling this).
    fn line_of(&self, char_off: usize) -> usize {
        self.starts.partition_point(|&(c, _)| c <= char_off).saturating_sub(1)
    }

    /// A line's own end: the char offset of its own trailing `\n` (i.e. one
    /// *before* the next line's start), or `total_chars` on the last line —
    /// matching `line_end`'s public contract.
    fn line_end_of(&self, line: usize) -> usize {
        self.starts.get(line + 1).map_or(self.total_chars, |&(c, _)| c - 1)
    }

    /// `(line, column)` of a char offset, both 0-based. `column` is a char
    /// count within the line, not a display column (tabs count as one).
    pub(super) fn line_col(&self, char_off: usize) -> (usize, usize) {
        let char_off = char_off.min(self.total_chars);
        let line = self.line_of(char_off);
        (line, char_off - self.starts[line].0)
    }

    /// Inverse of `line_col`, clamped: a `target_col` past the line's end
    /// lands at the line's end (just before its newline, or at end-of-text
    /// for the last line), matching how a caret keeps its column moving
    /// through shorter lines. A `target_line` past the last line clamps to
    /// end-of-text.
    pub(super) fn line_col_to_char(&self, target_line: usize, target_col: usize) -> usize {
        if target_line > self.last_line() {
            return self.total_chars;
        }
        (self.starts[target_line].0 + target_col).min(self.line_end_of(target_line))
    }

    /// Char offset of the end of the line containing `char_off` — the next
    /// `\n`, or end-of-text on the last line.
    pub(super) fn line_end(&self, char_off: usize) -> usize {
        let line = self.line_of(char_off.min(self.total_chars));
        self.line_end_of(line)
    }

    /// Byte offset of `char_off` in `text` (which must be the same text this
    /// index was built from) — bounded to a scan of just `char_off`'s own
    /// line, rather than the whole buffer from byte 0. `char_off` past the
    /// end of the text clamps to `text.len()`, matching
    /// `text_offset::char_to_byte`'s own out-of-range behavior.
    pub(super) fn char_to_byte(&self, text: &str, char_off: usize) -> usize {
        let char_off = char_off.min(self.total_chars);
        let line = self.line_of(char_off);
        let (line_start_char, line_start_byte) = self.starts[line];
        text[line_start_byte..]
            .char_indices()
            .nth(char_off - line_start_char)
            .map(|(b, _)| line_start_byte + b)
            .unwrap_or(text.len())
    }
}

/// Replaces the caret's selection (or inserts at a collapsed caret) with
/// `insert`, returning the new text and a caret collapsed just past the
/// inserted text. The single primitive every text-producing edit routes
/// through. `index` must reflect `text` (the same one `LineIndex::build` was
/// last called on for this text) — every caller threads a freshly-rebuilt one
/// after its own last edit, see `LineIndex`'s own doc comment.
pub fn replace_selection(text: &str, index: &LineIndex, caret: Caret, insert: &str) -> (String, Caret) {
    let range = caret.range();
    let start_byte = index.char_to_byte(text, range.start);
    let end_byte = index.char_to_byte(text, range.end);
    let mut out = String::with_capacity(text.len() - (end_byte - start_byte) + insert.len());
    out.push_str(&text[..start_byte]);
    out.push_str(insert);
    out.push_str(&text[end_byte..]);
    let caret = Caret::collapsed_at(range.start + insert.chars().count());
    (out, caret)
}

/// Backspace: deletes the selection if there is one, else the char before the
/// caret. A no-op (returns `None`) at the very start with no selection.
pub fn backspace(text: &str, index: &LineIndex, caret: Caret) -> Option<(String, Caret)> {
    if !caret.is_collapsed() {
        return Some(replace_selection(text, index, caret, ""));
    }
    if caret.primary == 0 {
        return None;
    }
    let del = Caret {
        primary: caret.primary - 1,
        anchor: caret.primary,
    };
    Some(replace_selection(text, index, del, ""))
}

/// Delete (forward): deletes the selection if there is one, else the char after
/// the caret. A no-op at the very end with no selection.
pub fn delete_forward(text: &str, index: &LineIndex, caret: Caret) -> Option<(String, Caret)> {
    if !caret.is_collapsed() {
        return Some(replace_selection(text, index, caret, ""));
    }
    if caret.primary >= index.char_len() {
        return None;
    }
    let del = Caret {
        primary: caret.primary,
        anchor: caret.primary + 1,
    };
    Some(replace_selection(text, index, del, ""))
}

/// Left arrow. With a selection and no `extend`, collapses to the selection's
/// left edge (not one char further left) — the standard editor behaviour.
pub fn move_left(caret: Caret, extend: bool) -> Caret {
    if !extend && !caret.is_collapsed() {
        return Caret::collapsed_at(caret.range().start);
    }
    caret.moved_to(caret.primary.saturating_sub(1), extend)
}

/// Right arrow. With a selection and no `extend`, collapses to the right edge.
pub fn move_right(index: &LineIndex, caret: Caret, extend: bool) -> Caret {
    if !extend && !caret.is_collapsed() {
        return Caret::collapsed_at(caret.range().end);
    }
    caret.moved_to((caret.primary + 1).min(index.char_len()), extend)
}

/// Up arrow, preserving `preferred_col` (the column the caret is "trying" to
/// keep across shorter lines). Returns the new caret and the column to carry
/// forward. On the first line, moves to the document start.
pub fn move_up(index: &LineIndex, caret: Caret, extend: bool, preferred_col: usize) -> (Caret, usize) {
    let (line, _) = index.line_col(caret.primary);
    if line == 0 {
        return (caret.moved_to(0, extend), preferred_col);
    }
    let target = index.line_col_to_char(line - 1, preferred_col);
    (caret.moved_to(target, extend), preferred_col)
}

/// Down arrow, preserving `preferred_col`. On the last line, moves to the
/// document end.
pub fn move_down(index: &LineIndex, caret: Caret, extend: bool, preferred_col: usize) -> (Caret, usize) {
    let (line, _) = index.line_col(caret.primary);
    if line >= index.last_line() {
        return (caret.moved_to(index.char_len(), extend), preferred_col);
    }
    let target = index.line_col_to_char(line + 1, preferred_col);
    (caret.moved_to(target, extend), preferred_col)
}

/// End key: to the end of the current line.
pub fn move_end(index: &LineIndex, caret: Caret, extend: bool) -> Caret {
    caret.moved_to(index.line_end(caret.primary), extend)
}

/// Home key: to column 0 of the current line. Plain column-0 semantics —
/// the "first non-whitespace vs. column 0" Smart Home toggle
/// (`auto_edit::smart_home_target`) is an interception layered on top by the
/// caller, the same way it layers on top of `egui::TextEdit` today.
pub fn move_home(index: &LineIndex, caret: Caret, extend: bool) -> Caret {
    let (line, _) = index.line_col(caret.primary);
    caret.moved_to(index.line_col_to_char(line, 0), extend)
}

/// If `char_off`'s line falls inside any of `hidden`'s line ranges, returns
/// the char offset at the end of the line just above that range (a folded
/// region's marker line) instead — the pure half of PLAN.md 3d's "caret
/// clamp-into-marker" rule: a hidden line is never a valid resting place, so
/// motion or a click landing on one snaps to the nearest visible line, which
/// is always the marker line above it (where the fold's own `⋯` affordance
/// sits — landing there reads as "you're at the fold," not at some arbitrary
/// point inside content you can't see). A no-op when the line isn't hidden.
/// `hidden` is expected sorted and non-overlapping, the same invariant
/// `text_area::FoldMap` requires of it.
pub fn clamp_out_of_hidden(index: &LineIndex, char_off: usize, hidden: &[std::ops::Range<usize>]) -> usize {
    let (line, _) = index.line_col(char_off);
    let Some(range) = hidden.iter().find(|r| r.contains(&line)) else {
        return char_off;
    };
    let marker_line = range.start.saturating_sub(1);
    index.line_col_to_char(marker_line, usize::MAX)
}

/// The column (for `preferred_col` bookkeeping) of a caret position.
pub fn column_of(index: &LineIndex, char_off: usize) -> usize {
    index.line_col(char_off).1
}

#[cfg(test)]
mod tests;
