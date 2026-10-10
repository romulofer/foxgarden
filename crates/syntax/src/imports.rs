//! `PLAN.md` Track 24 Phase 6: annotation completion and the `import` that
//! makes an accepted annotation resolve, answered by whichever extension
//! owns the language.
//!
//! Which names an `@` offers, how an import statement is spelled and where
//! it goes among a file's existing ones are all a language's own rules, so
//! the core only asks. Same dispatch rule as `code_model`: handing over the
//! tree the caller already parsed, first extension that answers wins.

use fg_core::Language;
use fg_extension::{DocStub, ImportCandidate, ImportEdit};
use tree_sitter::Tree;

use crate::providers::providers;

/// The annotations offered after an `@` in a `language` file. Empty when no
/// extension has any, which is how the editor decides not to open a popup.
pub fn annotation_candidates(language: Language) -> Vec<ImportCandidate> {
    providers()
        .extensions
        .iter()
        .map(|extension| extension.annotation_candidates(language.id()))
        .find(|candidates| !candidates.is_empty())
        .unwrap_or_default()
}

/// The edit that makes `qualified_name` resolve in `source`. `None` when it
/// already does, and for languages no extension imports for.
pub fn import_edit(tree: &Tree, source: &str, language: Language, qualified_name: &str) -> Option<ImportEdit> {
    providers()
        .extensions
        .iter()
        .find_map(|extension| extension.import_edit(language.id(), tree, source, qualified_name))
}

/// A documentation-comment skeleton for the declaration at `byte`. `None`
/// when there is nothing to document there, or it already is documented.
pub fn doc_stub(tree: &Tree, source: &str, language: Language, byte: usize) -> Option<DocStub> {
    providers()
        .extensions
        .iter()
        .find_map(|extension| extension.doc_stub(language.id(), tree, source, byte))
}

#[cfg(test)]
#[path = "imports_test.rs"]
mod imports_test;
