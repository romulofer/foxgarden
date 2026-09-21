//! Gradle model extraction (`PLAN.md` Track 21 Phase 2). Unlike `pom.xml`
//! (a static XML document `maven.rs` reads directly), a Gradle build's real
//! shape only exists after Groovy/Kotlin DSL build scripts are *evaluated* —
//! parsing `build.gradle(.kts)` as text can't answer "what are this
//! project's actual dependencies" (a `dependencies { }` block can compute
//! its contents from arbitrary code). This module instead shells out to a
//! real `gradle`/`gradlew` process with a `--init-script` that hooks every
//! project post-evaluation and dumps its own resolved model as JSON — the
//! "offline init-script dump" approach `PLAN.md` Phase 1 called for
//! validating before committing to it, confirmed this session against a
//! real multi-module Kotlin/Spring Gradle project (see this module's own
//! tests for the captured, real JSON that validation produced) rather than
//! attempting to parse Groovy/Kotlin DSL as text, which `PLAN.md` itself
//! rules out.
//!
//! `--offline` throughout: this only needs each configuration's *declared*
//! dependency notation (group/artifact/version as written, or a project
//! reference), never actual artifact resolution, so no network access or
//! resolved-jar download is ever triggered by running this.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const DUMP_TASK: &str = "foxgardenGradleModelDump";

/// Test-only Rust twin of `INIT_SCRIPT`'s own `isDependencyConfig` Groovy
/// closure (kept in sync by hand — the real filtering happens inside the
/// Groovy script that actually runs against a project, since that's the
/// only place a `Configuration` object exists at all; this exists purely so
/// the *decision* is unit-testable from `cargo test` without a real Gradle
/// process). Configuration names this module treats as real, user-facing
/// dependency declarations. A real dump against a Kotlin/Spring project
/// (this module's own validation run) also surfaced several purely
/// internal tooling configurations (`kotlinCompilerPluginClasspathMain`,
/// `kotlinBuildToolsApiClasspath`, every plain `*Classpath` resolvable
/// configuration duplicating what `implementation`/`testImplementation`
/// etc. already declare) that have nothing to do with what a user actually
/// wrote in their own `dependencies { }` block — filtered out by an
/// allow-list (exact names, or a handful of known suffixes covering both
/// main and `test`-prefixed variants) rather than a deny-list, since a
/// third-party plugin can invent arbitrary configuration names a deny-list
/// would miss.
#[cfg(test)]
fn is_dependency_configuration(name: &str) -> bool {
    const EXACT: &[&str] = &[
        "implementation",
        "api",
        "compileOnly",
        "compileOnlyApi",
        "runtimeOnly",
        "annotationProcessor",
        "developmentOnly",
    ];
    const SUFFIXES: &[&str] = &[
        "Implementation",
        "Api",
        "CompileOnly",
        "CompileOnlyApi",
        "RuntimeOnly",
        "AnnotationProcessor",
    ];

    EXACT.contains(&name) || name.starts_with("kapt") || SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// One Gradle (sub)project's own declared shape, mirroring `MavenProject`'s
/// own "one module's declared shape, not a fully-resolved model" scope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GradleProject {
    /// Gradle's own project-path notation (`":"` for the root project,
    /// `":backend"` for a subproject, `":sub:nested"` for a nested one).
    pub path: String,
    pub name: String,
    pub group: String,
    pub version: String,
    #[serde(rename = "projectDir")]
    pub project_dir: PathBuf,
    pub dependencies: Vec<GradleDependency>,
}

/// One declared dependency from a real (see `is_dependency_configuration`)
/// configuration. Two kinds, exactly like `dep instanceof
/// org.gradle.api.artifacts.ProjectDependency` distinguishes them on the
/// Gradle side: a reference to another module in the same build
/// (`project(":database")`), or an external module coordinate
/// (`"group:artifact"`/`"group:artifact:version"`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GradleDependency {
    Project {
        configuration: String,
        /// The referenced project's own Gradle path (`":database"`), not a
        /// resolved filesystem location — `GradleProject::path` on the
        /// matching entry in the same dump has that.
        path: String,
    },
    Module {
        configuration: String,
        group: String,
        artifact: String,
        /// `None` when unversioned — resolved elsewhere (a version catalog,
        /// `io.spring.dependency-management`'s own inherited BOM, a
        /// platform constraint, ...), the exact same "don't chase it down
        /// here" non-goal `maven.rs`'s own `MavenDependency::version`
        /// already established for `pom.xml`.
        version: Option<String>,
    },
}

