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
            similar::DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => DiffLineOp::Equal {
                old: old_index..old_index + len,
                new: new_index..new_index + len,
            },
            similar::DiffOp::Delete { old_index, old_len, .. } => DiffLineOp::Delete {
                old: old_index..old_index + old_len,
            },
            similar::DiffOp::Insert { new_index, new_len, .. } => DiffLineOp::Insert {
                new: new_index..new_index + new_len,
            },
            similar::DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => DiffLineOp::Replace {
                old: old_index..old_index + old_len,
                new: new_index..new_index + new_len,
            },
        })
        .collect()
}

/// Line-level added/removed counts between `old` and `new` — the same
/// `diff_line_ops` a full render would use, reduced to just the two totals
/// a compact "+N -M" summary needs (e.g. one row of a file-history snapshot
/// list, `PLAN.md` Track 4 Phase 2) where a full rendered diff would be
/// far more than that one row has room for.
pub fn diff_stat(old: &str, new: &str) -> (usize, usize) {
    diff_line_ops(old, new)
        .into_iter()
        .fold((0, 0), |(added, removed), op| match op {
            DiffLineOp::Insert { new } => (added + new.len(), removed),
            DiffLineOp::Delete { old } => (added, removed + old.len()),
            DiffLineOp::Replace { old, new } => (added + new.len(), removed + old.len()),
            DiffLineOp::Equal { .. } => (added, removed),
        })
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
    Line {
        old: Option<&'a str>,
        new: Option<&'a str>,
        old_changed: bool,
        new_changed: bool,
    },
    Collapsed(usize),
}

/// Pairs `old_lines`/`new_lines` up per `ops` into row-aligned
/// `SideBySideRow`s, collapsing the middle of any `Equal` run longer than
/// `2 * context` lines down to a single `Collapsed` marker. Pure/no I/O,
/// directly testable against known ops.
pub fn side_by_side_rows<'a>(
    old_lines: &[&'a str],
    new_lines: &[&'a str],
    ops: &[DiffLineOp],
    context: usize,
) -> Vec<SideBySideRow<'a>> {
    let mut rows = Vec::new();
    for op in ops {
        match op {
            DiffLineOp::Equal { old, new } => match split_equal_run(old.len(), context) {
                None => {
                    for (old_line, new_line) in old_lines[old.clone()].iter().zip(&new_lines[new.clone()]) {
                        rows.push(SideBySideRow::Line {
                            old: Some(old_line),
                            new: Some(new_line),
                            old_changed: false,
                            new_changed: false,
                        });
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
                    rows.push(SideBySideRow::Line {
                        old: Some(old_line),
                        new: None,
                        old_changed: true,
                        new_changed: false,
                    });
                }
            }
            DiffLineOp::Insert { new } => {
                for new_line in &new_lines[new.clone()] {
                    rows.push(SideBySideRow::Line {
                        old: None,
                        new: Some(new_line),
                        old_changed: false,
                        new_changed: true,
                    });
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
pub fn inline_rows<'a>(
    old_lines: &[&'a str],
    new_lines: &[&'a str],
    ops: &[DiffLineOp],
    context: usize,
) -> Vec<InlineRow<'a>> {
    let mut rows = Vec::new();
    for op in ops {
        match op {
            DiffLineOp::Equal { old, .. } => match split_equal_run(old.len(), context) {
                None => {
                    rows.extend(old_lines[old.clone()].iter().map(|&text| InlineRow::Line {
                        text,
                        kind: InlineRowKind::Equal,
                    }));
                }
                Some((lead, hidden, trail)) => {
                    rows.extend(lead.map(|i| InlineRow::Line {
                        text: old_lines[old.start + i],
                        kind: InlineRowKind::Equal,
                    }));
                    rows.push(InlineRow::Collapsed(hidden));
                    rows.extend(trail.map(|i| InlineRow::Line {
                        text: old_lines[old.start + i],
                        kind: InlineRowKind::Equal,
                    }));
                }
            },
            DiffLineOp::Delete { old } => {
                rows.extend(old_lines[old.clone()].iter().map(|&text| InlineRow::Line {
                    text,
                    kind: InlineRowKind::Deleted,
                }));
            }
            DiffLineOp::Insert { new } => {
                rows.extend(new_lines[new.clone()].iter().map(|&text| InlineRow::Line {
                    text,
                    kind: InlineRowKind::Inserted,
                }));
            }
            DiffLineOp::Replace { old, new } => {
                rows.extend(old_lines[old.clone()].iter().map(|&text| InlineRow::Line {
                    text,
                    kind: InlineRowKind::Deleted,
                }));
                rows.extend(new_lines[new.clone()].iter().map(|&text| InlineRow::Line {
                    text,
                    kind: InlineRowKind::Inserted,
                }));
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
        DiffMode::SideBySide => show_side_by_side(
            ui,
            &side_by_side_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES),
            &font_id,
            dark_mode,
        ),
        DiffMode::Inline => show_inline(
            ui,
            &inline_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES),
            &font_id,
            dark_mode,
        ),
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

fn show_side_by_side(
    ui: &mut egui::Ui,
    rows: &[SideBySideRow<'_>],
    font_id: &FontId,
    dark_mode: bool,
) -> egui::Response {
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
                    SideBySideRow::Line {
                        old,
                        new,
                        old_changed,
                        new_changed,
                    } => {
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
    egui::Frame::new()
        .fill(bg.unwrap_or(Color32::TRANSPARENT))
        .inner_margin(2)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(text.unwrap_or_default())
                    .font(font_id.clone())
                    .color(text_color),
            );
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
                InlineRow::Line {
                    text,
                    kind: InlineRowKind::Equal,
                } => (format!("  {text}"), text_color, Color32::TRANSPARENT),
                InlineRow::Line {
                    text,
                    kind: InlineRowKind::Deleted,
                } => (format!("- {text}"), text_color, removed_bg),
                InlineRow::Line {
                    text,
                    kind: InlineRowKind::Inserted,
                } => (format!("+ {text}"), text_color, added_bg),
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
#[path = "diff_view_test.rs"]
mod diff_view_test;
