//! Annotation completion + auto-import: typing `@` in a file offers the
//! annotations an extension knows (`@Component`, `@Autowired`, ...);
//! accepting one inserts the annotation and, if the file doesn't already
//! import it, the `import` edit the extension computes
//! (`syntax::import_edit`). Which names are offered and what an import looks
//! like are the extension's; this module only turns them into completion
//! items and splices the edit.

use fg_core::Language;
use syntax::{Tree, annotation_candidates, import_edit};

use super::completion::{CompletionItem, CompletionKind};

/// Every annotation an extension offers for `language`, as completion
/// candidates — unfiltered, same "let `CompletionState::visible`'s own prefix
/// filter narrow it" shape every other candidate source in this codebase
/// already uses. `detail` shows the annotation's own import path, so the
/// popup doubles as a reminder of which package it comes from before
/// accepting. Empty when the language has none, so no popup opens.
pub(super) fn import_candidates(language: Language) -> Vec<CompletionItem> {
    annotation_candidates(language)
        .into_iter()
        .map(|candidate| CompletionItem {
            label: candidate.name,
            kind: CompletionKind::Annotation,
            detail: Some(candidate.qualified_name),
            has_params: false,
        })
        .collect()
}

/// Combines `insert_completion`'s own single-span replacement
/// (`completed_text`/`cursor_after_label`, already computed by the
/// caller from the *pre*-completion `old_text`/`tree`) with the accepted
/// `item`'s own import, if it needs one — one final text and cursor
/// position. Splicing the import's byte (computed against `old_text`'s own
/// coordinates) directly into `completed_text` is only valid because every
/// byte of `completed_text` before `anchor_byte` is still identical to
/// `old_text` (the completion's own edit only ever touches
/// `[anchor_byte..cursor_byte)`) — guarded explicitly rather than assumed:
/// an import that would sort *after* `anchor_byte` (not a normal file shape —
/// imports always precede where an annotation gets typed) skips the import
/// rather than risk splicing into already-edited text.
pub(super) fn apply_with_import(
    old_text: &str,
    tree: &Tree,
    language: Language,
    item: &CompletionItem,
    anchor_byte: usize,
    completed_text: String,
    cursor_after_label: usize,
) -> (String, usize) {
    let Some(qualified_name) = item.detail.as_deref() else {
        return (completed_text, cursor_after_label);
    };
    let Some(edit) = import_edit(tree, old_text, language, qualified_name) else {
        return (completed_text, cursor_after_label);
    };
    if edit.byte > anchor_byte {
        return (completed_text, cursor_after_label);
    }

    let mut final_text = completed_text;
    final_text.insert_str(edit.byte, &edit.text);
    let cursor_shift = edit.text.chars().count();
    (final_text, cursor_after_label + cursor_shift)
}
