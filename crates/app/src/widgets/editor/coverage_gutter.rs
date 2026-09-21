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
#[path = "coverage_gutter_test.rs"]
mod coverage_gutter_test;
