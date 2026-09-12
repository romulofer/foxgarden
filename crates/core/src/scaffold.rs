//! Generates a brand-new project's starting files (`PLAN.md` Track 29
//! Phase 3) — pure text generation, mirroring `gradle.rs::INIT_SCRIPT`'s own
//! "Rust string constant, values substituted in" shape. Nothing here runs
//! `mvn`/`gradle`; the result is exactly the files a real `mvn`/`gradle` on
//! the user's own machine can build (Track 22, Build/run/test integration,
//! is what actually compiles/runs it — out of scope here).

use std::path::{Path, PathBuf};

/// The language a scaffolded project's sources are written in — Java
/// (Phase 3/4) or Kotlin (Phase 5). Each picks its own source root
/// (`src/main/java` vs `src/main/kotlin`), entry-point file, and — for
/// Maven — build plugin; Gradle differs only by the `kotlin("jvm")` plugin
/// vs the bare `java` one. The `kotlin-language-server` gaps that once
/// deferred this (`TECHNICAL_DEBT.md` #17/#18) are about in-app *analysis*,
/// not skeleton correctness — a scaffolded Kotlin project still has to
/// build with a real `mvn`/`gradle`, which is what these files guarantee.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ProjectLanguage {
    #[default]
    Java,
    Kotlin,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BuildTool {
    #[default]
    Maven,
    Gradle,
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
const GRADLE_GITIGNORE: &str = "build/\n.gradle/\n*.class\n.idea/\n*.iml\n";

/// The Kotlin release pinned across every generated Kotlin build file — a
/// real, resolvable Maven Central coordinate for both `kotlin-stdlib` and
/// the `kotlin-maven-plugin`/`kotlin("jvm")` Gradle plugin, so a scaffolded
/// project builds without the user having to pick a version.
const KOTLIN_VERSION: &str = "2.0.21";

/// Kotlin's `jvmTarget` (the Maven plugin's own accepted spelling): `"1.8"`
/// for Java 8, the bare number otherwise. Unlike Gradle's `jvmToolchain`,
/// which takes a plain integer, `kotlin-maven-plugin`'s `<jvmTarget>` still
/// wants the legacy `1.8` form for 8 and rejects `8`.
fn kotlin_jvm_target(java_release: u32) -> String {
    if java_release == 8 { "1.8".to_string() } else { java_release.to_string() }
}

/// The generated `pom.xml` for a Kotlin project: `kotlin-stdlib` on the
/// classpath, `src/main/kotlin` as the source root, and the
/// `kotlin-maven-plugin` bound to `compile` (its own documented minimal
/// setup). `<maven.compiler.release>` is kept too — harmless for a
/// Kotlin build, and it keeps `java_release::release_from_pom` reading the
/// wizard's promised level back the same way it does for a Java project.
fn kotlin_pom_xml(spec: &ScaffoldSpec) -> String {
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
    <kotlin.version>{kotlin_version}</kotlin.version>
    <maven.compiler.release>{java_release}</maven.compiler.release>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>

  <dependencies>
    <dependency>
      <groupId>org.jetbrains.kotlin</groupId>
      <artifactId>kotlin-stdlib</artifactId>
      <version>${{kotlin.version}}</version>
    </dependency>
  </dependencies>

  <build>
    <sourceDirectory>src/main/kotlin</sourceDirectory>
    <plugins>
      <plugin>
        <groupId>org.jetbrains.kotlin</groupId>
        <artifactId>kotlin-maven-plugin</artifactId>
        <version>${{kotlin.version}}</version>
        <executions>
          <execution>
            <id>compile</id>
            <phase>compile</phase>
            <goals>
              <goal>compile</goal>
            </goals>
          </execution>
        </executions>
        <configuration>
          <jvmTarget>{jvm_target}</jvmTarget>
        </configuration>
      </plugin>
    </plugins>
  </build>
</project>
"#,
        group_id = spec.group_id,
        artifact_id = spec.artifact_id,
        kotlin_version = KOTLIN_VERSION,
        java_release = spec.java_release,
        jvm_target = kotlin_jvm_target(spec.java_release),
    )
}

/// The generated `src/main/kotlin/<package path>/Main.kt` — a top-level
/// `fun main()`, the idiomatic Kotlin entry point (no wrapper class), which
/// the compiler exposes as the JVM class `<package>.MainKt` (see
/// `kotlin_build_gradle_kts`'s own `mainClass`).
fn main_kotlin(spec: &ScaffoldSpec) -> String {
    let package = package_path_components(&spec.group_id).join(".");
    let mut out = String::new();
    if !package.is_empty() {
        out.push_str(&format!("package {package}\n\n"));
    }
    out.push_str("fun main() {\n    println(\"Hello, world!\")\n}\n");
    out
}

