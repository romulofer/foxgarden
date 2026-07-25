//! Read-only virtualized render path (PLAN.md Phase 2b): given a buffer and a
//! font, allocate a content-sized rect so the enclosing `ScrollArea`'s
//! scrollbar is correct, then shape and paint **only the rows the viewport
//! actually touches**. No caret, selection, or input yet — those are 2d–2f;
//! this is the piece that proves virtualization renders the right pixels.
//!
//! Lines are shaped as plain single-color text for this milestone; per-token
//! syntax highlighting slots in with the overlay/`char_rect` work (2c), where
//! the visible-range span slicing lives.

use std::ops::Range;
use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{Color32, FontId, Galley, Sense, TextFormat};
use ropey::Rope;

use super::{content_height, visible_rows, FoldMap};

/// What a virtualized editor frame produces for the overlay/interception code
/// to read — the hand-built analogue of `egui::text_edit::TextEditOutput`.
/// Grows a caret/selection field in 2d; for now it carries the geometry and
/// the per-visible-row galleys the overlays will position against.
pub struct TextAreaOutput {
    pub response: egui::Response,
    /// Uniform height of one row, in points.
    pub row_height: f32,
    /// Screen position of logical line 0, column 0 — already scroll-adjusted,
    /// so `content_origin.y + line * row_height` is a line's screen y.
    pub content_origin: egui::Pos2,
    /// The visual-row range shaped and painted this frame.
    pub visible_rows: Range<usize>,
    /// `(logical line index, shaped galley)` for each visible row, in render
    /// order — the overlays index into these for intra-line x positions.
    pub row_galleys: Vec<(usize, Arc<Galley>)>,
}

impl TextAreaOutput {
    /// Screen rect of the character at `char_offset` (a thin caret-width box
    /// one row tall), or `None` if that offset's line isn't shaped this frame
    /// (scrolled off, or hidden inside a fold). The virtualized replacement for
    /// the overlays' `galley.pos_from_cursor(CCursor::new(char))` idiom — an
    /// overlay maps each decoration's char offset through this and simply skips
    /// the `None`s, since there's nothing visible to decorate off-screen.
    pub fn char_rect(&self, buffer: &Rope, char_offset: usize) -> Option<egui::Rect> {
        let clamped = char_offset.min(buffer.len_chars());
        let line = buffer.char_to_line(clamped);
        let col = clamped - buffer.line_to_char(line);
        let idx = self.row_galleys.iter().position(|(l, _)| *l == line)?;
        let galley = &self.row_galleys[idx].1;
        let visual_row = self.visible_rows.start + idx;
        let y = self.content_origin.y + visual_row as f32 * self.row_height;
        let local = galley.pos_from_cursor(egui::text::CCursor::new(col));
        let x = self.content_origin.x + local.left();
        Some(egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(1.0, self.row_height)))
    }
}

/// Paints `buffer` read-only into the current (scroll-area) `ui`, virtualized:
/// only rows inside `ui.clip_rect()` are shaped. `hidden` is the fold set
/// (empty in Phase 2 — no folding yet).
pub fn show_readonly(
    ui: &mut egui::Ui,
    buffer: &Rope,
    font_id: FontId,
    text_color: Color32,
    hidden: &[Range<usize>],
) -> TextAreaOutput {
    let row_height = ui.fonts_mut(|f| f.row_height(&font_id));
    let total_lines = buffer.len_lines().max(1);
    let map = FoldMap::new(hidden);
    let total_rows = map.visual_count(total_lines);

    // Reserve the full virtual extent so the ScrollArea scrolls the whole
    // buffer even though only a slice is painted. Width is the viewport width
    // for now (real max-line-width horizontal scroll is a later refinement);
    // `desired_width`-style infinite growth isn't needed since we paint
    // absolutely, not via layout.
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, content_height(total_rows, row_height)), Sense::click_and_drag());
    let content_origin = rect.min;

    // How far the content's top has scrolled above the viewport's top.
    let clip = ui.clip_rect();
    let scroll_y = (clip.top() - rect.top()).max(0.0);
    let rows = visible_rows(scroll_y, clip.height(), row_height, total_rows);

    let painter = ui.painter();
    let mut row_galleys = Vec::with_capacity(rows.len());
    for visual_row in rows.clone() {
        let logical = map.to_logical(visual_row);
        let galley = shape_line(ui, buffer, logical, &font_id, text_color);
        let pos = egui::pos2(content_origin.x, content_origin.y + visual_row as f32 * row_height);
        painter.galley(pos, galley.clone(), text_color);
        row_galleys.push((logical, galley));
    }

    TextAreaOutput { response, row_height, content_origin, visible_rows: rows, row_galleys }
}

/// Shapes one logical line (its trailing newline stripped, so it stays a
/// single row) into a plain-format galley.
fn shape_line(ui: &egui::Ui, buffer: &Rope, logical: usize, font_id: &FontId, color: Color32) -> Arc<Galley> {
    let raw = buffer.line(logical.min(buffer.len_lines().saturating_sub(1)));
    let text: String = raw.chars().filter(|&c| c != '\n' && c != '\r').collect();
    let mut job = LayoutJob::single_section(text, TextFormat { font_id: font_id.clone(), color, ..Default::default() });
    job.wrap.max_width = f32::INFINITY; // no wrap in v1 — uniform row height
    ui.fonts_mut(|f| f.layout_job(job))
}
