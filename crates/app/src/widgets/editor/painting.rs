use std::collections::HashMap;
use std::ops::Range;

use egui::text::CCursor;
use egui::{Align2, Color32, FontId, Shape, Stroke};
use fg_core::{BlameLine, Diagnostic};
use ropey::Rope;

use super::text_area::TextAreaOutput;
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
    for (char_count, (byte_idx, _)) in text
        .char_indices()
        .chain(std::iter::once((text.len(), '\0')))
        .enumerate()
    {
        while qi < queries.len() && queries[qi] == byte_idx {
            result.insert(byte_idx, char_count);
            qi += 1;
        }
    }
    result
}

/// A diagnostic's start/end char offset resolves to `None` exactly when
/// that offset's line isn't among this frame's shaped rows (`TextAreaOutput::
/// char_rect`'s own off-screen signal) — every overlay in this file skips
/// painting a decoration whose position isn't currently visible, rather than
/// forcing a shape of an off-screen row just to decorate it. That's the
/// point of the virtualized render this file's overlays now sit on top of.
pub(super) fn paint_diagnostics(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    buffer: &Rope,
    text: &str,
    diagnostics: &[&Diagnostic],
) {
    if diagnostics.is_empty() {
        return;
    }

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
        let diag = diagnostics[i];
        let char_start = char_offset_for[&start];
        let char_end = char_offset_for[&end].max(char_start + 1);

        let Some(start_rect) = out.char_rect(buffer, char_start) else {
            continue;
        };
        let x_end = out
            .char_rect(buffer, char_end)
            .map_or(start_rect.left() + 4.0, |r| r.left())
            .max(start_rect.left() + 4.0);
        let (x_start, top, y) = (start_rect.left(), start_rect.top(), start_rect.bottom());

        paint_squiggle(painter, y, x_start, x_end, squiggle_color);

        // Sense::hover() only — this must not steal clicks/drags from the
        // editor underneath, just report when the pointer is sitting over
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

/// A translucent full-width band over the debuggee's current paused line
/// (`PLAN.md` Track 23 Phase 2) — deliberately *not* built on
/// `paint_occurrence_highlights` (span-width, sized to matched text) or
/// `paint_blame_annotation` (draws text past the line's own content); this
/// is the one painter in this module that fills an entire row regardless
/// of how much of it the line's own galley actually covers, using `out.
/// response.rect`'s own left/right for the full editor content width
/// rather than a glyph-derived one. A no-op if `line` isn't among this
/// frame's shaped rows (scrolled out of view).
pub(super) fn paint_paused_line_highlight(ui: &egui::Ui, out: &TextAreaOutput, line: usize, dark_mode: bool) {
    let Some(i) = out.row_galleys.iter().position(|(logical, _)| *logical == line) else { return };
    let y = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
    let rect = egui::Rect::from_min_max(
        egui::pos2(out.response.rect.left(), y),
        egui::pos2(out.response.rect.right(), y + out.row_height),
    );
    ui.painter().rect_filled(rect, 0.0, theme::debug_current_line(dark_mode));
}

/// Paints one right-aligned line number per row `out` actually shaped this
/// frame, flush against `gutter_right_edge` — virtualized the same way the
/// text itself is, rather than iterating a whole-buffer galley's rows.
pub(super) fn paint_line_numbers(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    gutter_right_edge: f32,
    font_id: FontId,
    dark_mode: bool,
) {
    let painter = ui.painter();
    let color = theme::line_number(dark_mode);
    for (i, (logical, _)) in out.row_galleys.iter().enumerate() {
        let y = out.content_origin.y + (out.row_offsets[i] as f32 + 0.5) * out.row_height;
        painter.text(
            egui::pos2(gutter_right_edge, y),
            Align2::RIGHT_CENTER,
            logical + 1,
            font_id.clone(),
            color,
        );
    }
}

/// A blame-porcelain all-zero sha marks a line that only exists in the
/// working tree, not any real commit (`fg_core::blame`'s own doc comment) —
/// git's own generated `summary`/`author` for that case ("Version of X from
/// X", "Not Committed Yet") is accurate but reads as clutter next to every
/// other, real commit's summary, so it gets a plain, deliberately shorter
/// label instead.
const UNCOMMITTED_SHA: &str = "0000000000000000000000000000000000000000";

/// One line's worth of blame, formatted for the cursor-line annotation:
/// `"<author> • <relative time> • <summary>"`, or a short "uncommitted"
/// label for a line with local, not-yet-committed edits (see
/// `UNCOMMITTED_SHA`).
pub(super) fn blame_annotation_text(line: &BlameLine, now_unix: i64) -> String {
    if line.sha == UNCOMMITTED_SHA {
        return "Uncommitted change".to_string();
    }
    format!("{} • {} • {}", line.author, relative_time(now_unix, line.author_time), line.summary)
}

/// A coarse, bucketed "how long ago" string — no date/time dependency
/// needed for this, just integer division on the two already-Unix-second
/// timestamps `git blame --porcelain`'s own `author-time` and `SystemTime::
/// now()` give. `then_unix` in the future (a clock skew edge case, not a
/// real one for a commit that's already landed) clamps to "just now" rather
/// than printing a negative duration.
pub(super) fn relative_time(now_unix: i64, then_unix: i64) -> String {
    let secs = (now_unix - then_unix).max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else if secs < 30 * 86_400 {
        format!("{}d ago", secs / 86_400)
    } else if secs < 365 * 86_400 {
        format!("{}mo ago", secs / (30 * 86_400))
    } else {
        format!("{}y ago", secs / (365 * 86_400))
    }
}

/// Paints a dimmed blame annotation just past the end of `cursor_line`'s own
/// rendered text (`PLAN.md` Track 9 Phase 2) — the same `theme::line_number`
/// color the gutter's own line numbers use, since that's already this
/// palette's dimmest text-like color and an inline annotation shouldn't
/// outcompete the code itself for attention. A no-op when `cursor_line`
/// isn't currently shaped (out of the virtualized viewport, or past the end
/// of `blame` — e.g. before the first scan has completed).
pub(super) fn paint_blame_annotation(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    blame: &[BlameLine],
    cursor_line: usize,
    now_unix: i64,
    font_id: FontId,
    dark_mode: bool,
) {
    let Some(line) = blame.get(cursor_line) else { return };
    let Some(i) = out.row_galleys.iter().position(|(logical, _)| *logical == cursor_line) else { return };
    let (_, galley) = &out.row_galleys[i];
    let y = out.content_origin.y + (out.row_offsets[i] as f32 + 0.5) * out.row_height;
    let x = out.content_origin.x + galley.rect.width() + BLAME_ANNOTATION_PADDING;
    ui.painter().text(
        egui::pos2(x, y),
        Align2::LEFT_CENTER,
        blame_annotation_text(line, now_unix),
        font_id,
        theme::line_number(dark_mode),
    );
}

/// Horizontal gap between a line's own last character and its blame
/// annotation — enough to read as a separate, secondary piece of text
/// rather than a continuation of the code itself.
const BLAME_ANNOTATION_PADDING: f32 = 24.0;

/// Paints the Ctrl+D secondary cursors/selections: a thin caret for a bare
/// position, or a translucent rect for a claimed occurrence.
pub(super) fn paint_extra_selections(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    buffer: &Rope,
    extra_selections: &[Range<usize>],
) {
    let painter = ui.painter();
    let caret_color = ui.visuals().text_cursor.stroke.color;
    let selection_color = ui.visuals().selection.bg_fill;

    for range in extra_selections {
        let Some(start_rect) = out.char_rect(buffer, range.start) else {
            continue;
        };
        let (y_top, y_bottom, x_start) = (start_rect.top(), start_rect.bottom(), start_rect.left());

        if range.is_empty() {
            painter.line_segment(
                [egui::pos2(x_start, y_top), egui::pos2(x_start, y_bottom)],
                Stroke::new(1.5, caret_color),
            );
        } else if let Some(end_rect) = out.char_rect(buffer, range.end) {
            let x_end = end_rect.left();
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x_start, y_top), egui::pos2(x_end, y_bottom)),
                0.0,
                selection_color,
            );
        }
    }
}

