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

mod analyzers;
mod capability;
mod code_model;
mod editing;
mod imports;
mod info;
mod registry;
mod runtimes;
mod servers;
mod tooling;

pub use analyzers::{
    AnalyzerContribution, AnalyzerFinding, AnalyzerInput, AnalyzerInstall, AnalyzerPackaging, AnalyzerRun, LatestRelease,
};
pub use capability::{Capability, required as required_capabilities};
pub use code_model::{
    CodeGeneration, MemberKind, MemberView, ReceiverType, TypeDeclaration, TypeFields, TypeMember,
};
pub use editing::Snippet;
pub use imports::{DocStub, ImportCandidate, ImportEdit};
pub use info::ExtensionInfo;
pub use registry::{RegisterError, Registry};
pub use runtimes::{RuntimeContribution, RuntimeHandle, RuntimeInstall};
pub use servers::{ArchiveKind, LatestVersionSource, ServerInstall, ServerInstallHandle};
pub use tooling::{
    AnalyzerHandle, BuildProblem, BuildTask, BuildToolContribution, BuildToolHandle, BuildToolId, CommandSpec, ConfigProperty,
    CoverageReport, Counterpart, ExtensionHandle, FileDiagnostic, HttpRoute, ProjectTask, ProjectTaskHandle, TaskTestResult, ProjectRelease, RunKind, RunTarget, ScaffoldContribution, ScaffoldSpec,
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
/// parses it when it builds the `initialize` request.
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
    /// How to install the server from bytes the extension carries. `None`
    /// for a server the user provides themselves.
    pub install: Option<ServerInstall>,
    /// Documents the server serves under a URI scheme of its own (a
    /// decompiled library class, say) rather than from a file on disk.
    pub virtual_documents: Option<VirtualDocuments>,
    /// The runtime the server itself runs on (a JVM, say), when it has one
    /// the user may need to point it at. `None` for a self-contained binary.
    pub host_runtime: Option<HostRuntime>,
}

/// A server's non-file documents: definitions can point at `uri_scheme`
/// URIs, whose text is fetched by sending `request_method` with `{"uri": ..}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualDocuments {
    /// Without the colon: `"jdt"`.
    pub uri_scheme: String,
    pub request_method: String,
    /// The extension the text is saved under to open it, without the dot:
    /// `"java"`.
    pub file_extension: String,
}

/// The runtime a language server runs on, and the oldest version it accepts.
/// The settings dialog offers a home-directory field for it, filled in from
/// the matching [`RuntimeContribution`]'s discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntime {
    /// The id of the [`RuntimeContribution`] whose installs can host it.
    pub runtime_id: String,
    /// The oldest `RuntimeInstall::major_version` the server runs on.
    pub minimum_major_version: u32,
    /// The field's label: `"Java Home"`.
    pub home_label: String,
    /// Explains the requirement, shown as the field's tooltip.
    pub note: String,
    /// Shown under the field when a scan found no install that qualifies.
    pub none_found: String,
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
    /// The home of the runtime the server runs on, from settings (empty =
    /// auto-detect). Only meaningful for servers that declare a
    /// `LanguageServerContribution::host_runtime`.
    pub runtime_home: String,
    /// The Java release the current project declares, if any.
    pub java_release: Option<u32>,
    /// Every JDK found on this machine, for servers that need to tell the
    /// language server which runtimes are available.
    pub jdk_runtimes: Vec<JdkRuntime>,
    /// The open project's root, for a server that is found in, or configured
    /// from, the project itself (a Python virtual environment, say).
    pub project_root: Option<PathBuf>,
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
    /// External static-analysis tools (`Extension::run_analyzer`).
    pub analyzers: Vec<AnalyzerContribution>,
    /// Kinds of runtime the user keeps an inventory of (Settings lists one
    /// entry per kind).
    pub runtimes: Vec<RuntimeContribution>,
    pub scaffolds: Vec<ScaffoldContribution>,
    /// Languages this extension finds HTTP routes in (`Extension::
    /// http_routes`). Declared up front so a project-wide route scan parses
    /// only the files someone can answer for, rather than every file it
    /// has a grammar for. Several extensions may name the same language —
    /// two web frameworks on one language is ordinary — and each is asked.
    pub http_route_languages: Vec<LanguageId>,
    /// How the editor debugs a language — what decides which files get a
    /// breakpoint gutter and how a session is started.
    pub debug_adapters: Vec<DebugAdapterContribution>,
    /// Directory names that mark a source root (`java`, `kotlin`): everything
    /// single-child beneath one is package structure, which the project tree
    /// shows joined with `.` instead of `/`.
    pub source_root_dir_names: Vec<String>,
    /// The glyph the project tree and tabs draw beside a language's files.
    pub file_icons: Vec<FileIcon>,
    /// Where one language's files hold text in another (a `<script>` in a
    /// Svelte or HTML file).
    pub injections: Vec<InjectionContribution>,
    /// Language-server command names whose first argument is a
    /// `WorkspaceEdit` for the client to apply (what jdtls sends for
    /// "Organize imports" in place of a `CodeAction` with an `edit`).
    pub workspace_edit_commands: Vec<String>,
}

