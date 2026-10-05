//! `PLAN.md` Track 24 Phase 6: a file's runnable entry points, found by
//! whichever extension owns the language rather than by a `match` here.
//!
//! What this replaces is a Java/Kotlin-shaped analysis in this crate — the
//! `public static void main(String[])` signature check, the package +
//! nested-class qualification rules, and the `…Kt` name a Kotlin top-level
//! `main` compiles into. None of that is a general editor concern; what is
//! general is "ask whoever owns this language where its entry points are,
//! and put a ▶ on those lines".
//!
//! Asks the extensions `providers` holds, installed beside the grammars.
//!
//! The extension is handed the tree the editor already parsed, never asked
//! to parse the file itself: this runs again on every edit, and a second
//! full parse per keystroke is exactly the kind of cost this editor exists
//! not to pay.

use std::sync::Arc;

use fg_core::Language;
use fg_extension::RunTarget;
use tree_sitter::Tree;

/// One entry point plus the extension that reported it. The entry point is
/// written in that extension's own notation, so launching it has to go
/// through a build tool the same extension owns — another extension's tool
/// claiming the project directory is no reason to think it can read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunMarker {
    pub extension_id: Arc<str>,
    pub target: RunTarget,
}

/// Every runnable entry point in `source`, in source order — the lines the
/// editor's run gutter marks.
///
/// `file_stem` is the file's name without its extension, which some
/// toolchains need to name what a file compiles into. An extension that
/// does not own `language` contributes nothing, so a language nobody claims
/// simply has no run markers, exactly as before.
pub fn main_entries(tree: &Tree, source: &str, language: Language, file_stem: &str) -> Vec<RunMarker> {
    crate::providers::providers()
        .extensions
        .iter()
        .flat_map(|extension| {
            extension
                .run_targets(language.id(), tree, source, file_stem)
                .into_iter()
                .map(|target| RunMarker {
                    extension_id: extension.id.clone(),
                    target,
                })
        })
        .collect()
}

#[cfg(test)]
#[path = "run_targets_test.rs"]
mod run_targets_test;
