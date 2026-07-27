//! Cell-grid rendering for the terminal panel's active session (`PLAN.md`
//! terminal-panel track, Phases 7 and 9). Reads the `vt100::Screen`
//! `PtySession::screen()` already parses out of the raw pty byte stream once
//! per frame and paints it as a monospace grid using the editor's own
//! `EditorFont`/`font_size` — one `egui::Galley` per row, mirroring
//! `text_area::render::shape_range`/`paint_rows`'s own "shape a `LayoutJob`,
//! paint the whole galley in one `painter.galley()` call" precedent rather
//! than a slower per-glyph `painter.text()` loop. Before painting, also
//! recomputes rows/cols from the available rect and resizes the session to
//! match (`grid_size`, `SPEC.md` §8.6) — so a font-size change, side-panel
//! drag, or window resize all keep the pty (and the shell/full-screen
//! program running inside it) in sync with what's actually on screen.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Stroke};

use crate::pty_session::PtySession;
use crate::style::fonts::EditorFont;
use crate::style::theme;

/// Paints `session`'s current screen and returns the interactive response
/// for the whole grid, registered under the caller's own stable `id` (same
/// "`ui.interact` with a caller-supplied `Id` rather than `allocate_exact_
/// size`'s auto-generated one" reasoning as `text_area::render::
/// layout_visible` — `panels::terminal_panel` needs to check this exact
/// response's focus state independently of whether it calls `show` again
/// this frame). `last_interaction`/`cursor_blink` drive the cursor's blink
/// the same way `text_area::shell::caret_visible` drives the editor's own
/// caret — see `caret_visible` below, a direct port of that function
/// (private to `shell.rs`, so not reachable from here) rather than a second,
/// differently-tuned timer. `focused` is the caller's own already-known
/// focus state (checked *before* calling this, since it decides on-screen
/// via memory that this call itself only updates afterward) — whether to
/// paint the cursor at all.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    id: egui::Id,
    session: &mut PtySession,
    editor_font: EditorFont,
    font_size: f32,
    dark_mode: bool,
    last_interaction: f64,
    cursor_blink: bool,
    focused: bool,
) -> egui::Response {
    let font_id = FontId::new(font_size, editor_font.family());
    let row_height = ui.fonts_mut(|f| f.row_height(&font_id));
    // JetBrains Mono (and `FontFamily::Monospace`) is fixed-width, so any
    // glyph's width doubles as the grid's column width — same reasoning
    // `widget.rs`'s own gutter sizing already relies on for digit width.
    let col_width = ui.fonts_mut(|f| f.glyph_width(&font_id, ' '));

    // Recomputed every frame (cheap — pure arithmetic) rather than only on
    // a resize event, since there's no single "the panel resized" signal
    // available here: a font-size change, a side-panel drag, or a window
    // resize all end up changing `ui.available_size()` without a distinct
    // event of their own to hook (`PLAN.md` Phase 9, `SPEC.md` §8.6).
    // `session.resize` itself is a no-op-if-unchanged check away (see its
    // own doc comment), so this costs nothing on the common case where
    // nothing actually changed since last frame.
    let (desired_rows, desired_cols) = grid_size(ui.available_size(), row_height, col_width);
    if session.screen().size() != (desired_rows, desired_cols) {
        session.resize(desired_rows, desired_cols);
    }

    let screen = session.screen();
    let (rows, cols) = screen.size();

    let desired_size = egui::vec2(col_width * f32::from(cols), row_height * f32::from(rows));
    let (_, rect) = ui.allocate_space(desired_size);
    let response = ui.interact(rect, id, egui::Sense::click());
    let painter = ui.painter_at(rect);

    // A concrete stand-in for "the panel's own background" — only needed
    // when an inverse-video cell has no explicit background of its own to
    // swap into the foreground slot (`shape_row`'s own doc comment).
    let default_bg = ui.visuals().panel_fill;

    for row in 0..rows {
        let job = shape_row(screen, row, cols, &font_id, dark_mode, default_bg);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let pos = egui::pos2(rect.left(), rect.top() + f32::from(row) * row_height);
        painter.galley(pos, galley, theme::default_text(dark_mode));
    }

    if focused && !screen.hide_cursor() && caret_visible(ui, last_interaction, cursor_blink) {
        let (cursor_row, cursor_col) = screen.cursor_position();
        let cursor_pos = egui::pos2(
            rect.left() + f32::from(cursor_col) * col_width,
            rect.top() + f32::from(cursor_row) * row_height,
        );
        // A block cursor (not the editor's own thin bar) — the shape every
        // real terminal emulator uses, and distinct enough at a glance that
        // a focused terminal session doesn't get mistaken for a focused
        // editor tab above it.
        painter.rect_filled(
            egui::Rect::from_min_size(cursor_pos, egui::vec2(col_width, row_height)),
            0.0,
            ui.visuals().text_cursor.stroke.color.gamma_multiply(0.5),
        );
    }

    response
}

