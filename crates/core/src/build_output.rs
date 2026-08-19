//! Detecting an existing project's build tool and assembling the real `mvn
//! compile`/`gradle compileJava` invocation for it (`PLAN.md` Track 22
//! Phase 1 — "Build"), plus a pure parser turning one line of that
//! invocation's own output into a [`BuildProblem`] when the line names a
//! compiler diagnostic.
//!
//! Spawning/streaming the process itself lives in `crates/app`
//! (`panels::build_panel`), not here — this crate stays headless
//! (`AGENTS.md`), and unlike every other `Command::output()` caller in this
//! module's siblings (`gradle_projects`, `maven_classpath`, ...), a build
//! needs to stream partial output *while the process is still running*,
//! which only app-side background-thread/channel machinery can do.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::diagnostic::Severity;
use crate::gradle::gradle_command;
use crate::scaffold::BuildTool;

/// Which build tool an *existing* project on disk uses — the same
/// `pom.xml` vs `build.gradle[.kts]` file-presence check
/// `crates::panels::spring_config::scan_project` already established as
/// this codebase's only precedent for the question (Track 21 left no
/// `BuildTool`-tagged project metadata to read instead).
pub fn detect_build_tool(project_root: &Path) -> Option<BuildTool> {
    if project_root.join("pom.xml").is_file() {
        Some(BuildTool::Maven)
    } else if project_root.join("build.gradle.kts").is_file() || project_root.join("build.gradle").is_file() {
        Some(BuildTool::Gradle)
    } else {
        None
    }
}

/// Prefers a project-pinned `mvnw` over a bare `mvn` on `PATH` — unlike
/// `maven_classpath` (always a bare `mvn`, a read-only classpath dump where
/// the exact Maven version doesn't matter), mirroring `gradle_command`'s
/// own "prefer what the project actually specifies" wrapper-first choice,
/// now that this is a real build.
fn maven_command(project_root: &Path) -> Command {
    let wrapper = project_root.join("mvnw");
    if wrapper.is_file() {
        Command::new(wrapper)
    } else {
        Command::new("mvn")
    }
}

/// Assembles (but does not spawn) the real build invocation for `tool`, cwd
/// already set to `project_root`. Verified against real `mvn 3.9.3`/
/// `gradle 9.6.1` runs, not assumed: `-B` (Maven's own batch/non-
/// interactive mode) and `--console=plain` (Gradle) both keep the output
/// free of the interactive progress noise a piped, non-TTY stdout doesn't
/// automatically get stripped of in every configuration.
pub fn build_command(project_root: &Path, tool: BuildTool) -> Command {
    let mut command = match tool {
        BuildTool::Maven => {
            let mut c = maven_command(project_root);
            c.arg("-B").arg("compile");
            c
        }
        BuildTool::Gradle => {
            let mut c = gradle_command(project_root);
            c.arg("--console=plain").arg("compileJava");
            c
        }
    };
    command.current_dir(project_root);
    command
}

/// `tool`'s own default compiled-classes output directory under
/// `project_root` — `run::run_command`'s own classpath-prefix logic
/// (`PLAN.md` Track 22 Phase 2), pulled out so `spotbugs_diagnostics`'s
/// caller (`PLAN.md` Track 5 Phase 3, which needs the *same* directory to
/// point SpotBugs' bytecode analysis at) doesn't duplicate it. Not verified
/// to exist on disk — a `Build` that hasn't run yet (or that failed) simply
/// means an empty/missing directory; callers decide what that means for
/// them.
pub fn default_classes_dir(project_root: &Path, tool: BuildTool) -> PathBuf {
    match tool {
        BuildTool::Maven => project_root.join("target").join("classes"),
        BuildTool::Gradle => project_root.join("build").join("classes").join("java").join("main"),
    }
}

