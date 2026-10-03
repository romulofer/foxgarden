//! The run gutter (IntelliJ's ▶ beside a `main`): a click-to-run triangle
//! on every line that declares a JVM entry point, as found by
//! `syntax::main_entries`.
//!
//! Built the way `folding::show_fold_gutter` is rather than the way
//! `breakpoint_gutter` is: only a row that actually *has* an entry point
//! becomes interactive, since there's nothing for a click on any other line
//! to mean — the opposite of the breakpoint column, whose whole purpose is
//! to let any line be clicked.

use egui::Sense;
use fg_i18n::msg;
use syntax::MainEntry;

use super::text_area::TextAreaOutput;
use crate::style::theme;

/// Reserved only while the open document has at least one entry point
/// (`widget.rs` passes `0.0` otherwise), the same "don't pay for a column
/// with nothing in it" rule the fold/diff/coverage columns already follow —
/// a file with no `main` keeps exactly the gutter it had before this
/// existed.
pub(super) const RUN_GUTTER_WIDTH: f32 = 14.0;

/// Paints a ▶ on each shaped row whose logical line declares a `main`, and
/// returns the entry whose marker was clicked this frame (at most one — two
/// markers can't be clicked in the same frame). `gutter_left` is this
/// column's own left edge.
pub(super) fn show_run_gutter(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    entries: &[MainEntry],
    id_salt: &str,
    gutter_left: f32,
    dark_mode: bool,
) -> Option<MainEntry> {
    if entries.is_empty() {
        return None;
    }
    let painter = ui.painter();
    let mut clicked = None;

    for (i, (logical, _)) in out.row_galleys.iter().enumerate() {
        // A wrapped row repeats its logical line, and a fold can hide the
        // declaration entirely — one marker per *logical* line, on its first
        // shaped row, keeps a wrapped `main(String[] args)` from growing a
        // second triangle underneath the first.
        if i > 0 && out.row_galleys[i - 1].0 == *logical {
            continue;
        }
        let Some(entry) = entries.iter().find(|entry| entry.line == *logical) else {
            continue;
        };

        let y = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
        let rect = egui::Rect::from_min_size(egui::pos2(gutter_left, y), egui::vec2(RUN_GUTTER_WIDTH, out.row_height));
        let id = egui::Id::new(("run_marker", id_salt, *logical));
        let label = msg::run_main_class(&entry.label);
        let response = ui
            .interact(rect, id, Sense::click())
            .on_hover_text(label.clone())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        // A bare `interact` rect is invisible to the accessibility tree —
        // and so to a screen reader and to the e2e harness both. The marker
        // is a button in every way that matters, so it says so.
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));

        let color = if response.hovered() {
            theme::run_marker_hovered(dark_mode)
        } else {
            theme::run_marker(dark_mode)
        };
        painter.add(egui::Shape::convex_polygon(triangle(rect), color, egui::Stroke::NONE));

        if response.clicked() {
            clicked = Some(entry.clone());
        }
    }
    clicked
}

/// The marker itself — a right-pointing triangle inscribed in `rect`, sized
/// off the row height so it scales with the editor font rather than being
/// pinned to one pixel size.
fn triangle(rect: egui::Rect) -> Vec<egui::Pos2> {
    let size = (RUN_GUTTER_WIDTH * 0.62).min(rect.height() * 0.62);
    let center = rect.center();
    let half = size / 2.0;
    vec![
        egui::pos2(center.x - half * 0.8, center.y - half),
        egui::pos2(center.x - half * 0.8, center.y + half),
        egui::pos2(center.x + half, center.y),
    ]
}