/// `build.gradle.kts` for a Kotlin project: the `kotlin("jvm")` plugin
/// (which pulls `kotlin-stdlib` in on its own, so no explicit dependency is
/// needed) plus `application`, and `kotlin { jvmToolchain(N) }` — the exact
/// form `java_release::release_from_gradle` ranks highest as
/// `kotlin.jvmToolchain`, so a scaffolded Kotlin project reads back at the
/// release the wizard promised, same guarantee the Java path has. The same
/// no-wrapper stance as `build_gradle_kts` applies (`PLAN.md` Track 29
/// Phase 4).
fn kotlin_build_gradle_kts(spec: &ScaffoldSpec) -> String {
    let package = package_path_components(&spec.group_id).join(".");
    let main_class =
        if package.is_empty() { "MainKt".to_string() } else { format!("{package}.MainKt") };
    format!(
        r#"plugins {{
    kotlin("jvm") version "{kotlin_version}"
    application
}}

kotlin {{
    jvmToolchain({java_release})
}}

application {{
    mainClass = "{main_class}"
}}

group = "{group_id}"
version = "1.0-SNAPSHOT"

repositories {{
    mavenCentral()
}}
"#,
        kotlin_version = KOTLIN_VERSION,
        java_release = spec.java_release,
        group_id = spec.group_id,
    )
}

/// `settings.gradle.kts`: just the root project name, the same single
/// responsibility a real `gradle init` gives this file.
fn settings_gradle_kts(spec: &ScaffoldSpec) -> String {
    format!("rootProject.name = \"{}\"\n", spec.artifact_id)
}

/// `build.gradle.kts`. The toolchain block is deliberately the one form
/// `java_release::release_from_gradle` ranks highest (`java.toolchain.
/// languageVersion`) — a scaffolded project has to read back at the release
/// the wizard promised, the same regression `pom_xml`'s own
/// `pom_xml_release_reads_back_through_java_release_detect_at_the_requested_
/// value` test already guards for Maven. No Gradle wrapper is generated
/// (`PLAN.md` Track 29 Phase 4's own stated non-goal — needs a real network
/// fetch or vendoring, the same trade-off `lsp_manager.rs` already reasons
/// through for jdt.ls) — the wizard's own help text says so explicitly
/// rather than leaving a silently-missing `gradlew`.
fn build_gradle_kts(spec: &ScaffoldSpec) -> String {
    let package = package_path_components(&spec.group_id).join(".");
    let main_class =
        if package.is_empty() { "Main".to_string() } else { format!("{package}.Main") };
    format!(
        r#"plugins {{
    java
    application
}}

java {{
    toolchain {{
        languageVersion = JavaLanguageVersion.of({java_release})
    }}
}}

application {{
    mainClass = "{main_class}"
}}

group = "{group_id}"
version = "1.0-SNAPSHOT"

repositories {{
    mavenCentral()
}}
"#,
        java_release = spec.java_release,
        group_id = spec.group_id,
    )
}

