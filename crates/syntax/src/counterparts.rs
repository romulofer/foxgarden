//! `PLAN.md` Track 30 Phase 2: the declarations that stand for the one under
//! the caret (`expect` <-> `actual`), found by whichever extension knows the
//! project layout rather than by a `match` here.

use std::path::Path;

use fg_extension::Counterpart;

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
