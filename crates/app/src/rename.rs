//! Rename symbol (`PLAN.md` Track 20 Phase 7): once `widgets::editor::
//! rename::RenameBox` hands a confirmed `(char_offset, new_name)` back to
//! `app.rs`, this module fires `textDocument/rename`, decodes the
//! server's reply (a `WorkspaceEdit`), and rewrites every file it names —
//! an open tab's own live buffer for one that's open, straight to disk
//! for one that isn't. No UI of its own; `app.rs` surfaces a failure
//! through its existing `last_error` the same way every other fallible
//! background op here already does.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use fg_core::{Document, EditorState};
use ropey::Rope;

use crate::lsp_client::ResponseError;
use crate::lsp_state::{LspState, uri_to_path, utf16_range_to_bytes};
use crate::panels::tabs;
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
/// applies it. `Ok(0)` on a `null` result (nothing to rename) or a
/// malformed reply — the latter dropped silently rather than surfaced,
/// matching this app's every other "can't make sense of what the server
/// sent" decode path.
fn apply_reply(value: serde_json::Value, state: &mut EditorState, parsers: &mut [Option<IncrementalParser>]) -> Result<usize, String> {
    let Some(edit) = serde_json::from_value::<Option<lsp_types::WorkspaceEdit>>(value).ok().flatten() else {
        return Ok(0);
    };
    let per_file = edits_by_file(&edit);
    let changed = per_file.len();
    for (path, edits) in per_file {
        apply_file_edits(&path, &edits, state, parsers)?;
    }
    Ok(changed)
}

/// Collects a `WorkspaceEdit`'s own per-file edit lists — `document_
/// changes` (`DocumentChanges::Edits` only; a `ResourceOp` — create/
/// rename/delete a file outright — is a real, if rare, shape a rename
/// reply could carry, but no server this app talks to has ever been
/// observed sending one for a plain symbol rename, so it's skipped
/// rather than handled) takes precedence when present, per the LSP
/// spec's own stated preference; `changes` is the fallback for a server
/// that only ever fills that field. A location this app can't resolve a
/// real path for (`uri_to_path` returning `None`) is dropped, same
/// best-effort degrade every other LSP decode path here already has.
fn edits_by_file(edit: &lsp_types::WorkspaceEdit) -> Vec<(PathBuf, Vec<lsp_types::TextEdit>)> {
    if let Some(lsp_types::DocumentChanges::Edits(document_edits)) = &edit.document_changes {
        return document_edits
            .iter()
            .filter_map(|document_edit| {
                let path = uri_to_path(&document_edit.text_document.uri)?;
                let edits = document_edit.edits.iter().map(as_text_edit).collect();
                Some((path, edits))
            })
            .collect();
    }
    let Some(changes) = &edit.changes else { return Vec::new() };
    changes.iter().filter_map(|(uri, edits)| Some((uri_to_path(uri)?, edits.clone()))).collect()
}

fn as_text_edit(edit: &lsp_types::OneOf<lsp_types::TextEdit, lsp_types::AnnotatedTextEdit>) -> lsp_types::TextEdit {
    match edit {
        lsp_types::OneOf::Left(edit) => edit.clone(),
        lsp_types::OneOf::Right(annotated) => annotated.text_edit.clone(),
    }
}

