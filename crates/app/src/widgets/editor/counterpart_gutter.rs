//! The expect/actual gutter (`PLAN.md` Track 30 Phase 2): a two-way arrow
//! beside each line an extension says declares something with counterparts
//! (`syntax::counterpart_lines`), clicking which jumps to them.
//!
//! Shaped like `run_gutter`, and for the same reason: only a line that has
//! something to jump to becomes interactive. It marks lines from the text
//! alone; whether the declaration really has a counterpart is found when the
//! marker is clicked, since that reads the whole module.

use egui::{Sense, Stroke};
use fg_i18n::t;

use super::text_area::TextAreaOutput;
use crate::style::theme;

/// Reserved only while the open document has a marked line, like the run
/// column.
pub(super) const COUNTERPART_GUTTER_WIDTH: f32 = 14.0;

/// Paints the arrow on each shaped row whose logical line is in `lines`, and
/// returns the 0-based line whose marker was clicked this frame.
pub(super) fn show_counterpart_gutter(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    lines: &[usize],
    id_salt: &str,
    gutter_left: f32,
    dark_mode: bool,
) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    let painter = ui.painter();
    let mut clicked = None;

    for (i, (logical, _)) in out.row_galleys.iter().enumerate() {
        // One marker per logical line, on its first shaped row (a wrapped
        // declaration repeats its line).
        if i > 0 && out.row_galleys[i - 1].0 == *logical {
            continue;
        }
        if lines.binary_search(logical).is_err() {
            continue;
        }

        let y = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
        let rect = egui::Rect::from_min_size(
            egui::pos2(gutter_left, y),
            egui::vec2(COUNTERPART_GUTTER_WIDTH, out.row_height),
        );
        let id = egui::Id::new(("counterpart_marker", id_salt, *logical));
        let label = t().palettes.go_to_counterpart;
        let response = ui
            .interact(rect, id, Sense::click())
            .on_hover_text(label)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        // Same as the run marker: a bare `interact` rect is invisible to the
        // accessibility tree and the e2e harness unless it says it's a button.
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));

        let color = if response.hovered() {
            theme::counterpart_marker_hovered(dark_mode)
        } else {
            theme::counterpart_marker(dark_mode)
        };
        paint_arrows(painter, rect, Stroke::new(1.4, color));

        if response.clicked() {
            clicked = Some(*logical);
        }
    }
    clicked
}

/// Two opposed arrows — one over the other — inscribed in `rect`, sized off
/// the row height so they scale with the editor font.
fn paint_arrows(painter: &egui::Painter, rect: egui::Rect, stroke: Stroke) {
    let half_width = (COUNTERPART_GUTTER_WIDTH * 0.36).min(rect.height() * 0.36);
    let gap = half_width * 0.5;
    let head = half_width * 0.45;
    let center = rect.center();
    let (left, right) = (center.x - half_width, center.x + half_width);

    let top = center.y - gap;
    painter.line_segment([egui::pos2(left, top), egui::pos2(right, top)], stroke);
    painter.line_segment([egui::pos2(right - head, top - head), egui::pos2(right, top)], stroke);
    painter.line_segment([egui::pos2(right - head, top + head), egui::pos2(right, top)], stroke);

    let bottom = center.y + gap;
    painter.line_segment([egui::pos2(left, bottom), egui::pos2(right, bottom)], stroke);
    painter.line_segment([egui::pos2(left + head, bottom - head), egui::pos2(left, bottom)], stroke);
    painter.line_segment([egui::pos2(left + head, bottom + head), egui::pos2(left, bottom)], stroke);
}
