//! Code folding (PLAN.md Phase 3): turns `syntax::foldable_ranges` plus a
//! per-document `folded_lines` set (`fg_core::Document::folded_lines`,
//! session-only membership — see that field's doc comment for why there's
//! no separate reconciliation step) into the line-space hidden-range list
//! `text_area`'s `FoldMap` already knows how to skip, plus the gutter
//! fold-arrow rendering and click-to-toggle that drives it.

use std::collections::HashSet;
use std::ops::Range;

use egui::{Align2, FontId, Sense, Shape};
use ropey::Rope;
use syntax::FoldRange;

use super::text_area::TextAreaOutput;
use crate::style::theme;

/// Extra gutter width reserved for the fold-arrow column, to the left of the
/// line numbers — only added when the file actually has foldable regions
/// (`widget.rs` checks `!folds.is_empty()`), so a plain-text file's gutter
/// stays exactly as narrow as before this feature existed.
pub(super) const FOLD_GUTTER_WIDTH: f32 = 14.0;

/// Converts one `FoldRange` (byte-space, from `syntax::foldable_ranges`)
/// into the line-space `start..end` `text_area::FoldMap` hides when this
/// fold is collapsed: `start` is the first hidden line (just below the
/// marker), `end` is one past the last hidden line — the line the fold's
/// closing brace/comment terminator sits on is included, since that's the
/// line the `⋯` collapsed affordance replaces along with everything above it.
pub(super) fn hidden_line_range(buffer: &Rope, fold: &FoldRange) -> Range<usize> {
    let len_bytes = buffer.len_bytes();
    let start = buffer.byte_to_line(fold.start_byte.min(len_bytes));
    let last_hidden_byte = fold
        .end_byte
        .saturating_sub(1)
        .min(len_bytes.saturating_sub(1));
    let end = (buffer.byte_to_line(last_hidden_byte) + 1).max(start);
    start..end
}

/// The line-space hidden ranges for every currently-collapsed fold, sorted
/// and merged the way `text_area::FoldMap::new` requires (ascending,
/// non-overlapping) — collapsing both an outer fold (a class body) and one
/// nested inside it (one of its methods) at once would otherwise hand
/// `FoldMap` two overlapping ranges, violating its invariant.
pub(super) fn hidden_ranges(
    buffer: &Rope,
    folds: &[FoldRange],
    folded_lines: &HashSet<usize>,
) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = folds
        .iter()
        .filter(|f| folded_lines.contains(&f.marker_line))
        .map(|f| hidden_line_range(buffer, f))
        .collect();
    ranges.sort_by_key(|r| r.start);
    merge_overlapping(ranges)
}

fn merge_overlapping(ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for r in ranges {
        if let Some(last) = merged.last_mut()
            && r.start <= last.end
        {
            last.end = last.end.max(r.end);
        } else {
            merged.push(r);
        }
    }
    merged
}

/// Collapses every foldable region — Tools/View "Fold All".
pub(super) fn fold_all(folds: &[FoldRange], folded_lines: &mut HashSet<usize>) {
    folded_lines.extend(folds.iter().map(|f| f.marker_line));
}

/// Expands every collapsed region — Tools/View "Expand All".
pub(super) fn expand_all(folded_lines: &mut HashSet<usize>) {
    folded_lines.clear();
}

