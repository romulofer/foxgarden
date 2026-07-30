//! `PLAN.md` Track 18: a reusable, read-only diff-rendering widget — a
//! shared prerequisite for whatever full-diff view `Git diff gutter` (§9)
//! ever grows beyond its own gutter-marks-only scope, and for `Local file
//! history` (§4)'s own snapshot-vs-live comparison, per `SPEC.md` §18's own
//! reasoning for building this once rather than each of those two features
//! growing a bespoke diff renderer. Line-level diffing only (no word-level
//! highlighting within a changed line, for a first pass) and no inline
//! *editing* through the diff view — a `Replace` op's old/new lines are
//! just a paired red/green row each, never an accept/reject affordance the
//! way a merge-conflict UI would need (out of scope here, same as it is
//! for the diff gutter).

use std::ops::Range;

use egui::{Color32, FontId, RichText};

use crate::style::fonts::EditorFont;
use crate::style::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    SideBySide,
    Inline,
}

/// How many unchanged lines to keep bordering each side of a run of
/// `Equal` lines before collapsing the middle — git's own `-U3` default
/// (already this codebase's own choice for `fg_core::git_file_diff`'s real
/// context, `PLAN.md` Track 9 Phase 4), so a long untouched stretch reads
/// the same "abridged" way here as it already does in a real `git diff`.
pub const DEFAULT_CONTEXT_LINES: usize = 3;

/// One line-level diff operation, in document order. `old`/`new` are
/// 0-based line-*index* ranges (not byte offsets) into `old.lines()`/
/// `new.lines()` — verified to match `similar::TextDiff::from_lines`'s own
/// indexing exactly against a real run (see this module's own tests), so a
/// caller already holding the split line slices can index straight into
/// them with no further translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLineOp {
    Equal { old: Range<usize>, new: Range<usize> },
    Delete { old: Range<usize> },
    Insert { new: Range<usize> },
    Replace { old: Range<usize>, new: Range<usize> },
}

/// Computes a line-level diff between `old` and `new` into an ordered list
/// of `DiffLineOp`s, via the `similar` crate — actively maintained and
/// current on crates.io as of this session (verified before pinning a
/// version, per this project's own discipline for external dependencies,
/// rather than assuming `SPEC.md`'s own naming stayed accurate).
pub fn diff_line_ops(old: &str, new: &str) -> Vec<DiffLineOp> {
    let diff = similar::TextDiff::from_lines(old, new);
    diff.ops()
        .iter()
        .map(|op| match *op {
            similar::DiffOp::Equal { old_index, new_index, len } => {
                DiffLineOp::Equal { old: old_index..old_index + len, new: new_index..new_index + len }
            }
            similar::DiffOp::Delete { old_index, old_len, .. } => DiffLineOp::Delete { old: old_index..old_index + old_len },
            similar::DiffOp::Insert { new_index, new_len, .. } => DiffLineOp::Insert { new: new_index..new_index + new_len },
            similar::DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                DiffLineOp::Replace { old: old_index..old_index + old_len, new: new_index..new_index + new_len }
            }
        })
        .collect()
}

/// Splits an `Equal` run of `len` lines into `(leading, hidden, trailing)`
/// *local* offsets within the run — `context` lines kept at each end, the
/// `hidden` count of lines in between collapsed away — or `None` when
/// `len` isn't even long enough for collapsing to hide anything (`<=
/// 2 * context`, so keeping both ends in full already shows every line).
/// Pure/no I/O, directly testable.
fn split_equal_run(len: usize, context: usize) -> Option<(Range<usize>, usize, Range<usize>)> {
    if len <= 2 * context {
        return None;
    }
    Some((0..context, len - 2 * context, (len - context)..len))
}

/// One row of a `SideBySide` render — either a real line pair (`old`/`new`
/// are `None` exactly when that side has nothing to show at this row: an
/// `Insert`'s old side, a `Delete`'s new side, or a `Replace` whose old/new
/// line counts differ, the shorter side padding with blank rows so both
/// columns stay aligned row-for-row) or a `Collapsed` placeholder standing
/// in for a run of untouched lines too long to show in full (see
/// `DEFAULT_CONTEXT_LINES`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideBySideRow<'a> {
    Line { old: Option<&'a str>, new: Option<&'a str>, old_changed: bool, new_changed: bool },
    Collapsed(usize),
}

