//! Generates a brand-new project's starting files (`PLAN.md` Track 29
//! Phase 3) — pure text generation, mirroring `gradle.rs::INIT_SCRIPT`'s own
//! "Rust string constant, values substituted in" shape. Nothing here runs
//! `mvn`/`gradle`; the result is exactly the files a real `mvn`/`gradle` on
//! the user's own machine can build (Track 22, Build/run/test integration,
//! is what actually compiles/runs it — out of scope here).

use std::path::{Path, PathBuf};

/// Only Java is generated yet — Kotlin is Phase 5 (stretch), deferred
/// behind two open `kotlin-language-server` gaps (TECHNICAL_DEBT.md
/// #17/#18's own history) that make a fresh Kotlin project's actual
/// in-app analysis experience uncertain regardless of skeleton
/// correctness. Kept as an enum rather than skipped entirely so
/// `ScaffoldSpec`'s shape doesn't have to change again once Phase 5 lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectLanguage {
    Java,
}

/// Only Maven is generated yet — Gradle is Phase 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTool {
    Maven,
}

/// What a new project needs to be scaffolded: enough to write a real,
/// buildable `pom.xml`/`Main.java`/`.gitignore`, nothing about *how* it
/// gets built or run (Track 22's own concern).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldSpec {
    /// Maven's `groupId` — also becomes the generated `Main.java`'s own
    /// Java package, the same convention a real Maven archetype uses.
    pub group_id: String,
    /// Maven's `artifactId`.
    pub artifact_id: String,
    pub java_release: u32,
    pub build_tool: BuildTool,
    pub language: ProjectLanguage,
}

/// A `group_id` like `com.example.app`, as the package-path components
/// `src/main/java` sits under: `["com", "example", "app"]`. Empty/invalid
/// segments (a stray `..`, a leading/trailing dot) are dropped rather than
/// rejected outright — a scaffolded project should never fail to generate
/// over a typo'd group ID; `mvn`/`javac` will say so clearly enough later
/// if what's left doesn't compile.
fn package_path_components(group_id: &str) -> Vec<&str> {
    group_id.split('.').filter(|segment| !segment.is_empty()).collect()
}

/// The generated `pom.xml`: a bare `<maven.compiler.release>` property is
/// enough — no `maven-compiler-plugin` configuration needed, since the
/// plugin already reads that property itself (the exact mechanism `PLAN.md`
/// Track 29 Phase 0 live-verified jdt.ls also honors with zero client-side
/// help).
fn pom_xml(spec: &ScaffoldSpec) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>

  <groupId>{group_id}</groupId>
  <artifactId>{artifact_id}</artifactId>
  <version>1.0-SNAPSHOT</version>
  <packaging>jar</packaging>

  <properties>
    <maven.compiler.release>{java_release}</maven.compiler.release>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
</project>
"#,
        group_id = spec.group_id,
        artifact_id = spec.artifact_id,
        java_release = spec.java_release,
    )
}

/// The generated `src/main/java/<package path>/Main.java` — a real entry
/// point (unlike `boilerplate::generate`'s own empty-class skeleton for a
/// file added to an *existing* project), so `mvn -q compile`/`exec:java`
/// has something to actually build and run.
fn main_java(spec: &ScaffoldSpec) -> String {
    let package = package_path_components(&spec.group_id).join(".");
    let mut out = String::new();
    if !package.is_empty() {
        out.push_str(&format!("package {package};\n\n"));
    }
    out.push_str("public class Main {\n    public static void main(String[] args) {\n        System.out.println(\"Hello, world!\");\n    }\n}\n");
    out
}

const GITIGNORE: &str = "target/\n*.class\n.idea/\n*.iml\n";

/// The files a new project needs, as `(path relative to the project root,
/// content)` pairs — pure generation, no filesystem access, so it's cheap
/// to test exactly (`write_scaffold` is the only function that touches
/// disk).
pub fn scaffold_files(spec: &ScaffoldSpec) -> Vec<(PathBuf, String)> {
    match (spec.build_tool, spec.language) {
        (BuildTool::Maven, ProjectLanguage::Java) => {
            let package_dir: PathBuf =
                package_path_components(&spec.group_id).into_iter().collect();
            let main_path = Path::new("src/main/java").join(package_dir).join("Main.java");
            vec![
                (PathBuf::from("pom.xml"), pom_xml(spec)),
                (main_path, main_java(spec)),
                (PathBuf::from(".gitignore"), GITIGNORE.to_string()),
            ]
        }
    }
}

