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
#[path = "run_config_test.rs"]
mod run_config_test;
