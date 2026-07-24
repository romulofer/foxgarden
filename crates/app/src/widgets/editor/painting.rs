use std::collections::HashMap;
use std::ops::Range;

use egui::text::CCursor;
use egui::{Align2, Color32, FontId, Shape, Stroke};
use fg_core::Diagnostic;

use crate::style::indent::IndentSettings;
use crate::style::theme;

/// Maps every byte offset in `queries` (assumed sorted, deduped, and each a
/// valid char boundary of `text` — `paint_diagnostics` guarantees this
/// before calling in) to its char offset, in one forward pass over `text`
/// rather than restarting a `text[..b].chars().count()` scan from byte 0
/// per offset. `text.len()` is a valid query (the "end of the last
/// diagnostic touches end of file" case) even though it's one past the
/// last real char, so it's queried via a synthetic trailing entry rather
/// than `char_indices()`, which never yields it.
fn char_offsets_for(text: &str, queries: &[usize]) -> HashMap<usize, usize> {
    let mut result = HashMap::with_capacity(queries.len());
    let mut qi = 0;
    for (char_count, (byte_idx, _)) in text.char_indices().chain(std::iter::once((text.len(), '\0'))).enumerate() {
        while qi < queries.len() && queries[qi] == byte_idx {
            result.insert(byte_idx, char_count);
            qi += 1;
        }
    }
    result
}

pub(super) fn paint_diagnostics(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    text: &str,
    diagnostics: &[Diagnostic],
) {
    if diagnostics.is_empty() {
        return;
    }

    // Clamp/validate each diagnostic's byte range up front, keeping its
    // original index so painting order below matches `diagnostics`' order
    // unchanged from before this function stopped scanning per-diagnostic.
    let spans: Vec<(usize, usize, usize)> = diagnostics
        .iter()
        .enumerate()
        .filter_map(|(i, diag)| {
            let start = diag.range.start.min(text.len());
            let end = diag.range.end.min(text.len()).max(start);
            (text.is_char_boundary(start) && text.is_char_boundary(end)).then_some((i, start, end))
        })
        .collect();

    let mut queries: Vec<usize> = spans.iter().flat_map(|&(_, start, end)| [start, end]).collect();
    queries.sort_unstable();
    queries.dedup();
    let char_offset_for = char_offsets_for(text, &queries);

    let painter = ui.painter();
    let squiggle_color = theme::error_squiggle(ui.visuals().dark_mode);
    for (i, start, end) in spans {
        let diag = &diagnostics[i];
        let char_start = char_offset_for[&start];
        let char_end = char_offset_for[&end].max(char_start + 1);

        let start_rect = output.galley.pos_from_cursor(CCursor::new(char_start));
        let end_rect = output.galley.pos_from_cursor(CCursor::new(char_end));

        let top = output.galley_pos.y + start_rect.top();
        let y = output.galley_pos.y + start_rect.bottom();
        let x_start = output.galley_pos.x + start_rect.left();
        let x_end = (output.galley_pos.x + end_rect.left()).max(x_start + 4.0);

        paint_squiggle(painter, y, x_start, x_end, squiggle_color);

        // Sense::hover() only — this must not steal clicks/drags from the
        // TextEdit underneath, just report when the pointer is sitting over
        // this squiggle so its message can show as a tooltip.
        let hover_rect = egui::Rect::from_min_max(egui::pos2(x_start, top), egui::pos2(x_end, y));
        let id = egui::Id::new(("diagnostic_tooltip", start, end));
        ui.interact(hover_rect, id, egui::Sense::hover())
            .on_hover_text(&diag.message);
    }
}

fn paint_squiggle(painter: &egui::Painter, y: f32, x_start: f32, x_end: f32, color: Color32) {
    let amplitude = 2.0;
    let step = 3.0;
    let mut points = vec![egui::pos2(x_start, y)];
    let mut x = x_start;
    let mut up = true;
    while x < x_end {
        x = (x + step).min(x_end);
        let yy = if up { y - amplitude } else { y + amplitude };
        points.push(egui::pos2(x, yy));
        up = !up;
    }
    painter.add(Shape::line(points, Stroke::new(1.5, color)));
}

/// Paints one right-aligned line number per visual row of `output.galley`,
/// flush against `gutter_right_edge`. Driven directly by the galley's own
/// rows (each row's `pos`/`size`, via `PlacedRow::rect`) rather than
/// independently recomputing row positions from font metrics — that keeps
/// the numbers pixel-aligned with the text no matter what the row height
/// actually is, and automatically scrolls in sync since `output.galley_pos`
/// already accounts for the `ScrollArea`'s current offset (same technique
/// `paint_diagnostics`/`paint_extra_selections` use).
pub(super) fn paint_line_numbers(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    gutter_right_edge: f32,
    font_id: FontId,
    dark_mode: bool,
) {
    let painter = ui.painter();
    let color = theme::line_number(dark_mode);
    for (index, row) in output.galley.rows.iter().enumerate() {
        let y = output.galley_pos.y + row.rect().center().y;
        painter.text(
            egui::pos2(gutter_right_edge, y),
            Align2::RIGHT_CENTER,
            index + 1,
            font_id.clone(),
            color,
        );
    }
}