/// Where a language's files embed other languages, as a tree-sitter query over
/// the language's own grammar.
///
/// A pattern captures the embedded text as `@injection.content` and names the
/// language it is written in with `(#set! injection.language "css")`, where
/// the name is a registered language id. A pattern may test the surrounding
/// nodes with the usual predicates (`#eq?`, `#any-of?`). Of several patterns
/// that capture the same text, the last wins, so a general pattern goes first
/// and the specific ones after it. A name nobody registered injects nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectionContribution {
    /// The language whose files hold the embedded text — the one the query is
    /// written against.
    pub language_id: LanguageId,
    pub query: String,
}

/// The icon drawn beside the files of one language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIcon {
    pub language_id: LanguageId,
    /// A Nerd Fonts codepoint: the app embeds that symbols font, so the glyph
    /// looks the same on every machine.
    pub glyph: char,
}

/// A debugger for one language, reached through that language's server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugAdapterContribution {
    pub language_id: LanguageId,
    /// The `adapterID` sent in the DAP `initialize` request.
    pub adapter_id: String,
    /// The `workspace/executeCommand` the language server answers with the
    /// TCP port its debug adapter listens on, once started.
    pub start_command: String,
}

/// Everything an extension needs to write the DAP `launch` arguments for one
/// run configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugLaunch {
    /// The run configuration's own name.
    pub name: String,
    pub project_name: String,
    pub run: RunSpec,
    /// Where the debuggee runs: the run configuration's directory, else the
    /// project root.
    pub working_dir: PathBuf,
    pub classpath: Vec<PathBuf>,
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

    /// What this extension needs to be allowed to do. The registry refuses
    /// contributions that need more than is declared here, so the default —
    /// nothing — is right for a purely declarative extension.
    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }

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

    /// The bundled archive of server `server_id`, described by its
    /// `LanguageServerContribution::install`. `None` — the default — for a
    /// server this extension does not carry.
    fn server_archive(&self, _server_id: &str) -> Option<&'static [u8]> {
        None
    }

    /// Whether this machine can run server `server_id` (a missing or too-old
    /// runtime, say), checked before its archive is extracted. `Err` is a
    /// message for the user. Blocking: called off the UI thread.
    fn check_server_prerequisites(&self, _server_id: &str) -> Result<(), String> {
        Ok(())
    }

    /// The DAP `launch` arguments, as unparsed JSON, for debug adapter
    /// `adapter_id`. `None` — the default — for an adapter this extension
    /// does not own.
    fn debug_launch_arguments(&self, _adapter_id: &str, _launch: &DebugLaunch) -> Option<String> {
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

    /// The process that runs only the tests `filter` names for build tool
    /// `tool_id`: a class (`com.example.CalcTest`), or one test in it
    /// (`com.example.CalcTest#addIsBroken`). `None` when this extension does
    /// not own that tool or cannot narrow its test run.
    fn test_command(&self, _tool_id: &str, _project_root: &Path, _filter: &str) -> Option<CommandSpec> {
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

    /// Starter content for a file just created at `file_path` in
    /// `project_root`, which this extension's `language_id` claims — a class
    /// declaration and `package` line, say. `None` when it has nothing
    /// useful to put there, and the file starts empty.
    fn new_file_template(&self, _language_id: &str, _project_root: &Path, _file_path: &Path) -> Option<String> {
        None
    }

    /// Which runtime release `project_root` declares, read from whatever
    /// build files this extension understands. `None` means "nothing I
    /// recognize says", which is a normal answer and leaves the decision to
    /// whoever asked.
    fn project_release(&self, _project_root: &Path) -> Option<ProjectRelease> {
        None
    }

    /// The named group of sources `file` belongs to in a project that splits
    /// its code by platform or role (`commonMain`, `jvmTest`), as shown beside
    /// the file's language. `None` — the default — for a file this extension
    /// has no such notion for, including every file of an ordinary project.
    /// Reads the project's build files, so ask once per file opened, not per
    /// frame.
    fn source_set(&self, _project_root: &Path, _file: &Path) -> Option<String> {
        None
    }

    /// The extra jobs this extension offers for the project at `project_root`
    /// (a Gradle task per target). Empty — the default — when it recognizes
    /// nothing. Reads build files, so ask when a list is about to be shown,
    /// not per frame.
    fn project_tasks(&self, _project_root: &Path) -> Vec<ProjectTask> {
        Vec::new()
    }

    /// The task that runs the tests `file`'s source set covers (a platform's
    /// tests for a file of that platform, every target's for common code), if
    /// this extension has one. Reads build files, so ask on a command.
    fn file_task(&self, _project_root: &Path, _file: &Path) -> Option<ProjectTask> {
        None
    }

    /// The tests reported in `report_dirs`, the directories a [`ProjectTask`]
    /// of this extension named. Empty — the default — when there are none.
    fn task_test_results(&self, _report_dirs: &[PathBuf]) -> Vec<TaskTestResult> {
        Vec::new()
    }

    /// Problems in `file`, whose text is `source`, that need the project
    /// around it to see (an `expect` with no `actual`). Empty — the default —
    /// when there are none. Asked when a file opens, reloads or is saved, off
    /// the UI thread, never per keystroke: it may read many files.
    fn file_diagnostics(&self, _project_root: &Path, _file: &Path, _source: &str) -> Vec<FileDiagnostic> {
        Vec::new()
    }

    /// The declarations that stand for the one at `byte` in `file`, whose text
    /// is `source`: the `actual`s of an `expect`, or the `expect` of an
    /// `actual`. Empty — the default — when `byte` is not on such a
    /// declaration. Scans the project's sources, so it answers an explicit
    /// request (a command), never a frame.
    fn counterparts(&self, _project_root: &Path, _file: &Path, _source: &str, _byte: usize) -> Vec<Counterpart> {
        Vec::new()
    }

    /// Declarations the identifier at `byte` in `file` (whose text is `source`)
    /// may refer to, for a go-to-definition a language server could not
    /// answer: a server that misreads the project's layout resolves nothing
    /// for a call to a Kotlin Multiplatform `expect`, say. Empty — the default
    /// — when there are none. Reads the project's sources, so it answers an
    /// explicit request, never a frame.
    fn definitions(&self, _project_root: &Path, _file: &Path, _source: &str, _byte: usize) -> Vec<Counterpart> {
        Vec::new()
    }

    /// Whether a language server's diagnostic on `file` is wrong because the
    /// server does not understand the project's layout, and should be dropped.
    /// `code` and `message` are the server's own. False — the default — for
    /// everything, so a server's diagnostics are shown unless an extension
    /// knows better. Asked once per published diagnostic, so it must return
    /// without touching the disk for a `code` it has no opinion on.
    fn is_spurious_server_diagnostic(&self, _project_root: &Path, _file: &Path, _code: &str, _message: &str) -> bool {
        false
    }

    /// The 0-based lines of `source` that declare something with
    /// [`counterparts`](Self::counterparts) — where the gutter puts its marker.
    /// Empty — the default — when there are none. Reads the text alone, no
    /// project, because it is asked again after every edit; whether a
    /// declaration really has a counterpart is `counterparts`' question, asked
    /// when the marker is clicked.
    fn counterpart_lines(&self, _source: &str) -> Vec<usize> {
        Vec::new()
    }

    /// Every runnable entry point and test in `source`, whose parse `tree` the
    /// editor already built — in source order.
    ///
    /// Takes the tree rather than only the text on purpose: this is asked
    /// again on every edit to a file, and re-parsing it here would put a
    /// second full parse on the keystroke path. `file` is the file's path,
    /// which some toolchains need: a JVM top-level function compiles into a
    /// class named after the *file*, and a script is run by its path.
    fn run_targets(
        &self,
        _language_id: &str,
        _tree: &tree_sitter::Tree,
        _source: &str,
        _file: &Path,
    ) -> Vec<RunTarget> {
        Vec::new()
    }

    /// Every HTTP route declared in `source`, whose parse `tree` the editor
    /// built, in source order. Asked only for the languages this extension
    /// listed in `Contributions::http_route_languages`.
    fn http_routes(&self, _language_id: &str, _tree: &tree_sitter::Tree, _source: &str) -> Vec<HttpRoute> {
        Vec::new()
    }

    /// The type whose body `byte` sits in, innermost first. `None` outside
    /// any type, and for every language this extension does not own.
    fn enclosing_type(
        &self,
        _language_id: &str,
        _tree: &tree_sitter::Tree,
        _source: &str,
        _byte: usize,
    ) -> Option<TypeDeclaration> {
        None
    }

    /// The simple name of the one type `type_name` (declared in `source`)
    /// extends or implements first — the one place "Override Method" and
    /// `super.` completion look for inherited members.
    fn supertype(&self, _language_id: &str, _tree: &tree_sitter::Tree, _source: &str, _type_name: &str) -> Option<String> {
        None
    }

    /// `type_name`'s own members as `view` sees them, in source order —
    /// only what the declaration in `source` itself declares, never
    /// inherited ones (the core walks to the supertype itself, through
    /// `supertype`).
    fn type_members(
        &self,
        _language_id: &str,
        _tree: &tree_sitter::Tree,
        _source: &str,
        _type_name: &str,
        _view: MemberView,
    ) -> Vec<TypeMember> {
        Vec::new()
    }

    /// What `receiver` — the word written right before a `.` at `byte` —
    /// resolves to: the type to list members of, and from which side.
    /// `None` when it cannot be resolved syntactically, which closes
    /// completion quietly rather than guessing.
    fn receiver_type(
        &self,
        _language_id: &str,
        _tree: &tree_sitter::Tree,
        _source: &str,
        _byte: usize,
        _receiver: &str,
    ) -> Option<ReceiverType> {
        None
    }

    /// Whether this extension generates code (`generate_code`) for
    /// `language_id` — asked before anything is parsed, so the editor can
    /// say "not for this kind of file" rather than "nothing found".
    fn generates_code(&self, _language_id: &str) -> bool {
        false
    }

    /// Every type in `source` with at least one field code generation can
    /// use, in source order, nested types included.
    fn types_with_fields(&self, _language_id: &str, _tree: &tree_sitter::Tree, _source: &str) -> Vec<TypeFields> {
        Vec::new()
    }

    /// The source text `request` asks for, each line indented with
    /// `indent_unit` — one block, ready to insert before a type body's
    /// closing delimiter. `Some("")` when the request produces nothing (a
    /// setter-only request over read-only fields), `None` when this
    /// extension does not generate code for `language_id`.
    fn generate_code(&self, _language_id: &str, _request: CodeGeneration<'_>, _indent_unit: &str) -> Option<String> {
        None
    }

    /// Runs analyzer `analyzer_id` — one of this extension's own
    /// `Contributions::analyzers` — and reads back what it found. Called off
    /// the UI thread: this spawns the tool and waits for it. `Err` is a
    /// message for the user, for a run that failed rather than one that found
    /// nothing.
    fn run_analyzer(&self, _analyzer_id: &str, _run: &AnalyzerRun) -> Result<Vec<AnalyzerFinding>, String> {
        Err("this extension does not run that analyzer".to_string())
    }

    /// Verifies that `home` holds an install of runtime `runtime_id` — one of
    /// this extension's own `Contributions::runtimes` — by running it. Called
    /// off the UI thread. `Err` is a message for the user.
    fn detect_runtime(&self, _runtime_id: &str, _home: &Path) -> Result<RuntimeInstall, String> {
        Err("this extension does not recognize that runtime".to_string())
    }

    /// Every install of runtime `runtime_id` on this machine. Blocking (it
    /// spawns a process per candidate), so called off the UI thread.
    fn discover_runtimes(&self, _runtime_id: &str) -> Vec<RuntimeInstall> {
        Vec::new()
    }

    /// A documentation-comment skeleton for the declaration `byte` sits in:
    /// the innermost type or method, with a line per parameter, return value
    /// and declared exception to fill in. `None` when `byte` is in no
    /// declaration, when that declaration is already documented, and for
    /// languages this extension does not own.
    fn doc_stub(
        &self,
        _language_id: &str,
        _tree: &tree_sitter::Tree,
        _source: &str,
        _byte: usize,
    ) -> Option<DocStub> {
        None
    }

    /// The live templates this extension offers in `language_id` files.
    /// Empty for every language it does not own.
    fn snippets(&self, _language_id: &str) -> Vec<Snippet> {
        Vec::new()
    }

    /// The words reserved in `language_id`, offered by word completion.
    fn keywords(&self, _language_id: &str) -> Vec<String> {
        Vec::new()
    }

    /// The annotations offered after an `@` in a `language_id` file, each
    /// with the name it needs imported. Empty for every language this
    /// extension does not own, which is also how the editor knows not to open
    /// a completion popup at all.
    fn annotation_candidates(&self, _language_id: &str) -> Vec<ImportCandidate> {
        Vec::new()
    }

    /// The edit that makes `qualified_name` resolve in `source`, whose parse
    /// `tree` the editor built: an `import` at its alphabetical place among
    /// the file's own, or after the `package` line when it has none. `None`
    /// when it is already imported, and for languages this extension does not
    /// own.
    fn import_edit(
        &self,
        _language_id: &str,
        _tree: &tree_sitter::Tree,
        _source: &str,
        _qualified_name: &str,
    ) -> Option<ImportEdit> {
        None
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
