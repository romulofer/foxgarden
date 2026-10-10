//! `PLAN.md` Track 24 Phase 6: external static-analysis tools.
//!
//! Checkstyle, PMD and SpotBugs are three instances of one idea: a
//! command-line tool the user points the editor at (or lets it download),
//! which is run over the project on demand and prints findings the editor
//! paints as squiggles next to the syntax errors. Which tools exist, how a
//! release is downloaded and verified, how each one is invoked and how its
//! report is read are all the contributing extension's; what the editor
//! keeps is the lifecycle — settings, install, background run, squiggles.
//!
//! Findings carry line/column rather than byte offsets for the reason
//! [`crate::BuildProblem`] does: a byte range needs the file's current text,
//! which the core reads once per file when it converts them.

use std::path::PathBuf;

use crate::ProblemSeverity;

/// What a tool analyzes, which decides what the editor must have ready
/// before it can run one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzerInput {
    /// The project's source files; runnable any time.
    Sources,
    /// The build's compiled output, so the project must have been built
    /// first. The editor asks the detected build tool where that is, and
    /// says "build first" itself when there is nothing there yet.
    BuildOutput,
}

/// One external analysis tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzerContribution {
    /// Stable key — settings, results and running state are all keyed by
    /// it, so it must never change once shipped. Lowercase, like `"pmd"`.
    pub id: String,
    /// How the UI names it: `"PMD"`.
    pub display_name: String,
    pub input: AnalyzerInput,
    /// The label of the tool's configuration field, `"Ruleset (-R)"`. `None`
    /// when the tool takes no configuration of its own.
    pub config_label: Option<String>,
    /// How the editor can fetch the tool for the user. `None` when the user
    /// has to point at an install of their own.
    pub install: Option<AnalyzerInstall>,
}

/// A pinned, verifiable release of a tool.
///
/// Pinning the version is not enough for integrity: a release asset can be
/// deleted and re-uploaded under the same tag, and the download becomes
/// code the editor then runs. Hence `sha256` is required, and an install
/// whose download does not hash to it is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzerInstall {
    /// The release being installed, shown to the user and recorded as the
    /// installed version.
    pub version: String,
    pub download_url: String,
    /// Lowercase hex SHA-256 of exactly what `download_url` serves.
    pub sha256: String,
    pub packaging: AnalyzerPackaging,
    /// The configuration the tool is set to after installing, when it ships
    /// a usable default of its own.
    pub default_config: Option<String>,
    /// Where to ask what the newest release is, for "Check for Updates".
    pub latest_release: Option<LatestRelease>,
}

/// How a downloaded release becomes something runnable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalyzerPackaging {
    /// The download is the tool itself, saved under this file name. A bare
    /// `.jar` is launched through the extension, which knows how.
    File { name: String },
    /// The download is a zip whose single top-level directory holds the
    /// launcher.
    Zip {
        /// A substring of that directory's name, to find it without
        /// hardcoding the archive's versioned layout.
        dir_hint: String,
        /// The launcher's path inside that directory, `bin/pmd`. Spelled per
        /// platform by the extension: a Windows archive ships a differently
        /// named script.
        launcher: String,
    },
}

/// Where a tool publishes its newest release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestRelease {
    /// A GitHub-style `releases/latest` endpoint answering JSON with a
    /// `tag_name`.
    pub api_url: String,
    /// Stripped from the tag to leave a bare version, comparable with
    /// [`AnalyzerInstall::version`]. Empty when tags are bare versions.
    pub tag_prefix: String,
}

/// What an analyzer is run against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzerRun {
    /// The configured launcher or jar.
    pub binary: PathBuf,
    /// The configured configuration, verbatim, when the tool has a config
    /// field. Not necessarily a path: it can name a ruleset bundled inside
    /// the tool.
    pub config: String,
    pub project_root: PathBuf,
    /// The build's compiled output. `Some` exactly for
    /// [`AnalyzerInput::BuildOutput`] tools.
    pub classes_dir: Option<PathBuf>,
}

/// One finding an analyzer reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzerFinding {
    /// Absolute path of the file the finding is in.
    pub file: PathBuf,
    /// 1-based.
    pub line: usize,
    /// 1-based character offset in `line`. `None` for a whole-line finding.
    pub column: Option<usize>,
    /// Where the finding ends, as an inclusive 1-based `(line, column)`.
    /// `None` for a tool that reports a point, which is painted as one
    /// character.
    pub end: Option<(usize, usize)>,
    pub severity: ProblemSeverity,
    pub message: String,
}
