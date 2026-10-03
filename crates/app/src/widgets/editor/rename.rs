//! Rename symbol (`PLAN.md` Track 20 Phase 7): F2 opens an inline "new
//! name" box pre-filled with the identifier under the caret. This module
//! owns only that box — reading the input, deciding when Enter confirms
//! it, Escape/click-outside cancels it — the same "one small popup, one
//! per-tab slot" shape `peek::PeekState`/`references::
//! FindReferencesState` already use. Everything past a confirmed Enter
//! (firing `textDocument/rename`, decoding the reply, actually rewriting
//! every affected file) is `crate::rename::RenameState`'s own job, an
//! app.rs-level concern since applying a `WorkspaceEdit` can touch files
//! this one tab's own widget has no access to — `take_confirmed` is the
//! handoff between the two, the same shape `references::
//! FindReferencesState::take_navigation` already uses to hand a row
//! click back to `app.rs`.

use fg_core::Document;

use super::hover::identifier_span;
use super::text_area::TextAreaOutput;

struct Open {
    /// Where the box is anchored — the identifier's own first char,
    /// captured once at `start` time (not re-read from the live caret),
    /// same reasoning `peek::PeekState::anchor_char`'s own doc comment
    /// already gives: the caret is free to move away while the box is
    /// still open.
    anchor_char: usize,
    /// The identifier's own original text, so a confirm that changes
    /// nothing (Enter with no edit made) can be told apart from a real
    /// rename and skipped — asking a language server to "rename" a
    /// symbol to its own current name is a wasted round-trip at best.
    original: String,
    input: String,
}

/// One slot for whichever tab is currently focused — same "one field,
/// not one per open tab" shape `hover`/`goto_definition`/`peek`/
/// `find_references` all already use, for the same reason: only the
/// focused tab's own caret can ever have this box open.
#[derive(Default)]
pub struct RenameBox {
    open: Option<Open>,
    /// Set once Enter confirms a real (non-empty, actually-changed) new
    /// name; taken by `take_confirmed` — `app.rs`'s own poll-once-a-frame
    /// handoff to `crate::rename::RenameState::request`.
    confirmed: Option<(usize, String)>,
}

impl RenameBox {
    /// Opens the box, pre-filled with the identifier under `char_offset`
    /// — a no-op if the caret isn't actually inside/touching one
    /// (`identifier_span` returning empty), same as every other
    /// span-driven popup in this app degrading silently rather than
    /// opening on nothing to act on.
    pub(super) fn start(&mut self, doc: &Document, char_offset: usize) {
        let span = identifier_span(&doc.buffer, char_offset);
        if span.is_empty() {
            return;
        }
        let original = doc.buffer.slice(span.clone()).to_string();
        self.open = Some(Open {
            anchor_char: span.start,
            input: original.clone(),
            original,
        });
    }

    /// Drops whatever's open — used on a tab switch, same "a different
    /// tab's buffer is now on screen" reasoning `HoverState::clear`'s own
    /// call site already has.
    pub fn clear(&mut self) {
        self.open = None;
    }

    /// Takes whatever rename was just confirmed (if any) — `app.rs`
    /// polls this once a frame, right after this widget's own `paint`.
    pub fn take_confirmed(&mut self) -> Option<(usize, String)> {
        self.confirmed.take()
    }

    /// Renders the open box (if any) as a floating popup anchored just
    /// below the identifier's own position, reusing `completion`'s
    /// caret-relative popup-anchoring math the same way `hover`/`peek`/
    /// `find_references` already do. A no-op while nothing is open.
    /// Escape or a click outside the popup's own rect cancels; Enter
    /// with a real, actually-different, non-empty name confirms — either
    /// way the box closes.
    pub(super) fn paint(
        &mut self,
        ui: &egui::Ui,
        id: egui::Id,
        out: &TextAreaOutput,
        buffer: &ropey::Rope,
        pane_rect: egui::Rect,
    ) {
        let Some(open) = &mut self.open else { return };
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.open = None;
            return;
        }
        let Some(char_rect) = out.char_rect(buffer, open.anchor_char) else {
            self.open = None;
            return;
        };
        let popup_size = ui
            .ctx()
            .memory(|mem| mem.area_rect(id))
            .map_or(egui::vec2(1.0, 1.0), |r| r.size());
        let pos = super::completion::popup_position(char_rect, popup_size, pane_rect);

        let mut confirm_requested = false;
        let mut close_requested = false;
        let area_response = egui::Area::new(id)
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(200.0);
                    let response = ui.text_edit_singleline(&mut open.input);
                    if !response.has_focus() && !response.lost_focus() {
                        response.request_focus();
                    }
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        confirm_requested = true;
                    }
                });
            });

        let clicked_outside = ui.ctx().input(|i| i.pointer.any_click())
            && ui
                .ctx()
                .pointer_interact_pos()
                .is_some_and(|pos| !area_response.response.rect.contains(pos));

        if confirm_requested {
            let trimmed = open.input.trim();
            if !trimmed.is_empty() && trimmed != open.original {
                self.confirmed = Some((open.anchor_char, trimmed.to_string()));
            }
            close_requested = true;
        }
        if close_requested || clicked_outside {
            self.open = None;
        }
    }
}

#[cfg(test)]
#[path = "rename_test.rs"]
mod rename_test;