/// One compiler diagnostic parsed out of a build's own output — enough to
/// paint a row and jump to it, not a full `Diagnostic` (whose `range` needs
/// a byte offset this only has line/column for). The app-side click
/// handler converts once the target file's real buffer is open, the same
/// deferred-conversion shape `pending_navigation`'s only other producer,
/// the Spring endpoint map, already uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProblem {
    pub path: PathBuf,
    /// 1-based, as both tools report it.
    pub line: usize,
    /// 1-based; `1` when the tool's own output doesn't report a real one
    /// (see `parse_javac_line`).
    pub column: usize,
    pub severity: Severity,
    pub message: String,
}

/// Parses one line of `mvn`/`gradle` build output into a `BuildProblem`, if
/// that line names a compiler diagnostic. Two independent shapes, verified
/// against real `mvn 3.9.3 -B compile`/`gradle 9.6.1 --console=plain
/// compileJava` runs against a deliberately broken two-error fixture (not
/// assumed from either tool's docs):
///
/// - Maven wraps javac's own line in `[LEVEL] `: `[ERROR] /abs/path/
///   Foo.java:[12,34] message text` (also `[WARNING]`) — emitted on
///   **stdout**.
/// - Gradle's `compileJava` prints javac's own unwrapped format instead:
///   `/abs/path/Foo.java:12: error: message text` (also `: warning:`),
///   also indented by 2 spaces in the "What went wrong" summary that
///   repeats it — emitted on **stderr**, unlike Maven's own shape. No
///   column at all here (javac instead points at it with a `^` under a
///   repeated source line on the two lines that follow); `column` is
///   reported as `1` rather than parsing the caret line, since landing on
///   the right *line* is this phase's own checkpoint, not the exact column.
pub fn parse_build_output_line(line: &str) -> Option<BuildProblem> {
    parse_maven_line(line).or_else(|| parse_javac_line(line))
}

fn parse_maven_line(line: &str) -> Option<BuildProblem> {
    let (severity, rest) = if let Some(rest) = line.strip_prefix("[ERROR] ") {
        (Severity::Error, rest)
    } else {
        let rest = line.strip_prefix("[WARNING] ")?;
        (Severity::Warning, rest)
    };

    let marker = ".java:[";
    let marker_at = rest.find(marker)?;
    let path = &rest[..marker_at + ".java".len()];
    if path.is_empty() {
        return None;
    }
    let after = &rest[marker_at + marker.len()..]; // "12,34] message"
    let close_at = after.find(']')?;
    let (line_str, col_str) = after[..close_at].split_once(',')?;
    let line_no: usize = line_str.trim().parse().ok()?;
    let col_no: usize = col_str.trim().parse().ok()?;
    let message = after[close_at + 1..].trim();
    if message.is_empty() {
        return None;
    }
    Some(BuildProblem {
        path: PathBuf::from(path),
        line: line_no,
        column: col_no,
        severity,
        message: message.to_string(),
    })
}