/// Paints the Ctrl+D secondary cursors/selections: a thin caret for a bare
/// position, or a translucent rect for a claimed occurrence — same
/// `galley.pos_from_cursor` technique `paint_diagnostics` uses, just
/// char-index based instead of byte based since `extra_selections` is
/// already in char space.
pub(super) fn paint_extra_selections(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    extra_selections: &[Range<usize>],
) {
    let painter = ui.painter();
    let caret_color = ui.visuals().text_cursor.stroke.color;
    let selection_color = ui.visuals().selection.bg_fill;

    for range in extra_selections {
        let start_rect = output.galley.pos_from_cursor(CCursor::new(range.start));
        let y_top = output.galley_pos.y + start_rect.top();
        let y_bottom = output.galley_pos.y + start_rect.bottom();
        let x_start = output.galley_pos.x + start_rect.left();

        if range.is_empty() {
            painter.line_segment(
                [egui::pos2(x_start, y_top), egui::pos2(x_start, y_bottom)],
                Stroke::new(1.5, caret_color),
            );
        } else {
            let end_rect = output.galley.pos_from_cursor(CCursor::new(range.end));
            let x_end = output.galley_pos.x + end_rect.left();
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x_start, y_top), egui::pos2(x_end, y_bottom)),
                0.0,
                selection_color,
            );
        }
    }
}

/// Paints a subtle background rect behind every occurrence of the word
/// under the cursor (passive, read-only — see `widgets::editor::show`'s
/// comment on when this runs), including the one the cursor itself sits
/// on/in. Same `galley.pos_from_cursor` technique `paint_extra_selections`
/// uses, deliberately with a much less prominent fill so it never reads as
/// an active selection.
pub(super) fn paint_occurrence_highlights(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    occurrences: &[Range<usize>],
) {
    let painter = ui.painter();
    let fill = theme::occurrence_highlight(ui.visuals().dark_mode);

    for range in occurrences {
        let start_rect = output.galley.pos_from_cursor(CCursor::new(range.start));
        let end_rect = output.galley.pos_from_cursor(CCursor::new(range.end));
        let y_top = output.galley_pos.y + start_rect.top();
        let y_bottom = output.galley_pos.y + start_rect.bottom();
        let x_start = output.galley_pos.x + start_rect.left();
        let x_end = output.galley_pos.x + end_rect.left();

        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x_start, y_top), egui::pos2(x_end, y_bottom)),
            2.0,
            fill,
        );
    }
}

/// Paints a subtle outline box around each bracket of a matched pair — the
/// bracket-pair-highlighting overlay. `pair` holds each bracket's **byte**
/// range (as `syntax::bracket_match` returns them, tree-sitter's own
/// byte-range currency), converted to char offsets here the same way
/// `paint_diagnostics` does, just without that function's `HashMap`
/// batching: there are only ever two ranges to convert for a single matched
/// pair, not a per-diagnostic-file count, so a direct
/// `text[..byte].chars().count()` call twice is simpler and just as cheap.
/// An outline (not `paint_occurrence_highlights`' filled rect) deliberately
/// reads as "these two characters pair up," not "this span is
/// selected/repeated" — a different visual vocabulary for a different kind
/// of highlight.
pub(super) fn paint_bracket_match(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    text: &str,
    pair: (Range<usize>, Range<usize>),
) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.0, theme::bracket_match(ui.visuals().dark_mode));

    for range in [pair.0, pair.1] {
        let char_start = text[..range.start].chars().count();
        let char_end = text[..range.end].chars().count();
        let start_rect = output.galley.pos_from_cursor(CCursor::new(char_start));
        let end_rect = output.galley.pos_from_cursor(CCursor::new(char_end));
        let rect = egui::Rect::from_min_max(
            egui::pos2(output.galley_pos.x + start_rect.left(), output.galley_pos.y + start_rect.top()),
            egui::pos2(output.galley_pos.x + end_rect.left(), output.galley_pos.y + end_rect.bottom()),
        );
        painter.rect_stroke(rect, 1.0, stroke, egui::StrokeKind::Inside);
    }
}

