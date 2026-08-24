//! Detecting a project's Docker/Compose setup and assembling the real
//! `docker build`/`docker run`/`docker compose up` invocations for it
//! (`PLAN.md` Track 14 Phase 1). Spawning/streaming lives in `crates/app`
//! (`panels::build_panel`, reusing the same background-thread-plus-channel
//! plumbing `Build/run/test integration` (Track 22) already built), not
//! here — this crate stays headless (`AGENTS.md`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// A `Dockerfile` at the project root (`docker build`'s own default build
/// context) — no support for a Dockerfile named or located elsewhere,
/// matching `detect_build_tool`'s own "root-only" scope for `pom.xml`/
/// `build.gradle`.
pub fn has_dockerfile(project_root: &Path) -> bool {
    project_root.join("Dockerfile").is_file()
}

/// The Compose file at `project_root`, if any, in Compose's own real
/// discovery precedence — verified against the Compose Specification's
/// documented file-discovery order, not guessed: `compose.yaml` first
/// (the current spec's preferred name and extension), then `compose.yml`,
/// then the two legacy `docker-compose.y[a]ml` names kept only for
/// backwards compatibility.
pub fn compose_file(project_root: &Path) -> Option<PathBuf> {
    ["compose.yaml", "compose.yml", "docker-compose.yaml", "docker-compose.yml"]
        .into_iter()
        .map(|name| project_root.join(name))
        .find(|path| path.is_file())
}

/// A valid, stable `docker build -t` tag derived from the project
/// directory's own name — Docker tags must start with an alphanumeric and
/// contain only `[a-zA-Z0-9_.-]` afterward, which an arbitrary directory
/// name (spaces, accents, a leading dot) isn't guaranteed to satisfy.
/// `foxgarden-` prefixed so a build this app starts is visibly
/// distinguishable in `docker images`/`docker ps` from one the user built
/// by hand.
fn docker_image_tag(project_root: &Path) -> String {
    let name = project_root.file_name().and_then(|n| n.to_str()).unwrap_or("project");
    let sanitized: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '-' })
        .collect();
    format!("foxgarden-{sanitized}")
}

/// Assembles (but does not spawn) `docker build` for the `Dockerfile` at
/// `project_root`, cwd set there so the build context is the project root
/// itself. `--progress=plain` forces the same one-line-per-step, no-
/// cursor-movement output regardless of whether stdout is a real terminal
/// — verified against a real BuildKit run: without it, a piped (non-tty)
/// stdout already falls back to a plain-ish mode on its own, but pinning it
/// explicitly (mirroring `build_command`'s own `-B`/`--console=plain`
/// precedent) keeps that behavior from depending on BuildKit's own
/// auto-detection heuristic.
pub fn docker_build_command(project_root: &Path) -> Command {
    let mut command = Command::new("docker");
    command
        .args(["build", "--progress=plain", "-t", &docker_image_tag(project_root), "."])
        .current_dir(project_root);
    command
}

/// Assembles `docker run --rm` for the image `docker_build_command` just
/// built for `project_root` — `--rm` so a one-shot run doesn't leave a
/// stopped container behind every time (Phase 2's own lifecycle tracking
/// is for the container while it's *running*, not for accumulating exited
/// ones after each Build & Run).
pub fn docker_run_command(project_root: &Path) -> Command {
    let mut command = Command::new("docker");
    command.args(["run", "--rm", &docker_image_tag(project_root)]);
    command
}

/// Assembles `docker compose up --build` against `compose_file` (as found
/// by `compose_file`), cwd set to its parent directory so any relative
/// `build.context:`/volume path inside it resolves the way running `docker
/// compose` by hand from the project root would.
pub fn docker_compose_up_command(compose_file: &Path) -> Command {
    let mut command = Command::new("docker");
    command.arg("compose").arg("-f").arg(compose_file).arg("up").arg("--build");
    if let Some(parent) = compose_file.parent() {
        command.current_dir(parent);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_dockerfile_is_true_only_with_a_real_file_at_the_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_dockerfile(dir.path()));
        std::fs::write(dir.path().join("Dockerfile"), "").unwrap();
        assert!(has_dockerfile(dir.path()));
    }

    #[test]
    fn compose_file_prefers_compose_yaml_over_every_other_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("docker-compose.yml"), "").unwrap();
        std::fs::write(dir.path().join("docker-compose.yaml"), "").unwrap();
        std::fs::write(dir.path().join("compose.yml"), "").unwrap();
        std::fs::write(dir.path().join("compose.yaml"), "").unwrap();
        assert_eq!(compose_file(dir.path()), Some(dir.path().join("compose.yaml")));
    }

    #[test]
    fn compose_file_falls_back_to_legacy_names_in_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("docker-compose.yml"), "").unwrap();
        std::fs::write(dir.path().join("docker-compose.yaml"), "").unwrap();
        assert_eq!(compose_file(dir.path()), Some(dir.path().join("docker-compose.yaml")));
    }

    #[test]
    fn compose_file_is_none_with_no_compose_file_present() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(compose_file(dir.path()), None);
    }

    #[test]
    fn docker_build_command_tags_and_sets_the_build_context_to_the_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let command = docker_build_command(dir.path());
        assert_eq!(command.get_program(), "docker");
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args[0], "build");
        assert!(args.contains(&"-t".to_string()));
        assert_eq!(command.get_current_dir(), Some(dir.path()));
    }

    #[test]
    fn docker_image_tag_sanitizes_a_directory_name_with_spaces_and_accents() {
        let dir = tempfile::tempdir().unwrap().path().join("Meu Projeto Ção");
        std::fs::create_dir_all(&dir).unwrap();
        let tag = docker_image_tag(&dir);
        assert!(tag.starts_with("foxgarden-"));
        assert!(tag.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')));
    }

    #[test]
    fn docker_run_command_uses_the_same_tag_docker_build_command_would() {
        let dir = tempfile::tempdir().unwrap();
        let build = docker_build_command(dir.path());
        let run = docker_run_command(dir.path());
        let build_args: Vec<_> = build.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let run_args: Vec<_> = run.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let tag = &build_args[build_args.iter().position(|a| a == "-t").unwrap() + 1];
        assert!(run_args.contains(tag));
    }

    #[test]
    fn docker_compose_up_command_sets_cwd_to_the_compose_files_own_directory() {
        let dir = tempfile::tempdir().unwrap();
        let compose = dir.path().join("compose.yaml");
        std::fs::write(&compose, "").unwrap();
        let command = docker_compose_up_command(&compose);
        assert_eq!(command.get_current_dir(), Some(dir.path()));
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, vec!["compose", "-f", compose.to_str().unwrap(), "up", "--build"]);
    }
}
