//! Hand-built **virtualized** text editor widget — replaces
//! `egui::TextEdit::multiline`, laying out and painting only the *visible*
//! line range instead of the whole buffer every frame (PLAN.md Phase 2). This
//! is what fixes large-file layout cost and gives code folding (Phase 3) a
//! place to hide rows, which `egui::TextEdit`'s single-blob model structurally
//! couldn't. `widget.rs` drives this as the live editor (since the PLAN 2h
//! parity swap) via `show_interactive`; `show_readonly` remains for any
//! future read-only-viewer use case.
//!
//! Submodules: `text_area.rs` itself is the pure geometry core (2a) —
//! uniform-row-height math and the visual↔logical `FoldMap` folding feeds a
//! hidden-line set into. `input.rs`/`history.rs` are the pure caret/edit/undo
//! model (2d/2e). `render.rs` shapes+paints visible rows, with real
//! syntax-highlighting spans baked in per line. `shell.rs` is the interactive
//! `show()` — input handling, focus, IME (2f).

use std::ops::Range;

mod cache;
mod history;
mod input;
mod render;
mod shell;

#[expect(
    unused_imports,
    reason = "PLAN.md Phase 0 prerequisite; no widget.rs-side cache needs a hidden-ranges key yet"
)]
pub(super) use cache::hash_hidden;
pub(super) use render::ContentKey;
pub(super) use input::Caret;
pub(super) use render::{HighlightSpan, TextAreaOutput};
pub(super) use shell::{char_offset_for_pos, set_caret, show as show_interactive};
pub use shell::peek_caret;

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
/// than nowhere). Unused in production until Phase 3's fold-arrow gutter
/// hit-testing needs a total-row-space row index (`shell::char_offset_for_pos`
/// needs an index into the *already-visible* `row_galleys` slice instead, a
/// different domain, so it doesn't reuse this); exercised directly by
/// `text_area/tests.rs` today.
#[cfg_attr(not(test), expect(dead_code, reason = "Phase 3 fold-gutter hit-testing API surface"))]
pub(super) fn row_at_y(y: f32, content_top: f32, row_height: f32, total_rows: usize) -> usize {
    if row_height <= 0.0 || total_rows == 0 {
        return 0;
    }
    let row = ((y - content_top) / row_height).floor().max(0.0) as usize;
    row.min(total_rows - 1)
}

/// PLAN.md Phase 4 — word-wrap's geometry core: the same virtualization
/// `visible_rows`/`content_height` give the no-wrap case, generalized to
/// lines that can each occupy more than one visual row. `row_counts[i]` is
/// how many visual rows logical line `i` occupies this frame (`0` for a
/// folded-away line, `1` for a normal unwrapped one, `N` for one wrapped
/// across `N` rows) — `prefix_rows` turns that into a cumulative table,
/// `visible_lines` binary-searches it instead of dividing by a uniform row
/// height. The no-wrap/no-fold case (`row_counts` all `1`s) makes
/// `prefix_rows`/`visible_lines` produce the exact same answers `visible_
/// rows` already did, by construction — see this pair's tests.
///
/// Cumulative visual-row count *before* each logical line: `prefix[i]` is
/// the visual row line `i`'s own block starts at, and `prefix[row_counts.
/// len()]` (the last entry) is the total visual row count. Always `row_
/// counts.len() + 1` entries, monotonically non-decreasing.
pub(super) fn prefix_rows(row_counts: &[usize]) -> Vec<usize> {
    let mut prefix = Vec::with_capacity(row_counts.len() + 1);
    let mut acc = 0;
    prefix.push(0);
    for &n in row_counts {
        acc += n;
        prefix.push(acc);
    }
    prefix
}

