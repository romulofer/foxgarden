//! Git diff gutter (`PLAN.md` Track 9 Phase 1): paints a thin colored bar
//! per changed line, from `fg_core::Document::diff_hunks` (already parsed
//! from a real `git diff -U0` run — see `fg_core::diff`). This is the
//! visual half of the phase; the background scan that fills `diff_hunks`
//! itself is `panels::git_diff`.

use egui::Color32;
use fg_core::{DiffHunk, DiffLineKind};

use super::text_area::TextAreaOutput;
use crate::style::theme;

/// Extra gutter width reserved for the diff-mark column, flush against the
/// gutter's own right edge (immediately before the text starts) — only
/// added when `doc.diff_hunks` is non-empty (`widget.rs` checks this the
/// same way it already does for `folding::FOLD_GUTTER_WIDTH`), so a file
/// with nothing to show (untracked, unchanged, or no project open at all)
/// keeps exactly the gutter width it had before this feature existed.
pub(super) const DIFF_GUTTER_WIDTH: f32 = 4.0;

/// Paints one filled rect per currently-shaped row that falls inside an
/// `Added`/`Modified` hunk's `lines` range, plus a thin notch at the right
/// row edge for each `Removed` hunk's empty-range marker (see `DiffHunk`'s
/// own doc comment for why a removal has no line of its own to fill).
/// `gutter_right_edge` is where the text area itself starts — this column
/// sits flush against it, the innermost sliver of the gutter. Iterates
/// `hunks` (typically a handful) rather than `out.row_galleys` (every
/// visible row) as the outer loop, since that's the smaller of the two in
/// the common case.
pub(super) fn paint_diff_gutter(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    hunks: &[DiffHunk],
    gutter_right_edge: f32,
    dark_mode: bool,
) {
    if hunks.is_empty() {
        return;
    }
    let painter = ui.painter();
    let x0 = gutter_right_edge - DIFF_GUTTER_WIDTH;
    let x1 = gutter_right_edge;
    let row_of = |line: usize| out.row_galleys.iter().position(|(logical, _)| *logical == line);
    let color_of = |kind: DiffLineKind| match kind {
        DiffLineKind::Added => theme::diff_added(dark_mode),
        DiffLineKind::Removed => theme::diff_removed(dark_mode),
        DiffLineKind::Modified => theme::diff_modified(dark_mode),
    };

    for hunk in hunks {
        let color = color_of(hunk.kind);
        if hunk.lines.is_empty() {
            paint_removal_notch(painter, out, hunk.lines.start, row_of, x0, x1, color);
        } else {
            for line in hunk.lines.clone() {
                if let Some(i) = row_of(line) {
                    let y = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
                    painter.rect_filled(
                        egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + out.row_height)),
                        0.0,
                        color,
                    );
                }
            }
        }
    }
}

/// A removal marker's `at` (see `DiffHunk`'s own doc comment) names the line
/// immediately *after* the deleted content, not a line that still exists to
/// paint a bar across — so this attaches a thin band to the boundary
/// between two rows instead: the top edge of line 0 for a deletion at the
/// very start of the file (no line before it to attach to), else the
/// bottom edge of the line right before `at`, which also correctly lands on
/// the last real line's own bottom edge for a deletion at the very end of
/// the file, with no separate "is this the last line" check needed.
fn paint_removal_notch(
    painter: &egui::Painter,
    out: &TextAreaOutput,
    at: usize,
    row_of: impl Fn(usize) -> Option<usize>,
    x0: f32,
    x1: f32,
    color: Color32,
) {
    let (line, at_top) = if at == 0 { (0, true) } else { (at - 1, false) };
    let Some(i) = row_of(line) else { return };
    let top = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
    let y = if at_top { top } else { top + out.row_height };
    painter.rect_filled(
        egui::Rect::from_center_size(egui::pos2((x0 + x1) / 2.0, y), egui::vec2(x1 - x0, 3.0)),
        0.0,
        color,
    );
}