#[derive(Debug)]
pub enum GradleError {
    /// The `gradle`/`gradlew` process couldn't even be launched.
    Spawn(std::io::Error),
    /// The process ran, but its stdout wasn't the expected dump shape —
    /// same "distinguish launch failure from a bad/missing report" split
    /// `static_analysis::StaticAnalysisError` already uses.
    Report(String),
}

impl std::fmt::Display for GradleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GradleError::Spawn(e) => write!(f, "failed to run gradle: {e}"),
            GradleError::Report(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for GradleError {}

/// The init script every `gradle_projects` invocation writes to a temp file
/// and passes via `--init-script`. Registers one task per project
/// (`allprojects { }`, so it reaches the root project and every subproject
/// alike) that prints its own model as one JSON object per project, each
/// wrapped in a plain-text `FOXGARDEN_JSON_BEGIN`/`FOXGARDEN_JSON_END`
/// marker pair (`parse_dump_output` splits on these) rather than assembling
/// one combined JSON array across every project — Gradle's own configuration
/// phase logs and any plugin's own stray stdout output are already
/// guaranteed to appear *outside* those markers, so this stays robust to
/// build-script noise a single top-level JSON document would have no way
/// to recover from if any of it landed mid-document.
const INIT_SCRIPT: &str = r#"
import groovy.json.JsonOutput

def isDependencyConfig = { String name ->
    def exact = ["implementation", "api", "compileOnly", "compileOnlyApi", "runtimeOnly", "annotationProcessor", "developmentOnly"] as Set
    if (exact.contains(name)) return true
    if (name.startsWith("kapt")) return true
    def suffixes = ["Implementation", "Api", "CompileOnly", "CompileOnlyApi", "RuntimeOnly", "AnnotationProcessor"]
    return suffixes.any { name.endsWith(it) }
}

allprojects {
    tasks.register("__DUMP_TASK__") {
        doLast {
            def deps = []
            project.configurations.each { cfg ->
                if (!isDependencyConfig(cfg.name)) return
                cfg.dependencies.each { dep ->
                    if (dep instanceof org.gradle.api.artifacts.ProjectDependency) {
                        deps << [configuration: cfg.name, kind: "project", path: dep.path]
                    } else {
                        deps << [configuration: cfg.name, kind: "module", group: dep.group, artifact: dep.name, version: dep.version]
                    }
                }
            }
            def model = [
                path: project.path,
                name: project.name,
                group: project.group.toString(),
                version: project.version.toString(),
                projectDir: project.projectDir.toString(),
                dependencies: deps,
            ]
            println("FOXGARDEN_JSON_BEGIN")
            println(JsonOutput.toJson(model))
            println("FOXGARDEN_JSON_END")
        }
    }
}
"#;

/// Picks `<project_root>/gradlew`/`gradlew.bat` when present (a project's
/// own pinned wrapper — the right version to actually build with) over a
/// bare `gradle` on `PATH`, mirroring `command_for_binary`'s own "prefer
/// what the project actually specifies" reasoning in `static_analysis.rs`,
/// though for a different concrete problem (a wrapper script vs. a bare
/// `.jar`, not a JVM-launcher distinction). Two platform-gated bodies, not
/// one that picks the extension internally: `gradlew` is a POSIX shell
/// script with no PE header, so `Command::new`ing it directly on Windows
/// fails outright rather than falling through to the bare-`gradle` branch —
/// the wrapper generator always emits *both* `gradlew`/`gradlew.bat`
/// together, so picking the right one per-OS, the same split
/// `../references/java`'s own `task_helper::build_tool::which_wrapper`
/// already uses, is correct rather than a guess.
#[cfg(windows)]
pub(crate) fn gradle_command(project_root: &Path) -> Command {
    let wrapper = project_root.join("gradlew.bat");
    if wrapper.is_file() {
        Command::new(wrapper)
    } else {
        Command::new("gradle")
    }
}

#[cfg(not(windows))]
pub(crate) fn gradle_command(project_root: &Path) -> Command {
    let wrapper = project_root.join("gradlew");
    if wrapper.is_file() {
        Command::new(wrapper)
    } else {
        Command::new("gradle")
    }
}

/// Runs the dump described by `INIT_SCRIPT` against every project in the
/// Gradle build rooted at `project_root` and parses the result. `--offline`
/// always set (see this module's own top-level doc comment). `--no-parallel`
/// always set too — a real, found-not-assumed bug: a project with
/// `org.gradle.parallel=true` in its own `gradle.properties` (the real
/// `boost` project this module validated against has exactly this) runs
/// each project's `doLast` concurrently, and their `println` output
/// interleaves *line-by-line* across projects — confirmed by a real run
/// producing `FOXGARDEN_JSON_BEGIN`/`FOXGARDEN_JSON_END` markers with
/// another project's own markers spliced in between, silently corrupting
/// `parse_dump_output`'s block structure. `--no-parallel` forces this one
/// invocation to run serially regardless of the project's own setting —
/// harmless for a read-only metadata dump that isn't a real build, and it's
/// the only thing that keeps each project's block atomic on stdout.
pub fn gradle_projects(project_root: &Path) -> Result<Vec<GradleProject>, GradleError> {
    let content = INIT_SCRIPT.replace("__DUMP_TASK__", DUMP_TASK);
    let script_path = write_temp_script("dump", &content).map_err(GradleError::Spawn)?;

    let output = gradle_command(project_root)
        .current_dir(project_root)
        .arg("--offline")
        .arg("--no-parallel")
        .arg("--init-script")
        .arg(&script_path)
        .arg("-q")
        .arg(DUMP_TASK)
        .output();
    let _ = std::fs::remove_file(&script_path);
    let output = output.map_err(GradleError::Spawn)?;

    parse_dump_output(&String::from_utf8_lossy(&output.stdout)).map_err(GradleError::Report)
}

const CLASSPATH_TASK: &str = "foxgardenGradleClasspathDump";

/// One project's own resolved classpath — real jar files on disk, not
/// declared coordinates (`GradleDependency`'s own scope). `compile`/
/// `runtime` mirror Gradle's own `compileClasspath`/`runtimeClasspath`
/// configurations; a project with neither (the real `boost` project's own
/// `frontend` module, for instance — no JVM plugin applied at all) simply
/// never appears in `gradle_classpaths`' own result rather than appearing
/// with two empty lists, so a caller can't mistake "not a JVM module" for
/// "a JVM module with zero dependencies."
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GradleClasspath {
    pub path: String,
    pub compile: Vec<PathBuf>,
    pub runtime: Vec<PathBuf>,
}

/// The init script `gradle_classpaths` uses — same `allprojects { }`/
/// per-project-JSON-block shape `INIT_SCRIPT` establishes, but resolving
/// `compileClasspath`/`runtimeClasspath` into real jar paths instead of
/// reading declared dependency notation. Unlike `INIT_SCRIPT`, this one
/// triggers genuine dependency *resolution* (downloading anything not
/// already cached, exactly like a real `gradle build` would) — `gradle_
/// classpaths` deliberately does **not** pass `--offline`, unlike `gradle_
/// projects`, since forcing offline here would just turn "not cached yet"
/// into a hard failure instead of a real (if slower) download.
const CLASSPATH_INIT_SCRIPT: &str = r#"
allprojects {
    tasks.register("__CLASSPATH_TASK__") {
        doLast {
            def compileCfg = project.configurations.findByName("compileClasspath")
            def runtimeCfg = project.configurations.findByName("runtimeClasspath")
            if (compileCfg == null && runtimeCfg == null) return
            if ((compileCfg != null && !compileCfg.canBeResolved) && (runtimeCfg != null && !runtimeCfg.canBeResolved)) return

            def compileFiles = (compileCfg != null && compileCfg.canBeResolved) ? compileCfg.resolve().collect { it.toString() } : []
            def runtimeFiles = (runtimeCfg != null && runtimeCfg.canBeResolved) ? runtimeCfg.resolve().collect { it.toString() } : []

            println("FOXGARDEN_CP_JSON_BEGIN")
            println(groovy.json.JsonOutput.toJson([path: project.path, compile: compileFiles, runtime: runtimeFiles]))
            println("FOXGARDEN_CP_JSON_END")
        }
    }
}
"#;

/// Resolves every JVM project's own real classpath in the Gradle build
/// rooted at `project_root` — the jar-file-list counterpart to `gradle_
/// projects`' own declared-dependency-notation dump, and the piece Phase 3
/// actually asks for ("Gradle's own resolution task, parsed into a
/// resolved jar-file list"). `--no-parallel` for the same real, found-not-
/// assumed reason `gradle_projects` needs it (see that function's own doc
/// comment) — classpath resolution is genuinely slow enough for the race
/// to actually manifest, which is exactly how this session found it in the
/// first place.
pub fn gradle_classpaths(project_root: &Path) -> Result<Vec<GradleClasspath>, GradleError> {
    let content = CLASSPATH_INIT_SCRIPT.replace("__CLASSPATH_TASK__", CLASSPATH_TASK);
    let script_path = write_temp_script("classpath", &content).map_err(GradleError::Spawn)?;

    let output = gradle_command(project_root)
        .current_dir(project_root)
        .arg("--no-parallel")
        .arg("--init-script")
        .arg(&script_path)
        .arg("-q")
        .arg(CLASSPATH_TASK)
        .output();
    let _ = std::fs::remove_file(&script_path);
    let output = output.map_err(GradleError::Spawn)?;

    parse_classpath_output(&String::from_utf8_lossy(&output.stdout)).map_err(GradleError::Report)
}

/// Pure parser for `gradle_classpaths`' own captured stdout — verified
/// against real captured output from the same real `boost` project
/// (this module's tests), not a guessed shape.
fn parse_classpath_output(stdout: &str) -> Result<Vec<GradleClasspath>, String> {
    split_marked_blocks(stdout, "FOXGARDEN_CP_JSON_BEGIN", "FOXGARDEN_CP_JSON_END")?
        .into_iter()
        .map(|json| serde_json::from_str(json).map_err(|e| e.to_string()))
        .collect()
}

/// Writes `content` to a fresh temp file named with `label` and this
/// process's own pid (so two concurrent scans, or a project-model dump
/// running alongside a classpath resolution, don't race on the same path)
/// and returns its path — a real file on disk since `--init-script` takes a
/// path, not stdin.
fn write_temp_script(label: &str, content: &str) -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("foxgarden-gradle-{label}-{}.gradle", std::process::id()));
    std::fs::write(&path, content)?;
    Ok(path)
}