/// The logical-line range whose visual-row blocks touch a viewport
/// `viewport_h` tall, scrolled to `scroll_y`, given `prefix` (see
/// `prefix_rows`; `row_height` is one visual row's height, same as every
/// other geometry function here). A folded-away line (`row_counts[i] == 0`,
/// so `prefix[i] == prefix[i + 1]`) never satisfies either search below on
/// its own, so it's naturally skipped without `FoldMap` needing a separate
/// pass — it contributes no visible-row band for a viewport position to
/// land in.
pub(super) fn visible_lines(scroll_y: f32, viewport_h: f32, row_height: f32, prefix: &[usize]) -> Range<usize> {
    if row_height <= 0.0 || prefix.len() < 2 {
        return 0..0;
    }
    let total_rows = *prefix.last().expect("checked len >= 2 above");
    let first_row = ((scroll_y / row_height).floor().max(0.0) as usize).min(total_rows);
    let last_row = (((scroll_y + viewport_h) / row_height).ceil().max(0.0) as usize).min(total_rows);
    // Scrolled at-or-past the end (or a degenerate zero/negative-height
    // viewport): nothing to show, same as `visible_rows`'s own `first.min(
    // total)..last.min(total)` collapsing to an empty range in that case.
    if first_row >= last_row {
        return 0..0;
    }

    // Last line whose block *starts* at-or-before `first_row` — the line
    // `first_row` itself falls inside, since a real (non-folded) line's
    // block is never empty.
    let start_line = prefix.partition_point(|&r| r <= first_row).saturating_sub(1);
    // First line whose block starts *at or after* `last_row` — everything
    // before that has at least a sliver inside `[first_row, last_row)`.
    let end_line = prefix.partition_point(|&r| r < last_row);

    start_line..end_line.max(start_line).min(prefix.len() - 1)
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
    /// currently hidden inside a fold — `shell::show`'s scroll-follows-
    /// cursor logic uses this (the no-wrap case) to find where an off-
    /// screen caret move landed, since that row was never actually shaped
    /// this frame (`layout_visible` only shapes what's inside the current
    /// viewport) and so has no `TextAreaOutput::char_rect` to read back.
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
mod text_area_test;

/// The logical lines the editor is about to paint, as best as can be known
/// *before* the text area lays itself out — used by `widget.rs` to run
/// syntax highlighting over the visible window instead of the whole file.
///
/// It's an estimate on purpose. The exact answer only exists after
/// `layout_visible*` has allocated its own rect, which is too late to
/// decide what to highlight; this reads the same scroll geometry from the
/// `Ui` the layout is about to use (`cursor()` is where that rect will
/// start, `clip_rect()` is the viewport), so it agrees with the real
/// answer except at the boundaries — which is why callers widen it before
/// using it.
///
/// `None` means "no reliable window, use the whole document": either
/// folding is active without word-wrap (visual rows and logical lines
/// diverge in a way this shortcut doesn't model) or the geometry isn't
/// usable yet.
pub(super) fn visible_line_window(
    ui: &mut egui::Ui,
    id: egui::Id,
    content: ContentKey,
    font_id: &egui::FontId,
    word_wrap: bool,
    hidden: &[Range<usize>],
    total_lines: usize,
) -> Option<Range<usize>> {
    let row_height = ui.fonts_mut(|f| f.row_height(font_id));
    if row_height <= 0.0 {
        return None;
    }
    let clip = ui.clip_rect();
    let scroll_y = (clip.top() - ui.cursor().top()).max(0.0);

    if word_wrap {
        let counts = render::cached_row_counts(ui, id, content, font_id, ui.available_width(), hidden, total_lines);
        let prefix = prefix_rows(&counts);
        let lines = visible_lines(scroll_y, clip.height(), row_height, &prefix);
        (!lines.is_empty()).then_some(lines)
    } else if hidden.is_empty() {
        // No wrap and no folds: one visual row per logical line exactly.
        let rows = visible_rows(scroll_y, clip.height(), row_height, total_lines);
        (!rows.is_empty()).then_some(rows)
    } else {
        None
    }
}
