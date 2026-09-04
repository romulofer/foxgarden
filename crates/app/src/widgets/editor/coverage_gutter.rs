//! Coverage gutter (`PLAN.md` Track 13 Phase 1, Maven-only): paints a thin
//! colored bar per line JaCoCo reported coverage data for, from
//! `fg_core::Document::coverage_lines` (parsed from the last "Run with
//! Coverage" run — see `fg_core::coverage::parse_jacoco_xml`). This is the
//! visual half; the background run/parse itself is `panels::build_panel`.
//! Mirrors `diff_gutter.rs` exactly — same const-width/paint-fn/row-lookup
//! shape, one more independent gutter column.

use fg_core::{CoverageStatus, LineCoverage};

use super::text_area::TextAreaOutput;
use crate::style::theme;

/// Extra gutter width reserved for the coverage-mark column — only added
/// when `doc.coverage_lines` is non-empty (`widget.rs` checks this the
/// same way it already does for `diff_gutter::DIFF_GUTTER_WIDTH`), so a
/// file with no coverage data (never run, or a Gradle project) keeps
/// exactly the gutter width it had before this feature existed.
pub(super) const COVERAGE_GUTTER_WIDTH: f32 = 4.0;

/// Paints one filled rect per currently-shaped row that has a matching
/// `LineCoverage` entry. Iterates `lines` (typically the file's own
/// covered-method count, a handful to a few hundred) rather than
/// `out.row_galleys` (every visible row) as the outer loop, same reasoning
/// `paint_diff_gutter` already uses for `hunks`.
pub(super) fn paint_coverage_gutter(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    lines: &[LineCoverage],
    gutter_right_edge: f32,
    dark_mode: bool,
) {
    if lines.is_empty() {
        return;
    }
    let painter = ui.painter();
    let x0 = gutter_right_edge - COVERAGE_GUTTER_WIDTH;
    let x1 = gutter_right_edge;
    let row_of = |line: usize| out.row_galleys.iter().position(|(logical, _)| *logical == line);
    let color_of = |status: CoverageStatus| match status {
        CoverageStatus::Covered => theme::coverage_covered(dark_mode),
        CoverageStatus::Missed => theme::coverage_missed(dark_mode),
        CoverageStatus::Partial => theme::coverage_partial(dark_mode),
    };

    for entry in lines {
        if let Some(i) = row_of(entry.line) {
            let y = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + out.row_height)),
                0.0,
                color_of(entry.status),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    use super::*;

    fn painted_rect_shapes(lines: Vec<LineCoverage>, buffer_text: &str) -> Vec<(Color32, egui::Rect, egui::Rect)> {
        let ctx = egui::Context::default();
        let id = egui::Id::new("coverage_gutter_test");
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
                paint_coverage_gutter(ui, &shell_out.base, &lines, 200.0, false);
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
    fn a_covered_line_paints_a_full_height_bar_in_the_covered_color() {
        let lines = vec![LineCoverage { line: 1, status: CoverageStatus::Covered }];
        let shapes = painted_rect_shapes(lines, "l1\nl2\nl3\n");

        let green = theme::coverage_covered(false);
        let bar = shapes.iter().find(|(fill, ..)| *fill == green).expect("a rect filled with the covered color");
        let (_, rect, clip) = bar;
        assert!(clip.contains_rect(*rect), "the bar must be fully visible");
    }

    #[test]
    fn a_missed_line_paints_in_the_missed_color_not_the_covered_one() {
        let lines = vec![LineCoverage { line: 0, status: CoverageStatus::Missed }];
        let shapes = painted_rect_shapes(lines, "l1\nl2\n");

        let red = theme::coverage_missed(false);
        assert!(shapes.iter().any(|(fill, ..)| *fill == red), "expected a rect filled with the missed color");
    }

    #[test]
    fn no_coverage_data_paints_nothing() {
        let shapes = painted_rect_shapes(Vec::new(), "l1\nl2\n");
        let green = theme::coverage_covered(false);
        let red = theme::coverage_missed(false);
        let amber = theme::coverage_partial(false);
        assert!(!shapes.iter().any(|(fill, ..)| *fill == green || *fill == red || *fill == amber));
    }
}
