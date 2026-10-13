//! Machine-wide inventory of the runtimes an extension keeps one for (JDKs,
//! for the JVM extension) — `PLAN.md` Track 29 Phase 1. What a runtime is
//! and how an install is recognized belongs to the extension
//! (`fg_extension::RuntimeHandle`); this is only the list the user has
//! registered. Separate from a server's `HostRuntime` home
//! (`LspSettings::runtime_home_for`), which is "which install runs the
//! language server itself", where this is "which installs exist on this machine
//! to *target*" (any version). Persisted globally via `eframe::Storage`,
//! same as `LspSettings` — an install is a fact about the machine, not any
//! one project.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fg_extension::RuntimeInstall;
use serde::{Deserialize, Serialize};

/// One inventory per runtime kind, keyed by `RuntimeContribution::id`.
pub type RuntimeInventories = BTreeMap<String, RuntimeInventory>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredRuntime {
    pub label: String,
    pub home: PathBuf,
    /// `None` only if an entry saved by an older FoxGarden version is ever
    /// re-detected as unreadable — an install is only ever added after the
    /// extension verified it, so within one session every entry has `Some`.
    pub major_version: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInventory {
    /// `jdks` is what this list was called while it only held JDKs.
    #[serde(alias = "jdks")]
    pub runtimes: Vec<RegisteredRuntime>,
}

impl RuntimeInventory {
    /// Adds a verified install. Silently skips a `home` already registered,
    /// so re-running a scan against an unchanged machine is a no-op rather
    /// than piling up duplicate entries.
    pub fn add(&mut self, install: RuntimeInstall) {
        if self.runtimes.iter().any(|runtime| runtime.home == install.home) {
            return;
        }
        self.runtimes.push(RegisteredRuntime {
            label: install.label,
            home: install.home,
            major_version: Some(install.major_version),
        });
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.runtimes.len() {
            self.runtimes.remove(index);
        }
    }

    /// The best registered match for targeting `release`: an exact version
    /// match first, else the smallest registered version that's still
    /// `>= release` (a newer install can typically still target an older
    /// release, e.g. via `--release`), else the largest one available —
    /// better than nothing if every registered install is older than needed.
    /// Entries with an unverified (`None`) version are never considered.
    ///
    /// Not called yet — `PLAN.md` Track 29's later phases (the unmanaged-
    /// file `java.configuration.runtimes` wiring, the New Project wizard's
    /// own JDK picker) are this method's real callers; kept and tested now
    /// since it's part of Phase 1's own registry API, not speculative.
    #[allow(dead_code)]
    pub fn closest_for(&self, release: u32) -> Option<&RegisteredRuntime> {
        let known: Vec<&RegisteredRuntime> = self.runtimes.iter().filter(|r| r.major_version.is_some()).collect();
        known
            .iter()
            .find(|r| r.major_version == Some(release))
            .copied()
            .or_else(|| {
                known
                    .iter()
                    .filter(|r| r.major_version.unwrap() >= release)
                    .min_by_key(|r| r.major_version.unwrap())
                    .copied()
            })
            .or_else(|| known.iter().max_by_key(|r| r.major_version.unwrap()).copied())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Malformed or empty input decodes to an empty inventory rather than an
    /// error — same "corrupt/missing persisted state is never fatal, just
    /// starts fresh" discipline every other `eframe::Storage`-backed value
    /// in this app already follows.
    pub fn from_json(input: &str) -> Self {
        serde_json::from_str(input).unwrap_or_default()
    }
}

fn storage_key(kind: &str) -> String {
    format!("runtimes_{kind}")
}

/// What the key was while the only kind was the JDK inventory
/// (`jdk_registry`). Still read when the new key has nothing, so an
/// existing install keeps its JDKs.
fn legacy_storage_key(kind: &str) -> String {
    format!("{kind}_registry")
}

pub fn restore(storage: &dyn eframe::Storage, kind: &str) -> RuntimeInventory {
    storage
        .get_string(&storage_key(kind))
        .or_else(|| storage.get_string(&legacy_storage_key(kind)))
        .map(|saved| RuntimeInventory::from_json(&saved))
        .unwrap_or_default()
}

pub fn persist(storage: &mut dyn eframe::Storage, kind: &str, inventory: &RuntimeInventory) {
    storage.set_string(&storage_key(kind), inventory.to_json());
}

#[cfg(test)]
#[path = "runtime_inventory_test.rs"]
mod runtime_inventory_test;