/// Paints a small dot for each space and a short arrow for each tab —
/// `ViewSettings::show_whitespace`. Walks `text` char by char rather than
/// reusing the range-based technique every other overlay here uses: there's
/// no contiguous span to paint, just individually-scattered single
/// characters, so a per-char `pos_from_cursor` call is the natural fit
/// (same cost class as `paint_indent_guides`'s per-level lookups below, not
/// the per-diagnostic-file count `paint_diagnostics` optimizes for).
pub(super) fn paint_whitespace(ui: &egui::Ui, output: &egui::text_edit::TextEditOutput, text: &str) {
    let painter = ui.painter();
    let color = theme::structure(ui.visuals().dark_mode);

    for (char_idx, c) in text.chars().enumerate() {
        match c {
            ' ' => {
                let rect = output.galley.pos_from_cursor(CCursor::new(char_idx));
                let center = egui::pos2(output.galley_pos.x + rect.center().x, output.galley_pos.y + rect.center().y);
                painter.circle_filled(center, 1.5, color);
            }
            '\t' => {
                let start = output.galley.pos_from_cursor(CCursor::new(char_idx));
                let end = output.galley.pos_from_cursor(CCursor::new(char_idx + 1));
                let y = output.galley_pos.y + start.center().y;
                let x_start = output.galley_pos.x + start.left() + 2.0;
                let x_end = (output.galley_pos.x + end.left() - 2.0).max(x_start + 2.0);
                painter.line_segment([egui::pos2(x_start, y), egui::pos2(x_end, y)], Stroke::new(1.0, color));
                painter.line_segment([egui::pos2(x_end, y), egui::pos2(x_end - 3.0, y - 3.0)], Stroke::new(1.0, color));
                painter.line_segment([egui::pos2(x_end, y), egui::pos2(x_end - 3.0, y + 3.0)], Stroke::new(1.0, color));
            }
            _ => {}
        }
    }
}

/// Paints a thin vertical line through every indent level a line's leading
/// whitespace spans — `ViewSettings::show_indent_guides`. Assumes each
/// `output.galley.rows` entry corresponds 1:1 to a logical (`\n`-separated)
/// line of `text`, the same simplifying assumption `paint_line_numbers`
/// already makes (both break down identically for a wrapped long line —
/// see that function's doc comment); `indent_settings` decides how many
/// leading whitespace characters make up one level, same as every other
/// indent-aware transform in `auto_edit.rs`.
pub(super) fn paint_indent_guides(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    text: &str,
    indent_settings: IndentSettings,
) {
    let painter = ui.painter();
    let color = theme::structure(ui.visuals().dark_mode);
    let unit_width = if indent_settings.use_tabs { 1 } else { indent_settings.width.max(1) };

    let mut char_offset = 0usize;
    for (row_index, line) in text.split('\n').enumerate() {
        let Some(row) = output.galley.rows.get(row_index) else { break };
        let leading_ws = line.chars().take_while(|&c| c == ' ' || c == '\t').count();
        let levels = leading_ws / unit_width;

        let y_top = output.galley_pos.y + row.rect().top();
        let y_bottom = output.galley_pos.y + row.rect().bottom();

        for level in 0..levels {
            let level_char = char_offset + level * unit_width;
            let pos = output.galley.pos_from_cursor(CCursor::new(level_char));
            let x = output.galley_pos.x + pos.left();
            painter.line_segment([egui::pos2(x, y_top), egui::pos2(x, y_bottom)], Stroke::new(1.0, color));
        }

        char_offset += line.chars().count() + 1; // +1 skips the '\n' separator itself
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ground truth for every case below: the same per-offset conversion
    /// `paint_diagnostics` used before this file's byte→char batching (see
    /// `TECHNICAL_DEBT.md`'s now-resolved entry on this function) — kept
    /// here purely as an oracle to check `char_offsets_for` against, not as
    /// production code.
    fn naive_char_offset(text: &str, byte_offset: usize) -> usize {
        text[..byte_offset].chars().count()
    }

    #[test]
    fn char_offsets_for_matches_naive_conversion_on_ascii() {
        let text = "abcde";
        let queries = vec![0, 2, 5];
        let offsets = char_offsets_for(text, &queries);

        for &q in &queries {
            assert_eq!(offsets[&q], naive_char_offset(text, q));
        }
    }

    #[test]
    fn char_offsets_for_matches_naive_conversion_across_multi_byte_chars() {
        // "h" (1 byte) + "é" (2 bytes, U+00E9) + "llo" (3 bytes) = 6 bytes,
        // 5 chars — byte offsets land mid-string on both sides of the
        // 2-byte character.
        let text = "héllo";
        let queries = vec![0, 1, 3, 4, 5, 6];
        let offsets = char_offsets_for(text, &queries);

        for &q in &queries {
            assert_eq!(
                offsets[&q],
                naive_char_offset(text, q),
                "byte offset {q} in {text:?} converted incorrectly"
            );
        }
        // Spot-check the interesting one directly: byte 3 sits right after
        // the 2-byte "é", so exactly 2 chars ("h", "é") precede it.
        assert_eq!(offsets[&3], 2);
    }

    #[test]
    fn char_offsets_for_handles_empty_text() {
        let offsets = char_offsets_for("", &[0]);
        assert_eq!(offsets[&0], 0);
    }
}
