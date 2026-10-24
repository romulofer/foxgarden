//! Extensions installed on disk (`PLAN.md` Track 24 Phase B, first slice):
//! the declarative tier.
//!
//! An installed extension is a directory holding an `extension.toml` and
//! whatever files it names (a grammar library, a highlight query). It
//! carries no code of its own, so loading it cannot run anything: what it
//! can contribute is exactly the data the manifest has a table for, and an
//! unknown table or key is refused rather than ignored, because a manifest
//! that asks for something this build cannot give would otherwise appear to
//! load and quietly do less than it says.
//!
//! A language server is declarable too: the manifest only names the program
//! and its arguments, the host starts it (when the user opts in), and the
//! extension runs no code of its own. Starting a program is why such a
//! manifest must declare `process_exec`; the registry refuses it otherwise.
//!
//! An analyzer needs the extension's own code to read the tool's report, so a
//! manifest that declares one also names a `[process]`: a program inside the
//! extension directory that the host starts once per question (see
//! [`crate::process`]). Build tools are not parseable here yet.

use std::path::{Path, PathBuf};

use fg_extension::{
    AnalyzerContribution, AnalyzerFinding, AnalyzerInput, AnalyzerRun, Capability, ProblemSeverity, Contributions, Extension, ExtensionManifest, FileIcon, FilenamePattern, GrammarContribution, GrammarSource,
    LanguageContribution, LanguageServerContribution, NodeKinds, Snippet,
};
use serde::Deserialize;

use crate::process::{ProcessCommand, inside};

/// The manifest file name inside an extension directory.
pub const MANIFEST_FILE: &str = "extension.toml";

