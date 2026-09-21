//! Quick-fix intention actions (`PLAN.md` Track 15 Phase 1): a gutter
//! lightbulb on the caret's own line, shown once a `textDocument/
//! codeAction` request against that line's active LSP diagnostic (if any)
//! resolves with at least one offer this client can apply on its own —
//! only those are ever offered, matching this phase's own stated scope
//! ("picking one applies its `WorkspaceEdit` via the existing edit-
//! application path"). See `offer_from_item`'s own doc comment for the two
//! real reply shapes that counts as (a `CodeAction` literal's own `edit`
//! field, and jdtls' real `java.apply.workspaceEdit` `Command` convention
//! — found live, not assumed, while closing this very checkpoint: every
//! quick fix a real jdtls 1.60.0 offered for an unused-import warning came
//! back the second way, not the first). Any other bare `Command` is
//! filtered out rather than attempted: actually invoking one for real
//! would need `workspace/executeCommand` plus handling a server-initiated
//! `workspace/applyEdit` request in response, neither of which exists in
//! this codebase yet — out of scope here, not silently broken.
//!
//! Structurally mirrors `hover::HoverState`'s own request-in-flight/
//! poll-once-a-frame shape, just keyed off the caret's line and its
//! diagnostic rather than pointer dwell (and with no artificial delay — a
//! lightbulb appearing is its own soft signal, not an intrusive popup).
//! Like `rename::RenameBox`, this only owns the gutter/popup UI and the
//! request itself; actually applying a picked edit happens at `app.rs`
//! level (`take_confirmed`) via the shared `crate::workspace_edit::apply`
//! — a code action's edit can touch files this one document's own widget
//! has no access to, same as a rename's can. Unlike rename, picking an
//! offer needs no further network round-trip (the edit was already
//! resolved by the initial reply), so there's no app.rs-level polling
//! state of its own the way `rename::RenameState` needs.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use fg_core::Document;

use super::completion::popup_position;
use super::text_area::TextAreaOutput;
use crate::lsp_client::ResponseError;
use crate::lsp_state::LspState;
use crate::style::theme;

/// Extra gutter width reserved for the lightbulb column — only added
/// (`widget.rs`) on a frame where the caret's own line actually has an
/// offer to show, the same "reserve only when there's something to show"
/// rule `folding::FOLD_GUTTER_WIDTH`/`diff_gutter::DIFF_GUTTER_WIDTH`
/// already establish.
pub(super) const GUTTER_WIDTH: f32 = 14.0;

/// One resolved offer: a title to show in the picker, and the edit it
/// applies if picked. Only ever built from a `CodeAction` whose own `edit`
/// field was `Some` — see this module's own doc comment for why a bare
/// `Command` is filtered out rather than attempted.
struct Offer {
    title: String,
    edit: lsp_types::WorkspaceEdit,
}

struct Tracked {
    doc_path: PathBuf,
    line: usize,
    version: i32,
    pending: Option<Receiver<Result<serde_json::Value, ResponseError>>>,
    offers: Vec<Offer>,
    /// Set once the lightbulb itself is clicked — the picker popup stays
    /// open until a title is picked or focus moves elsewhere, same
    /// "click outside/Escape closes it" shape `RenameBox`'s own popup
    /// uses.
    open: bool,
}

/// One slot for whichever tab is currently focused — same "one field, not
/// one per open tab" shape `hover`/`rename`/`peek`/`find_references` all
/// already use, for the same reason: only the focused tab's own caret can
/// ever have this tracked.
#[derive(Default)]
pub struct CodeActionGutter {
    tracked: Option<Tracked>,
    /// Set once a popup title is picked; taken by `take_confirmed` —
    /// `app.rs`'s own poll-once-a-frame handoff to `workspace_edit::apply`.
    confirmed: Option<lsp_types::WorkspaceEdit>,
}

impl CodeActionGutter {
    /// Same unprompted-background-message concern `HoverState::
    /// wants_repaint` already documents.
    pub fn wants_repaint(&self) -> bool {
        self.tracked.as_ref().is_some_and(|t| t.pending.is_some())
    }

    /// Takes whatever offer was just picked (if any) — `app.rs` polls this
    /// once a frame, right after this widget's own `paint`.
    pub fn take_confirmed(&mut self) -> Option<lsp_types::WorkspaceEdit> {
        self.confirmed.take()
    }

