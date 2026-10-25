//! `PLAN.md` Track 30 Phase 2: the declarations that stand for the one under
//! the caret (`expect` <-> `actual`), found by whichever extension knows the
//! project layout rather than by a `match` here.

use std::path::Path;

use fg_extension::{Counterpart, FileDiagnostic};

/// The counterparts of the declaration at `byte` in `file`, from every
/// installed extension that has any. Reads the project's sources, so call it
/// for an explicit request, not per frame.
pub fn counterparts(project_root: &Path, file: &Path, source: &str, byte: usize) -> Vec<Counterpart> {
    crate::providers::providers()
        .extensions
        .iter()
        .flat_map(|extension| extension.counterparts(project_root, file, source, byte))
        .collect()
}

/// Declarations the identifier at `byte` in `file` may refer to, from every
/// installed extension — what go-to-definition falls back on when the language
/// server has no answer. Reads the project's sources, so call it for an
/// explicit request, not per frame.
pub fn definitions(project_root: &Path, file: &Path, source: &str, byte: usize) -> Vec<Counterpart> {
    crate::providers::providers()
        .extensions
        .iter()
        .flat_map(|extension| extension.definitions(project_root, file, source, byte))
        .collect()
}

/// The 0-based lines of `source` that declare something with counterparts,
/// from every installed extension. Reads the text only, so it is cheap
/// enough to ask again after each edit.
pub fn counterpart_lines(source: &str) -> Vec<usize> {
    let mut lines: Vec<usize> = crate::providers::providers()
        .extensions
        .iter()
        .flat_map(|extension| extension.counterpart_lines(source))
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// The problems every installed extension finds in `file` (whose text is
/// `source`) from the project around it. Reads the project's sources: ask on
/// open and save, off the UI thread.
pub fn file_diagnostics(project_root: &Path, file: &Path, source: &str) -> Vec<FileDiagnostic> {
    crate::providers::providers()
        .extensions
        .iter()
        .flat_map(|extension| extension.file_diagnostics(project_root, file, source))
        .collect()
}