fn parse_javac_line(line: &str) -> Option<BuildProblem> {
    let trimmed = line.trim_start();
    let marker = ".java:";
    let marker_at = trimmed.find(marker)?;
    let path = &trimmed[..marker_at + ".java".len()];
    // A real path never contains a space or an early ':' — guards against
    // misreading an unrelated prose line that merely happens to contain the
    // substring ".java:" somewhere past its start.
    if path.is_empty() || path.contains(' ') || path.contains(':') {
        return None;
    }
    let after = &trimmed[marker_at + marker.len()..]; // "12: error: message"
    let (line_str, rest) = after.split_once(':')?;
    let line_no: usize = line_str.trim().parse().ok()?;
    let rest = rest.trim_start();
    let (severity, message) = if let Some(m) = rest.strip_prefix("error:") {
        (Severity::Error, m)
    } else {
        let m = rest.strip_prefix("warning:")?;
        (Severity::Warning, m)
    };
    let message = message.trim();
    if message.is_empty() {
        return None;
    }
    Some(BuildProblem {
        path: PathBuf::from(path),
        line: line_no,
        column: 1,
        severity,
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_maven_by_pom_xml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "").unwrap();
        assert_eq!(detect_build_tool(dir.path()), Some(BuildTool::Maven));
    }

    #[test]
    fn detects_gradle_by_build_gradle_kts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();
        assert_eq!(detect_build_tool(dir.path()), Some(BuildTool::Gradle));
    }

    #[test]
    fn detects_gradle_by_plain_build_gradle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();
        assert_eq!(detect_build_tool(dir.path()), Some(BuildTool::Gradle));
    }

    #[test]
    fn detects_neither_with_no_build_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_build_tool(dir.path()), None);
    }

    // --- Maven: real `mvn -B compile` output against a deliberately broken
    // two-error fixture, captured verbatim (see PLAN.md Track 22 Phase 1) ---

    #[test]
    fn parses_a_real_maven_compiler_error_line() {
        let line = "[ERROR] /home/user/project/src/main/java/com/example/Foo.java:[5,17] cannot find symbol";
        let problem = parse_build_output_line(line).unwrap();
        assert_eq!(
            problem.path,
            PathBuf::from("/home/user/project/src/main/java/com/example/Foo.java")
        );
        assert_eq!(problem.line, 5);
        assert_eq!(problem.column, 17);
        assert_eq!(problem.severity, Severity::Error);
        assert_eq!(problem.message, "cannot find symbol");
    }

    #[test]
    fn parses_a_real_maven_compiler_warning_line() {
        let line = "[WARNING] /home/user/project/src/main/java/Foo.java:[3,1] some warning text";
        let problem = parse_build_output_line(line).unwrap();
        assert_eq!(problem.severity, Severity::Warning);
        assert_eq!(problem.line, 3);
        assert_eq!(problem.column, 1);
    }

    #[test]
    fn ignores_maven_lines_with_no_source_location() {
        assert!(
            parse_build_output_line(
                "[ERROR] Failed to execute goal org.apache.maven.plugins:maven-compiler-plugin:3.11.0:compile"
            )
            .is_none()
        );
        assert!(parse_build_output_line("[ERROR] COMPILATION ERROR : ").is_none());
        assert!(parse_build_output_line("[ERROR]   symbol:   variable undefinedSymbol").is_none());
    }

    // --- Gradle: real `gradle --console=plain compileJava` output (plain
    // javac shape, no Maven wrapper) ---

    #[test]
    fn parses_a_real_gradle_javac_error_line() {
        let line = "/home/user/project/src/main/java/com/example/Foo.java:5: error: cannot find symbol";
        let problem = parse_build_output_line(line).unwrap();
        assert_eq!(
            problem.path,
            PathBuf::from("/home/user/project/src/main/java/com/example/Foo.java")
        );
        assert_eq!(problem.line, 5);
        assert_eq!(problem.column, 1);
        assert_eq!(problem.severity, Severity::Error);
        assert_eq!(problem.message, "cannot find symbol");
    }

    #[test]
    fn parses_the_indented_copy_gradles_own_summary_repeats() {
        // The exact 2-space-indented repetition Gradle's own "* What went
        // wrong" section prints for the same error.
        let line = "  /home/user/project/src/main/java/com/example/Foo.java:5: error: cannot find symbol";
        let problem = parse_build_output_line(line).unwrap();
        assert_eq!(problem.line, 5);
    }

    #[test]
    fn parses_a_real_gradle_javac_warning_line() {
        let line = "/home/user/project/src/main/java/Foo.java:9: warning: some warning text";
        let problem = parse_build_output_line(line).unwrap();
        assert_eq!(problem.severity, Severity::Warning);
    }

    #[test]
    fn ignores_gradle_source_echo_and_caret_lines() {
        assert!(parse_build_output_line("        int x = undefinedSymbol;").is_none());
        assert!(parse_build_output_line("                ^").is_none());
        assert!(parse_build_output_line("  symbol:   variable undefinedSymbol").is_none());
        assert!(parse_build_output_line("2 errors").is_none());
    }

    #[test]
    fn ignores_plain_prose_lines() {
        assert!(parse_build_output_line("BUILD FAILED in 3s").is_none());
        assert!(parse_build_output_line("").is_none());
    }
}