/// Paints a `▾` (expanded) / `▸` (collapsed) marker in the fold-arrow gutter
/// column for every currently-shaped row that opens a foldable region, and
/// toggles `folded_lines` when one is clicked. `gutter_left` is the gutter's
/// own left edge (the fold column sits flush against it, line numbers start
/// `FOLD_GUTTER_WIDTH` further right — see `widget.rs`'s gutter layout).
pub(super) fn show_fold_gutter(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    folds: &[FoldRange],
    folded_lines: &mut HashSet<usize>,
    id_salt: &str,
    gutter_left: f32,
    dark_mode: bool,
) {
    if folds.is_empty() {
        return;
    }
    let painter = ui.painter();
    let color = theme::line_number(dark_mode);

    for (i, (logical, _)) in out.row_galleys.iter().enumerate() {
        let Some(fold) = folds.iter().find(|f| f.marker_line == *logical) else {
            continue;
        };
        let y = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
        let rect = egui::Rect::from_min_size(
            egui::pos2(gutter_left, y),
            egui::vec2(FOLD_GUTTER_WIDTH, out.row_height),
        );

        let id = egui::Id::new(("fold_arrow", id_salt, fold.marker_line));
        let response = ui.interact(rect, id, Sense::click());
        let collapsed = folded_lines.contains(logical);
        let fill = if response.hovered() {
            theme::default_text(dark_mode)
        } else {
            color
        };

        // A small filled triangle, drawn as a shape rather than a text
        // glyph — `▸`/`▾` aren't guaranteed to exist in every font this
        // app's `Settings > Font` choice can select, and a missing glyph
        // renders as a tofu box instead of an arrow.
        paint_triangle(painter, rect.center(), 4.0, collapsed, fill);

        if response.clicked() {
            if collapsed {
                folded_lines.remove(logical);
            } else {
                folded_lines.insert(*logical);
            }
        }
    }
}

/// A small filled triangle centered on `center`: pointing right (collapsed,
/// matching `▸`) or down (expanded, matching `▾`). `half_size` is the
/// triangle's half-height (collapsed) or half-width (expanded).
fn paint_triangle(
    painter: &egui::Painter,
    center: egui::Pos2,
    half_size: f32,
    pointing_right: bool,
    fill: egui::Color32,
) {
    let points = if pointing_right {
        vec![
            egui::pos2(center.x - half_size * 0.6, center.y - half_size),
            egui::pos2(center.x - half_size * 0.6, center.y + half_size),
            egui::pos2(center.x + half_size * 0.8, center.y),
        ]
    } else {
        vec![
            egui::pos2(center.x - half_size, center.y - half_size * 0.6),
            egui::pos2(center.x + half_size, center.y - half_size * 0.6),
            egui::pos2(center.x, center.y + half_size * 0.8),
        ]
    };
    painter.add(Shape::convex_polygon(points, fill, egui::Stroke::NONE));
}

