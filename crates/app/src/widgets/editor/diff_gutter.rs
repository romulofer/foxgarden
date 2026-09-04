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
/// paint a bar across — so this attaches a thin band to a nearby row's own
/// edge instead: the top edge of line 0 for a deletion at the very start of
/// the file (no line before it to attach to), else the bottom edge of the
/// line right before `at`, which also correctly lands on the last real
/// line's own bottom edge for a deletion at the very end of the file, with
/// no separate "is this the last line" check needed. Returns `None` when
/// the anchor line isn't part of this frame's shaped rows at all (out of
/// the virtualized viewport) — nothing to paint.
fn removal_notch_target(at: usize) -> (usize, bool) {
    if at == 0 { (0, true) } else { (at - 1, false) }
}

/// A removal marker paints inside the anchor row's own bounds (the top few
/// pixels for `at_top`, the bottom few for not) rather than centered on the
/// row boundary — straddling the boundary would put half the band outside
/// a row `layout_visible` actually shaped, which `ui.painter()`'s clip rect
/// (this all runs inside a `ScrollArea`) can legitimately cut off for a
/// removal sitting right at the very top or bottom edge of the current
/// scroll position.
const NOTCH_HEIGHT: f32 = 3.0;

fn paint_removal_notch(
    painter: &egui::Painter,
    out: &TextAreaOutput,
    at: usize,
    row_of: impl Fn(usize) -> Option<usize>,
    x0: f32,
    x1: f32,
    color: Color32,
) {
    let (line, at_top) = removal_notch_target(at);
    let Some(i) = row_of(line) else { return };
    let top = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
    let y0 = if at_top { top } else { top + out.row_height - NOTCH_HEIGHT };
    painter.rect_filled(
        egui::Rect::from_min_size(egui::pos2(x0, y0), egui::vec2(x1 - x0, NOTCH_HEIGHT)),
        0.0,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removal_at_the_very_start_of_the_file_targets_line_zeros_top_edge() {
        assert_eq!(removal_notch_target(0), (0, true));
    }

    #[test]
    fn removal_in_the_middle_targets_the_preceding_lines_bottom_edge() {
        // Matches fg_core::diff's own captured "delete line 3 of 5"
        // fixture: the marker's `at` is 2, and the notch belongs right
        // below line 1 (the line now immediately preceding where the
        // deleted content used to be).
        assert_eq!(removal_notch_target(2), (1, false));
    }

    #[test]
    fn removal_at_the_very_end_of_the_file_also_targets_the_preceding_lines_bottom_edge() {
        // Matches fg_core::diff's own "delete the last of 5 lines"
        // fixture (`at` = 4): lands on the new last line's (index 3)
        // bottom edge, with no separate "is this the last line" branch
        // needed.
        assert_eq!(removal_notch_target(4), (3, false));
    }

    /// Drives the real `text_area::show_interactive` pipeline (the same one
    /// `widget.rs` itself calls) to get a genuine `TextAreaOutput`, then
    /// inspects the actual `egui::Shape`s `paint_diff_gutter` produces —
    /// this file's own painting had no coverage at all before this (every
    /// other paint fn in this widget is likewise only "doesn't panic"
    /// tested, or not tested directly — see `painting.rs`'s own tests), and
    /// a user report that the removal notch wasn't showing up is what
    /// prompted actually verifying the painted geometry rather than just
    /// the pure targeting math above.
    fn painted_rect_shapes(hunks: Vec<DiffHunk>, buffer_text: &str) -> Vec<(Color32, egui::Rect, egui::Rect)> {
        let ctx = egui::Context::default();
        let id = egui::Id::new("diff_gutter_test");
        let buffer = ropey::Rope::from_str(buffer_text);

        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0))),
            ..Default::default()
        };
        let output = ctx.run_ui(raw_input, |ui| {
            ui.memory_mut(|m| m.request_focus(id));
            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                let shell_out = super::super::text_area::show_interactive(
                    ui,
                    id,
                    &buffer,
                    0,
                    buffer_text,
                    egui::FontId::monospace(14.0),
                    egui::Color32::WHITE,
                    false,
                    &[],
                    &[],
                    false,
                    true,
                );
                paint_diff_gutter(ui, &shell_out.base, &hunks, 200.0, false);
            });
        });

        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(r) => Some((r.fill, r.rect, clipped.clip_rect)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_removed_hunk_paints_a_notch_fully_inside_its_visible_clip_rect() {
        let hunks = vec![DiffHunk { kind: DiffLineKind::Removed, lines: 2..2 }];
        let shapes = painted_rect_shapes(hunks, "l1\nl2\nl4\nl5\n");

        let red = theme::diff_removed(false);
        let notch = shapes
            .iter()
            .find(|(fill, ..)| *fill == red)
            .expect("a rect filled with the removal color");
        let (_, rect, clip) = notch;
        assert!(clip.contains_rect(*rect), "the notch must be fully visible, not clipped away at a row boundary");
    }

    #[test]
    fn an_added_hunk_paints_a_full_height_bar_on_its_own_line() {
        let hunks = vec![DiffHunk { kind: DiffLineKind::Added, lines: 1..2 }];
        let shapes = painted_rect_shapes(hunks, "l1\nNEW\nl3\n");

        let green = theme::diff_added(false);
        let bar = shapes.iter().find(|(fill, ..)| *fill == green).expect("a rect filled with the added color");
        let (_, rect, clip) = bar;
        assert!(clip.contains_rect(*rect), "the bar must be fully visible");
        assert!(rect.height() > NOTCH_HEIGHT, "an Added bar spans the full row height, not just a thin notch");
    }
}