/// The files a new project needs, as `(path relative to the project root,
/// content)` pairs — pure generation, no filesystem access, so it's cheap
/// to test exactly (`write_scaffold` is the only function that touches
/// disk).
pub fn scaffold_files(spec: &ScaffoldSpec) -> Vec<(PathBuf, String)> {
    let package_dir: PathBuf = package_path_components(&spec.group_id).into_iter().collect();
    // Source root and entry-point file follow the language, the same
    // `src/main/<lang>` convention Maven and Gradle both use.
    let (src_root, main_file) = match spec.language {
        ProjectLanguage::Java => ("src/main/java", "Main.java"),
        ProjectLanguage::Kotlin => ("src/main/kotlin", "Main.kt"),
    };
    let main_path = Path::new(src_root).join(package_dir).join(main_file);
    let main_source = match spec.language {
        ProjectLanguage::Java => main_java(spec),
        ProjectLanguage::Kotlin => main_kotlin(spec),
    };
    match (spec.build_tool, spec.language) {
        (BuildTool::Maven, ProjectLanguage::Java) => vec![
            (PathBuf::from("pom.xml"), pom_xml(spec)),
            (main_path, main_source),
            (PathBuf::from(".gitignore"), GITIGNORE.to_string()),
        ],
        (BuildTool::Maven, ProjectLanguage::Kotlin) => vec![
            (PathBuf::from("pom.xml"), kotlin_pom_xml(spec)),
            (main_path, main_source),
            (PathBuf::from(".gitignore"), GITIGNORE.to_string()),
        ],
        (BuildTool::Gradle, ProjectLanguage::Java) => vec![
            (PathBuf::from("settings.gradle.kts"), settings_gradle_kts(spec)),
            (PathBuf::from("build.gradle.kts"), build_gradle_kts(spec)),
            (main_path, main_source),
            (PathBuf::from(".gitignore"), GRADLE_GITIGNORE.to_string()),
        ],
        (BuildTool::Gradle, ProjectLanguage::Kotlin) => vec![
            (PathBuf::from("settings.gradle.kts"), settings_gradle_kts(spec)),
            (PathBuf::from("build.gradle.kts"), kotlin_build_gradle_kts(spec)),
            (main_path, main_source),
            (PathBuf::from(".gitignore"), GRADLE_GITIGNORE.to_string()),
        ],
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

    fn gradle_sample() -> ScaffoldSpec {
        ScaffoldSpec { build_tool: BuildTool::Gradle, ..sample() }
    }

    #[test]
    fn gradle_scaffold_files_returns_the_expected_paths() {
        let files = scaffold_files(&gradle_sample());
        let paths: Vec<&Path> = files.iter().map(|(p, _)| p.as_path()).collect();
        assert_eq!(
            paths,
            vec![
                Path::new("settings.gradle.kts"),
                Path::new("build.gradle.kts"),
                Path::new("src/main/java/com/example/app/Main.java"),
                Path::new(".gitignore"),
            ]
        );
    }

    #[test]
    fn gradle_settings_names_the_root_project_after_the_artifact_id() {
        let files = scaffold_files(&gradle_sample());
        let (_, settings) = &files[0];
        assert_eq!(settings, "rootProject.name = \"my-app\"\n");
    }

    #[test]
    fn gradle_build_states_the_requested_release_group_and_main_class() {
        let files = scaffold_files(&gradle_sample());
        let (_, build) = &files[1];
        assert!(build.contains("languageVersion = JavaLanguageVersion.of(17)"), "{build}");
        assert!(build.contains(r#"group = "com.example.app""#), "{build}");
        assert!(build.contains(r#"mainClass = "com.example.app.Main""#), "{build}");
    }

    #[test]
    fn gradle_build_release_reads_back_through_java_release_detect_at_the_requested_value() {
        // Same regression as Maven's own equivalent test: a scaffolded
        // build.gradle.kts has to be readable by java_release::detect at
        // exactly the release the wizard promised.
        let spec = ScaffoldSpec { java_release: 21, ..gradle_sample() };
        let (_, build) = &scaffold_files(&spec)[1];
        assert_eq!(crate::java_release::release_from_gradle(build).map(|(major, _)| major), Some(21));
    }

    #[test]
    fn a_single_segment_group_id_still_produces_a_qualified_main_class() {
        let spec = ScaffoldSpec { group_id: "app".to_string(), ..gradle_sample() };
        let files = scaffold_files(&spec);
        let (_, build) = &files[1];
        assert!(build.contains(r#"mainClass = "app.Main""#), "{build}");
    }

    #[test]
    fn write_scaffold_creates_every_gradle_file_under_the_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("my-app");
        write_scaffold(&root, &scaffold_files(&gradle_sample())).unwrap();

        assert!(root.join("settings.gradle.kts").exists());
        assert!(root.join("build.gradle.kts").exists());
        assert!(root.join("src/main/java/com/example/app/Main.java").exists());
        assert!(root.join(".gitignore").exists());
    }

    fn kotlin_maven_sample() -> ScaffoldSpec {
        ScaffoldSpec { language: ProjectLanguage::Kotlin, ..sample() }
    }

    fn kotlin_gradle_sample() -> ScaffoldSpec {
        ScaffoldSpec {
            build_tool: BuildTool::Gradle,
            language: ProjectLanguage::Kotlin,
            ..sample()
        }
    }

    #[test]
    fn kotlin_maven_scaffold_puts_sources_under_src_main_kotlin_as_a_kt_file() {
        let files = scaffold_files(&kotlin_maven_sample());
        let paths: Vec<&Path> = files.iter().map(|(p, _)| p.as_path()).collect();
        assert_eq!(
            paths,
            vec![
                Path::new("pom.xml"),
                Path::new("src/main/kotlin/com/example/app/Main.kt"),
                Path::new(".gitignore"),
            ]
        );
    }

    #[test]
    fn kotlin_main_declares_the_package_and_a_top_level_main() {
        let files = scaffold_files(&kotlin_maven_sample());
        let (_, main) = &files[1];
        assert!(main.starts_with("package com.example.app\n\n"), "{main}");
        assert!(main.contains("fun main() {"), "{main}");
        assert!(main.contains("println(\"Hello, world!\")"), "{main}");
        // No wrapper class — the whole point of the idiomatic Kotlin entry
        // point over a Java-style `class Main`.
        assert!(!main.contains("class Main"), "{main}");
    }

    #[test]
    fn kotlin_maven_pom_wires_the_kotlin_plugin_stdlib_and_source_root() {
        let files = scaffold_files(&kotlin_maven_sample());
        let (_, pom) = &files[0];
        assert!(pom.contains("<artifactId>kotlin-maven-plugin</artifactId>"), "{pom}");
        assert!(pom.contains("<artifactId>kotlin-stdlib</artifactId>"), "{pom}");
        assert!(pom.contains("<sourceDirectory>src/main/kotlin</sourceDirectory>"), "{pom}");
        assert!(pom.contains(&format!("<kotlin.version>{KOTLIN_VERSION}</kotlin.version>")), "{pom}");
    }

    #[test]
    fn kotlin_maven_pom_release_reads_back_through_java_release_detect() {
        // Same regression guard as the Java pom: a scaffolded Kotlin pom
        // still has to report the wizard's promised level to this project's
        // own detector.
        let spec = ScaffoldSpec { java_release: 8, ..kotlin_maven_sample() };
        let (_, pom) = &scaffold_files(&spec)[0];
        assert_eq!(crate::java_release::release_from_pom(pom).map(|(major, _)| major), Some(8));
    }

    #[test]
    fn kotlin_maven_jvm_target_uses_the_legacy_1_8_spelling_for_java_8() {
        let spec = ScaffoldSpec { java_release: 8, ..kotlin_maven_sample() };
        let (_, pom) = &scaffold_files(&spec)[0];
        assert!(pom.contains("<jvmTarget>1.8</jvmTarget>"), "{pom}");

        let spec = ScaffoldSpec { java_release: 17, ..kotlin_maven_sample() };
        let (_, pom) = &scaffold_files(&spec)[0];
        assert!(pom.contains("<jvmTarget>17</jvmTarget>"), "{pom}");
    }

    #[test]
    fn kotlin_gradle_scaffold_returns_the_expected_paths() {
        let files = scaffold_files(&kotlin_gradle_sample());
        let paths: Vec<&Path> = files.iter().map(|(p, _)| p.as_path()).collect();
        assert_eq!(
            paths,
            vec![
                Path::new("settings.gradle.kts"),
                Path::new("build.gradle.kts"),
                Path::new("src/main/kotlin/com/example/app/Main.kt"),
                Path::new(".gitignore"),
            ]
        );
    }

    #[test]
    fn kotlin_gradle_build_wires_the_jvm_plugin_and_the_kt_main_class() {
        let files = scaffold_files(&kotlin_gradle_sample());
        let (_, build) = &files[1];
        assert!(build.contains(&format!(r#"kotlin("jvm") version "{KOTLIN_VERSION}""#)), "{build}");
        // A top-level `fun main` compiles to `<package>.MainKt`, not `Main`.
        assert!(build.contains(r#"mainClass = "com.example.app.MainKt""#), "{build}");
    }

    #[test]
    fn kotlin_gradle_build_release_reads_back_through_java_release_detect() {
        // `kotlin { jvmToolchain(N) }` is exactly the form
        // `release_from_gradle` ranks highest, so a Kotlin Gradle project
        // reads back at the promised release just like the Java one.
        let spec = ScaffoldSpec { java_release: 21, ..kotlin_gradle_sample() };
        let (_, build) = &scaffold_files(&spec)[1];
        assert_eq!(crate::java_release::release_from_gradle(build).map(|(major, _)| major), Some(21));
    }

    #[test]
    fn kotlin_single_segment_group_id_still_produces_a_valid_package_path_and_main_class() {
        let spec = ScaffoldSpec { group_id: "app".to_string(), ..kotlin_gradle_sample() };
        let files = scaffold_files(&spec);
        assert_eq!(files[2].0, Path::new("src/main/kotlin/app/Main.kt"));
        assert!(files[2].1.starts_with("package app\n\n"));
        assert!(files[1].1.contains(r#"mainClass = "app.MainKt""#));
    }
}
