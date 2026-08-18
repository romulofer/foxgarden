use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Per-project settings written once at scaffold time (`PLAN.md` Track 29
/// Phase 3), sibling of `run_config.rs`'s own `.foxgarden/run_configs.json`
/// convention — checked into the project like the code it describes, not
/// this app's own `eframe::Storage` (that's for settings that shouldn't
/// travel with the project, like the last-open project or the UI theme).
///
/// An *existing* project opened normally (not created through the wizard)
/// simply has none of this — `#[serde(default)]` plus `load_project_config`
/// returning the default on a missing file means "no `.foxgarden/
/// project.json` at all" and "one that says nothing in particular" are the
/// same case, and Track 29 Phase 2's own `java_release()`/jdt.ls-native-
/// import logic is exactly the fallback that already handles it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    /// The release the project was scaffolded for — redundant with what
    /// `pom.xml`/`build.gradle(.kts)` itself already states (Track 29 Phase
    /// 2 reads that directly), kept here anyway as the wizard's own record
    /// of intent, independent of a build file a user could hand-edit later.
    pub java_release: Option<u32>,
    /// The specific JDK (from the machine-wide `JdkRegistry`, `PLAN.md`
    /// Track 29 Phase 1) the wizard scaffolded this project against, if the
    /// user picked one rather than leaving it to auto-detection.
    #[serde(with = "jdk_home_as_string")]
    pub jdk_home: Option<PathBuf>,
}

/// Same "lossy but infallible" reasoning as `run_config.rs`'s own
/// `working_dir_as_string`: a non-UTF-8 `jdk_home` (real, if rare, on Unix)
/// should never turn one field into a hard failure that loses the rest of
/// the file.
mod jdk_home_as_string {
    use std::path::PathBuf;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(path: &Option<PathBuf>, serializer: S) -> Result<S::Ok, S::Error> {
        path.as_ref().map(|p| p.display().to_string()).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<PathBuf>, D::Error> {
        Ok(Option::<String>::deserialize(deserializer)?.map(PathBuf::from))
    }
}

fn project_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".foxgarden").join("project.json")
}

/// Parses whatever `save_project_config` last wrote — or, forward-
/// compatibly, whatever a newer FoxGarden version wrote and this one
/// doesn't fully understand yet: an unrecognized key is ignored, and
/// `#[serde(default)]` means a missing field fills in from
/// `ProjectConfig::default()` rather than failing the whole parse.
/// Malformed JSON (or no file at all, via `load_project_config`) yields the
/// default config rather than an error — same "missing/corrupt persisted
/// state is never fatal" discipline every other file like this in this
/// codebase already follows.
pub fn parse_project_config(input: &str) -> ProjectConfig {
    serde_json::from_str(input).unwrap_or_default()
}

pub fn serialize_project_config(config: &ProjectConfig) -> String {
    serde_json::to_string_pretty(config).expect("ProjectConfig serialization is infallible")
}

/// The config saved for the project at `project_root` — `ProjectConfig::
/// default()`, not an error, if none was ever saved (the common case: only
/// a project created through the New Project wizard has one at all).
pub fn load_project_config(project_root: &Path) -> ProjectConfig {
    std::fs::read_to_string(project_config_path(project_root))
        .map(|s| parse_project_config(&s))
        .unwrap_or_default()
}

/// Inverse of `load_project_config`: writes `config` to `.foxgarden/
/// project.json` under `project_root`, creating the `.foxgarden` directory
/// first if it doesn't exist yet.
pub fn save_project_config(project_root: &Path, config: &ProjectConfig) -> std::io::Result<()> {
    let path = project_config_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serialize_project_config(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ProjectConfig {
        ProjectConfig { java_release: Some(17), jdk_home: Some(PathBuf::from("/opt/jdk17")) }
    }

    #[test]
    fn a_config_round_trips() {
        let config = sample();
        let serialized = serialize_project_config(&config);
        assert_eq!(parse_project_config(&serialized), config);
    }

    #[test]
    fn no_jdk_home_round_trips_as_none() {
        let config = ProjectConfig { jdk_home: None, ..sample() };
        let serialized = serialize_project_config(&config);
        assert_eq!(parse_project_config(&serialized), config);
    }

    #[test]
    fn empty_input_parses_to_the_default_config() {
        assert_eq!(parse_project_config(""), ProjectConfig::default());
        assert_eq!(parse_project_config("not json"), ProjectConfig::default());
    }

    #[test]
    fn a_missing_field_defaults_rather_than_failing_the_whole_parse() {
        let config = parse_project_config(r#"{"java_release": 21}"#);
        assert_eq!(config.java_release, Some(21));
        assert_eq!(config.jdk_home, None);
    }

    #[test]
    fn unrecognized_keys_are_skipped_rather_than_erroring() {
        let config = parse_project_config(r#"{"java_release": 21, "future_field": "something new"}"#);
        assert_eq!(config.java_release, Some(21));
    }

    #[test]
    fn load_project_config_with_no_saved_file_returns_the_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_project_config(dir.path()), ProjectConfig::default());
    }

    #[test]
    fn load_project_config_with_malformed_json_returns_the_default_rather_than_panicking() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".foxgarden")).unwrap();
        std::fs::write(dir.path().join(".foxgarden").join("project.json"), "not json").unwrap();
        assert_eq!(load_project_config(dir.path()), ProjectConfig::default());
    }

    #[test]
    fn save_then_load_round_trips_through_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let config = sample();

        save_project_config(dir.path(), &config).unwrap();
        let loaded = load_project_config(dir.path());

        assert_eq!(loaded, config);
        assert!(dir.path().join(".foxgarden").join("project.json").exists());
    }
}
