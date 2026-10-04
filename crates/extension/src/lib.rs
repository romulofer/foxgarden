//! `PLAN.md` Track 24 Phase 1: what an extension may contribute, and the
//! registry that holds it.
//!
//! This crate is the seam between a language-agnostic core and the things
//! that know about particular languages. It deliberately depends on
//! neither `fg-core` nor `syntax`: `fg-core` still owns Maven/Gradle/JDK
//! concepts today, and a registry that could reach them would be free to
//! grow JVM assumptions the same way the rest of the tree already has.
//! Keeping the dependency edge absent makes that a compile error rather
//! than a matter of discipline — which, given 76 of 138 non-test files
//! currently name a JVM concept, is the only enforcement worth relying on.
//!
//! The registry is what the running app consults for every language
//! question: which language a file is, its grammar, its language servers,
//! its build tools, what a new project of a kind is made of, and where a
//! file's run markers go. The JVM support itself lives in the `spring`
//! extension, compiled in through `fg-languages`.
//!
//! The shape follows Zed's own extension host (`../references/zed`,
//! `crates/extension/src/extension_host_proxy.rs`): an extension declares
//! a manifest and pushes contributions through `register_*` calls. The
//! call an in-process module makes here in Phase A is the same call a
//! dynamically loaded extension will make in Phase B, which is the point —
//! the API gets exercised for real by `spring` long before anything is
//! loaded from outside the binary.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tree_sitter_language::LanguageFn;

mod registry;
mod tooling;

pub use registry::{RegisterError, Registry};
pub use tooling::{
    BuildProblem, BuildTask, BuildToolContribution, BuildToolHandle, BuildToolId, CommandSpec, ConfigProperty,
    CoverageReport, ExtensionHandle, ProjectRelease, RunTarget, ScaffoldContribution, ScaffoldSpec,
    CoverageStatus, LineCoverage, ProblemSeverity, RunSpec, TestCase, TestFailureLocation, TestOutcome, TestSummary,
    summarize,
};

/// A registered language's stable identifier — `"java"`, `"kotlin"`,
/// `"yaml"`. Lowercase by convention and used as the key everywhere a
/// contribution needs to name a language, including across extensions: a
/// language server contributed by one extension may serve a language
/// contributed by another, and an id is the only thing they can agree on.
pub type LanguageId = String;

/// Identity and API-compatibility declaration for one extension.
///
/// `schema_version` is what makes Phase B's stability promise
/// enforceable: Phase A is explicitly allowed to churn this crate's API,
/// and the version is what a host will eventually refuse to load against
/// when an extension was built for an incompatible one. It is recorded
/// from the start rather than added later because an extension format
/// with no version field has no way to *become* versioned without
/// breaking every extension that already exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub schema_version: u32,
}

/// The `schema_version` this build understands. Phase A churns freely, so
/// this stays 0 until Phase B freezes the API; a 0 here is a deliberate
/// "no compatibility promised yet", not an unset field.
pub const CURRENT_SCHEMA_VERSION: u32 = 0;

/// Everything the core needs to recognize a file as belonging to a
/// language, with no knowledge of which language it is.
///
/// Replaces what `fg_core::Language` hardcodes today: its `from_extension`
/// map, its `display_name`, and its `from_filename` special case. That
/// last one is why `filename_patterns` exists at all — a bare `Dockerfile`
/// has no extension to key off, and real projects write it as
/// `Dockerfile`, `Dockerfile.dev` and `dev.Dockerfile` interchangeably, so
/// extension matching alone cannot recognize it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageContribution {
    pub id: LanguageId,
    /// How the language is named in the UI — the status bar's language
    /// indicator. Kept separate from `id` so that renaming what users see
    /// never silently changes the key other contributions match on.
    pub display_name: String,
    /// Extensions with no leading dot, lowercase: `["yml", "yaml"]`.
    pub file_extensions: Vec<String>,
    /// Whole-filename patterns, for files an extension cannot identify.
    pub filename_patterns: Vec<FilenamePattern>,
}