/// Pairs `old_lines`/`new_lines` up per `ops` into row-aligned
/// `SideBySideRow`s, collapsing the middle of any `Equal` run longer than
/// `2 * context` lines down to a single `Collapsed` marker. Pure/no I/O,
/// directly testable against known ops.
pub fn side_by_side_rows<'a>(old_lines: &[&'a str], new_lines: &[&'a str], ops: &[DiffLineOp], context: usize) -> Vec<SideBySideRow<'a>> {
    let mut rows = Vec::new();
    for op in ops {
        match op {
            DiffLineOp::Equal { old, new } => match split_equal_run(old.len(), context) {
                None => {
                    for (old_line, new_line) in old_lines[old.clone()].iter().zip(&new_lines[new.clone()]) {
                        rows.push(SideBySideRow::Line { old: Some(old_line), new: Some(new_line), old_changed: false, new_changed: false });
                    }
                }
                Some((lead, hidden, trail)) => {
                    for i in lead {
                        rows.push(SideBySideRow::Line {
                            old: Some(old_lines[old.start + i]),
                            new: Some(new_lines[new.start + i]),
                            old_changed: false,
                            new_changed: false,
                        });
                    }
                    rows.push(SideBySideRow::Collapsed(hidden));
                    for i in trail {
                        rows.push(SideBySideRow::Line {
                            old: Some(old_lines[old.start + i]),
                            new: Some(new_lines[new.start + i]),
                            old_changed: false,
                            new_changed: false,
                        });
                    }
                }
            },
            DiffLineOp::Delete { old } => {
                for old_line in &old_lines[old.clone()] {
                    rows.push(SideBySideRow::Line { old: Some(old_line), new: None, old_changed: true, new_changed: false });
                }
            }
            DiffLineOp::Insert { new } => {
                for new_line in &new_lines[new.clone()] {
                    rows.push(SideBySideRow::Line { old: None, new: Some(new_line), old_changed: false, new_changed: true });
                }
            }
            DiffLineOp::Replace { old, new } => {
                let old_slice = &old_lines[old.clone()];
                let new_slice = &new_lines[new.clone()];
                for i in 0..old_slice.len().max(new_slice.len()) {
                    rows.push(SideBySideRow::Line {
                        old: old_slice.get(i).copied(),
                        new: new_slice.get(i).copied(),
                        old_changed: i < old_slice.len(),
                        new_changed: i < new_slice.len(),
                    });
                }
            }
        }
    }
    rows
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineRowKind {
    Equal,
    Deleted,
    Inserted,
}

/// One row of an `Inline` render — either a real line (`text`/`kind`) or a
/// `Collapsed` placeholder standing in for a run of untouched lines too
/// long to show in full (see `DEFAULT_CONTEXT_LINES`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineRow<'a> {
    Line { text: &'a str, kind: InlineRowKind },
    Collapsed(usize),
}

/// Flattens `ops` into one `InlineRow` per line, unified-diff order — a
/// `Replace`'s old lines (all `Deleted`) immediately followed by its new
/// lines (all `Inserted`), per `SPEC.md` §18's own "a paired red/green row
/// for a replace" — collapsing the middle of any `Equal` run longer than
/// `2 * context` lines down to a single `Collapsed` marker. Pure/no I/O,
/// directly testable against known ops.
pub fn inline_rows<'a>(old_lines: &[&'a str], new_lines: &[&'a str], ops: &[DiffLineOp], context: usize) -> Vec<InlineRow<'a>> {
    let mut rows = Vec::new();
    for op in ops {
        match op {
            DiffLineOp::Equal { old, .. } => match split_equal_run(old.len(), context) {
                None => {
                    rows.extend(old_lines[old.clone()].iter().map(|&text| InlineRow::Line { text, kind: InlineRowKind::Equal }));
                }
                Some((lead, hidden, trail)) => {
                    rows.extend(lead.map(|i| InlineRow::Line { text: old_lines[old.start + i], kind: InlineRowKind::Equal }));
                    rows.push(InlineRow::Collapsed(hidden));
                    rows.extend(trail.map(|i| InlineRow::Line { text: old_lines[old.start + i], kind: InlineRowKind::Equal }));
                }
            },
            DiffLineOp::Delete { old } => {
                rows.extend(old_lines[old.clone()].iter().map(|&text| InlineRow::Line { text, kind: InlineRowKind::Deleted }));
            }
            DiffLineOp::Insert { new } => {
                rows.extend(new_lines[new.clone()].iter().map(|&text| InlineRow::Line { text, kind: InlineRowKind::Inserted }));
            }
            DiffLineOp::Replace { old, new } => {
                rows.extend(old_lines[old.clone()].iter().map(|&text| InlineRow::Line { text, kind: InlineRowKind::Deleted }));
                rows.extend(new_lines[new.clone()].iter().map(|&text| InlineRow::Line { text, kind: InlineRowKind::Inserted }));
            }
        }
    }
    rows
}

/// A translucent wash of `color`, for a full-row background — `theme::
/// diff_added`/`diff_removed` are deliberately full-opacity for the diff
/// *gutter*'s own thin 4px bar (see that module's doc comment), which would
/// overwhelm this widget's own line text used at that same strength as a
/// full-row fill; the text itself stays `theme::default_text` throughout
/// (the row background alone carries the added/removed distinction, the
/// same convention every mainstream diff view already uses).
fn row_wash(color: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 30)
}

/// Renders `old` vs `new` as a read-only diff, per `mode` — `SideBySide`
/// (two columns, row-aligned) or `Inline` (one column, unified-style rows)
/// — using the editor's own font (`editor_font`/`font_size`, the same pair
/// every other font-aware widget here already takes) and theme (`dark_
/// mode`, `theme::diff_added`/`diff_removed` — the identical green/red
/// `PLAN.md` Track 9 Phase 1's own diff gutter already established, so
/// nothing new has to be learned to read this widget). Long untouched
/// stretches are abridged to `DEFAULT_CONTEXT_LINES` of context at each
/// end, per `side_by_side_rows`/`inline_rows`'s own `context` parameter.
pub fn show_diff(
    ui: &mut egui::Ui,
    old: &str,
    new: &str,
    mode: DiffMode,
    editor_font: EditorFont,
    font_size: f32,
    dark_mode: bool,
) -> egui::Response {
    let font_id = FontId::new(font_size, editor_font.family());
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let ops = diff_line_ops(old, new);

    match mode {
        DiffMode::SideBySide => {
            show_side_by_side(ui, &side_by_side_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES), &font_id, dark_mode)
        }
        DiffMode::Inline => show_inline(ui, &inline_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES), &font_id, dark_mode),
    }
}

