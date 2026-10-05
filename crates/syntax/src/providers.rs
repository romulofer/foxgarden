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

use fg_extension::{ExtensionHandle, Registry};

#[derive(Default)]
pub(crate) struct Providers {
    /// Every installed extension, in installation order.
    pub(crate) extensions: Vec<ExtensionHandle>,
    /// Each registered language's file extensions, keyed by language id —
    /// what `code_model` needs to find the file a type is declared in.
    pub(crate) file_extensions: HashMap<String, Vec<String>>,
}

impl Providers {
    fn add(&mut self, registry: &Registry) {
        for handle in registry.handles() {
            if !self.extensions.iter().any(|existing| existing.id == handle.id) {
                self.extensions.push(handle);
            }
        }
        for registered in registry.languages() {
            self.file_extensions
                .entry(registered.language.id.clone())
                .or_insert_with(|| registered.language.file_extensions.clone());
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
