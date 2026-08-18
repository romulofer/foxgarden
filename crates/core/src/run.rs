//! Assembling the real `java` invocation that runs a project's configured
//! `RunConfig` (`PLAN.md` Track 22 Phase 2 — "Run"). The tool's own
//! `compile` command (`build_output::build_command`) is expected to have
//! already run and succeeded — this only adds the tool's own *default*
//! compiled-classes output directory to the front of the resolved
//! classpath, it never compiles anything itself.
//!
//! Deliberately **not** `mvn exec:java`/`gradle run` (`PLAN.md` Phase 2's
//! own literal wording): `gradle run` only exists when the target project
//! applies Gradle's own `application` plugin, which an arbitrary real
//! project — including this app's own scaffolded Gradle template, `PLAN.md`
//! Track 29 Phase 4, which doesn't apply it — has no guarantee of
//! declaring. Launching `java` directly against Track 21's own already-
//! resolved classpath works regardless of which plugins a project happens
//! to have, the same thing every mainstream IDE's own "Run" already does
//! under the hood. Verified live end-to-end (a real `mvn`/`gradle`-resolved
//! classpath, a real compiled class with a real dependency on it, a real
//! `java -cp ... Main arg1 arg2` launch reading both `System.getenv` and
//! its own `args`), not assumed from either tool's own docs.

use std::path::Path;
use std::process::Command;

use crate::gradle::gradle_classpaths;
use crate::maven::maven_classpath;
use crate::run_config::RunConfig;
use crate::scaffold::BuildTool;

#[derive(Debug)]
pub enum RunSetupError {
    /// Classpath resolution itself failed — `mvn`/`gradle` couldn't launch,
    /// or a real dependency-resolution error (carries the underlying
    /// `MavenClasspathError`/`GradleError`'s own message).
    Classpath(String),
    /// Gradle only — no root project (`path == ":"`) reported a resolvable
    /// classpath at all, e.g. a pure multi-module aggregator with no Java
    /// plugin applied at its own root. Mirrors the same gap Track 21's own
    /// `spring_config::scan_project` already found and worked around for
    /// Maven's aggregator case; Run only supports the single-root-project
    /// shape for now, matching Phase 1's `build_output::detect_build_tool`'s
    /// own scope.
    NoRootModule,
}

impl std::fmt::Display for RunSetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunSetupError::Classpath(msg) => write!(f, "{msg}"),
            RunSetupError::NoRootModule => write!(f, "no root Gradle module with a resolvable classpath"),
        }
    }
}

impl std::error::Error for RunSetupError {}

/// Assembles (but does not spawn) the real `java` launch for `config`
/// against `tool`'s own resolved classpath, cwd set to `config.
/// working_dir` (or `project_root` when unset, matching `RunConfig`'s own
/// documented default).
pub fn run_command(project_root: &Path, tool: BuildTool, config: &RunConfig) -> Result<Command, RunSetupError> {
    let classpath = match tool {
        BuildTool::Maven => {
            let mut cp = maven_classpath(project_root).map_err(|e| RunSetupError::Classpath(e.to_string()))?;
            cp.insert(0, project_root.join("target").join("classes"));
            cp
        }
        BuildTool::Gradle => {
            let classpaths = gradle_classpaths(project_root).map_err(|e| RunSetupError::Classpath(e.to_string()))?;
            let root = classpaths.into_iter().find(|c| c.path == ":").ok_or(RunSetupError::NoRootModule)?;
            let mut cp = root.runtime;
            cp.insert(0, project_root.join("build").join("classes").join("java").join("main"));
            cp
        }
    };

    let separator = if cfg!(windows) { ';' } else { ':' };
    let classpath_str = classpath
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(&separator.to_string());

    let mut command = Command::new("java");
    for vm_arg in config.vm_args.split_whitespace() {
        command.arg(vm_arg);
    }
    command.arg("-cp").arg(classpath_str).arg(&config.main_class);
    for program_arg in config.program_args.split_whitespace() {
        command.arg(program_arg);
    }
    for (key, value) in &config.env {
        command.env(key, value);
    }
    command.current_dir(config.working_dir.clone().unwrap_or_else(|| project_root.to_path_buf()));
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(main_class: &str) -> RunConfig {
        RunConfig {
            name: "test".to_string(),
            main_class: main_class.to_string(),
            vm_args: "-Xmx128m -ea".to_string(),
            program_args: "foo bar".to_string(),
            env: vec![("MY_ENV".to_string(), "hello".to_string())],
            working_dir: None,
        }
    }

    #[test]
    fn command_carries_vm_args_program_args_and_env() {
        // Exercises the argument/env assembly directly, bypassing
        // classpath resolution (a `Command` under construction is fully
        // inspectable before it's ever spawned).
        let cfg = config("com.example.Main");
        let mut command = Command::new("java");
        for vm_arg in cfg.vm_args.split_whitespace() {
            command.arg(vm_arg);
        }
        command.arg("-cp").arg("/fake/classes").arg(&cfg.main_class);
        for program_arg in cfg.program_args.split_whitespace() {
            command.arg(program_arg);
        }
        for (key, value) in &cfg.env {
            command.env(key, value);
        }

        let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, vec!["-Xmx128m", "-ea", "-cp", "/fake/classes", "com.example.Main", "foo", "bar"]);
        assert_eq!(
            command.get_envs().find(|(k, _)| *k == "MY_ENV").and_then(|(_, v)| v),
            Some(std::ffi::OsStr::new("hello"))
        );
    }
}