/// Paints a `...` affordance right after a collapsed fold's marker line's
/// visible content, for every currently-shaped row that both opens a fold
/// and has that fold in `folded_lines` — the visual cue that more (hidden)
/// content follows on that line. Plain ASCII dots rather than the single
/// `⋯` glyph, for the same reason `show_fold_gutter`'s arrows are drawn as
/// shapes rather than text — no dependency on the active font actually
/// having that glyph.
pub(super) fn paint_collapsed_markers(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    folds: &[FoldRange],
    folded_lines: &HashSet<usize>,
    dark_mode: bool,
) {
    if folded_lines.is_empty() {
        return;
    }
    let painter = ui.painter();
    let color = theme::structure(dark_mode);
    let marker_font = FontId::proportional((out.row_height * 0.8).max(8.0));

    for (i, (logical, galley)) in out.row_galleys.iter().enumerate() {
        if !folded_lines.contains(logical) || !folds.iter().any(|f| f.marker_line == *logical) {
            continue;
        }
        let y_top = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
        let text_end_x = out.content_origin.x + galley.rect.width();
        let pos = egui::pos2(text_end_x + 4.0, y_top + out.row_height / 2.0);
        painter.text(pos, Align2::LEFT_CENTER, "...", marker_font.clone(), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope(s: &str) -> Rope {
        Rope::from_str(s)
    }

    #[test]
    fn hidden_line_range_excludes_the_marker_line_and_includes_the_closing_line() {
        // "class Foo {\n    int x;\n}\n" — fold starts right after line 0's
        // newline (byte 12) and ends at the buffer's end (byte 26, the `}`
        // line's own newline included in the node span up to `}`).
        let source = "class Foo {\n    int x;\n}\n";
        let buffer = rope(source);
        let start_byte = source.find('\n').unwrap() + 1;
        let end_byte = source.rfind('}').unwrap() + 1; // one past the closing brace
        let fold = FoldRange {
            marker_line: 0,
            start_byte,
            end_byte,
        };

        let hidden = hidden_line_range(&buffer, &fold);
        assert_eq!(
            hidden,
            1..3,
            "lines 1 (\"int x;\") and 2 (the closing brace) are hidden; line 0 stays visible"
        );
    }

    #[test]
    fn hidden_ranges_skips_folds_not_in_the_folded_set() {
        let buffer = rope("a\nb\nc\nd\n");
        let folds = vec![
            FoldRange {
                marker_line: 0,
                start_byte: 2,
                end_byte: 4,
            },
            FoldRange {
                marker_line: 2,
                start_byte: 6,
                end_byte: 8,
            },
        ];
        let folded = HashSet::from([2]);

        let hidden = hidden_ranges(&buffer, &folds, &folded);
        assert_eq!(hidden, vec![3..4]);
    }

    #[test]
    fn hidden_ranges_merges_a_nested_fold_collapsed_inside_an_outer_one() {
        // "l0\nl1\nl2\nl3\nl4\nl5\n" — each "lN\n" is 3 bytes, so line k
        // starts at byte 3*k. Outer fold (marker line 0) hides lines 1..5;
        // a fold nested inside it (marker line 1) hides lines 2..4. Both
        // collapsed at once must merge to one non-overlapping range.
        let buffer = rope("l0\nl1\nl2\nl3\nl4\nl5\n");
        let outer = FoldRange {
            marker_line: 0,
            start_byte: 3,
            end_byte: 15,
        };
        let inner = FoldRange {
            marker_line: 1,
            start_byte: 6,
            end_byte: 12,
        };
        assert_eq!(
            hidden_line_range(&buffer, &outer),
            1..5,
            "sanity: outer hides lines 1..5"
        );
        assert_eq!(
            hidden_line_range(&buffer, &inner),
            2..4,
            "sanity: inner hides lines 2..4"
        );

        let folds = vec![outer, inner];
        let folded = HashSet::from([0, 1]);
        let hidden = hidden_ranges(&buffer, &folds, &folded);
        assert_eq!(
            hidden,
            vec![1..5],
            "the nested range must merge into the outer one, not appear separately"
        );
    }

    #[test]
    fn fold_all_collapses_every_marker_line() {
        let folds = vec![
            FoldRange {
                marker_line: 0,
                start_byte: 0,
                end_byte: 1,
            },
            FoldRange {
                marker_line: 5,
                start_byte: 0,
                end_byte: 1,
            },
        ];
        let mut folded = HashSet::new();
        fold_all(&folds, &mut folded);
        assert_eq!(folded, HashSet::from([0, 5]));
    }

    #[test]
    fn expand_all_clears_the_set() {
        let mut folded = HashSet::from([1, 2, 3]);
        expand_all(&mut folded);
        assert!(folded.is_empty());
    }

    #[test]
    fn hidden_ranges_ignores_a_stale_marker_line_no_longer_present_in_folds() {
        // Simulates PLAN.md 3e: `folded_lines` still has line 4 from before
        // an edit, but the freshly recomputed `folds` no longer has a fold
        // opening there — it should just be silently dropped, not panic or
        // produce a bogus range.
        let buffer = rope("a\nb\nc\n");
        let folds = vec![FoldRange {
            marker_line: 0,
            start_byte: 2,
            end_byte: 4,
        }];
        let folded = HashSet::from([0, 4]);
        let hidden = hidden_ranges(&buffer, &folds, &folded);
        assert_eq!(hidden, vec![1..2]);
    }
}