/// The rows/cols a `(width, height)` rect fits at the given glyph metrics —
/// pure arithmetic, no `egui::Ui` dependency, so it's headlessly testable
/// against a known rect/font-metric pair rather than "eyeballing a live
/// resize" (`PLAN.md` Phase 9, `SPEC.md` §8.6's own explicit "treat a
/// resize as a real, testable event" requirement). Floors rather than
/// rounds — a partial row/column can't actually fit a full cell — and
/// floors to at least 1 each, since a session can never legitimately have
/// zero rows or columns.
fn grid_size(available: egui::Vec2, row_height: f32, col_width: f32) -> (u16, u16) {
    let rows = (available.y / row_height).floor().max(1.0) as u16;
    let cols = (available.x / col_width).floor().max(1.0) as u16;
    (rows, cols)
}

/// Shapes one screen row into a `LayoutJob`: one `TextFormat` section per
/// run of contiguous cells sharing the same resolved foreground/background/
/// italic/underline (mirrors `shape_line`'s own "one section per contiguous
/// same-color span" shape, just grouped by cell attributes instead of
/// syntax-highlight spans). A cell with no explicit background renders
/// `Color32::TRANSPARENT` (`theme::terminal_bg`) — except under `inverse()`,
/// which needs a *real* color to swap into the foreground slot, hence
/// `default_bg`.
fn shape_row(
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    font_id: &FontId,
    dark_mode: bool,
    default_bg: Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut run_text = String::new();
    let mut run_format: Option<TextFormat> = None;

    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        if cell.is_wide_continuation() {
            // Its glyph was already emitted by the preceding wide cell.
            continue;
        }

        let mut fg = theme::terminal_fg(cell.fgcolor(), cell.bold(), dark_mode);
        let mut bg = theme::terminal_bg(cell.bgcolor(), dark_mode);
        if cell.inverse() {
            let concrete_bg = if bg == Color32::TRANSPARENT { default_bg } else { bg };
            (fg, bg) = (concrete_bg, fg);
        }

        let format = TextFormat {
            font_id: font_id.clone(),
            color: fg,
            background: bg,
            italics: cell.italic(),
            underline: if cell.underline() { Stroke::new(1.0, fg) } else { Stroke::NONE },
            ..Default::default()
        };
        let text = if cell.has_contents() { cell.contents() } else { " ".to_string() };

        match &run_format {
            Some(current) if *current == format => run_text.push_str(&text),
            _ => {
                if let Some(f) = run_format.take() {
                    job.append(&run_text, 0.0, f);
                }
                run_text = text;
                run_format = Some(format);
            }
        }
    }
    if let Some(f) = run_format {
        job.append(&run_text, 0.0, f);
    }

    job
}

/// Direct port of `text_area::shell::caret_visible` (private to that module,
/// so not reachable from here) — same blink-cycle math against `Visuals::
/// text_cursor`'s `on_duration`/`off_duration`, so the terminal's cursor
/// blinks identically to the editor's own caret rather than a second,
/// differently-tuned animation.
fn caret_visible(ui: &egui::Ui, last_interaction: f64, cursor_blink: bool) -> bool {
    if !cursor_blink || !ui.visuals().text_cursor.blink || !ui.input(|i| i.focused) {
        return true;
    }

    let on_duration = ui.visuals().text_cursor.on_duration;
    let off_duration = ui.visuals().text_cursor.off_duration;
    let total_duration = on_duration + off_duration;
    if total_duration <= 0.0 {
        return true;
    }

    let now = ui.input(|i| i.time);
    let time_since_interaction = (now - last_interaction).max(0.0);
    let time_in_cycle = (time_since_interaction % total_duration as f64) as f32;

    let (visible, wake_in) = if time_in_cycle < on_duration {
        (true, on_duration - time_in_cycle)
    } else {
        (false, total_duration - time_in_cycle)
    };
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs_f32(wake_in.max(0.0)));
    visible
}

#[cfg(test)]
mod grid_size_tests {
    use super::*;

    #[test]
    fn exact_multiple_fits_with_no_remainder() {
        assert_eq!(grid_size(egui::vec2(800.0, 480.0), 20.0, 8.0), (24, 100));
    }

    #[test]
    fn a_partial_trailing_row_or_column_is_floored_away() {
        // 479px / 20px-tall rows only fully fits 23, not 24 — the 24th
        // row would be clipped, so it shouldn't count as fitting.
        assert_eq!(grid_size(egui::vec2(799.0, 479.0), 20.0, 8.0), (23, 99));
    }

    #[test]
    fn a_rect_smaller_than_one_cell_still_floors_to_a_single_row_and_column() {
        assert_eq!(grid_size(egui::vec2(2.0, 2.0), 20.0, 8.0), (1, 1));
    }

    #[test]
    fn a_wider_font_yields_fewer_columns_for_the_same_width() {
        assert_eq!(grid_size(egui::vec2(800.0, 480.0), 20.0, 16.0), (24, 50));
    }
}
