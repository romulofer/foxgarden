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
//! **Installed process-wide, like the grammars next door**, and for the
//! same reason: the answer is needed deep inside the editor's paint path
//! (`widgets::editor::widget`), which has no registry in reach, and the set
//! of extensions never changes after startup. `install` is called next to
//! `grammars::install`, so an editor that has one has the other.
//!
//! The extension is handed the tree the editor already parsed, never asked
//! to parse the file itself: this runs again on every edit, and a second
//! full parse per keystroke is exactly the kind of cost this editor exists
//! not to pay.

use std::sync::{Arc, OnceLock, RwLock};

use fg_core::Language;
use fg_extension::{ExtensionHandle, Registry, RunTarget};
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

type Providers = RwLock<Vec<ExtensionHandle>>;

fn providers() -> &'static Providers {
    static PROVIDERS: OnceLock<Providers> = OnceLock::new();
    PROVIDERS.get_or_init(|| {
        #[allow(unused_mut)]
        let mut installed: Vec<ExtensionHandle> = Vec::new();
        // Same arrangement `grammars::store` makes for this crate's own
        // tests: the shipped extensions are installed for them here, so a
        // test about run markers is not also a test about installation.
        #[cfg(test)]
        installed.extend(fg_languages::builtin_registry().handles());
        RwLock::new(installed)
    })
}

/// Adds `registry`'s extensions to the ones [`main_entries`] asks. Keeps
/// whichever handle was installed first for an extension id, the rule
/// `grammars::install` follows next door: the two are installed side by
/// side, and opposite rules would let a second registry (another app in the
/// same test process) leave grammars from one and run markers from another.
pub fn install(registry: &Registry) {
    let mut installed = providers().write().expect("run target providers lock");
    for handle in registry.handles() {
        if !installed.iter().any(|existing| existing.id == handle.id) {
            installed.push(handle);
        }
    }
}

/// Every runnable entry point in `source`, in source order — the lines the
/// editor's run gutter marks.
///
/// `file_stem` is the file's name without its extension, which some
/// toolchains need to name what a file compiles into. An extension that
/// does not own `language` contributes nothing, so a language nobody claims
/// simply has no run markers, exactly as before.
pub fn main_entries(tree: &Tree, source: &str, language: Language, file_stem: &str) -> Vec<RunMarker> {
    let providers = providers().read().expect("run target providers lock");
    providers
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
