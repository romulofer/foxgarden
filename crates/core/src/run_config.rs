use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A named "how to run this project" configuration — main class, VM/
/// program args, environment variables, working directory. Stored
/// per-project (`.foxgarden/run_configs.json` under the project root, see
/// `load_run_configs`/`save_run_configs`), not in this app's own
/// `eframe::Storage` settings: a teammate opening the same project
/// benefits from the same "run Main" config already existing, the way
/// they would if it were checked into version control alongside the code
/// it describes — this app's local settings (font, theme, last-open
/// project) are the opposite, deliberately *not* meant to travel with the
/// project.
///
/// Storage/editing only for now — actually *running* one needs process
/// management this project doesn't have yet (see `FEATURES.md`'s "Build/
/// run/test integration").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunConfig {
    pub name: String,
    pub main_class: String,
    pub vm_args: String,
    pub program_args: String,
    /// `(key, value)` pairs, in the order they were added.
    pub env: Vec<(String, String)>,
    /// `None` means "the project root" — the sensible default a run
    /// config doesn't need to state explicitly.
    #[serde(with = "working_dir_as_string")]
    pub working_dir: Option<PathBuf>,
}

/// `PathBuf`'s own `serde` impl rejects a non-UTF-8 path (real, if rare, on
/// Unix) rather than losing information — the wrong tradeoff here, since it
/// would turn one odd `working_dir` into a hard failure that loses every
/// *other* config in the same save. `.display().to_string()` on the way out
/// (lossy for non-UTF-8 bytes, same as the previous hand-rolled format
/// already was) and a plain `PathBuf::from` on the way back keep
/// `serialize_run_configs` genuinely infallible.
mod working_dir_as_string {
    use std::path::PathBuf;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(path: &Option<PathBuf>, serializer: S) -> Result<S::Ok, S::Error> {
        // Delegates to `Option<String>`'s own `Serialize` impl rather than
        // hand-writing the `Some`/`None` match.
        path.as_ref().map(|p| p.display().to_string()).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<PathBuf>, D::Error> {
        Ok(Option::<String>::deserialize(deserializer)?.map(PathBuf::from))
    }
}

/// Parses whatever `save_run_configs` last wrote. Forward-compatible the
/// same way the previous hand-rolled format was: an unrecognized JSON key
/// is ignored by `serde_json` without any extra configuration, and
/// `#[serde(default)]` on `RunConfig` means a field missing entirely (an
/// older FoxGarden version's save, or hand-edited JSON) fills in from
/// `RunConfig::default()` instead of failing the whole parse. Malformed
/// JSON (or no file at all, via `load_run_configs`) yields an empty `Vec`
/// rather than an error — same "most projects never have one yet, and
/// that's not a failure" reasoning `load_run_configs` already documents.
pub fn parse_run_configs(input: &str) -> Vec<RunConfig> {
    serde_json::from_str(input).unwrap_or_default()
}

/// Inverse of `parse_run_configs`. Pretty-printed, since this file is meant
/// to be human-readable/editable alongside the project (see `RunConfig`'s
/// own doc comment) — genuinely infallible for this struct once
/// `working_dir`'s own serialization can't fail (see `working_dir_as_string`
/// above): every other field is a `String`/`Vec<(String, String)>`, neither
/// of which `serde_json` can fail to encode.
pub fn serialize_run_configs(configs: &[RunConfig]) -> String {
    serde_json::to_string_pretty(configs).expect("RunConfig serialization is infallible")
}

fn run_configs_path(project_root: &Path) -> PathBuf {
    project_root.join(".foxgarden").join("run_configs.json")
}

/// Every run config saved for the project at `project_root` — an empty
/// `Vec`, not an error, if none have been saved yet (the common case: most
/// projects never get one).
pub fn load_run_configs(project_root: &Path) -> Vec<RunConfig> {
    std::fs::read_to_string(run_configs_path(project_root))
        .map(|s| parse_run_configs(&s))
        .unwrap_or_default()
}

/// Inverse of `load_run_configs`: writes `configs` to
/// `.foxgarden/run_configs.json` under `project_root`, creating the
/// `.foxgarden` directory first if it doesn't exist yet.
pub fn save_run_configs(project_root: &Path, configs: &[RunConfig]) -> std::io::Result<()> {
    let path = run_configs_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serialize_run_configs(configs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RunConfig {
        RunConfig {
            name: "Run App".to_string(),
            main_class: "com.example.Main".to_string(),
            vm_args: "-Xmx512m".to_string(),
            program_args: "--debug".to_string(),
            env: vec![
                ("DEBUG".to_string(), "true".to_string()),
                ("PORT".to_string(), "8080".to_string()),
            ],
            working_dir: Some(PathBuf::from("/home/user/project")),
        }
    }

    #[test]
    fn a_single_config_round_trips() {
        let configs = vec![sample()];
        let serialized = serialize_run_configs(&configs);
        assert_eq!(parse_run_configs(&serialized), configs);
    }

    #[test]
    fn multiple_configs_round_trip() {
        let mut second = sample();
        second.name = "Run Tests".to_string();
        second.working_dir = None;
        second.env.clear();
        let configs = vec![sample(), second];

        let serialized = serialize_run_configs(&configs);
        assert_eq!(parse_run_configs(&serialized), configs);
    }

    #[test]
    fn an_env_value_containing_equals_signs_round_trips() {
        let mut config = RunConfig::default();
        config.env.push((
            "JDBC_URL".to_string(),
            "jdbc:postgresql://host/db?user=a&pass=b".to_string(),
        ));

        let serialized = serialize_run_configs(&[config.clone()]);
        assert_eq!(parse_run_configs(&serialized), vec![config]);
    }

    #[test]
    fn no_working_dir_round_trips_as_none() {
        let mut config = sample();
        config.working_dir = None;

        let serialized = serialize_run_configs(&[config.clone()]);
        assert_eq!(parse_run_configs(&serialized), vec![config]);
    }

    #[test]
    fn empty_input_parses_to_no_configs() {
        assert_eq!(parse_run_configs(""), vec![]);
        assert_eq!(parse_run_configs("   \n\n  "), vec![]);
    }

    #[test]
    fn unrecognized_keys_are_skipped_rather_than_erroring() {
        let input = r#"[{"name": "Foo", "future_field": "something new", "main_class": "Foo"}]"#;
        let configs = parse_run_configs(input);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "Foo");
        assert_eq!(configs[0].main_class, "Foo");
    }

    #[test]
    fn a_missing_field_defaults_rather_than_failing_the_whole_parse() {
        let input = r#"[{"name": "Foo"}]"#;
        let configs = parse_run_configs(input);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "Foo");
        assert_eq!(configs[0].main_class, "");
        assert_eq!(configs[0].env, vec![]);
        assert_eq!(configs[0].working_dir, None);
    }

    #[test]
    fn load_run_configs_with_no_saved_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_run_configs(dir.path()), vec![]);
    }

    #[test]
    fn load_run_configs_with_malformed_json_returns_empty_rather_than_panicking() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".foxgarden")).unwrap();
        std::fs::write(dir.path().join(".foxgarden").join("run_configs.json"), "not json").unwrap();
        assert_eq!(load_run_configs(dir.path()), vec![]);
    }

    #[test]
    fn save_then_load_round_trips_through_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let configs = vec![sample()];

        save_run_configs(dir.path(), &configs).unwrap();
        let loaded = load_run_configs(dir.path());

        assert_eq!(loaded, configs);
        assert!(dir.path().join(".foxgarden").join("run_configs.json").exists());
    }
}