/// Paints a subtle background rect behind every occurrence of the word
/// under the cursor (passive, read-only), including the one the cursor
/// itself sits on/in. Deliberately a much less prominent fill than
/// `paint_extra_selections` so it never reads as an active selection.
pub(super) fn paint_occurrence_highlights(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    buffer: &Rope,
    occurrences: &[Range<usize>],
) {
    let painter = ui.painter();
    let fill = theme::occurrence_highlight(ui.visuals().dark_mode);

    for range in occurrences {
        let (Some(start_rect), Some(end_rect)) = (out.char_rect(buffer, range.start), out.char_rect(buffer, range.end))
        else {
            continue;
        };
        let rect = egui::Rect::from_min_max(
            egui::pos2(start_rect.left(), start_rect.top()),
            egui::pos2(end_rect.left(), end_rect.bottom()),
        );
        painter.rect_filled(rect, 2.0, fill);
    }
}

/// Paints a subtle outline box around each bracket of a matched pair —
/// `pair` holds each bracket's **byte** range (as `syntax::bracket_match`
/// returns them), converted to char offsets via `buffer.byte_to_char`
/// (`Rope`'s own O(log n) lookup — no need for `paint_diagnostics`'
/// batching machinery for just two ranges). An outline (not
/// `paint_occurrence_highlights`' filled rect) deliberately reads as "these
/// two characters pair up," not "this span is selected/repeated."
pub(super) fn paint_bracket_match(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    buffer: &Rope,
    pair: (Range<usize>, Range<usize>),
) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.0, theme::bracket_match(ui.visuals().dark_mode));

    for range in [pair.0, pair.1] {
        let char_start = buffer.byte_to_char(range.start.min(buffer.len_bytes()));
        let char_end = buffer.byte_to_char(range.end.min(buffer.len_bytes()));
        let (Some(start_rect), Some(end_rect)) = (out.char_rect(buffer, char_start), out.char_rect(buffer, char_end))
        else {
            continue;
        };
        let rect = egui::Rect::from_min_max(
            egui::pos2(start_rect.left(), start_rect.top()),
            egui::pos2(end_rect.left(), end_rect.bottom()),
        );
        painter.rect_stroke(rect, 1.0, stroke, egui::StrokeKind::Inside);
    }
}