/// A whole-filename match, for languages whose files may carry no usable
/// extension. Case-insensitive on `stem`, because a lowercase `dockerfile`
/// is common on case-sensitive filesystems, but never on the surrounding
/// free text: a stage suffix like `.dev` is user-chosen, not a keyword.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilenamePattern {
    /// The filename is exactly `stem`.
    Exact { stem: String },
    /// `stem`, or `stem.<anything>`, or `<anything>.stem` — the three
    /// spellings real Dockerfiles appear as.
    StemWithAffix { stem: String },
}

impl FilenamePattern {
    /// Whether `filename` (a bare file name, not a path) matches.
    pub fn matches(&self, filename: &str) -> bool {
        let lower = filename.to_lowercase();
        match self {
            Self::Exact { stem } => lower == stem.to_lowercase(),
            Self::StemWithAffix { stem } => {
                let stem = stem.to_lowercase();
                lower == stem
                    || lower.starts_with(&format!("{stem}."))
                    || lower.ends_with(&format!(".{stem}"))
            }
        }
    }
}

/// A tree-sitter grammar plus the highlight query that goes with it.
///
/// Carries its own `highlight_query` rather than leaving it to a separate
/// contribution because a query is written against one specific grammar's
/// node names and is meaningless apart from it — splitting them would
/// invite pairing a query with a grammar it cannot compile against.
#[derive(Debug, Clone)]
pub struct GrammarContribution {
    pub language_id: LanguageId,
    pub source: GrammarSource,
    /// Tree-sitter query source (`.scm`). `None` means the language parses
    /// but paints unhighlighted, which is already how this codebase treats
    /// a file with no grammar at all.
    pub highlight_query: Option<String>,
    /// Which of this grammar's node kinds mean what to the editor. Travels
    /// with the grammar for the same reason the query does: a node name is
    /// one grammar's vocabulary and means nothing against another's.
    pub node_kinds: NodeKinds,
}

/// The node kinds an editor feature needs to name, per grammar.
///
/// Every field defaults to empty, and empty means "this feature is a no-op
/// for this language" — which is exactly how the core treated a language it
/// had no entry for when these lists were hardcoded. A new grammar is
/// therefore never *wrong* here, only quiet, until its author fills it in
/// against the grammar's own `node-types.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeKinds {
    /// Declarations whose header line sticky scroll pins while their body
    /// scrolls underneath. Declarations only, not control flow — matching
    /// what mainstream editors pin by default.
    pub scopes: Vec<String>,
    /// Nodes whose body collapses when folded: the *bodies* and block
    /// comments, not the declarations in `scopes`, since folding hides what
    /// sits between the delimiters and leaves the opening line (where the
    /// fold marker is) visible.
    pub foldable: Vec<String>,
    /// The node kind one import statement is, for folding an import block
    /// as a unit. `None` disables that.
    pub import: Option<String>,
}

/// Where a grammar's parser comes from.
///
/// Both arms are real and both are needed: Phase 3 moves today's grammars
/// off `Builtin` and onto `SharedLibrary`, but `Builtin` does not go away,
/// because an extension compiled into the binary (which is every extension
/// in Phase A) has no reason to pay for a runtime load.
#[derive(Clone)]
pub enum GrammarSource {
    /// Linked into the binary, as every grammar is today.
    Builtin(LanguageFn),
    /// Loaded at runtime from a shared library exporting `symbol`.
    ///
    /// Verified workable against the pinned tree-sitter before this API
    /// was written — see `PLAN.md` Track 24 Checkpoint 0 for the spike,
    /// its measured 150µs load cost, and the two constraints it surfaced:
    /// the loaded library must outlive every tree parsed with it (so
    /// unloading is not supported), and a grammar's ABI version must be
    /// checked against this build's accepted range rather than assumed.
    SharedLibrary { path: PathBuf, symbol: String },
}

impl std::fmt::Debug for GrammarSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // `LanguageFn` is a bare function pointer with no useful
            // representation, so naming the arm is all this can honestly say.
            Self::Builtin(_) => f.write_str("Builtin(..)"),
            Self::SharedLibrary { path, symbol } => f
                .debug_struct("SharedLibrary")
                .field("path", path)
                .field("symbol", symbol)
                .finish(),
        }
    }
}

