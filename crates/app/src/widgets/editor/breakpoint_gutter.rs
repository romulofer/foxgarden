//! Breakpoint gutter (`PLAN.md` Track 23 Phase 2): a click-to-toggle dot
//! column, leftmost in the gutter — the universal IDE convention (VS Code,
//! IntelliJ both put it left of the line numbers).
//!
//! Deliberately **not** built the way `folding::show_fold_gutter` is:
//! that function only makes a row interactive when it already has
//! something to show (a fold marker) — every other row is skipped
//! entirely. A breakpoint column has the opposite requirement: the user
//! must be able to click *any* line to create its first breakpoint, so
//! every currently-shaped row gets an interactive rect here, not just
//! rows already in `breakpoints`.

use std::collections::HashSet;

use egui::Sense;

use super::text_area::TextAreaOutput;
use crate::style::theme;

/// Always reserved for a debuggable document (`widget.rs` gates this on
/// `doc.language == Some(Language::Java)`), regardless of whether any
/// breakpoint is set yet — unlike every other gutter column here, whose
/// width is reserved only once there's already something to show, this one
/// exists specifically so the user has somewhere to click to create the
/// first mark.
pub(super) const BREAKPOINT_GUTTER_WIDTH: f32 = 12.0;

/// Paints a filled dot on every currently-shaped row whose logical line is
/// in `breakpoints`, and toggles membership when any row in the column is
/// clicked (whether or not that row already has a dot). `gutter_left` is
/// this column's own left edge — flush against the pane's left edge, the
/// outermost sliver of the gutter.
pub(super) fn show_breakpoint_gutter(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    breakpoints: &mut HashSet<usize>,
    id_salt: &str,
    gutter_left: f32,
    dark_mode: bool,
) {
    let painter = ui.painter();
    let color = theme::breakpoint(dark_mode);
    let radius = (BREAKPOINT_GUTTER_WIDTH * 0.35).min(out.row_height * 0.35);

    for (i, (logical, _)) in out.row_galleys.iter().enumerate() {
        let y = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
        let rect = egui::Rect::from_min_size(
            egui::pos2(gutter_left, y),
            egui::vec2(BREAKPOINT_GUTTER_WIDTH, out.row_height),
        );

        let id = egui::Id::new(("breakpoint", id_salt, *logical));
        let response = ui.interact(rect, id, Sense::click());

        if breakpoints.contains(logical) {
            painter.circle_filled(rect.center(), radius, color);
        } else if response.hovered() {
            painter.circle_stroke(rect.center(), radius, egui::Stroke::new(1.0, color));
        }

        if response.clicked() {
            if breakpoints.contains(logical) {
                breakpoints.remove(logical);
            } else {
                breakpoints.insert(*logical);
            }
        }
    }
}