    /// Whether the caret's own current line already has at least one
    /// resolved offer to show — read by `widget.rs` *before* this frame's
    /// gutter layout runs, so the lightbulb column's own width is known up
    /// front instead of shifting the whole editor sideways mid-frame. This
    /// only inspects whatever `update` already resolved as of an earlier
    /// frame; `update` itself (which actually fires the network request)
    /// runs later in the same frame, once `doc`/the caret are in scope for
    /// it — by design, not an ordering bug: the same one-frame-behind
    /// approximation `folding`/`diff_gutter`'s own reserved columns already
    /// accept implicitly (their own inputs can change frame to frame too).
    pub(super) fn has_offer(&self, doc_path: &Path, line: usize) -> bool {
        self.tracked
            .as_ref()
            .is_some_and(|t| t.doc_path == doc_path && t.line == line && !t.offers.is_empty())
    }

    /// Drops whatever's tracked — used on a tab switch, same "a different
    /// tab's buffer is now on screen" reasoning `HoverState::clear`'s own
    /// call site already has.
    pub fn clear(&mut self) {
        self.tracked = None;
    }

    /// Opens the picker popup for the caret line's own offers from the
    /// keyboard (Alt+Enter), the same popup clicking the lightbulb toggles
    /// open — `SPEC.md` §15's "clicking it (or a keyboard shortcut with the
    /// cursor on that line)". A no-op when the caret's line has no resolved
    /// offer yet: nothing to show, exactly as the lightbulb simply isn't
    /// painted in that case. Only inspects offers already resolved as of an
    /// earlier frame, the same one-frame-behind lifecycle `has_offer`/the
    /// gutter width already accept.
    pub(super) fn open_picker(&mut self) {
        if let Some(tracked) = self.tracked.as_mut()
            && !tracked.offers.is_empty()
        {
            tracked.open = true;
        }
    }

    /// Called once a frame with `doc` and whichever line the caret is
    /// currently on. Fires a `textDocument/codeAction` request the instant
    /// that line's own active diagnostic (`doc.lsp_diagnostics` only — the
    /// Checkstyle/PMD/SpotBugs sources have no language server behind them
    /// to ask) changes, and polls whatever's already in flight.
    pub(super) fn update(&mut self, doc: &mut Document, caret_line: usize, lsp: &mut LspState) {
        let Some(diagnostic) = diagnostic_on_line(doc, caret_line) else {
            self.tracked = None;
            return;
        };
        let stays = self
            .tracked
            .as_ref()
            .is_some_and(|t| t.doc_path == doc.path && t.line == caret_line && t.version == doc.lsp_version);
        if !stays {
            let pending = lsp.request_code_action(doc, &diagnostic);
            self.tracked = Some(Tracked {
                doc_path: doc.path.clone(),
                line: caret_line,
                version: doc.lsp_version,
                pending,
                offers: Vec::new(),
                open: false,
            });
        }
        let Some(tracked) = self.tracked.as_mut() else { return };
        if let Some(rx) = &tracked.pending {
            match rx.try_recv() {
                Ok(Ok(value)) => {
                    tracked.offers = offers_from_response(value);
                    tracked.pending = None;
                }
                // A rejected request or a server that exited mid-request
                // both degrade the same way every other best-effort LSP
                // path in this app does: no lightbulb, nothing surfaced.
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => tracked.pending = None,
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    /// Paints the lightbulb — only once `tracked.offers` is non-empty (no
    /// placeholder while a request is still in flight, matching `hover`'s
    /// own "nothing shown until there's real content" rule) — at the
    /// caret line's own row, in the gutter sliver `gutter_left` names.
    /// Clicking it opens the picker popup; clicking a title in it records
    /// that offer's edit into `confirmed`; Escape or a click outside
    /// closes the popup without picking.
    pub(super) fn paint(
        &mut self,
        ui: &egui::Ui,
        id: egui::Id,
        out: &TextAreaOutput,
        gutter_left: f32,
        pane_rect: egui::Rect,
        dark_mode: bool,
    ) {
        let Some(tracked) = &mut self.tracked else { return };
        if tracked.offers.is_empty() {
            return;
        }
        let Some(row_index) = out.row_galleys.iter().position(|(logical, _)| *logical == tracked.line) else {
            return;
        };
        let y = out.content_origin.y + out.row_offsets[row_index] as f32 * out.row_height;
        let rect = egui::Rect::from_min_size(egui::pos2(gutter_left, y), egui::vec2(GUTTER_WIDTH, out.row_height));

        let bulb_id = id.with(("code_action_lightbulb", tracked.line));
        let response = ui.interact(rect, bulb_id, egui::Sense::click());
        paint_lightbulb(ui.painter(), rect.center(), 4.5, theme::lightbulb(dark_mode));
        if response.clicked() {
            tracked.open = !tracked.open;
        }

        let mut picked_edit = None;
        if tracked.open {
            if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                tracked.open = false;
            } else {
                let popup_size = ui
                    .ctx()
                    .memory(|mem| mem.area_rect(id))
                    .map_or(egui::vec2(1.0, 1.0), |r| r.size());
                let pos = popup_position(rect, popup_size, pane_rect);
                let mut picked_index = None;
                let area_response =
                    egui::Area::new(id)
                        .fixed_pos(pos)
                        .order(egui::Order::Foreground)
                        .show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.set_min_width(220.0);
                                for (i, offer) in tracked.offers.iter().enumerate() {
                                    if ui.selectable_label(false, &offer.title).clicked() {
                                        picked_index = Some(i);
                                    }
                                }
                            });
                        });
                let clicked_outside = ui.ctx().input(|i| i.pointer.any_click())
                    && ui
                        .ctx()
                        .pointer_interact_pos()
                        .is_some_and(|pos| !area_response.response.rect.contains(pos) && !rect.contains(pos));
                if let Some(i) = picked_index {
                    picked_edit = Some(tracked.offers[i].edit.clone());
                    tracked.open = false;
                } else if clicked_outside {
                    tracked.open = false;
                }
            }
        }
        if picked_edit.is_some() {
            self.confirmed = picked_edit;
        }
    }
}