/// A language server an extension provides, as a description rather than a
/// running process — the core owns process lifecycle (`lsp_state`), and an
/// extension only says what to start and how to configure it.
///
/// `initialization_options` is a plain JSON string rather than a parsed
/// value specifically so this crate needs no JSON dependency: the core
/// parses it when it builds the `initialize` request. That matters because
/// it is the field that currently lives as a hardcoded `match` on
/// `ServerKind` inside `lsp_state`, carrying jdt.ls-specific settings in
/// core code; moving it here is Phase 4's main job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageServerContribution {
    pub id: String,
    /// Human-readable name for error messages and the status bar — "Eclipse
    /// JDT Language Server", not the stable `id` the registry keys on.
    pub display_name: String,
    /// Which registered languages this server serves. More than one is
    /// legitimate — a single server handling several languages is common.
    pub language_ids: Vec<LanguageId>,
    /// The executable to launch, resolved by the core against settings and
    /// `PATH` the same way it resolves external tools today.
    pub binary_name: String,
    pub args: Vec<String>,
    /// LSP `initializationOptions`, as unparsed JSON. `None` sends none.
    pub initialization_options: Option<String>,
}

/// One JDK installation, for extensions that need to communicate which JVMs
/// are available on this machine. Named `JdkRuntime` rather than the
/// jdt.ls-specific vocabulary (`java.configuration.runtimes`) to stay
/// language-server-neutral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JdkRuntime {
    /// Eclipse execution environment name that jdt.ls uses to identify this
    /// release — e.g. `"JavaSE-21"`, `"JavaSE-1.8"`.
    pub name: String,
    pub path: PathBuf,
    pub major: u32,
}

/// Runtime context the core passes to an extension when starting a language
/// server — everything an extension might need to compute a final binary
/// path, launch arguments, and `initializationOptions`.
#[derive(Debug, Clone, Default)]
pub struct ServerStartContext {
    /// The binary path the user configured for this server — empty means
    /// "no path configured yet". An extension may treat that as an error
    /// (jdt.ls cannot be found on `PATH` by a bare name); a server started
    /// as declared falls back to its `binary_name`, looked up on `PATH`.
    pub configured_binary: String,
    /// A JVM home hint from settings (empty = auto-detect). Only meaningful
    /// for servers that run on the JVM themselves.
    pub java_home: String,
    /// The Java release the current project declares, if any.
    pub java_release: Option<u32>,
    /// Every JDK found on this machine, for servers that need to tell the
    /// language server which runtimes are available.
    pub jdk_runtimes: Vec<JdkRuntime>,
}

/// How an extension says to actually start one of its servers, computed from
/// the `ServerStartContext` the core supplies.
#[derive(Debug, Clone)]
pub struct ResolvedServerStart {
    pub binary: PathBuf,
    pub args: Vec<String>,
    /// `initializationOptions`, as unparsed JSON. `None` sends none.
    pub initialization_options: Option<String>,
    /// Opaque string the core stores as a restart-detection key. If this
    /// value differs from the previously stored one the running session is
    /// retired and a new one started. An empty string means "never restart
    /// because of environment changes" — correct for servers whose
    /// configuration is entirely static.
    pub restart_key: String,
}

/// What one extension contributes, gathered in one place.
///
/// An extension builds this and hands it over; it does not reach into the
/// registry itself. Keeping registration a single handover rather than a
/// sequence of calls means a contribution set can be validated as a whole
/// (and rejected as a whole) before any of it is visible to the core —
/// there is no state in which half an extension is registered.
#[derive(Debug, Default)]
pub struct Contributions {
    pub languages: Vec<LanguageContribution>,
    pub grammars: Vec<GrammarContribution>,
    pub language_servers: Vec<LanguageServerContribution>,
    pub build_tools: Vec<BuildToolContribution>,
    pub scaffolds: Vec<ScaffoldContribution>,
}

/// One extension.
///
/// In Phase A every implementor is a module compiled into the binary
/// (`spring` being the first and, for now, only one). Phase B adds
/// implementors backed by something loaded at runtime, which is why this
/// is a trait rather than a struct: the core consumes extensions through
/// it without knowing which kind it has.
pub trait Extension: Send + Sync {
    fn manifest(&self) -> ExtensionManifest;
    fn contributions(&self) -> Contributions;

