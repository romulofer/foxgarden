//! Machine-wide inventory of registered JDKs (`PLAN.md` Track 29 Phase 1)
//! — separate from `LspSettings::jdtls_java_home`, which is "which JVM runs
//! jdt.ls itself" (always 21+, `lsp_manager::resolve_jdtls_java`); this is
//! "which JDKs exist on this machine to *target*" (any version), for Track
//! 29's later phases (the unmanaged-file `java.configuration.runtimes`
//! wiring, the New Project wizard's own JDK picker). Persisted globally via
//! `eframe::Storage`, same as `LspSettings` — a JDK install is a fact about
//! the machine, not any one project.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::jdk;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredJdk {
    pub label: String,
    pub home: PathBuf,
    /// `None` only if a JDK saved by an older FoxGarden version is ever
    /// re-detected as unreadable — `detect_and_add` itself always fails
    /// rather than pushing an entry it couldn't verify, so within one
    /// session every entry has `Some`.
    pub major_version: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JdkRegistry {
    pub jdks: Vec<RegisteredJdk>,
}

impl JdkRegistry {
    /// Detects `home`'s major version via a real `java -version` and adds
    /// it, labeled from that version (e.g. "Java 21"). Fails rather than
    /// adding an entry this registry couldn't actually verify — an
    /// unverified entry would silently mislead `closest_for` later.
    pub fn detect_and_add(&mut self, home: PathBuf) -> Result<(), String> {
        let major_version = jdk::detect_major_version(&home.to_string_lossy())?;
        self.jdks.push(RegisteredJdk {
            label: format!("Java {major_version}"),
            home,
            major_version: Some(major_version),
        });
        Ok(())
    }

    /// Adds `home` at an already-known `major_version`, skipping a real
    /// `java -version` spawn — for a caller (an auto-detect scan) that
    /// verified it another way. Silently skips a `home` already registered,
    /// so re-running a scan against an unchanged machine is a no-op rather
    /// than piling up duplicate entries (see TECHNICAL_DEBT.md #24: this is
    /// what lets `lsp_manager::installed_runtimes`'s own machine-scanning
    /// knowledge — sdkman/asdf/jenv layouts, macOS bundles — feed this
    /// registry instead of being reimplemented here a second time).
    pub fn add_known(&mut self, home: PathBuf, major_version: u32) {
        if self.jdks.iter().any(|jdk| jdk.home == home) {
            return;
        }
        self.jdks.push(RegisteredJdk {
            label: format!("Java {major_version}"),
            home,
            major_version: Some(major_version),
        });
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.jdks.len() {
            self.jdks.remove(index);
        }
    }

    /// The best registered match for targeting `release`: an exact version
    /// match first, else the smallest registered version that's still
    /// `>= release` (a newer JDK can typically still target an older
    /// release, e.g. via `--release`), else the largest one available —
    /// better than nothing if every registered JDK is older than needed.
    /// Entries with an unverified (`None`) version are never considered.
    ///
    /// Not called yet — `PLAN.md` Track 29's later phases (the unmanaged-
    /// file `java.configuration.runtimes` wiring, the New Project wizard's
    /// own JDK picker) are this method's real callers; kept and tested now
    /// since it's part of Phase 1's own registry API, not speculative.
    #[allow(dead_code)]
    pub fn closest_for(&self, release: u32) -> Option<&RegisteredJdk> {
        let known: Vec<&RegisteredJdk> = self.jdks.iter().filter(|j| j.major_version.is_some()).collect();
        known
            .iter()
            .find(|j| j.major_version == Some(release))
            .copied()
            .or_else(|| {
                known
                    .iter()
                    .filter(|j| j.major_version.unwrap() >= release)
                    .min_by_key(|j| j.major_version.unwrap())
                    .copied()
            })
            .or_else(|| known.iter().max_by_key(|j| j.major_version.unwrap()).copied())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Malformed or empty input decodes to an empty registry rather than an
    /// error — same "corrupt/missing persisted state is never fatal, just
    /// starts fresh" discipline every other `eframe::Storage`-backed value
    /// in this app already follows.
    pub fn from_json(input: &str) -> Self {
        serde_json::from_str(input).unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "jdk_registry_test.rs"]
mod jdk_registry_test;
