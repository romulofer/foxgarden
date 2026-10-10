//! The extensions asked per-file questions from inside this crate — where a
//! file's run markers go (`run_targets`), which HTTP routes it declares
//! (`http_routes`), what a type's members are (`code_model`).
//!
//! **Installed process-wide, like the grammars next door**, and for the
//! same reason: the answers are wanted deep inside the editor's paint path
//! (`widgets::editor::widget`) and on background scan threads, neither of
//! which has a registry in reach, and the set of extensions never changes
//! after startup. `install` is called next to `grammars::install`, so an
//! editor that has one has the other.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock, RwLockReadGuard};

use fg_extension::{ExtensionHandle, Registry, Snippet};

#[derive(Default)]
pub(crate) struct Providers {
    /// Every installed extension, in installation order.
    pub(crate) extensions: Vec<ExtensionHandle>,
    /// Each registered language's file extensions, keyed by language id —
    /// what `code_model` needs to find the file a type is declared in.
    pub(crate) file_extensions: HashMap<String, Vec<String>>,
    /// Each language's live templates and reserved words, gathered once at
    /// installation from every extension that has any: word completion asks
    /// for them on a keystroke, and they never change afterwards.
    pub(crate) snippets: HashMap<String, Vec<Snippet>>,
    pub(crate) keywords: HashMap<String, Vec<String>>,
    /// Languages a breakpoint gutter is drawn for.
    pub(crate) debuggable: std::collections::HashSet<String>,
}

impl Providers {
    fn add(&mut self, registry: &Registry) {
        for handle in registry.handles() {
            if !self.extensions.iter().any(|existing| existing.id == handle.id) {
                self.extensions.push(handle);
            }
        }
        for registered in registry.languages() {
            let id = &registered.language.id;
            if registry.is_debuggable(id) {
                self.debuggable.insert(id.clone());
            }
            if self.file_extensions.contains_key(id) {
                continue;
            }
            self.file_extensions
                .insert(id.clone(), registered.language.file_extensions.clone());
            let mut snippets: Vec<Snippet> = Vec::new();
            let mut keywords: Vec<String> = Vec::new();
            for extension in &self.extensions {
                for snippet in extension.snippets(id) {
                    // Two extensions on one language may define the same
                    // trigger; the first installed wins, as with every
                    // other first-answer-wins lookup here.
                    if !snippets.iter().any(|s| s.trigger == snippet.trigger) {
                        snippets.push(snippet);
                    }
                }
                for keyword in extension.keywords(id) {
                    if !keywords.contains(&keyword) {
                        keywords.push(keyword);
                    }
                }
            }
            self.snippets.insert(id.clone(), snippets);
            self.keywords.insert(id.clone(), keywords);
        }
    }
}

fn store() -> &'static RwLock<Providers> {
    static PROVIDERS: OnceLock<RwLock<Providers>> = OnceLock::new();
    PROVIDERS.get_or_init(|| {
        #[allow(unused_mut)]
        let mut installed = Providers::default();
        // Same arrangement `grammars::store` makes for this crate's own
        // tests: the shipped extensions are installed for them here, so a
        // test about run markers is not also a test about installation.
        #[cfg(test)]
        installed.add(&fg_languages::builtin_registry());
        RwLock::new(installed)
    })
}

/// Every installed extension and what they registered.
pub(crate) fn providers() -> RwLockReadGuard<'static, Providers> {
    store().read().expect("extension providers lock")
}

/// Adds `registry`'s extensions to the ones this crate asks. Keeps
/// whichever handle was installed first for an extension id, the rule
/// `grammars::install` follows next door: the two are installed side by
/// side, and opposite rules would let a second registry (another app in the
/// same test process) leave grammars from one and run markers from another.
pub fn install(registry: &Registry) {
    store().write().expect("extension providers lock").add(registry);
}