    /// Every JDK this extension knows about on this machine — populated by
    /// extensions that know how to scan for JDKs (the spring extension is
    /// the canonical one). Called once per frame so the background scan
    /// result lands without a session restart.
    ///
    /// The default returns an empty list, correct for extensions that do
    /// not work with the JVM at all.
    fn jdk_runtimes(&self) -> Vec<JdkRuntime> {
        Vec::new()
    }

    /// How to start server `server_id`, given runtime `context`. Called
    /// every frame so the extension can react to environment changes
    /// (completed JDK scans, changed settings) by returning a different
    /// `restart_key`.
    ///
    /// `None` — the default — means "start it as declared": the registry
    /// then builds the start from the `LanguageServerContribution` it
    /// already holds (see `Registry::resolve_server_start`), which is right
    /// for a server whose configuration is fully static. Extensions with
    /// dynamic init options (jdt.ls runtimes, debug bundles) override this.
    /// The default used to rebuild this extension's whole `contributions()`
    /// on every call just to find one server, which is per-frame work.
    fn resolve_server_start(&self, _server_id: &str, _context: &ServerStartContext) -> Option<Result<ResolvedServerStart, String>> {
        None
    }

    /// The process that performs `task` for build tool `tool_id`, or `None`
    /// when this extension does not own that tool, or owns it but has no
    /// support for that particular task (coverage being the realistic case).
    ///
    /// Every build-tool method below is defaulted to "nothing", so an
    /// extension that contributes no build tool implements none of them.
    fn build_command(&self, _tool_id: &str, _project_root: &Path, _task: BuildTask) -> Option<CommandSpec> {
        None
    }

    /// The process that launches the project's own program for `run`.
    /// `Some(Err(..))` when the tool owns this but could not assemble the
    /// launch (classpath resolution failed, no runnable module).
    fn run_command(&self, _tool_id: &str, _project_root: &Path, _run: &RunSpec) -> Option<Result<CommandSpec, String>> {
        None
    }

    /// Where `tool_id`'s own build drops compiled output under
    /// `project_root`. Not verified to exist — a project that has not been
    /// built yet simply has nothing there, which callers interpret
    /// themselves.
    fn classes_dir(&self, _tool_id: &str, _project_root: &Path) -> Option<PathBuf> {
        None
    }

    /// The project's full runtime path, compiled output included, most
    /// specific first.
    fn runtime_classpath(&self, _tool_id: &str, _project_root: &Path) -> Option<Result<Vec<PathBuf>, String>> {
        None
    }

