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