/// Why one installed extension was not loaded. Carries the directory so the
/// message can say whose manifest is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub dir: PathBuf,
    pub message: String,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.dir.display(), self.message)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    name: String,
    version: String,
    schema_version: u32,
    /// What the extension asks to be allowed to do (`process_exec`,
    /// `download_file`). Nothing a declarative extension can contribute needs
    /// one yet; they are read now so a manifest written for the executable
    /// tier says so from its first version.
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    languages: Vec<LanguageTable>,
    #[serde(default)]
    grammars: Vec<GrammarTable>,
    #[serde(default)]
    language_servers: Vec<LanguageServerTable>,
    process: Option<ProcessTable>,
    #[serde(default)]
    analyzers: Vec<AnalyzerTable>,
    #[serde(default)]
    file_icons: Vec<FileIconTable>,
    #[serde(default)]
    snippets: Vec<SnippetTable>,
    #[serde(default)]
    keywords: Vec<KeywordTable>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageTable {
    id: String,
    display_name: String,
    #[serde(default)]
    file_extensions: Vec<String>,
    #[serde(default)]
    filename_patterns: Vec<FilenamePatternTable>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilenamePatternTable {
    stem: String,
    /// `"exact"`, or `"with_affix"` for `stem`, `stem.x` and `x.stem`.
    #[serde(rename = "match")]
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrammarTable {
    language_id: String,
    /// The compiled grammar, relative to the extension directory.
    library: PathBuf,
    /// The exported entry symbol, `tree_sitter_<name>`.
    symbol: String,
    /// The highlight query, relative to the extension directory.
    highlights: Option<PathBuf>,
    #[serde(default)]
    node_kinds: NodeKindsTable,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeKindsTable {
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    foldable: Vec<String>,
    import: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageServerTable {
    id: String,
    display_name: String,
    language_ids: Vec<String>,
    /// The program to start, looked up on `PATH` unless the user points the
    /// server at another binary in Settings.
    binary_name: String,
    #[serde(default)]
    args: Vec<String>,
    /// LSP `initializationOptions` as JSON text.
    initialization_options: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessTable {
    /// The program, relative to the extension directory.
    command: PathBuf,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzerTable {
    id: String,
    display_name: String,
    /// `"sources"`, or `"build_output"` for a tool that reads compiled output.
    input: String,
    config_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileIconTable {
    language_id: String,
    glyph: char,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnippetTable {
    language_id: String,
    trigger: String,
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeywordTable {
    language_id: String,
    words: Vec<String>,
}

/// An extension read from a directory. Everything is resolved at load time
/// (queries read, paths made absolute), so answering a question later does no
/// I/O.
pub struct InstalledExtension {
    manifest: ExtensionManifest,
    languages: Vec<LanguageContribution>,
    grammars: Vec<GrammarContribution>,
    language_servers: Vec<LanguageServerContribution>,
    analyzers: Vec<AnalyzerContribution>,
    process: Option<ProcessCommand>,
    file_icons: Vec<FileIcon>,
    capabilities: Vec<Capability>,
    snippets: Vec<(String, Snippet)>,
    keywords: Vec<(String, Vec<String>)>,
}

impl InstalledExtension {
    /// Reads `dir/extension.toml` and the files it names.
    pub fn load(dir: &Path) -> Result<Self, LoadError> {
        let fail = |message: String| LoadError {
            dir: dir.to_path_buf(),
            message,
        };
        let text = std::fs::read_to_string(dir.join(MANIFEST_FILE)).map_err(|e| fail(format!("{MANIFEST_FILE}: {e}")))?;
        let manifest: Manifest = toml::from_str(&text).map_err(|e| fail(format!("{MANIFEST_FILE}: {e}")))?;

        let capabilities = manifest
            .capabilities
            .iter()
            .map(|id| {
                Capability::from_id(id).ok_or_else(|| fail(format!("unknown capability \"{id}\"")))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let languages = manifest
            .languages
            .into_iter()
            .map(|l| {
                let filename_patterns = l
                    .filename_patterns
                    .into_iter()
                    .map(|p| match p.kind.as_str() {
                        "exact" => Ok(FilenamePattern::Exact { stem: p.stem }),
                        "with_affix" => Ok(FilenamePattern::StemWithAffix { stem: p.stem }),
                        other => Err(fail(format!(
                            "language {}: filename pattern match must be \"exact\" or \"with_affix\", not \"{other}\"",
                            l.id
                        ))),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(LanguageContribution {
                    id: l.id,
                    display_name: l.display_name,
                    file_extensions: l.file_extensions.into_iter().map(|e| e.to_lowercase()).collect(),
                    filename_patterns,
                })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;

        let grammars = manifest
            .grammars
            .into_iter()
            .map(|g| {
                let library = inside(dir, &g.library).map_err(|e| fail(format!("grammar {}: {e}", g.language_id)))?;
                let highlight_query = g
                    .highlights
                    .map(|path| {
                        let path = inside(dir, &path).map_err(|e| fail(format!("grammar {}: {e}", g.language_id)))?;
                        std::fs::read_to_string(&path)
                            .map_err(|e| fail(format!("grammar {}: {}: {e}", g.language_id, path.display())))
                    })
                    .transpose()?;
                Ok(GrammarContribution {
                    language_id: g.language_id,
                    source: GrammarSource::SharedLibrary {
                        path: library,
                        symbol: g.symbol,
                    },
                    highlight_query,
                    node_kinds: NodeKinds {
                        scopes: g.node_kinds.scopes,
                        foldable: g.node_kinds.foldable,
                        import: g.node_kinds.import,
                    },
                })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;

        let language_servers = manifest
            .language_servers
            .into_iter()
            .map(|server| LanguageServerContribution {
                id: server.id,
                display_name: server.display_name,
                language_ids: server.language_ids,
                binary_name: server.binary_name,
                args: server.args,
                initialization_options: server.initialization_options,
                install: None,
                virtual_documents: None,
                host_runtime: None,
            })
            .collect();
        let process = manifest
            .process
            .map(|p| {
                let program = inside(dir, &p.command).map_err(|e| fail(format!("process: {e}")))?;
                if !program.is_file() {
                    return Err(fail(format!("process: {} is not a file", program.display())));
                }
                Ok(ProcessCommand {
                    program,
                    args: p.args,
                    dir: dir.to_path_buf(),
                })
            })
            .transpose()?;
        let analyzers = manifest
            .analyzers
            .into_iter()
            .map(|a| {
                let input = match a.input.as_str() {
                    "sources" => AnalyzerInput::Sources,
                    "build_output" => AnalyzerInput::BuildOutput,
                    other => {
                        return Err(fail(format!(
                            "analyzer {}: input must be \"sources\" or \"build_output\", not \"{other}\"",
                            a.id
                        )));
                    }
                };
                Ok(AnalyzerContribution {
                    id: a.id,
                    display_name: a.display_name,
                    input,
                    config_label: a.config_label,
                    install: None,
                })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;
        if !analyzers.is_empty() && process.is_none() {
            return Err(fail("analyzers need a [process] to run them".to_string()));
        }
        let file_icons = manifest
            .file_icons
            .into_iter()
            .map(|i| FileIcon {
                language_id: i.language_id,
                glyph: i.glyph,
            })
            .collect();
        Ok(Self {
            manifest: ExtensionManifest {
                id: manifest.id,
                name: manifest.name,
                version: manifest.version,
                schema_version: manifest.schema_version,
            },
            languages,
            grammars,
            language_servers,
            analyzers,
            process,
            file_icons,
            capabilities,
            snippets: manifest
                .snippets
                .into_iter()
                .map(|s| (s.language_id, Snippet::new(&s.trigger, &s.body)))
                .collect(),
            keywords: manifest.keywords.into_iter().map(|k| (k.language_id, k.words)).collect(),
        })
    }
}

/// One finding as an extension's process spells it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FindingReply {
    file: PathBuf,
    /// 1-based.
    line: usize,
    /// 1-based; absent for a whole-line finding.
    column: Option<usize>,
    /// Inclusive 1-based `[line, column]`; absent for a point.
    end: Option<(usize, usize)>,
    /// `"error"` or `"warning"`.
    severity: String,
    message: String,
}

impl FindingReply {
    fn into_finding(self) -> AnalyzerFinding {
        AnalyzerFinding {
            file: self.file,
            line: self.line,
            column: self.column,
            end: self.end,
            // Anything but "error" is shown as a warning: a finding worth
            // reporting is not worth losing over an unfamiliar severity.
            severity: if self.severity == "error" {
                ProblemSeverity::Error
            } else {
                ProblemSeverity::Warning
            },
            message: self.message,
        }
    }
}

impl Extension for InstalledExtension {
    fn manifest(&self) -> ExtensionManifest {
        self.manifest.clone()
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.clone()
    }

    fn contributions(&self) -> Contributions {
        Contributions {
            languages: self.languages.clone(),
            grammars: self.grammars.clone(),
            language_servers: self.language_servers.clone(),
            analyzers: self.analyzers.clone(),
            file_icons: self.file_icons.clone(),
            ..Default::default()
        }
    }

    fn run_analyzer(&self, analyzer_id: &str, run: &AnalyzerRun) -> Result<Vec<AnalyzerFinding>, String> {
        let process = self
            .process
            .as_ref()
            .ok_or_else(|| format!("{analyzer_id}: this extension has no process"))?;
        let params = serde_json::json!({
            "analyzer": analyzer_id,
            "binary": run.binary,
            "config": run.config,
            "project_root": run.project_root,
            "classes_dir": run.classes_dir,
        });
        let findings: Vec<FindingReply> = process.call("run_analyzer", &params)?;
        Ok(findings.into_iter().map(FindingReply::into_finding).collect())
    }

    fn snippets(&self, language_id: &str) -> Vec<Snippet> {
        self.snippets
            .iter()
            .filter(|(id, _)| id == language_id)
            .map(|(_, snippet)| snippet.clone())
            .collect()
    }

    fn keywords(&self, language_id: &str) -> Vec<String> {
        self.keywords
            .iter()
            .filter(|(id, _)| id == language_id)
            .flat_map(|(_, words)| words.clone())
            .collect()
    }
}

/// Loads every extension directory directly under `root`, in name order so
/// registration (and so which of two clashing extensions wins) does not
/// depend on the filesystem. A missing `root` is no extensions, not an error.
/// A bad one is reported and skipped; it never stops the others.
pub fn load_all(root: &Path) -> (Vec<InstalledExtension>, Vec<LoadError>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return (Vec::new(), Vec::new());
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join(MANIFEST_FILE).is_file())
        .collect();
    dirs.sort();
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    for dir in dirs {
        match InstalledExtension::load(&dir) {
            Ok(extension) => loaded.push(extension),
            Err(error) => errors.push(error),
        }
    }
    (loaded, errors)
}

#[cfg(test)]
#[path = "installed_test.rs"]
mod installed_test;
