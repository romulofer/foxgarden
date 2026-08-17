//! The editor's right-click context menu: the common actions an
//! IntelliJ-style menu offers (Undo, Redo, Cut, Copy, Paste, Select All,
//! Toggle Line Comment, Duplicate Line, Save), reachable without the menu
//! bar or a keyboard shortcut.

use egui::{Event, Key, Modifiers};
use fg_core::Document;
use fg_i18n::t;
use syntax::IncrementalParser;

use super::auto_edit::{duplicate_line, toggle_line_comments};
use super::text_area::Caret;
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
        modifiers: Modifiers {
            command: true,
            shift,
            ..Modifiers::NONE
        },
    }
}

/// Attaches the context menu to `response` (the editor widget's own
/// response) and handles every item's click. `text`/`manual_caret` are the
/// same locals `widget::show` threads through every other edit path, so an
/// edit made here flows through the identical apply-cursor-at-the-end
/// machinery; `pending_input` is drained back into real input at the top of
/// the *next* frame (see `widget::show`) for the three items the editor's
/// own key handling has to process itself (undo/redo/select-all are events
/// `text_area::shell`'s `process_events` reads from the same input queue
/// `egui::TextEdit` used to, so replaying a synthetic keypress into it next
/// frame still works unchanged).
#[expect(
    clippy::too_many_arguments,
    reason = "each parameter is independently threaded editor-frame state, not a bundle waiting to be a struct — see widget::show's own too-many-arguments allowance for the same shape"
)]
pub(super) fn show_context_menu(
    response: &egui::Response,
    widget_id: egui::Id,
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    primary_caret: Option<Caret>,
    text: &mut String,
    manual_caret: &mut Option<Caret>,
    pending_input: &mut Vec<Event>,
    last_error: &mut Option<String>,
    cached_clipboard_text: &mut Option<String>,
) {
    let has_selection = primary_caret.is_some_and(|c| !c.is_collapsed());
    let cursor_char = primary_caret.map(|c| c.primary);

    // Refreshed only on the frame the menu actually opens (a right click on
    // `response`), not on every frame it stays open — opening a clipboard
    // connection isn't free (an X11/Wayland round-trip under the hood on
    // Linux), and a user hovering the menu while deciding what to click
    // could otherwise reopen it many times in a row for no new information.
    // `response.secondary_clicked()` is the same condition egui's own
    // `context_menu` checks internally to decide whether to open the popup
    // in the first place, so it's true on exactly that one frame.
    if response.secondary_clicked() {
        *cached_clipboard_text = arboard::Clipboard::new()
            .and_then(|mut cb| cb.get_text())
            .ok()
            .filter(|s| !s.is_empty());
    }

    response.context_menu(|ui| {
        if ui
            .add(egui::Button::new(t().editor.undo).shortcut_text("Ctrl+Z"))
            .clicked()
        {
            pending_input.push(synthetic_shortcut(Key::Z, false));
            ui.memory_mut(|mem| mem.request_focus(widget_id));
            ui.close();
        }
        if ui
            .add(egui::Button::new(t().editor.redo).shortcut_text("Ctrl+Shift+Z"))
            .clicked()
        {
            pending_input.push(synthetic_shortcut(Key::Z, true));
            ui.memory_mut(|mem| mem.request_focus(widget_id));
            ui.close();
        }

        ui.separator();

        if ui
            .add_enabled(
                has_selection,
                egui::Button::new(t().common.cut).shortcut_text("Ctrl+X"),
            )
            .clicked()
        {
            if let Some(range) = primary_caret.map(|c| c.range()) {
                let start = char_to_byte(text, range.start);
                let end = char_to_byte(text, range.end);
                ui.ctx().copy_text(text[start..end].to_string());
                let new_text = format!("{}{}", &text[..start], &text[end..]);
                apply_edit(doc, parser, text, &new_text);
                *manual_caret = Some(Caret::at(range.start));
                *text = new_text;
            }
            ui.close();
        }
        if ui
            .add_enabled(
                has_selection,
                egui::Button::new(t().common.copy).shortcut_text("Ctrl+C"),
            )
            .clicked()
        {
            if let Some(range) = primary_caret.map(|c| c.range()) {
                let start = char_to_byte(text, range.start);
                let end = char_to_byte(text, range.end);
                ui.ctx().copy_text(text[start..end].to_string());
            }
            ui.close();
        }
        // egui has no public API to read the OS clipboard (only
        // `Context::copy_text` to write it), so this is the one item here
        // that needs `arboard` directly rather than something already
        // exposed by egui — see the cache refresh above this closure.
        if ui
            .add_enabled(
                cached_clipboard_text.is_some(),
                egui::Button::new(t().common.paste).shortcut_text("Ctrl+V"),
            )
            .clicked()
        {
            if let (Some(pasted), Some(range)) = (cached_clipboard_text.as_ref(), primary_caret.map(|c| c.range())) {
                let start = char_to_byte(text, range.start);
                let end = char_to_byte(text, range.end);
                let new_text = format!("{}{pasted}{}", &text[..start], &text[end..]);
                let new_cursor = range.start + pasted.chars().count();
                apply_edit(doc, parser, text, &new_text);
                *manual_caret = Some(Caret::at(new_cursor));
                *text = new_text;
            }
            ui.close();
        }
        if ui
            .add(egui::Button::new(t().editor.select_all).shortcut_text("Ctrl+A"))
            .clicked()
        {
            pending_input.push(synthetic_shortcut(Key::A, false));
            ui.memory_mut(|mem| mem.request_focus(widget_id));
            ui.close();
        }

        ui.separator();

        if ui
            .add(egui::Button::new(t().editor.toggle_line_comment).shortcut_text("Ctrl+/"))
            .clicked()
        {
            if let Some(range) = primary_caret.map(|c| c.range()) {
                let (commented, new_start, new_end) = toggle_line_comments(text, range.start, range.end);
                apply_edit(doc, parser, text, &commented);
                *manual_caret = Some(Caret {
                    primary: new_end,
                    anchor: new_start,
                });
                *text = commented;
            }
            ui.close();
        }
        if ui
            .add(egui::Button::new(t().editor.duplicate_line).shortcut_text("Alt+Shift+Up/Down"))
            .clicked()
        {
            if let Some(cursor_char) = cursor_char {
                let (duplicated, new_cursor) = duplicate_line(text, cursor_char);
                apply_edit(doc, parser, text, &duplicated);
                *manual_caret = Some(Caret::at(new_cursor));
                *text = duplicated;
            }
            ui.close();
        }

        ui.separator();

        if ui
            .add_enabled(
                doc.is_dirty(),
                egui::Button::new(t().common.save).shortcut_text("Ctrl+S"),
            )
            .clicked()
        {
            crate::panels::tabs::save_document(doc, parser, last_error);
            // `Document::save` trims trailing whitespace, which can change
            // `doc.buffer` out from under `text` — every other branch in
            // `widget::show` refreshes it right after an edit for exactly
            // this reason (see `apply_edit`'s doc comment); Save needs the
            // same treatment, or the rest of *this* frame (the trailing
            // `manual_cursor_range` apply) would keep working off pre-trim
            // content while `doc.buffer` has already moved on.
            *text = doc.buffer.to_string();
            ui.close();
        }
    });
}