/// Paints a small dot for each space and a short arrow for each tab —
/// `ViewSettings::show_whitespace`. Walks only the rows `out` actually
/// shaped this frame (each row's own logical line, read straight from
/// `buffer`) rather than every char in the whole buffer the way the
/// `egui::TextEdit`-era version had to (that version had only one
/// whole-buffer galley to query positions from in the first place) — a
/// genuine virtualization win for this specific overlay on a large file.
pub(super) fn paint_whitespace(ui: &egui::Ui, out: &TextAreaOutput, buffer: &Rope) {
    let painter = ui.painter();
    let color = theme::structure(ui.visuals().dark_mode);

    for (i, (logical, galley)) in out.row_galleys.iter().enumerate() {
        let y = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
        let line_start = buffer.line_to_char(*logical);
        let line = buffer.line(*logical);

        for (col, c) in line.chars().enumerate() {
            match c {
                ' ' => {
                    let rect = galley.pos_from_cursor(CCursor::new(col));
                    let center = egui::pos2(out.content_origin.x + rect.center().x, y + rect.center().y);
                    painter.circle_filled(center, 1.5, color);
                }
                '\t' => {
                    let start = galley.pos_from_cursor(CCursor::new(col));
                    let end = galley.pos_from_cursor(CCursor::new(col + 1));
                    let row_center_y = y + start.center().y;
                    let x_start = out.content_origin.x + start.left() + 2.0;
                    let x_end = (out.content_origin.x + end.left() - 2.0).max(x_start + 2.0);
                    painter.line_segment(
                        [egui::pos2(x_start, row_center_y), egui::pos2(x_end, row_center_y)],
                        Stroke::new(1.0, color),
                    );
                    painter.line_segment(
                        [
                            egui::pos2(x_end, row_center_y),
                            egui::pos2(x_end - 3.0, row_center_y - 3.0),
                        ],
                        Stroke::new(1.0, color),
                    );
                    painter.line_segment(
                        [
                            egui::pos2(x_end, row_center_y),
                            egui::pos2(x_end - 3.0, row_center_y + 3.0),
                        ],
                        Stroke::new(1.0, color),
                    );
                }
                '\n' | '\r' => break, // the line slice includes its own terminator
                _ => {}
            }
        }
        let _ = line_start; // kept for symmetry with paint_indent_guides's per-row lookups; not otherwise needed here
    }
}

/// Paints a thin vertical line through every indent level a line's leading
/// whitespace spans — `ViewSettings::show_indent_guides`. Iterates the rows
/// `out` actually shaped (its own logical line's leading whitespace, read
/// directly from `buffer`) instead of walking `text` char-by-char to
/// rediscover each row's line — a row **is** a logical line while word-wrap
/// is off (Phase 2/2h's scope; Phase 4 revisits this for wrapped rows).
pub(super) fn paint_indent_guides(ui: &egui::Ui, out: &TextAreaOutput, buffer: &Rope, indent_settings: IndentSettings) {
    let painter = ui.painter();
    let color = theme::structure(ui.visuals().dark_mode);
    let unit_width = if indent_settings.use_tabs {
        1
    } else {
        indent_settings.width.max(1)
    };

    for (i, (logical, galley)) in out.row_galleys.iter().enumerate() {
        let y_top = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
        let y_bottom = y_top + out.row_height;

        let leading_ws = buffer
            .line(*logical)
            .chars()
            .take_while(|&c| c == ' ' || c == '\t')
            .count();
        let levels = leading_ws / unit_width;

        for level in 0..levels {
            let level_char = level * unit_width;
            let pos = galley.pos_from_cursor(CCursor::new(level_char));
            let x = out.content_origin.x + pos.left();
            painter.line_segment([egui::pos2(x, y_top), egui::pos2(x, y_bottom)], Stroke::new(1.0, color));
        }
    }
}

