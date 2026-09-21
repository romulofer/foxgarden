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
//! **Nothing in here is wired into the running app yet, by design.** Phase
//! 1 only adds the API; Phase 2 makes `fg_core::Language` a lookup against
//! this registry, and Phase 3 does the same for grammars. The JVM support
//! that exists today is untouched and still compiled in.
//!
//! The shape follows Zed's own extension host (`../references/zed`,
//! `crates/extension/src/extension_host_proxy.rs`): an extension declares
//! a manifest and pushes contributions through `register_*` calls. The
//! call an in-process module makes here in Phase A is the same call a
//! dynamically loaded extension will make in Phase B, which is the point —
//! the API gets exercised for real by `spring` long before anything is
//! loaded from outside the binary.

use std::collections::HashMap;
use std::path::PathBuf;

use tree_sitter_language::LanguageFn;

mod registry;

pub use registry::{RegisterError, Registry};

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
}

/// One extension.
///
/// In Phase A every implementor is a module compiled into the binary
/// (`spring` being the first and, for now, only one). Phase B adds
/// implementors backed by something loaded at runtime, which is why this
/// is a trait rather than a struct: the core consumes extensions through
/// it without knowing which kind it has.
pub trait Extension {
    fn manifest(&self) -> ExtensionManifest;
    fn contributions(&self) -> Contributions;
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