/// The label text for a `Collapsed` placeholder — plain ASCII only
/// (deliberately, not a Unicode ellipsis or box-drawing glyph): a real,
/// live-verified bug this session (`▸`/`▾` rendering as a tofu box
/// elsewhere in the app) confirmed neither this app's own bundled fonts
/// nor egui's built-ins can be trusted to cover an arbitrary glyph in the
/// default UI font, and this is exactly the kind of small text label that
/// bug would silently recur in.
fn collapsed_label(hidden: usize) -> String {
    format!("... {hidden} unchanged line{} ...", if hidden == 1 { "" } else { "s" })
}

fn show_side_by_side(ui: &mut egui::Ui, rows: &[SideBySideRow<'_>], font_id: &FontId, dark_mode: bool) -> egui::Response {
    let text_color = theme::default_text(dark_mode);
    let weak_color = theme::line_number(dark_mode);
    let removed_bg = row_wash(theme::diff_removed(dark_mode));
    let added_bg = row_wash(theme::diff_added(dark_mode));

    egui::Grid::new("diff_view_side_by_side")
        .num_columns(2)
        .striped(false)
        .spacing([0.0, 0.0])
        .show(ui, |ui| {
            for row in rows {
                match row {
                    SideBySideRow::Line { old, new, old_changed, new_changed } => {
                        diff_cell(ui, *old, old_changed.then_some(removed_bg), font_id, text_color);
                        diff_cell(ui, *new, new_changed.then_some(added_bg), font_id, text_color);
                    }
                    SideBySideRow::Collapsed(hidden) => {
                        diff_cell(ui, Some(collapsed_label(*hidden).as_str()), None, font_id, weak_color);
                        diff_cell(ui, None, None, font_id, weak_color);
                    }
                }
                ui.end_row();
            }
        })
        .response
}

fn diff_cell(ui: &mut egui::Ui, text: Option<&str>, bg: Option<Color32>, font_id: &FontId, text_color: Color32) {
    egui::Frame::new().fill(bg.unwrap_or(Color32::TRANSPARENT)).inner_margin(2).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new(text.unwrap_or_default()).font(font_id.clone()).color(text_color));
    });
}