/// Paints sticky scroll: the `header_lines` text pinned as opaque bands at the
/// top of the editor viewport, outermost-first top-to-bottom, so the enclosing
/// class/method signatures stay visible while their body scrolls underneath.
///
/// Painted in **screen space off `ui.clip_rect().top()`** — a fixed viewport
/// pixel, deliberately *not* `out.content_origin.y` (which scrolls) — so the
/// bands stay put as the body moves under them. Each band is opaque
/// (`theme::sticky_background`) precisely to occlude that scrolling body; a
/// thin bottom line separates the whole stack from the live content below.
///
/// Header text is drawn in the plain editor text color rather than
/// syntax-highlighted for this first cut: re-deriving per-token colors here
/// would mean a whole-file `highlight_spans` pass every frame sticky scroll is
/// active, which isn't worth it for a handful of pinned signature lines — a
/// colored pinned header is a clear later refinement, not a correctness gap.
pub(super) fn paint_sticky_scroll(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    header_lines: &[String],
    font_id: FontId,
    dark_mode: bool,
) {
    if header_lines.is_empty() {
        return;
    }
    let row_height = out.row_height;

    let painter = ui.painter();
    let clip = ui.clip_rect();
    let band_bg = theme::sticky_background(dark_mode);
    let text_color = theme::default_text(dark_mode);
    // Keep the signature readable even when the body is scrolled right: clamp
    // the text's left edge into the viewport rather than letting it slide off
    // with the content's own x origin.
    let text_left = out.content_origin.x.max(clip.left() + 2.0);

    for (i, line) in header_lines.iter().enumerate() {
        let band_top = clip.top() + i as f32 * row_height;
        let band = egui::Rect::from_min_max(
            egui::pos2(clip.left(), band_top),
            egui::pos2(clip.right(), band_top + row_height),
        );
        painter.rect_filled(band, 0.0, band_bg);
        painter.text(
            egui::pos2(text_left, band_top),
            Align2::LEFT_TOP,
            line.trim_end_matches(['\n', '\r']),
            font_id.clone(),
            text_color,
        );
    }

    // One separator line under the whole pinned stack.
    let divider_y = clip.top() + header_lines.len() as f32 * row_height;
    painter.line_segment(
        [egui::pos2(clip.left(), divider_y), egui::pos2(clip.right(), divider_y)],
        Stroke::new(1.0, theme::structure(dark_mode)),
    );
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

    #[test]
    fn relative_time_buckets_span_seconds_to_years() {
        assert_eq!(relative_time(1000, 990), "just now");
        assert_eq!(relative_time(1000, 400), "10m ago");
        assert_eq!(relative_time(10_000, 3_600), "1h ago");
        assert_eq!(relative_time(200_000, 100_000), "1d ago");
        assert_eq!(relative_time(10_000_000, 5_000_000), "1mo ago");
        assert_eq!(relative_time(100_000_000, 10_000_000), "2y ago");
    }

    #[test]
    fn relative_time_clamps_a_timestamp_in_the_future_to_just_now() {
        assert_eq!(relative_time(1000, 2000), "just now");
    }

    fn line(sha: &str, author: &str, author_time: i64, summary: &str) -> BlameLine {
        BlameLine { sha: sha.to_string(), author: author.to_string(), author_time, summary: summary.to_string() }
    }

    #[test]
    fn blame_annotation_text_joins_author_relative_time_and_summary() {
        let l = line("abc123abc123abc123abc123abc123abc123abcd", "Ada", 400, "Fix the thing");
        assert_eq!(blame_annotation_text(&l, 1000), "Ada • 10m ago • Fix the thing");
    }

    #[test]
    fn blame_annotation_text_shows_a_short_label_for_an_uncommitted_line() {
        let l = line(UNCOMMITTED_SHA, "Not Committed Yet", 900, "Version of f.txt from f.txt");
        assert_eq!(blame_annotation_text(&l, 1000), "Uncommitted change");
    }
}
