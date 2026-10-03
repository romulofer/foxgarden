//! `PLAN.md` Track 24 Phase 5: what a build tool is, as far as the core is
//! concerned.
//!
//! The core's Build/Run/Test/Coverage machinery used to branch on a closed
//! `BuildTool { Maven, Gradle }` enum and assemble `mvn`/`gradle` command
//! lines itself. None of that is a generic editor concern: *which* process
//! compiles a project, where it drops its class files, what its diagnostic
//! output looks like and where it writes its test report are all answers
//! only the toolchain's own extension has. What stays generic is the
//! shape of those answers, which is what this module describes.
//!
//! Every type here is deliberately a plain data description rather than a
//! live process: an extension says *what* to run, and the core owns
//! spawning, streaming, cancelling and reporting — the same split
//! [`crate::LanguageServerContribution`] already makes for language
//! servers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Extension;

/// A registered build tool's stable identifier — `"maven"`, `"gradle"`.
/// Lowercase by convention, and the key everything else matches on.
pub type BuildToolId = String;

/// A build tool an extension provides.
///
/// `marker_files` is what replaces the core's own hardcoded "`pom.xml` means
/// Maven, `build.gradle[.kts]` means Gradle" check. Detection is file
/// presence at the project root and nothing more, which is all the previous
/// hardcoded version did too — it is stated as data here so that a tool the
/// core has never heard of is detectable on the same terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildToolContribution {
    pub id: BuildToolId,
    /// How the tool is named in the UI — "Maven", "Gradle". Kept separate
    /// from `id` for the same reason [`crate::LanguageContribution`] keeps
    /// them separate: renaming what a user sees must never change the key
    /// another contribution matches on.
    pub display_name: String,
    /// Any one of these existing at the project root means this tool builds
    /// the project. Checked in order, first match wins.
    pub marker_files: Vec<String>,
}

/// Which of a build tool's own standard jobs the core is asking for.
///
/// Not every tool supports every task — coverage in particular is a plugin
/// concern that only some toolchains answer — so a `None` from
/// [`BuildToolHandle::command`] is a normal result the UI reports, not an
/// error state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTask {
    /// Compile the project's own sources.
    Compile,
    /// Run the project's tests, writing whatever report
    /// [`BuildToolHandle::test_results`] later reads back.
    Test,
    /// Run the tests with coverage instrumentation, writing whatever report
    /// [`BuildToolHandle::coverage_results`] later reads back.
    Coverage,
}

/// A process to run, described rather than spawned.
///
/// Deliberately not a `std::process::Command`: an extension loaded from
/// outside this binary (Phase B) cannot hand over a live `Command`, so the
/// seam has to be plain data from the start. [`CommandSpec::to_command`] is
/// the one place that conversion happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Working directory. Required rather than optional — every build
    /// invocation this replaces already set one, and inheriting the editor's
    /// own cwd is never the right answer for a build.
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// The real, spawnable process. The core calls this; an extension never
    /// has to.
    pub fn to_command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.program);
        command.args(&self.args).current_dir(&self.cwd);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
    }
}

/// What the core knows about launching a project's own program, with no
/// opinion about which runtime launches it.
///
/// This is `fg_core::RunConfig` with the editor-side bookkeeping (its name,
/// its on-disk form) left behind — the subset an extension needs to turn a
/// saved run configuration into a real process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunSpec {
    /// What to run, in whatever notation the toolchain uses — a fully
    /// qualified main class on the JVM, a binary name elsewhere.
    pub entry_point: String,
    /// Arguments for the *runtime*, not the program (JVM `-X…` flags and
    /// the like). Whitespace-separated, as the user typed them.
    pub vm_args: String,
    /// Arguments for the program itself, whitespace-separated.
    pub program_args: String,
    pub env: Vec<(String, String)>,
    /// `None` means the project root.
    pub working_dir: Option<PathBuf>,
}

/// How bad one build-output diagnostic is. A deliberately smaller vocabulary
/// than `fg_core::Severity` (which also carries LSP's information/hint
/// levels): a compiler either refused to compile something or warned about
/// it, and inventing a mapping for levels no build tool emits would be
/// guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemSeverity {
    Error,
    Warning,
}

/// One compiler diagnostic an extension recognized in a line of its tool's
/// own build output — enough to paint a clickable row and jump to it.
///
/// Only line/column, never a byte offset: the core converts once the target
/// file's buffer is actually open, the same deferred conversion it already
/// does for every other "jump there" producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProblem {
    pub path: PathBuf,
    /// 1-based.
    pub line: usize,
    /// 1-based; `1` when the tool's output names no real column.
    pub column: usize,
    pub severity: ProblemSeverity,
    pub message: String,
}

/// Whether one source line's instructions/branches were covered, as the
/// coverage tool classified them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageStatus {
    Covered,
    Missed,
    Partial,
}

/// One covered line. `line` is 0-based — this codebase's own line space, so
/// the gutter painter can index rows directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCoverage {
    pub line: usize,
    pub status: CoverageStatus,
}

