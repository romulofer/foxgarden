use std::ops::Range;

use egui::text::CCursor;
use egui::{Color32, Shape, Stroke};
use fg_core::Diagnostic;

use crate::style::theme;

pub(super) fn paint_diagnostics(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    text: &str,
    diagnostics: &[Diagnostic],
) {
    let painter = ui.painter();
    let squiggle_color = theme::error_squiggle(ui.visuals().dark_mode);
    for diag in diagnostics {
        let start = diag.range.start.min(text.len());
        let end = diag.range.end.min(text.len()).max(start);
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }

        let char_start = text[..start].chars().count();
        let char_end = text[..end].chars().count().max(char_start + 1);

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