/// Splits `stdout` on every `begin`/`end`-delimited region, returning each
/// one's trimmed inner text in order. Shared by `parse_dump_output` and
/// `parse_classpath_output` — both wrap one JSON object per project in the
/// same plain-text marker-pair shape, differing only in which markers and
/// which JSON shape they use.
fn split_marked_blocks<'a>(stdout: &'a str, begin: &str, end: &str) -> Result<Vec<&'a str>, String> {
    let mut blocks = Vec::new();
    let mut rest = stdout;

    while let Some(start) = rest.find(begin) {
        let after_begin = &rest[start + begin.len()..];
        let end_at = after_begin
            .find(end)
            .ok_or_else(|| format!("{begin} with no matching {end}"))?;
        blocks.push(after_begin[..end_at].trim());
        rest = &after_begin[end_at + end.len()..];
    }

    Ok(blocks)
}

/// Parses `gradle_projects`' own captured stdout: one `GradleProject` per
/// `FOXGARDEN_JSON_BEGIN`/`FOXGARDEN_JSON_END`-delimited JSON object, in the
/// order Gradle evaluated the projects. Pure/no I/O — verified against real
/// captured output from a real multi-module Kotlin/Spring Gradle project
/// (this module's tests), not a guessed shape.
fn parse_dump_output(stdout: &str) -> Result<Vec<GradleProject>, String> {
    let mut projects = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();

    for json in split_marked_blocks(stdout, "FOXGARDEN_JSON_BEGIN", "FOXGARDEN_JSON_END")? {
        let project: GradleProject = serde_json::from_str(json).map_err(|e| e.to_string())?;
        // `allprojects { tasks.register(...) }` registers the dump task
        // exactly once per project, so a duplicate path would mean this
        // parser mis-split the markers, not a legitimate second project.
        if !seen_paths.insert(project.path.clone()) {
            return Err(format!("duplicate project path in dump output: {}", project.path));
        }
        projects.push(project);
    }

    Ok(projects)
}

#[cfg(test)]
#[path = "gradle_test.rs"]
mod gradle_test;