/// Per-file coverage data: one real source file and the lines the coverage
/// tool reported for it.
pub type CoverageReport = Vec<(PathBuf, Vec<LineCoverage>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOutcome {
    Passed,
    Failed,
    Errored,
    Skipped,
}

/// One test the last test run reported.
///
/// `classname`/`name` are the tool's own notions of "which group" and
/// "which case"; the core only ever displays them and hands them back to
/// the extension to resolve a source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    pub classname: String,
    pub name: String,
    pub outcome: TestOutcome,
    /// Failure message plus whatever trace the tool recorded — `Some`
    /// exactly when `outcome` is `Failed`/`Errored`.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TestSummary {
    pub total: usize,
    pub failed: usize,
    pub errored: usize,
    pub skipped: usize,
}

impl TestSummary {
    pub fn passed(&self) -> usize {
        self.total.saturating_sub(self.failed + self.errored + self.skipped)
    }

    pub fn all_passed(&self) -> bool {
        self.failed == 0 && self.errored == 0
    }
}

/// Counts outcomes. Generic arithmetic over whatever the extension reported,
/// which is why it stays on this side of the seam.
pub fn summarize(cases: &[TestCase]) -> TestSummary {
    let mut summary = TestSummary {
        total: cases.len(),
        ..Default::default()
    };
    for case in cases {
        match case.outcome {
            TestOutcome::Failed => summary.failed += 1,
            TestOutcome::Errored => summary.errored += 1,
            TestOutcome::Skipped => summary.skipped += 1,
            TestOutcome::Passed => {}
        }
    }
    summary
}

/// Where a failing test actually lives, as the owning extension resolved it
/// — a real file on disk plus the 1-based line its failure blames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFailureLocation {
    pub path: PathBuf,
    pub line: usize,
}

/// A detected build tool, bundled with the extension that owns it.
///
/// This is what the core passes around in place of the `BuildTool` enum it
/// used to match on. Holding an `Arc` to the owning extension rather than a
/// borrow of the registry is what lets a long-running job keep working
/// through it — a build streams output for minutes and a `&Registry`
/// borrowed across that would conflict with every other frame's mutable
/// access to editor state.
#[derive(Clone)]
pub struct BuildToolHandle {
    /// The tool's id, leaked so it lasts the process — the same bargain
    /// [`crate::RegisteredLanguage::static_id`] makes, and for the same
    /// reason: nothing registered is ever unregistered.
    pub id: &'static str,
    pub display_name: String,
    extension: Arc<dyn Extension>,
}

impl std::fmt::Debug for BuildToolHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildToolHandle")
            .field("id", &self.id)
            .field("display_name", &self.display_name)
            .finish_non_exhaustive()
    }
}

impl PartialEq for BuildToolHandle {
    /// Identity is the tool id — two handles to the same registered tool are
    /// the same tool, whichever lookup produced them.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl BuildToolHandle {
    pub(crate) fn new(id: &'static str, display_name: String, extension: Arc<dyn Extension>) -> Self {
        Self {
            id,
            display_name,
            extension,
        }
    }

    /// The process for one of the tool's standard tasks, or `None` when this
    /// tool does not support that task at all.
    pub fn command(&self, project_root: &Path, task: BuildTask) -> Option<CommandSpec> {
        self.extension.build_command(self.id, project_root, task)
    }

    /// The process that launches the project's own program. `None` when the
    /// tool does not know how to run anything; `Err` when it does but
    /// something (classpath resolution, a missing root module) failed.
    pub fn run_command(&self, project_root: &Path, run: &RunSpec) -> Option<Result<CommandSpec, String>> {
        self.extension.run_command(self.id, project_root, run)
    }

    /// Where a successful build leaves its compiled output — needed by
    /// anything that analyses compiled artifacts rather than sources.
    pub fn classes_dir(&self, project_root: &Path) -> Option<PathBuf> {
        self.extension.classes_dir(self.id, project_root)
    }

    /// Everything the project needs on its runtime path, most specific
    /// first, compiled output included. `Err` on a real resolution failure.
    pub fn runtime_classpath(&self, project_root: &Path) -> Option<Result<Vec<PathBuf>, String>> {
        self.extension.runtime_classpath(self.id, project_root)
    }

    /// A best-effort classpath for reading metadata out of the project's
    /// dependencies — tolerant where [`Self::runtime_classpath`] is strict,
    /// because a background scan nobody asked for must never surface an
    /// error.
    pub fn analysis_classpath(&self, project_root: &Path) -> Vec<PathBuf> {
        self.extension.analysis_classpath(self.id, project_root)
    }

    /// The problem one line of this tool's output names, if any.
    pub fn parse_output_line(&self, line: &str) -> Option<BuildProblem> {
        self.extension.parse_build_output_line(self.id, line)
    }

    /// The coverage data the last coverage run left on disk. `None` when the
    /// tool has no coverage support.
    pub fn coverage_results(&self, project_root: &Path) -> Option<Result<CoverageReport, String>> {
        self.extension.coverage_results(self.id, project_root)
    }

    /// Every test the last test run reported. Empty (not an error) when no
    /// report exists — a test task that never ran is an ordinary state.
    pub fn test_results(&self, project_root: &Path) -> Vec<TestCase> {
        self.extension.test_results(self.id, project_root)
    }

    /// Where `case` lives, for click-to-jump. `None` is normal: a generated
    /// or nonstandard-layout test simply shows without a jump target.
    pub fn test_failure_location(&self, project_root: &Path, case: &TestCase) -> Option<TestFailureLocation> {
        self.extension.test_failure_location(self.id, project_root, case)
    }
}
