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

impl RunConfig {
    /// The subset a build tool's extension needs to assemble a real launch —
    /// everything but this config's own name, which is editor bookkeeping.
    pub fn to_run_spec(&self) -> fg_extension::RunSpec {
        fg_extension::RunSpec {
            entry_point: self.main_class.clone(),
            vm_args: self.vm_args.clone(),
            program_args: self.program_args.clone(),
            env: self.env.clone(),
            working_dir: self.working_dir.clone(),
        }
    }
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
/// `RunConfig::default()` instead of failing the whole parse. An empty file
/// is no configs. Malformed JSON is an error, not an empty list: the file
/// is meant to be hand-edited, and reading a stray comma as "no configs"
/// would let the next save overwrite every config the user had.
pub fn parse_run_configs(input: &str) -> Result<Vec<RunConfig>, serde_json::Error> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(input)
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
/// projects never get one). A file that exists but cannot be read or
/// parsed is an `Err` naming it, so a caller never mistakes it for "none
/// saved" and writes over it.
pub fn load_run_configs(project_root: &Path) -> Result<Vec<RunConfig>, String> {
    let path = run_configs_path(project_root);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{}: {err}", path.display())),
    };
    parse_run_configs(&text).map_err(|err| format!("{}: {err}", path.display()))
}

/// Inverse of `load_run_configs`: writes `configs` to
/// `.foxgarden/run_configs.json` under `project_root`, creating the
/// `.foxgarden` directory first if it doesn't exist yet.
pub fn save_run_configs(project_root: &Path, configs: &[RunConfig]) -> std::io::Result<()> {
    let path = run_configs_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::write_atomically(&path, serialize_run_configs(configs).as_bytes())
}

#[cfg(test)]
#[path = "run_config_test.rs"]
mod run_config_test;
