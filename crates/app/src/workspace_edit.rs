//! Shared `WorkspaceEdit` application (`PLAN.md` Track 20 Phase 7 /
//! Track 15 Phase 1): rewriting every file a `WorkspaceEdit` names — an
//! open tab's own live buffer for one that's open, straight to disk for
//! one that isn't. `crate::rename::RenameState`'s own decoded
//! `textDocument/rename` reply and `widgets::editor::code_action::
//! CodeActionGutter`'s own picked quick-fix edit both funnel through this
//! one primitive (`apply`) rather than duplicating the per-file/
//! `TextEdit`-application logic twice — this module started as
//! `rename.rs`'s own private half, extracted once a second caller
//! (Track 15) needed the exact same thing.

use std::path::{Path, PathBuf};

use fg_core::EditorState;
use ropey::Rope;

use crate::lsp_state::{uri_to_path, utf16_range_to_bytes};
use crate::panels::tabs;
use syntax::IncrementalParser;

/// Applies every file `edit` names, returning the number of files changed
/// (`0` for a real edit with nothing in it — not an error). See
/// `edits_by_file`'s own doc comment for the `document_changes`-vs-
/// `changes` precedence and what's silently skipped; `Err` only for a real
/// failure applying an edit the server *did* send (a file that couldn't be
/// read or written, or a range that no longer fits the file's current
/// text), for the caller to surface through `last_error`.
pub(crate) fn apply(
    edit: lsp_types::WorkspaceEdit,
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
) -> Result<usize, String> {
    let per_file = edits_by_file(&edit);
    let changed = per_file.len();
    for (path, edits) in per_file {
        apply_file_edits(&path, &edits, state, parsers)?;
    }
    Ok(changed)
}

/// Collects a `WorkspaceEdit`'s own per-file edit lists — `document_
/// changes` (`DocumentChanges::Edits` only; a `ResourceOp` — create/
/// rename/delete a file outright — is a real, if rare, shape a reply could
/// carry, but no server this app talks to has ever been observed sending
/// one for a plain symbol rename or quick fix, so it's skipped rather than
/// handled) takes precedence when present, per the LSP spec's own stated
/// preference; `changes` is the fallback for a server that only ever fills
/// that field. A location this app can't resolve a real path for
/// (`uri_to_path` returning `None`) is dropped, same best-effort degrade
/// every other LSP decode path here already has.
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
    let Some(changes) = &edit.changes else {
        return Vec::new();
    };
    changes
        .iter()
        .filter_map(|(uri, edits)| Some((uri_to_path(uri)?, edits.clone())))
        .collect()
}

fn as_text_edit(edit: &lsp_types::OneOf<lsp_types::TextEdit, lsp_types::AnnotatedTextEdit>) -> lsp_types::TextEdit {
    match edit {
        lsp_types::OneOf::Left(edit) => edit.clone(),
        lsp_types::OneOf::Right(annotated) => annotated.text_edit.clone(),
    }
}

/// Applies `edits` to `path`'s own current text — its open tab's live
/// buffer if it has one (`EditorState::find_tab`), read from disk
/// otherwise — and writes the result back the matching way: into the open
/// tab (bumping `lsp_version`/`lsp_sync_pending` and getting a fresh
/// parser, the same "content changed by something other than typing"
/// sequence `app.rs`'s own `reload_tab_from_disk` already uses, though —
/// unlike that function — `saved_buffer` is deliberately left alone: this
/// is a real edit the user asked for (by confirming a rename, or picking a
/// quick fix), not a transparent resync with what's already on disk, so it
/// needs to show up as dirty, same as if they'd typed it themselves) or
/// straight to disk for a file with no open tab.
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
        doc.buffer.replace(Rope::from_str(&new_text));
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

/// Applies every edit in `edits` to `text`, returning the result. Converts
/// each `TextEdit`'s own UTF-16 `Range` to a byte range against `text`
/// first, then applies furthest-in-the-file-first (`sort_by` descending on
/// `range.start`) so an earlier edit's own byte offsets never shift out
/// from under a later one still waiting to apply — the same reasoning a
/// `Vec::remove`-in-a-loop needs to go back-to-front. `TextDocumentEdit`'s
/// own doc comment (in `lsp-types` itself) already guarantees a real
/// server's edits for one file never overlap, so this never needs to
/// reconcile two edits touching the same range.
fn apply_text_edits(text: &str, edits: &[lsp_types::TextEdit]) -> Result<String, String> {
    let mut ranges: Vec<(std::ops::Range<usize>, &str)> = edits
        .iter()
        .map(|edit| {
            utf16_range_to_bytes(text, edit.range)
                .map(|range| (range, edit.new_text.as_str()))
                .ok_or_else(|| "an edit's own range fell outside the file's current text".to_string())
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
#[path = "workspace_edit_test.rs"]
mod workspace_edit_test;
