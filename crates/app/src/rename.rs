//! Rename symbol (`PLAN.md` Track 20 Phase 7): once `widgets::editor::
//! rename::RenameBox` hands a confirmed `(char_offset, new_name)` back to
//! `app.rs`, this module fires `textDocument/rename`, decodes the
//! server's reply (a `WorkspaceEdit`), and rewrites every file it names —
//! an open tab's own live buffer for one that's open, straight to disk
//! for one that isn't (the actual per-file application is `crate::
//! workspace_edit::apply`, shared with `widgets::editor::code_action`'s
//! own picked quick-fix edit — Track 15). No UI of its own; `app.rs`
//! surfaces a failure through its existing `last_error` the same way
//! every other fallible background op here already does.

use std::sync::mpsc::{Receiver, TryRecvError};

use fg_core::{Document, EditorState};

use crate::lsp_client::ResponseError;
use crate::lsp_state::LspState;
use syntax::IncrementalParser;

/// One slot for whichever tab is currently focused — same "one field,
/// not one per open tab" shape `goto_definition::GotoDefinitionState`
/// already uses, for the same reason: a rename request only ever targets
/// wherever the caret/confirm just was in the focused tab.
#[derive(Default)]
pub struct RenameState {
    rx: Option<Receiver<Result<serde_json::Value, ResponseError>>>,
}

impl RenameState {
    /// Same unprompted-background-message concern `LspState::
    /// wants_repaint`/`GotoDefinitionState::wants_repaint` already
    /// document.
    pub fn wants_repaint(&self) -> bool {
        self.rx.is_some()
    }

    /// Fires the `textDocument/rename` request for `doc`'s own
    /// identifier at `char_offset`, replacing whatever was already
    /// tracked — mirrors `GotoDefinitionState::request`'s own "a second
    /// request supersedes the first" reasoning, though in practice
    /// `RenameBox` never confirms twice without a reply landing in
    /// between (its own popup closes the instant one confirms).
    pub fn request(&mut self, doc: &mut Document, char_offset: usize, new_name: &str, lsp: &mut LspState) {
        let byte_offset = doc.buffer.char_to_byte(char_offset);
        self.rx = lsp.request_rename(doc, byte_offset, new_name);
    }

    /// Polls whatever request is in flight, applying the resulting
    /// `WorkspaceEdit` across every file it names once it lands. `Some`
    /// only once a reply actually arrives: `Ok(n)` with the number of
    /// files changed (0 for a real "no rename possible here" reply, the
    /// protocol's own `null` result — not surfaced as an error, matching
    /// every other best-effort LSP decode in this app), `Err` only for a
    /// real failure applying an edit the server *did* send (a file that
    /// couldn't be read or written), for the caller to surface through
    /// `last_error`. `None` on every other frame, including a still-
    /// pending one (stays tracked) and a failed/rejected request (dropped
    /// silently, same as everywhere else).
    pub fn poll(&mut self, state: &mut EditorState, parsers: &mut [Option<IncrementalParser>]) -> Option<Result<usize, String>> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(Ok(value)) => {
                self.rx = None;
                Some(apply_reply(value, state, parsers))
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                self.rx = None;
                None
            }
            Err(TryRecvError::Empty) => None,
        }
    }
}

/// Decodes a raw `textDocument/rename` reply into a `WorkspaceEdit` and
/// applies it via the shared `workspace_edit::apply`. `Ok(0)` on a `null`
/// result (nothing to rename) or a malformed reply — the latter dropped
/// silently rather than surfaced, matching this app's every other "can't
/// make sense of what the server sent" decode path.
fn apply_reply(value: serde_json::Value, state: &mut EditorState, parsers: &mut [Option<IncrementalParser>]) -> Result<usize, String> {
    let Some(edit) = serde_json::from_value::<Option<lsp_types::WorkspaceEdit>>(value).ok().flatten() else {
        return Ok(0);
    };
    crate::workspace_edit::apply(edit, state, parsers)
}
