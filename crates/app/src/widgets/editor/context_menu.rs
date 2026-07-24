//! The editor's right-click context menu: the common actions an
//! IntelliJ-style menu offers (Undo, Redo, Cut, Copy, Paste, Select All,
//! Toggle Line Comment, Duplicate Line, Save), reachable without the menu
//! bar or a keyboard shortcut.

use egui::text::{CCursor, CCursorRange};
use egui::{Event, Key, Modifiers};
use fg_core::Document;
use syntax::IncrementalParser;

use super::auto_edit::{duplicate_line, toggle_line_comments};
use super::text_offset::char_to_byte;
use super::widget::apply_edit;

/// Builds a `Ctrl`(+`Shift`)+`key` press event, for the Undo/Redo/Select
/// All items to queue into `pending_input` — they can't act directly (see
/// `widget::show`'s doc comment on `pending_input`), so a real
/// keyboard-shaped event is what stands in for the click. `pub(super)`
/// rather than private: `widget`'s own tests build the exact same shape of
/// event to simulate what a menu click would queue (see
/// `queued_pending_input_is_drained_as_real_input_before_text_edit_runs`).
pub(super) fn synthetic_shortcut(key: Key, shift: bool) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers { command: true, shift, ..Modifiers::NONE },
    }
}

/// Attaches the context menu to `response` (the editor `TextEdit`'s own
/// response) and handles every item's click. `text`/`old_text`/
/// `manual_cursor_range` are the same locals `widget::show` threads
/// through every other edit path, so an edit made here flows through the
/// identical apply-cursor-at-the-end machinery; `pending_input` is drained
/// back into real input at the top of the *next* frame (see `widget::show`)
/// for the three items egui's own `TextEdit` has to handle itself.
#[expect(clippy::too_many_arguments, reason = "each parameter is independently threaded editor-frame state, not a bundle waiting to be a struct — see widget::show's own too-many-arguments allowance for the same shape")]
pub(super) fn show_context_menu(
    response: &egui::Response,
    widget_id: egui::Id,
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    primary_cursor_range: Option<CCursorRange>,
    text: &mut String,
    old_text: &mut String,
    manual_cursor_range: &mut Option<CCursorRange>,
    pending_input: &mut Vec<Event>,
    last_error: &mut Option<String>,
) {
    let has_selection = primary_cursor_range.is_some_and(|r| !r.is_empty());
    let cursor_char = primary_cursor_range.map(|r| r.primary.index.0);

    response.context_menu(|ui| {
        if ui.button("Undo").clicked() {
            pending_input.push(synthetic_shortcut(Key::Z, false));
            ui.memory_mut(|mem| mem.request_focus(widget_id));
            ui.close();
        }
        if ui.button("Redo").clicked() {
            pending_input.push(synthetic_shortcut(Key::Z, true));
            ui.memory_mut(|mem| mem.request_focus(widget_id));
            ui.close();
        }

        ui.separator();

        if ui.add_enabled(has_selection, egui::Button::new("Cut")).clicked() {
            if let Some(range) = primary_cursor_range.map(|r| r.as_sorted_char_range()) {
                let start = char_to_byte(text, range.start.0);
                let end = char_to_byte(text, range.end.0);
                ui.ctx().copy_text(text[start..end].to_string());
                let new_text = format!("{}{}", &text[..start], &text[end..]);
                apply_edit(doc, parser, text, &new_text);
                *manual_cursor_range = Some(CCursorRange::one(CCursor::new(range.start.0)));
                *old_text = new_text.clone();
                *text = new_text;
            }
            ui.close();
        }
        if ui.add_enabled(has_selection, egui::Button::new("Copy")).clicked() {
            if let Some(range) = primary_cursor_range.map(|r| r.as_sorted_char_range()) {
                let start = char_to_byte(text, range.start.0);
                let end = char_to_byte(text, range.end.0);
                ui.ctx().copy_text(text[start..end].to_string());
            }
            ui.close();
        }
        // Read fresh, only while the menu is actually open — egui has no
        // public API to read the OS clipboard (only `Context::copy_text`
        // to write it), so this is the one item here that needs `arboard`
        // directly rather than something already exposed by egui.
        let clipboard_text =
            arboard::Clipboard::new().and_then(|mut cb| cb.get_text()).ok().filter(|s| !s.is_empty());
        if ui.add_enabled(clipboard_text.is_some(), egui::Button::new("Paste")).clicked() {
            if let (Some(pasted), Some(range)) = (&clipboard_text, primary_cursor_range.map(|r| r.as_sorted_char_range()))
            {
                let start = char_to_byte(text, range.start.0);
                let end = char_to_byte(text, range.end.0);
                let new_text = format!("{}{pasted}{}", &text[..start], &text[end..]);
                let new_cursor = range.start.0 + pasted.chars().count();
                apply_edit(doc, parser, text, &new_text);
                *manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
                *old_text = new_text.clone();
                *text = new_text;
            }
            ui.close();
        }
        if ui.button("Select All").clicked() {
            pending_input.push(synthetic_shortcut(Key::A, false));
            ui.memory_mut(|mem| mem.request_focus(widget_id));
            ui.close();
        }

        ui.separator();

        if ui.button("Toggle Line Comment").clicked() {
            if let Some(range) = primary_cursor_range.map(|r| r.as_sorted_char_range()) {
                let (commented, new_start, new_end) = toggle_line_comments(text, range.start.0, range.end.0);
                apply_edit(doc, parser, text, &commented);
                *manual_cursor_range = Some(CCursorRange::two(CCursor::new(new_start), CCursor::new(new_end)));
                *old_text = commented.clone();
                *text = commented;
            }
            ui.close();
        }
        if ui.button("Duplicate Line").clicked() {
            if let Some(cursor_char) = cursor_char {
                let (duplicated, new_cursor) = duplicate_line(text, cursor_char);
                apply_edit(doc, parser, text, &duplicated);
                *manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
                *old_text = duplicated.clone();
                *text = duplicated;
            }
            ui.close();
        }

        ui.separator();

        if ui.add_enabled(doc.is_dirty(), egui::Button::new("Save")).clicked() {
            crate::panels::tabs::save_document(doc, parser, last_error);
            // `Document::save` trims trailing whitespace, which can change
            // `doc.buffer` out from under `text`/`old_text` — every other
            // branch in `widget::show` refreshes both right after an edit
            // for exactly this reason (see `apply_edit`'s doc comment);
            // Save needs the same treatment, or the rest of *this* frame
            // (the trailing `manual_cursor_range` apply) would keep working
            // off pre-trim content while `doc.buffer` has already moved on.
            let saved_text = doc.buffer.to_string();
            *old_text = saved_text.clone();
            *text = saved_text;
            ui.close();
        }
    });
}
