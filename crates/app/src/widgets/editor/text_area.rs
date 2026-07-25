//! Hand-built **virtualized** text editor widget — the eventual replacement
//! for `egui::TextEdit::multiline` that lays out and paints only the *visible*
//! line range instead of the whole buffer every frame (PLAN.md Phase 2). This
//! is what fixes large-file layout cost and gives code folding (Phase 3) a
//! place to hide rows, which `egui::TextEdit`'s single-blob model structurally
//! can't.
//!
//! Built incrementally behind the scenes: `widget.rs` still drives the live
//! editor until the parity gate (PLAN 2h). This file currently holds the pure
//! **geometry core** (2a) — the uniform-row-height math that makes
//! virtualization O(1) and the visual↔logical line mapping folding will feed a
//! fold set into. All of it is pure and frame-free, tested in `text_area/tests.rs`.

// The geometry/render core is complete and tested but not yet wired into the
// live editor path (that's 2c onward — interception repointing + the parity
// swap), so its items and the `show_readonly` re-export are legitimately unused
// for now. Both allowances are removed at the parity gate (2h), when `show`
// starts driving this widget.
#![allow(dead_code, unused_imports)]

use std::ops::Range;

mod history;
mod input;
mod render;

pub(super) use render::show_readonly;

/// The inclusive-start, exclusive-end range of **visual rows** at least
/// partially inside a viewport `viewport_h` tall scrolled down by `scroll_y`,
/// with each row `row_height` tall and `total_rows` rows total. Clamped to
/// `0..total_rows`. This is the whole point of virtualization: only these rows
/// get shaped and painted, no matter how big the buffer is.
pub(super) fn visible_rows(scroll_y: f32, viewport_h: f32, row_height: f32, total_rows: usize) -> Range<usize> {
    if row_height <= 0.0 || total_rows == 0 {
        return 0..0;
    }
    let first = (scroll_y / row_height).floor().max(0.0) as usize;
    // `ceil` so a row peeking in at the bottom edge still counts as visible.
    let last = ((scroll_y + viewport_h) / row_height).ceil().max(0.0) as usize;
    first.min(total_rows)..last.min(total_rows)
}

/// Total pixel height of `total_rows` rows — the widget's reported content
/// size, so the enclosing `ScrollArea`'s scrollbar is correct without laying
/// out a single line.
pub(super) fn content_height(total_rows: usize, row_height: f32) -> f32 {
    total_rows as f32 * row_height
}

/// The visual row containing screen-y `y`, where the content's top edge sits
/// at `content_top`. Clamped to the last row for a click in the empty space
/// past the final line (where editors place the caret on that last line rather
/// than nowhere).
pub(super) fn row_at_y(y: f32, content_top: f32, row_height: f32, total_rows: usize) -> usize {
    if row_height <= 0.0 || total_rows == 0 {
        return 0;
    }
    let row = ((y - content_top) / row_height).floor().max(0.0) as usize;
    row.min(total_rows - 1)
}

/// Maps between **visual rows** (rendered rows, with folded lines skipped) and
/// **logical lines** (the buffer's `\n`-separated lines), given the set of
/// currently-hidden logical-line ranges.
///
/// In Phase 2 the hidden set is always empty (no folding yet), so every
/// mapping is the identity — but the abstraction exists from the start so
/// Phase 3's fold set drops in without reworking any geometry. `hidden` must
/// be **sorted by `start` and non-overlapping**; the fold set is maintained
/// that way by construction.
pub(super) struct FoldMap<'a> {
    hidden: &'a [Range<usize>],
}

impl<'a> FoldMap<'a> {
    pub(super) fn new(hidden: &'a [Range<usize>]) -> Self {
        debug_assert!(
            hidden.windows(2).all(|w| w[0].end <= w[1].start),
            "hidden ranges must be sorted and non-overlapping"
        );
        Self { hidden }
    }

    /// How many visual rows `total_lines` logical lines collapse to once the
    /// hidden ranges are removed.
    pub(super) fn visual_count(&self, total_lines: usize) -> usize {
        let hidden_lines: usize = self.hidden.iter().map(Range::len).sum();
        total_lines.saturating_sub(hidden_lines)
    }

    /// The visual row a logical line renders on, or `None` if that line is
    /// currently hidden inside a fold.
    pub(super) fn to_visual(&self, logical_line: usize) -> Option<usize> {
        let mut skipped = 0;
        for range in self.hidden {
            if range.contains(&logical_line) {
                return None;
            }
            if range.end <= logical_line {
                skipped += range.len();
            } else {
                break; // sorted: no later range can precede `logical_line`
            }
        }
        Some(logical_line - skipped)
    }

    /// The logical line rendered on a given visual row — the inverse of
    /// `to_visual` for non-hidden lines. Shifts the row index forward past
    /// every fold that sits at or before where it lands.
    pub(super) fn to_logical(&self, visual_row: usize) -> usize {
        let mut logical = visual_row;
        for range in self.hidden {
            if range.start <= logical {
                logical += range.len();
            } else {
                break;
            }
        }
        logical
    }
}

#[cfg(test)]
mod tests;