    /// A tolerant classpath for metadata scanning — see
    /// [`BuildToolHandle::analysis_classpath`]. Defaults to nothing found
    /// rather than to `runtime_classpath`, since "strict resolution, errors
    /// surfaced" and "best effort, silence on failure" are genuinely
    /// different jobs and quietly conflating them would make a background
    /// scan able to fail a user-initiated Run.
    fn analysis_classpath(&self, _tool_id: &str, _project_root: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    /// The diagnostic `line` of `tool_id`'s own build output names, if any.
    /// Called once per output line as it arrives, so it must stay cheap and
    /// must never touch the filesystem.
    fn parse_build_output_line(&self, _tool_id: &str, _line: &str) -> Option<BuildProblem> {
        None
    }

    /// The coverage report the last coverage run wrote, read and resolved to
    /// real source files. `None` when the tool has no coverage support at
    /// all; `Err` when a report was expected but could not be read.
    fn coverage_results(&self, _tool_id: &str, _project_root: &Path) -> Option<Result<CoverageReport, String>> {
        None
    }

    /// Every test case the last test run reported. Empty rather than `None`
    /// — "the test task never ran" and "it ran and reported nothing" are
    /// both ordinary, and neither is an error worth a separate arm.
    fn test_results(&self, _tool_id: &str, _project_root: &Path) -> Vec<TestCase> {
        Vec::new()
    }

    /// The files a new project of `spec`'s own kind needs, as `(path
    /// relative to the project root, contents)` pairs. Pure generation —
    /// writing them to disk is the core's job (`fg_core::write_scaffold`),
    /// which keeps "refuse to scaffold into a non-empty directory" one rule
    /// in one place rather than a promise every extension has to keep.
    ///
    /// `None` when this extension does not scaffold that build tool/language
    /// pair.
    fn scaffold_files(&self, _spec: &ScaffoldSpec) -> Option<Vec<(PathBuf, String)>> {
        None
    }

    /// Which runtime release `project_root` declares, read from whatever
    /// build files this extension understands. `None` means "nothing I
    /// recognize says", which is a normal answer and leaves the decision to
    /// whoever asked.
    fn project_release(&self, _project_root: &Path) -> Option<ProjectRelease> {
        None
    }

    /// Every runnable entry point in `source`, whose parse `tree` the editor
    /// already built — in source order.
    ///
    /// Takes the tree rather than only the text on purpose: this is asked
    /// again on every edit to a file, and re-parsing it here would put a
    /// second full parse on the keystroke path. `file_stem` is the file's
    /// name without its extension, which some toolchains need (a JVM
    /// top-level function compiles into a class named after the *file*).
    fn run_targets(
        &self,
        _language_id: &str,
        _tree: &tree_sitter::Tree,
        _source: &str,
        _file_stem: &str,
    ) -> Vec<RunTarget> {
        Vec::new()
    }

    /// Configuration keys this extension can offer for completion in
    /// `project_root`'s own configuration files. Called off the UI thread —
    /// resolving these may shell out to a build tool.
    ///
    /// `build_tool` is the tool the registry detected for the project, which
    /// may belong to another extension. Handed over rather than left for each
    /// extension to work out again from marker files, so that every answer
    /// agrees with the registry's own detection.
    fn config_properties(&self, _project_root: &Path, _build_tool: &BuildToolHandle) -> Vec<ConfigProperty> {
        Vec::new()
    }

    /// Where a failing `case` lives, for click-to-jump.
    fn test_failure_location(
        &self,
        _tool_id: &str,
        _project_root: &Path,
        _case: &TestCase,
    ) -> Option<TestFailureLocation> {
        None
    }
}

/// Everything a registry knows about one registered language, gathered
/// from however many extensions contributed parts of it.
#[derive(Debug)]
pub struct RegisteredLanguage {
    pub language: LanguageContribution,
    /// The language's id, leaked so it lasts the process.
    ///
    /// This is what lets a `Language` handle stay `Copy` while the set of
    /// languages is open — the same bargain grammars already make (a
    /// loaded grammar's library must outlive every tree parsed with it, so
    /// nothing registered is ever unregistered). Leaking here is bounded
    /// by the number of registered languages, not by anything a user does.
    pub static_id: &'static str,
    /// Which extension contributed the language itself — the one to name
    /// in an error, and eventually the one to disable.
    pub extension_id: String,
    pub grammar: Option<GrammarContribution>,
    /// Which extension contributed `grammar`. Not necessarily
    /// `extension_id`: one extension may supply the grammar for a language
    /// another declared, and a grammar that fails to load has to be blamed
    /// on the extension that shipped it.
    pub grammar_extension_id: Option<String>,
}

/// Lookup index from a file extension or filename to a language id.
#[derive(Debug, Default)]
pub(crate) struct LanguageIndex {
    by_extension: HashMap<String, LanguageId>,
    by_pattern: Vec<(FilenamePattern, LanguageId)>,
}

impl LanguageIndex {
    pub(crate) fn insert(&mut self, language: &LanguageContribution) {
        for ext in &language.file_extensions {
            self.by_extension.insert(ext.to_lowercase(), language.id.clone());
        }
        for pattern in &language.filename_patterns {
            self.by_pattern.push((pattern.clone(), language.id.clone()));
        }
    }

    pub(crate) fn by_extension(&self, ext: &str) -> Option<&LanguageId> {
        self.by_extension.get(&ext.to_lowercase())
    }

    pub(crate) fn by_filename(&self, filename: &str) -> Option<&LanguageId> {
        self.by_pattern
            .iter()
            .find(|(pattern, _)| pattern.matches(filename))
            .map(|(_, id)| id)
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_test;