fn show_inline(ui: &mut egui::Ui, rows: &[InlineRow<'_>], font_id: &FontId, dark_mode: bool) -> egui::Response {
    let text_color = theme::default_text(dark_mode);
    let weak_color = theme::line_number(dark_mode);
    let removed_bg = row_wash(theme::diff_removed(dark_mode));
    let added_bg = row_wash(theme::diff_added(dark_mode));

    ui.vertical(|ui| {
        for row in rows {
            let (text, color, bg) = match row {
                InlineRow::Line { text, kind: InlineRowKind::Equal } => (format!("  {text}"), text_color, Color32::TRANSPARENT),
                InlineRow::Line { text, kind: InlineRowKind::Deleted } => (format!("- {text}"), text_color, removed_bg),
                InlineRow::Line { text, kind: InlineRowKind::Inserted } => (format!("+ {text}"), text_color, added_bg),
                InlineRow::Collapsed(hidden) => (collapsed_label(*hidden), weak_color, Color32::TRANSPARENT),
            };
            egui::Frame::new().fill(bg).inner_margin(2).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(text).font(font_id.clone()).color(color));
            });
        }
    })
    .response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_line_ops_reports_a_pure_modification_as_replace() {
        let ops = diff_line_ops("l1\nl2\nl3\n", "l1\nCHANGED\nl3\n");
        assert_eq!(
            ops,
            vec![
                DiffLineOp::Equal { old: 0..1, new: 0..1 },
                DiffLineOp::Replace { old: 1..2, new: 1..2 },
                DiffLineOp::Equal { old: 2..3, new: 2..3 },
            ]
        );
    }

    #[test]
    fn diff_line_ops_reports_a_pure_deletion() {
        let ops = diff_line_ops("l1\nl2\nl3\nl4\n", "l1\nl3\nl4\n");
        assert_eq!(
            ops,
            vec![
                DiffLineOp::Equal { old: 0..1, new: 0..1 },
                DiffLineOp::Delete { old: 1..2 },
                DiffLineOp::Equal { old: 2..4, new: 1..3 },
            ]
        );
    }

    #[test]
    fn diff_line_ops_reports_a_pure_insertion() {
        let ops = diff_line_ops("", "a\nb\n");
        assert_eq!(ops, vec![DiffLineOp::Insert { new: 0..2 }]);
    }

    #[test]
    fn diff_line_ops_indices_match_str_lines_directly() {
        // The exact contract `side_by_side_rows`/`inline_rows` depend on:
        // `old_index`/`new_index` line-index into `old.lines()`/
        // `new.lines()` with no further translation needed.
        let old = "l1\nl2\nl3\nl4\n";
        let new = "l1\nl3\nl4\n";
        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();
        let ops = diff_line_ops(old, new);
        let DiffLineOp::Delete { old: range } = &ops[1] else { panic!("expected a Delete op") };
        assert_eq!(&old_lines[range.clone()], &["l2"]);
        let DiffLineOp::Equal { new: range, .. } = &ops[2] else { panic!("expected an Equal op") };
        assert_eq!(&new_lines[range.clone()], &["l3", "l4"]);
    }

    #[test]
    fn split_equal_run_leaves_a_short_run_untouched() {
        assert_eq!(split_equal_run(6, 3), None);
    }

    #[test]
    fn split_equal_run_collapses_the_middle_of_a_long_run() {
        assert_eq!(split_equal_run(10, 3), Some((0..3, 4, 7..10)));
    }

    #[test]
    fn side_by_side_rows_aligns_a_pure_modification_one_to_one() {
        let old_lines = vec!["l1", "l2", "l3"];
        let new_lines = vec!["l1", "CHANGED", "l3"];
        let ops = diff_line_ops("l1\nl2\nl3\n", "l1\nCHANGED\nl3\n");
        let rows = side_by_side_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
        assert_eq!(
            rows,
            vec![
                SideBySideRow::Line { old: Some("l1"), new: Some("l1"), old_changed: false, new_changed: false },
                SideBySideRow::Line { old: Some("l2"), new: Some("CHANGED"), old_changed: true, new_changed: true },
                SideBySideRow::Line { old: Some("l3"), new: Some("l3"), old_changed: false, new_changed: false },
            ]
        );
    }

    #[test]
    fn side_by_side_rows_pads_the_shorter_side_of_an_uneven_replace() {
        let old_lines = vec!["only"];
        let new_lines = vec!["first", "second"];
        let ops = diff_line_ops("only\n", "first\nsecond\n");
        let rows = side_by_side_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
        assert_eq!(
            rows,
            vec![
                SideBySideRow::Line { old: Some("only"), new: Some("first"), old_changed: true, new_changed: true },
                SideBySideRow::Line { old: None, new: Some("second"), old_changed: false, new_changed: true },
            ]
        );
    }

    #[test]
    fn side_by_side_rows_leaves_the_opposite_side_blank_for_a_pure_insert_or_delete() {
        let old_lines = vec!["l1", "l2", "l3"];
        let new_lines = vec!["l1", "l3"];
        let ops = diff_line_ops("l1\nl2\nl3\n", "l1\nl3\n");
        let rows = side_by_side_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
        assert_eq!(
            rows,
            vec![
                SideBySideRow::Line { old: Some("l1"), new: Some("l1"), old_changed: false, new_changed: false },
                SideBySideRow::Line { old: Some("l2"), new: None, old_changed: true, new_changed: false },
                SideBySideRow::Line { old: Some("l3"), new: Some("l3"), old_changed: false, new_changed: false },
            ]
        );
    }

    #[test]
    fn side_by_side_rows_collapses_a_long_untouched_run_with_a_context_border() {
        let old_lines: Vec<&str> = vec!["c1", "c2", "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "c3"];
        let new_lines: Vec<&str> = vec!["c1", "c2", "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "c3"];
        // Manually crafted ops: a leading change, then a 8-line Equal run
        // (long enough to collapse with context=2), then a trailing change.
        let ops = vec![
            DiffLineOp::Insert { new: 0..2 },
            DiffLineOp::Equal { old: 2..10, new: 2..10 },
            DiffLineOp::Delete { old: 10..11 },
        ];
        let rows = side_by_side_rows(&old_lines, &new_lines, &ops, 2);
        assert_eq!(
            rows,
            vec![
                SideBySideRow::Line { old: None, new: Some("c1"), old_changed: false, new_changed: true },
                SideBySideRow::Line { old: None, new: Some("c2"), old_changed: false, new_changed: true },
                SideBySideRow::Line { old: Some("e1"), new: Some("e1"), old_changed: false, new_changed: false },
                SideBySideRow::Line { old: Some("e2"), new: Some("e2"), old_changed: false, new_changed: false },
                SideBySideRow::Collapsed(4),
                SideBySideRow::Line { old: Some("e7"), new: Some("e7"), old_changed: false, new_changed: false },
                SideBySideRow::Line { old: Some("e8"), new: Some("e8"), old_changed: false, new_changed: false },
                SideBySideRow::Line { old: Some("c3"), new: None, old_changed: true, new_changed: false },
            ]
        );
    }

    #[test]
    fn inline_rows_shows_a_replaces_old_lines_then_its_new_lines() {
        let old_lines = vec!["l1", "l2", "l3"];
        let new_lines = vec!["l1", "CHANGED", "l3"];
        let ops = diff_line_ops("l1\nl2\nl3\n", "l1\nCHANGED\nl3\n");
        let rows = inline_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
        assert_eq!(
            rows,
            vec![
                InlineRow::Line { text: "l1", kind: InlineRowKind::Equal },
                InlineRow::Line { text: "l2", kind: InlineRowKind::Deleted },
                InlineRow::Line { text: "CHANGED", kind: InlineRowKind::Inserted },
                InlineRow::Line { text: "l3", kind: InlineRowKind::Equal },
            ]
        );
    }

    #[test]
    fn inline_rows_on_identical_text_is_all_equal() {
        let old_lines = vec!["a", "b"];
        let new_lines = vec!["a", "b"];
        let ops = diff_line_ops("a\nb\n", "a\nb\n");
        let rows = inline_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
        assert_eq!(
            rows,
            vec![
                InlineRow::Line { text: "a", kind: InlineRowKind::Equal },
                InlineRow::Line { text: "b", kind: InlineRowKind::Equal }
            ]
        );
    }

    #[test]
    fn inline_rows_collapses_a_long_untouched_run_with_a_context_border() {
        let old_lines: Vec<&str> = vec!["e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8"];
        let new_lines: Vec<&str> = old_lines.clone();
        let ops = vec![DiffLineOp::Equal { old: 0..8, new: 0..8 }];
        let rows = inline_rows(&old_lines, &new_lines, &ops, 2);
        assert_eq!(
            rows,
            vec![
                InlineRow::Line { text: "e1", kind: InlineRowKind::Equal },
                InlineRow::Line { text: "e2", kind: InlineRowKind::Equal },
                InlineRow::Collapsed(4),
                InlineRow::Line { text: "e7", kind: InlineRowKind::Equal },
                InlineRow::Line { text: "e8", kind: InlineRowKind::Equal },
            ]
        );
    }

    /// Real font install (`crate::style::fonts::install`, exactly what
    /// `FoxGardenApp::new` runs at real startup), not `FontDefinitions::
    /// empty()` like most of this app's other headless widget tests —
    /// `EditorFont::JetBrainsMono` maps to a *named* `FontFamily` that,
    /// unlike the built-in `Monospace`/`Proportional` ones, panics on any
    /// lookup if nothing ever registered it (confirmed: an empty-fonts
    /// setup crashed this exact test before switching to a real install).
    fn headless_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        crate::style::fonts::install(&ctx);
        ctx
    }

    #[test]
    fn show_diff_side_by_side_does_not_panic_against_real_changed_text() {
        let ctx = headless_ctx();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show_diff(ui, "l1\nl2\nl3\n", "l1\nCHANGED\nl3\nl4\n", DiffMode::SideBySide, EditorFont::JetBrainsMono, 14.0, true);
        });
    }

    #[test]
    fn show_diff_inline_does_not_panic_against_real_changed_text() {
        let ctx = headless_ctx();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show_diff(ui, "l1\nl2\nl3\n", "l1\nCHANGED\nl3\nl4\n", DiffMode::Inline, EditorFont::Default, 14.0, false);
        });
    }

    #[test]
    fn show_diff_on_identical_text_does_not_panic() {
        let ctx = headless_ctx();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show_diff(ui, "same\n", "same\n", DiffMode::SideBySide, EditorFont::JetBrainsMono, 14.0, true);
        });
    }

    /// A larger real file (well beyond `2 * DEFAULT_CONTEXT_LINES`) with
    /// one small change in the middle — the actual scenario the user
    /// reported wanting abridged, exercised as a real render, not just the
    /// pure row-builder tests above.
    #[test]
    fn show_diff_abridges_a_large_mostly_unchanged_file_without_panicking() {
        let lines: Vec<String> = (1..=200).map(|n| format!("line {n}")).collect();
        let old = lines.join("\n") + "\n";
        let mut changed = lines.clone();
        changed[100] = "CHANGED".to_string();
        let new = changed.join("\n") + "\n";

        let ctx = headless_ctx();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show_diff(ui, &old, &new, DiffMode::SideBySide, EditorFont::Default, 14.0, true);
        });

        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();
        let ops = diff_line_ops(&old, &new);
        let rows = inline_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
        // Two collapsed runs (before and after the one changed line) plus
        // a small number of real lines — not all 200 shown in full.
        assert_eq!(rows.iter().filter(|r| matches!(r, InlineRow::Collapsed(_))).count(), 2);
        assert!(rows.len() < 20, "expected the bulk of the file to be abridged, got {} rows", rows.len());
    }
}
