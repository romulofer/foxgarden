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

use crate::widgets::editor::text_offset::char_to_byte;

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

/// Total char count of `text` — the one-past-the-end caret position.
fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// `(line, column)` of a char offset, both 0-based. `column` is a char count
/// within the line, not a display column (tabs count as one).
fn line_col(text: &str, char_off: usize) -> (usize, usize) {
    let (mut line, mut col) = (0, 0);
    for (i, c) in text.chars().enumerate() {
        if i == char_off {
            return (line, col);
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Inverse of `line_col`, clamped: a `target_col` past the line's end lands at
/// the line's end (just before its newline, or at end-of-text for the last
/// line), matching how a caret keeps its column moving through shorter lines.
fn line_col_to_char(text: &str, target_line: usize, target_col: usize) -> usize {
    let (mut line, mut col) = (0, 0);
    for (i, c) in text.chars().enumerate() {
        if line == target_line && col == target_col {
            return i;
        }
        if c == '\n' {
            if line == target_line {
                return i; // ran off the end of the target line → clamp to its end
            }
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    char_len(text)
}

/// Index of the last line (== number of `\n`s).
fn last_line(text: &str) -> usize {
    text.chars().filter(|&c| c == '\n').count()
}

/// Char offset of the end of the line containing `char_off` — the next `\n`, or
/// end-of-text on the last line.
fn line_end(text: &str, char_off: usize) -> usize {
    text.chars()
        .enumerate()
        .skip(char_off)
        .find(|&(_, c)| c == '\n')
        .map(|(i, _)| i)
        .unwrap_or_else(|| char_len(text))
}

/// Replaces the caret's selection (or inserts at a collapsed caret) with
/// `insert`, returning the new text and a caret collapsed just past the
/// inserted text. The single primitive every text-producing edit routes
/// through.
pub fn replace_selection(text: &str, caret: Caret, insert: &str) -> (String, Caret) {
    let range = caret.range();
    let start_byte = char_to_byte(text, range.start);
    let end_byte = char_to_byte(text, range.end);
    let mut out = String::with_capacity(text.len() - (end_byte - start_byte) + insert.len());
    out.push_str(&text[..start_byte]);
    out.push_str(insert);
    out.push_str(&text[end_byte..]);
    let caret = Caret::collapsed_at(range.start + insert.chars().count());
    (out, caret)
}

/// Backspace: deletes the selection if there is one, else the char before the
/// caret. A no-op (returns `None`) at the very start with no selection.
pub fn backspace(text: &str, caret: Caret) -> Option<(String, Caret)> {
    if !caret.is_collapsed() {
        return Some(replace_selection(text, caret, ""));
    }
    if caret.primary == 0 {
        return None;
    }
    let del = Caret {
        primary: caret.primary - 1,
        anchor: caret.primary,
    };
    Some(replace_selection(text, del, ""))
}

/// Delete (forward): deletes the selection if there is one, else the char after
/// the caret. A no-op at the very end with no selection.
pub fn delete_forward(text: &str, caret: Caret) -> Option<(String, Caret)> {
    if !caret.is_collapsed() {
        return Some(replace_selection(text, caret, ""));
    }
    if caret.primary >= char_len(text) {
        return None;
    }
    let del = Caret {
        primary: caret.primary,
        anchor: caret.primary + 1,
    };
    Some(replace_selection(text, del, ""))
}

/// Left arrow. With a selection and no `extend`, collapses to the selection's
/// left edge (not one char further left) — the standard editor behaviour.
pub fn move_left(text: &str, caret: Caret, extend: bool) -> Caret {
    let _ = text;
    if !extend && !caret.is_collapsed() {
        return Caret::collapsed_at(caret.range().start);
    }
    caret.moved_to(caret.primary.saturating_sub(1), extend)
}

/// Right arrow. With a selection and no `extend`, collapses to the right edge.
pub fn move_right(text: &str, caret: Caret, extend: bool) -> Caret {
    if !extend && !caret.is_collapsed() {
        return Caret::collapsed_at(caret.range().end);
    }
    caret.moved_to((caret.primary + 1).min(char_len(text)), extend)
}

/// Up arrow, preserving `preferred_col` (the column the caret is "trying" to
/// keep across shorter lines). Returns the new caret and the column to carry
/// forward. On the first line, moves to the document start.
pub fn move_up(text: &str, caret: Caret, extend: bool, preferred_col: usize) -> (Caret, usize) {
    let (line, _) = line_col(text, caret.primary);
    if line == 0 {
        return (caret.moved_to(0, extend), preferred_col);
    }
    let target = line_col_to_char(text, line - 1, preferred_col);
    (caret.moved_to(target, extend), preferred_col)
}

/// Down arrow, preserving `preferred_col`. On the last line, moves to the
/// document end.
pub fn move_down(text: &str, caret: Caret, extend: bool, preferred_col: usize) -> (Caret, usize) {
    let (line, _) = line_col(text, caret.primary);
    if line >= last_line(text) {
        return (caret.moved_to(char_len(text), extend), preferred_col);
    }
    let target = line_col_to_char(text, line + 1, preferred_col);
    (caret.moved_to(target, extend), preferred_col)
}

/// End key: to the end of the current line.
pub fn move_end(text: &str, caret: Caret, extend: bool) -> Caret {
    caret.moved_to(line_end(text, caret.primary), extend)
}

/// Home key: to column 0 of the current line. Plain column-0 semantics —
/// the "first non-whitespace vs. column 0" Smart Home toggle
/// (`auto_edit::smart_home_target`) is an interception layered on top by the
/// caller, the same way it layers on top of `egui::TextEdit` today.
pub fn move_home(text: &str, caret: Caret, extend: bool) -> Caret {
    let (line, _) = line_col(text, caret.primary);
    caret.moved_to(line_col_to_char(text, line, 0), extend)
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
pub fn clamp_out_of_hidden(
    text: &str,
    char_off: usize,
    hidden: &[std::ops::Range<usize>],
) -> usize {
    let (line, _) = line_col(text, char_off);
    let Some(range) = hidden.iter().find(|r| r.contains(&line)) else {
        return char_off;
    };
    let marker_line = range.start.saturating_sub(1);
    line_col_to_char(text, marker_line, usize::MAX)
}

/// The column (for `preferred_col` bookkeeping) of a caret position.
pub fn column_of(text: &str, char_off: usize) -> usize {
    line_col(text, char_off).1
}

#[cfg(test)]
mod tests;
