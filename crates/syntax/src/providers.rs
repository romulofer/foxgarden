//! The extensions asked per-file questions from inside this crate — where a
//! file's run markers go (`run_targets`), which HTTP routes it declares
//! (`http_routes`).
//!
//! **Installed process-wide, like the grammars next door**, and for the
//! same reason: the answers are wanted deep inside the editor's paint path
//! (`widgets::editor::widget`) and on background scan threads, neither of
//! which has a registry in reach, and the set of extensions never changes
//! after startup. `install` is called next to `grammars::install`, so an
//! editor that has one has the other.

use std::sync::{OnceLock, RwLock, RwLockReadGuard};

use fg_extension::{ExtensionHandle, Registry};

type Providers = RwLock<Vec<ExtensionHandle>>;

fn store() -> &'static Providers {
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

/// Every installed extension, in installation order.
pub(crate) fn providers() -> RwLockReadGuard<'static, Vec<ExtensionHandle>> {
    store().read().expect("extension providers lock")
}

/// Adds `registry`'s extensions to the ones this crate asks. Keeps
/// whichever handle was installed first for an extension id, the rule
/// `grammars::install` follows next door: the two are installed side by
/// side, and opposite rules would let a second registry (another app in the
/// same test process) leave grammars from one and run markers from another.
pub fn install(registry: &Registry) {
    let mut installed = store().write().expect("extension providers lock");
    for handle in registry.handles() {
        if !installed.iter().any(|existing| existing.id == handle.id) {
            installed.push(handle);
        }
    }
}