/// The first `doc.lsp_diagnostics` entry whose own range starts on `line`
/// — "active" is deliberately just "on the caret's line," not "the caret
/// sits inside its exact byte range," matching how a real IDE's lightbulb
/// reads as available for the whole line a squiggle touches, not just the
/// exact span under the cursor.
fn diagnostic_on_line(doc: &Document, line: usize) -> Option<fg_core::Diagnostic> {
    let len = doc.buffer.len_bytes();
    doc.lsp_diagnostics
        .iter()
        .find(|d| doc.buffer.byte_to_line(d.range.start.min(len)) == line)
        .cloned()
}

/// Decodes a raw `textDocument/codeAction` reply into the `Offer`s worth
/// showing — a `null`/malformed reply degrades to an empty list, the same
/// "can't make sense of what the server sent" / "nothing to show"
/// convention every other best-effort LSP decode in this app already
/// follows.
fn offers_from_response(value: serde_json::Value) -> Vec<Offer> {
    let Ok(items) = serde_json::from_value::<Vec<lsp_types::CodeActionOrCommand>>(value) else {
        return Vec::new();
    };
    items.into_iter().filter_map(offer_from_item).collect()
}

/// One `CodeActionOrCommand` item, resolved to an `Offer` if (and only if)
/// it already carries a real `WorkspaceEdit` — no further round-trip
/// needed to apply it. Two real shapes cover every quick fix observed live
/// against a real jdtls 1.60.0 (`PLAN.md` Track 15 Phase 1's own
/// checkpoint — an unused-import fix, among others): a `CodeAction`
/// literal with its own `edit` field set (the shape the LSP spec's
/// `CodeActionLiteralSupport` capability was written for), and — what
/// jdtls actually sends for "Organize imports"/"Remove unused import"/etc,
/// found by *reading the real reply* rather than assuming spec-shaped
/// data — a bare `Command` named `java.apply.workspaceEdit` whose sole
/// argument *is* the `WorkspaceEdit` to apply, client-side, with nothing
/// further sent back to the server. Every other `Command` name is a real
/// server-side action (`workspace/executeCommand`) this app doesn't
/// implement yet (see this module's own doc comment) and is correctly
/// left out.
fn offer_from_item(item: lsp_types::CodeActionOrCommand) -> Option<Offer> {
    match item {
        lsp_types::CodeActionOrCommand::CodeAction(action) => action.edit.map(|edit| Offer {
            title: action.title,
            edit,
        }),
        lsp_types::CodeActionOrCommand::Command(command) if command.command == "java.apply.workspaceEdit" => {
            let edit = command
                .arguments?
                .into_iter()
                .next()
                .and_then(|arg| serde_json::from_value(arg).ok())?;
            Some(Offer {
                title: command.title,
                edit,
            })
        }
        lsp_types::CodeActionOrCommand::Command(_) => None,
    }
}

/// A small filled lightbulb, drawn as shapes rather than a text glyph —
/// same reasoning `folding::paint_triangle`'s own doc comment gives: no
/// dependency on the active font actually having a `💡`-shaped glyph (it
/// almost certainly wouldn't, in a monospace code font).
fn paint_lightbulb(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: egui::Color32) {
    painter.circle_filled(center, radius, color);
    let base = egui::Rect::from_center_size(center + egui::vec2(0.0, radius + 1.5), egui::vec2(radius * 1.1, 2.0));
    painter.rect_filled(base, 0.5, color);
}

#[cfg(test)]
#[path = "code_action_test.rs"]
mod code_action_test;
