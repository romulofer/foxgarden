use std::path::{Path, PathBuf};

/// A named "how to run this project" configuration — main class, VM/
/// program args, environment variables, working directory. Stored
/// per-project (`.foxgarden/run_configs.txt` under the project root, see
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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunConfig {
    pub name: String,
    pub main_class: String,
    pub vm_args: String,
    pub program_args: String,
    /// `(key, value)` pairs, in the order they were added.
    pub env: Vec<(String, String)>,
    /// `None` means "the project root" — the sensible default a run
    /// config doesn't need to state explicitly.
    pub working_dir: Option<PathBuf>,
}

const ENV_KEY: &str = "env";
const WORKING_DIR_KEY: &str = "working_dir";

/// Serializes one `RunConfig` as `key=value` lines — `env` repeats once
/// per variable (`env=KEY=VALUE`), matching how a real environment
/// variable's own value can itself contain `=` (only the *first* `=` on
/// an `env` line is the key/value separator, same as `line.split_once('=')`
/// already assumes for every other key). No line ending up containing a
/// literal `\n` is relied on throughout — same "a value can't itself
/// contain the separator" contract `app.rs`'s `OPEN_TABS_KEY` encoding
/// already leans on for newline-joined paths.
fn serialize_one(config: &RunConfig) -> String {
    let mut lines = vec![
        format!("name={}", config.name),
        format!("main_class={}", config.main_class),
        format!("vm_args={}", config.vm_args),
        format!("program_args={}", config.program_args),
    ];
    if let Some(dir) = &config.working_dir {
        lines.push(format!("{WORKING_DIR_KEY}={}", dir.display()));
    }
    for (key, value) in &config.env {
        lines.push(format!("{ENV_KEY}={key}={value}"));
    }
    lines.join("\n")
}

fn parse_block(block: &str) -> RunConfig {
    let mut config = RunConfig::default();
    for line in block.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        match key {
            "name" => config.name = value.to_string(),
            "main_class" => config.main_class = value.to_string(),
            "vm_args" => config.vm_args = value.to_string(),
            "program_args" => config.program_args = value.to_string(),
            WORKING_DIR_KEY => config.working_dir = (!value.is_empty()).then(|| PathBuf::from(value)),
            ENV_KEY => {
                if let Some((env_key, env_value)) = value.split_once('=') {
                    config.env.push((env_key.to_string(), env_value.to_string()));
                }
            }
            // Forward-compatible: an unrecognized key (a newer FoxGarden
            // version's field, or hand-edited noise) is skipped rather
            // than treated as a parse error, so this format can grow
            // without a version marker or breaking older readers.
            _ => {}
        }
    }
    config
}

/// Parses the hand-rolled format `save_run_configs` writes: one config per
/// blank-line-separated block, `key=value` lines within it. No `serde`/
/// `toml` dependency needed for something this small (see `save_run_configs`'
/// doc comment) — a config with an empty/missing `name` is still parsed
/// (not skipped), it just renders as "(unnamed)" wherever a name is shown.
pub fn parse_run_configs(input: &str) -> Vec<RunConfig> {
    input.split("\n\n").map(str::trim).filter(|block| !block.is_empty()).map(parse_block).collect()
}

/// Inverse of `parse_run_configs`.
pub fn serialize_run_configs(configs: &[RunConfig]) -> String {
    configs.iter().map(serialize_one).collect::<Vec<_>>().join("\n\n")
}

fn run_configs_path(project_root: &Path) -> PathBuf {
    project_root.join(".foxgarden").join("run_configs.txt")
}

/// Every run config saved for the project at `project_root` — an empty
/// `Vec`, not an error, if none have been saved yet (the common case: most
/// projects never get one).
pub fn load_run_configs(project_root: &Path) -> Vec<RunConfig> {
    std::fs::read_to_string(run_configs_path(project_root)).map(|s| parse_run_configs(&s)).unwrap_or_default()
}

/// Inverse of `load_run_configs`: writes `configs` to
/// `.foxgarden/run_configs.txt` under `project_root`, creating the
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
            env: vec![("DEBUG".to_string(), "true".to_string()), ("PORT".to_string(), "8080".to_string())],
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
        config.env.push(("JDBC_URL".to_string(), "jdbc:postgresql://host/db?user=a&pass=b".to_string()));

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
        let input = "name=Foo\nfuture_field=something new\nmain_class=Foo";
        let configs = parse_run_configs(input);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "Foo");
        assert_eq!(configs[0].main_class, "Foo");
    }

    #[test]
    fn load_run_configs_with_no_saved_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_run_configs(dir.path()), vec![]);
    }

    #[test]
    fn save_then_load_round_trips_through_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let configs = vec![sample()];

        save_run_configs(dir.path(), &configs).unwrap();
        let loaded = load_run_configs(dir.path());

        assert_eq!(loaded, configs);
        assert!(dir.path().join(".foxgarden").join("run_configs.txt").exists());
    }
}
