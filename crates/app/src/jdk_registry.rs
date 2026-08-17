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
        self.jdks.push(RegisteredJdk { label: format!("Java {major_version}"), home, major_version: Some(major_version) });
        Ok(())
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
                known.iter().filter(|j| j.major_version.unwrap() >= release).min_by_key(|j| j.major_version.unwrap()).copied()
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
mod tests {
    use super::*;

    fn fake_jdk(dir: &std::path::Path, banner: &str) -> PathBuf {
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let script = bin.join("java");
        std::fs::write(&script, format!("#!/bin/sh\necho '{banner}' >&2\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(perms.mode() | 0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        dir.to_path_buf()
    }

    #[test]
    fn detect_and_add_records_the_real_detected_version() {
        let dir = test_support::tempdir();
        let home = fake_jdk(dir.path(), "openjdk version \"17.0.4\" 2022-07-19 LTS");

        let mut registry = JdkRegistry::default();
        registry.detect_and_add(home.clone()).expect("detects a fake JDK 17");

        assert_eq!(registry.jdks.len(), 1);
        assert_eq!(registry.jdks[0].major_version, Some(17));
        assert_eq!(registry.jdks[0].home, home);
        assert_eq!(registry.jdks[0].label, "Java 17");
    }

    #[test]
    fn detect_and_add_on_an_unreadable_home_is_an_error_and_adds_nothing() {
        let mut registry = JdkRegistry::default();
        let error = registry.detect_and_add(PathBuf::from("/does/not/exist")).expect_err("no java there");
        assert!(!error.is_empty());
        assert!(registry.jdks.is_empty());
    }

    fn known(major: u32) -> RegisteredJdk {
        RegisteredJdk { label: format!("Java {major}"), home: PathBuf::from(format!("/jdk{major}")), major_version: Some(major) }
    }

    #[test]
    fn closest_for_prefers_an_exact_match() {
        let registry = JdkRegistry { jdks: vec![known(8), known(17), known(21)] };
        assert_eq!(registry.closest_for(17).unwrap().major_version, Some(17));
    }

    #[test]
    fn closest_for_falls_back_to_the_smallest_newer_version() {
        let registry = JdkRegistry { jdks: vec![known(8), known(21)] };
        // No JDK 11 registered — 21 is the smallest one still >= 11.
        assert_eq!(registry.closest_for(11).unwrap().major_version, Some(21));
    }

    #[test]
    fn closest_for_falls_back_to_the_largest_available_when_everything_is_older() {
        let registry = JdkRegistry { jdks: vec![known(8), known(11)] };
        // Nothing registered is >= 21 — 11 (the largest available) beats
        // refusing to suggest anything at all.
        assert_eq!(registry.closest_for(21).unwrap().major_version, Some(11));
    }

    #[test]
    fn closest_for_on_an_empty_registry_is_none() {
        assert!(JdkRegistry::default().closest_for(17).is_none());
    }

    #[test]
    fn closest_for_ignores_entries_with_an_unverified_version() {
        let registry = JdkRegistry {
            jdks: vec![RegisteredJdk { label: "mystery".to_string(), home: PathBuf::from("/mystery"), major_version: None }],
        };
        assert!(registry.closest_for(17).is_none());
    }

    #[test]
    fn json_round_trips_through_to_json_and_from_json() {
        let registry = JdkRegistry { jdks: vec![known(17), known(21)] };
        let restored = JdkRegistry::from_json(&registry.to_json());
        assert_eq!(registry, restored);
    }

    #[test]
    fn from_json_on_malformed_input_is_an_empty_registry_not_a_panic() {
        assert_eq!(JdkRegistry::from_json("not json"), JdkRegistry::default());
        assert_eq!(JdkRegistry::from_json(""), JdkRegistry::default());
    }

    #[test]
    fn remove_drops_the_entry_at_the_given_index() {
        let mut registry = JdkRegistry { jdks: vec![known(8), known(17)] };
        registry.remove(0);
        assert_eq!(registry.jdks, vec![known(17)]);
    }

    #[test]
    fn remove_on_an_out_of_range_index_is_a_no_op_not_a_panic() {
        let mut registry = JdkRegistry { jdks: vec![known(8)] };
        registry.remove(5);
        assert_eq!(registry.jdks.len(), 1);
    }
}