/// Applies `edits` to `path`'s own current text — its open tab's live
/// buffer if it has one (`EditorState::find_tab`), read from disk
/// otherwise — and writes the result back the matching way: into the
/// open tab (bumping `lsp_version`/`lsp_sync_pending` and getting a
/// fresh parser, the same "content changed by something other than
/// typing" sequence `app.rs`'s own `reload_tab_from_disk` already uses,
/// though — unlike that function — `saved_buffer` is deliberately left
/// alone: a rename is a real edit the user asked for, not a transparent
/// resync with what's already on disk, so it needs to show up as dirty,
/// same as if they'd typed it themselves) or straight to disk for a
/// file with no open tab.
fn apply_file_edits(
    path: &Path,
    edits: &[lsp_types::TextEdit],
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
) -> Result<(), String> {
    if let Some(index) = state.find_tab(path) {
        let text = state.open_tabs[index].buffer.to_string();
        let new_text = apply_text_edits(&text, edits)?;
        let doc = &mut state.open_tabs[index];
        doc.buffer = Rope::from_str(&new_text);
        doc.lsp_version += 1;
        doc.lsp_sync_pending = true;
        parsers[index] = tabs::open_parser_for(doc);
    } else {
        let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
        let new_text = apply_text_edits(&text, edits)?;
        std::fs::write(path, new_text).map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// Applies every edit in `edits` to `text`, returning the result.
/// Converts each `TextEdit`'s own UTF-16 `Range` to a byte range against
/// `text` first, then applies furthest-in-the-file-first (`sort_by`
/// descending on `range.start`) so an earlier edit's own byte offsets
/// never shift out from under a later one still waiting to apply — the
/// same reasoning a `Vec::remove`-in-a-loop needs to go back-to-front.
/// `TextDocumentEdit`'s own doc comment (in `lsp-types` itself) already
/// guarantees a real server's edits for one file never overlap, so this
/// never needs to reconcile two edits touching the same range.
fn apply_text_edits(text: &str, edits: &[lsp_types::TextEdit]) -> Result<String, String> {
    let mut ranges: Vec<(std::ops::Range<usize>, &str)> = edits
        .iter()
        .map(|edit| {
            utf16_range_to_bytes(text, edit.range)
                .map(|range| (range, edit.new_text.as_str()))
                .ok_or_else(|| "a rename edit's own range fell outside the file's current text".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    ranges.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut result = text.to_string();
    for (range, new_text) in ranges {
        result.replace_range(range, new_text);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn edit(start_line: u32, start_char: u32, end_line: u32, end_char: u32, new_text: &str) -> lsp_types::TextEdit {
        lsp_types::TextEdit {
            range: lsp_types::Range {
                start: lsp_types::Position { line: start_line, character: start_char },
                end: lsp_types::Position { line: end_line, character: end_char },
            },
            new_text: new_text.to_string(),
        }
    }

    #[test]
    fn apply_text_edits_replaces_a_single_occurrence() {
        let text = "int oldName = 1;";
        let edits = vec![edit(0, 4, 0, 11, "newName")];
        assert_eq!(apply_text_edits(text, &edits).unwrap(), "int newName = 1;");
    }

    #[test]
    fn apply_text_edits_applies_multiple_edits_without_shifting_earlier_offsets() {
        let text = "oldName + oldName";
        let edits = vec![edit(0, 0, 0, 7, "newName"), edit(0, 10, 0, 17, "newName")];
        assert_eq!(apply_text_edits(text, &edits).unwrap(), "newName + newName");
    }

    #[test]
    fn apply_text_edits_handles_a_rename_that_spans_multiple_lines() {
        let text = "oldName\n.field";
        let edits = vec![edit(0, 0, 0, 7, "newName")];
        assert_eq!(apply_text_edits(text, &edits).unwrap(), "newName\n.field");
    }

    #[test]
    fn apply_text_edits_errors_on_a_range_outside_the_text() {
        let text = "short";
        let edits = vec![edit(5, 0, 5, 3, "x")];
        assert!(apply_text_edits(text, &edits).is_err());
    }

    #[test]
    fn edits_by_file_prefers_document_changes_over_changes() {
        let uri = lsp_types::Uri::from_str("file:///a/Foo.java").unwrap();
        let document_edit = lsp_types::TextDocumentEdit {
            text_document: lsp_types::OptionalVersionedTextDocumentIdentifier { uri: uri.clone(), version: None },
            edits: vec![lsp_types::OneOf::Left(edit(0, 0, 0, 3, "new"))],
        };
        let workspace_edit = lsp_types::WorkspaceEdit {
            changes: None,
            document_changes: Some(lsp_types::DocumentChanges::Edits(vec![document_edit])),
            change_annotations: None,
        };
        let result = edits_by_file(&workspace_edit);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.len(), 1);
    }

    #[test]
    fn edits_by_file_skips_a_document_change_operation() {
        let workspace_edit = lsp_types::WorkspaceEdit {
            changes: None,
            document_changes: Some(lsp_types::DocumentChanges::Operations(Vec::new())),
            change_annotations: None,
        };
        assert!(edits_by_file(&workspace_edit).is_empty());
    }
}