/// Writes `files` under `project_root`, creating parent directories as
/// needed. Refuses outright if `project_root` already exists and already
/// has anything in it — scaffolding into a real, non-empty directory (an
/// existing project, a home folder picked by mistake) would silently mix
/// generated files into whatever's already there; an empty or
/// not-yet-created directory is the only safe target.
pub fn write_scaffold(project_root: &Path, files: &[(PathBuf, String)]) -> Result<(), String> {
    if project_root.exists() {
        let mut entries = std::fs::read_dir(project_root)
            .map_err(|e| format!("couldn't read {}: {e}", project_root.display()))?;
        if entries.next().is_some() {
            return Err(format!("{} already exists and is not empty", project_root.display()));
        }
    }

    for (relative, content) in files {
        let path = project_root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("couldn't create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, content).map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ScaffoldSpec {
        ScaffoldSpec {
            group_id: "com.example.app".to_string(),
            artifact_id: "my-app".to_string(),
            java_release: 17,
            build_tool: BuildTool::Maven,
            language: ProjectLanguage::Java,
        }
    }

    #[test]
    fn scaffold_files_returns_the_expected_paths() {
        let files = scaffold_files(&sample());
        let paths: Vec<&Path> = files.iter().map(|(p, _)| p.as_path()).collect();
        assert_eq!(
            paths,
            vec![
                Path::new("pom.xml"),
                Path::new("src/main/java/com/example/app/Main.java"),
                Path::new(".gitignore"),
            ]
        );
    }

    #[test]
    fn pom_xml_states_the_requested_release_group_and_artifact() {
        let files = scaffold_files(&sample());
        let (_, pom) = &files[0];
        assert!(pom.contains("<maven.compiler.release>17</maven.compiler.release>"), "{pom}");
        assert!(pom.contains("<groupId>com.example.app</groupId>"), "{pom}");
        assert!(pom.contains("<artifactId>my-app</artifactId>"), "{pom}");
    }

    #[test]
    fn pom_xml_release_reads_back_through_java_release_detect_at_the_requested_value() {
        // Regression: a scaffolded pom.xml has to be readable by this
        // project's own detector (`PLAN.md` Track 29 Phase 2), or a
        // scaffolded project would silently get analyzed at jdt.ls' own
        // default release instead of the one the wizard promised.
        let spec = ScaffoldSpec { java_release: 8, ..sample() };
        let (_, pom) = &scaffold_files(&spec)[0];
        assert_eq!(crate::java_release::release_from_pom(pom).map(|(major, _)| major), Some(8));
    }

    #[test]
    fn main_java_declares_the_package_from_the_group_id_and_a_runnable_main() {
        let files = scaffold_files(&sample());
        let (_, main) = &files[1];
        assert!(main.starts_with("package com.example.app;\n\n"), "{main}");
        assert!(main.contains("public class Main"), "{main}");
        assert!(main.contains("public static void main(String[] args)"), "{main}");
    }

    #[test]
    fn a_single_segment_group_id_still_produces_a_valid_package_and_path() {
        let spec = ScaffoldSpec { group_id: "app".to_string(), ..sample() };
        let files = scaffold_files(&spec);
        assert_eq!(files[1].0, Path::new("src/main/java/app/Main.java"));
        assert!(files[1].1.starts_with("package app;\n\n"));
    }

    #[test]
    fn write_scaffold_creates_every_file_under_the_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("my-app");
        write_scaffold(&root, &scaffold_files(&sample())).unwrap();

        assert!(root.join("pom.xml").exists());
        assert!(root.join("src/main/java/com/example/app/Main.java").exists());
        assert!(root.join(".gitignore").exists());
    }

    #[test]
    fn write_scaffold_refuses_a_non_empty_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("already-here.txt"), "hi").unwrap();

        let error = write_scaffold(dir.path(), &scaffold_files(&sample())).unwrap_err();
        assert!(error.contains("not empty"), "{error}");
        assert!(!dir.path().join("pom.xml").exists());
    }

    #[test]
    fn write_scaffold_accepts_an_existing_but_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        write_scaffold(dir.path(), &scaffold_files(&sample())).unwrap();
        assert!(dir.path().join("pom.xml").exists());
    }
}
