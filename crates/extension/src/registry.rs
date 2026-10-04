use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::{
    BuildToolContribution, BuildToolHandle, Contributions, Extension, ExtensionHandle, ExtensionManifest,
    GrammarContribution, JdkRuntime, LanguageId, LanguageIndex, LanguageServerContribution, ProjectRelease,
    RegisteredLanguage, ResolvedServerStart, ScaffoldContribution, ScaffoldSpec, ServerStartContext,
    CURRENT_SCHEMA_VERSION,
};

/// Why a registration was refused.
///
/// Every variant names the offending extension, because the whole point of
/// a registry is that the thing at fault is no longer necessarily this
/// codebase — "a grammar failed to load" is unactionable, "extension
/// `spring` declared a grammar for unregistered language `scala`" is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// Built against an API this binary does not speak.
    IncompatibleSchema {
        extension_id: String,
        found: u32,
        expected: u32,
    },
    /// Two extensions claim the same extension id.
    DuplicateExtension { extension_id: String },
    /// Two extensions claim the same language id.
    DuplicateLanguage {
        language_id: LanguageId,
        extension_id: String,
        already_owned_by: String,
    },
    /// A grammar or language server names a language nobody registered.
    /// Not necessarily the contributor's fault — the language may live in
    /// an extension that is absent or disabled — so the message has to
    /// name both sides.
    UnknownLanguage {
        language_id: LanguageId,
        extension_id: String,
    },
    /// Two grammars for one language. Unlike language servers, of which a
    /// language may legitimately have several, a language parses with
    /// exactly one grammar.
    DuplicateGrammar {
        language_id: LanguageId,
        extension_id: String,
    },
    /// Two language servers claim the same server id.
    DuplicateLanguageServer {
        server_id: String,
        extension_id: String,
    },
    /// Two extensions claim the same build tool id.
    DuplicateBuildTool {
        tool_id: String,
        extension_id: String,
    },
    /// A scaffold names a build tool nobody registered. Like
    /// `UnknownLanguage`, not necessarily the contributor's fault — the tool
    /// may live in an extension that is absent — so both sides are named.
    UnknownBuildTool {
        tool_id: String,
        extension_id: String,
    },
    /// Two scaffolds for one build tool/language pair. A wizard can only
    /// offer the pair once, so a second contributor would never be asked.
    DuplicateScaffold {
        tool_id: String,
        language_id: LanguageId,
        extension_id: String,
    },
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IncompatibleSchema {
                extension_id,
                found,
                expected,
            } => write!(
                f,
                "extension {extension_id} declares schema version {found}, but this build speaks {expected}"
            ),
            Self::DuplicateExtension { extension_id } => {
                write!(f, "extension {extension_id} is already registered")
            }
            Self::DuplicateLanguage {
                language_id,
                extension_id,
                already_owned_by,
            } => write!(
                f,
                "extension {extension_id} declares language {language_id}, already contributed by {already_owned_by}"
            ),
            Self::UnknownLanguage {
                language_id,
                extension_id,
            } => write!(
                f,
                "extension {extension_id} refers to language {language_id}, which no registered extension contributes"
            ),
            Self::DuplicateGrammar {
                language_id,
                extension_id,
            } => write!(
                f,
                "extension {extension_id} declares a second grammar for language {language_id}"
            ),
            Self::DuplicateLanguageServer {
                server_id,
                extension_id,
            } => write!(
                f,
                "extension {extension_id} declares language server {server_id}, which is already registered"
            ),
            Self::DuplicateBuildTool { tool_id, extension_id } => write!(
                f,
                "extension {extension_id} declares build tool {tool_id}, which is already registered"
            ),
            Self::UnknownBuildTool { tool_id, extension_id } => write!(
                f,
                "extension {extension_id} scaffolds for build tool {tool_id}, which no registered extension contributes"
            ),
            Self::DuplicateScaffold {
                tool_id,
                language_id,
                extension_id,
            } => write!(
                f,
                "extension {extension_id} declares a scaffold for {tool_id}/{language_id}, which is already registered"
            ),
        }
    }
}

impl std::error::Error for RegisterError {}

/// Everything every registered extension contributes, and the lookups the
/// core does against it.
///
/// Registration is all-or-nothing per extension: `register` validates the
/// whole contribution set and returns `Err` without having changed
/// anything, so a rejected extension never leaves half its languages
/// visible. That is deliberately stricter than it needs to be for Phase A
/// — where the only extension is one we ship and control — because the
/// failure it prevents (an extension that is partly live) is exactly the
/// kind that becomes untraceable once extensions come from elsewhere.
#[derive(Default)]
pub struct Registry {
    /// Stored in registration order so index lookups for `server_extension`
    /// remain valid after further registrations.
    /// `Arc`, not `Box`: a [`BuildToolHandle`] handed to a build that
    /// streams output for minutes has to keep the owning extension alive
    /// without borrowing the registry for that whole time.
    extensions: Vec<Arc<dyn Extension>>,
    /// Manifests in the same order as `extensions`, kept separately so
    /// `extensions()` can return a slice without borrowing from temporaries.
    manifests: Vec<ExtensionManifest>,
    /// Maps each registered server id to the index of the extension that
    /// owns it in `extensions`, for fast dynamic dispatch.
    server_extension: HashMap<String, usize>,
    languages: HashMap<LanguageId, RegisteredLanguage>,
    language_servers: Vec<LanguageServerContribution>,
    /// Registration order, which is also detection order: the first
    /// registered tool whose marker file exists wins. A `Vec` rather than a
    /// map because order is the semantics here, not an implementation
    /// detail. The `usize` indexes `extensions`.
    build_tools: Vec<(BuildToolContribution, &'static str, usize)>,
    /// Scaffold targets with the index of the extension that generates them.
    scaffolds: Vec<(ScaffoldContribution, usize)>,
    index: LanguageIndex,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("extensions", &self.manifests.iter().map(|m| &m.id).collect::<Vec<_>>())
            .field("languages", &self.languages.keys().collect::<Vec<_>>())
            .field("language_servers", &self.language_servers.iter().map(|s| &s.id).collect::<Vec<_>>())
            .field("build_tools", &self.build_tools.iter().map(|(t, ..)| &t.id).collect::<Vec<_>>())
            .finish()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates and registers everything `extension` contributes.
    ///
    /// On `Err` the registry is untouched — the extension is dropped.
    pub fn register(&mut self, extension: Box<dyn Extension>) -> Result<(), RegisterError> {
        let manifest = extension.manifest();
        let contributions = extension.contributions();
        self.validate(&manifest, &contributions)?;
        let idx = self.extensions.len();
        for server in &contributions.language_servers {
            self.server_extension.insert(server.id.clone(), idx);
        }
        self.commit(manifest, contributions, idx);
        self.extensions.push(Arc::from(extension));
        Ok(())
    }

    /// Every check, run before anything is written, so that `commit` below
    /// cannot fail partway and leave the registry inconsistent.
    fn validate(
        &self,
        manifest: &ExtensionManifest,
        contributions: &Contributions,
    ) -> Result<(), RegisterError> {
        if manifest.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(RegisterError::IncompatibleSchema {
                extension_id: manifest.id.clone(),
                found: manifest.schema_version,
                expected: CURRENT_SCHEMA_VERSION,
            });
        }
        if self.manifests.iter().any(|e| e.id == manifest.id) {
            return Err(RegisterError::DuplicateExtension {
                extension_id: manifest.id.clone(),
            });
        }

        // Languages this call is adding, checked against both what is
        // already registered and the rest of this same contribution set —
        // an extension declaring one language twice is as broken as two
        // extensions colliding, and only the second is caught by the map.
        let mut incoming: Vec<&LanguageId> = Vec::new();
        for language in &contributions.languages {
            if let Some(existing) = self.languages.get(&language.id) {
                return Err(RegisterError::DuplicateLanguage {
                    language_id: language.id.clone(),
                    extension_id: manifest.id.clone(),
                    already_owned_by: existing.extension_id.clone(),
                });
            }
            if incoming.contains(&&language.id) {
                return Err(RegisterError::DuplicateLanguage {
                    language_id: language.id.clone(),
                    extension_id: manifest.id.clone(),
                    already_owned_by: manifest.id.clone(),
                });
            }
            incoming.push(&language.id);
        }

        let known = |id: &LanguageId| self.languages.contains_key(id) || incoming.contains(&id);

        let mut grammars_seen: Vec<&LanguageId> = Vec::new();
        for grammar in &contributions.grammars {
            if !known(&grammar.language_id) {
                return Err(RegisterError::UnknownLanguage {
                    language_id: grammar.language_id.clone(),
                    extension_id: manifest.id.clone(),
                });
            }
            let already_registered = self
                .languages
                .get(&grammar.language_id)
                .is_some_and(|l| l.grammar.is_some());
            if already_registered || grammars_seen.contains(&&grammar.language_id) {
                return Err(RegisterError::DuplicateGrammar {
                    language_id: grammar.language_id.clone(),
                    extension_id: manifest.id.clone(),
                });
            }
            grammars_seen.push(&grammar.language_id);
        }

        let mut servers_seen: Vec<&str> = Vec::new();
        for server in &contributions.language_servers {
            for language_id in &server.language_ids {
                if !known(language_id) {
                    return Err(RegisterError::UnknownLanguage {
                        language_id: language_id.clone(),
                        extension_id: manifest.id.clone(),
                    });
                }
            }
            if self.language_servers.iter().any(|s| s.id == server.id)
                || servers_seen.contains(&server.id.as_str())
            {
                return Err(RegisterError::DuplicateLanguageServer {
                    server_id: server.id.clone(),
                    extension_id: manifest.id.clone(),
                });
            }
            servers_seen.push(&server.id);
        }

        let mut tools_seen: Vec<&str> = Vec::new();
        for tool in &contributions.build_tools {
            if self.build_tools.iter().any(|(t, ..)| t.id == tool.id) || tools_seen.contains(&tool.id.as_str()) {
                return Err(RegisterError::DuplicateBuildTool {
                    tool_id: tool.id.clone(),
                    extension_id: manifest.id.clone(),
                });
            }
            tools_seen.push(&tool.id);
        }

        let mut scaffolds_seen: Vec<(&str, &LanguageId)> = Vec::new();
        for scaffold in &contributions.scaffolds {
            let pair = (scaffold.build_tool_id.as_str(), &scaffold.language_id);
            let already_registered = self
                .scaffolds
                .iter()
                .any(|(s, _)| s.build_tool_id == pair.0 && s.language_id == *pair.1);
            if already_registered || scaffolds_seen.contains(&pair) {
                return Err(RegisterError::DuplicateScaffold {
                    tool_id: scaffold.build_tool_id.clone(),
                    language_id: scaffold.language_id.clone(),
                    extension_id: manifest.id.clone(),
                });
            }
            scaffolds_seen.push(pair);
            if !known(&scaffold.language_id) {
                return Err(RegisterError::UnknownLanguage {
                    language_id: scaffold.language_id.clone(),
                    extension_id: manifest.id.clone(),
                });
            }
            let tool_known = self.build_tools.iter().any(|(t, ..)| t.id == scaffold.build_tool_id)
                || contributions.build_tools.iter().any(|t| t.id == scaffold.build_tool_id);
            if !tool_known {
                return Err(RegisterError::UnknownBuildTool {
                    tool_id: scaffold.build_tool_id.clone(),
                    extension_id: manifest.id.clone(),
                });
            }
        }

        Ok(())
    }

    /// Infallible by construction — `validate` has already rejected every
    /// case this could otherwise have to handle.
    fn commit(&mut self, manifest: ExtensionManifest, contributions: Contributions, extension_index: usize) {
        let extension_id = manifest.id.clone();
        self.manifests.push(manifest);
        for language in contributions.languages {
            self.index.insert(&language);
            let static_id: &'static str = Box::leak(language.id.clone().into_boxed_str());
            self.languages.insert(
                language.id.clone(),
                RegisteredLanguage {
                    language,
                    static_id,
                    extension_id: extension_id.clone(),
                    grammar: None,
                },
            );
        }
        for grammar in contributions.grammars {
            if let Some(registered) = self.languages.get_mut(&grammar.language_id) {
                registered.grammar = Some(grammar);
            }
        }
        self.language_servers.extend(contributions.language_servers);
        for tool in contributions.build_tools {
            let static_id: &'static str = Box::leak(tool.id.clone().into_boxed_str());
            self.build_tools.push((tool, static_id, extension_index));
        }
        for scaffold in contributions.scaffolds {
            self.scaffolds.push((scaffold, extension_index));
        }
    }

    /// The manifests of all registered extensions, in registration order.
    pub fn extensions(&self) -> &[ExtensionManifest] {
        &self.manifests
    }

    /// Every JDK found by any extension that scans for them.
    pub fn jdk_runtimes(&self) -> Vec<JdkRuntime> {
        self.extensions.iter().flat_map(|e| e.jdk_runtimes()).collect()
    }

    /// How to start `server_id`, as the owning extension sees fit. Returns
    /// `None` when no registered extension owns that server id.
    pub fn resolve_server_start(
        &self,
        server_id: &str,
        context: &ServerStartContext,
    ) -> Option<Result<ResolvedServerStart, String>> {
        let idx = self.server_extension.get(server_id)?;
        self.extensions[*idx].resolve_server_start(server_id, context)
    }

    pub fn language(&self, id: &str) -> Option<&RegisteredLanguage> {
        self.languages.get(id)
    }

    pub fn languages(&self) -> impl Iterator<Item = &RegisteredLanguage> {
        self.languages.values()
    }

    /// The language for a file extension (no leading dot). Replaces
    /// `fg_core::Language::from_extension` in Phase 2.
    pub fn language_for_extension(&self, ext: &str) -> Option<&RegisteredLanguage> {
        self.index.by_extension(ext).and_then(|id| self.languages.get(id))
    }

    /// The language for a bare file name, for files whose extension says
    /// nothing. Replaces `fg_core::Language::from_filename` in Phase 2.
    pub fn language_for_filename(&self, filename: &str) -> Option<&RegisteredLanguage> {
        self.index.by_filename(filename).and_then(|id| self.languages.get(id))
    }

    /// The language for a whole path, applying the precedence the core
    /// used to hardcode: an extension match wins, and the bare-file-name
    /// patterns are consulted only when there was no extension match at
    /// all.
    ///
    /// The ordering is load-bearing, not incidental. A plain `Dockerfile`
    /// has no extension to key off, so filename patterns have to exist;
    /// but `notes.dockerfile.bak` should be whatever `bak` says it is,
    /// rather than being second-guessed into a Dockerfile by a name
    /// pattern. Checking extensions first is what keeps both true.
    pub fn language_for_path(&self, path: &std::path::Path) -> Option<&RegisteredLanguage> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| self.language_for_extension(ext))
            .or_else(|| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| self.language_for_filename(name))
            })
    }

    pub fn grammar(&self, language_id: &str) -> Option<&GrammarContribution> {
        self.languages.get(language_id)?.grammar.as_ref()
    }

    /// Every language server registered for `language_id`. A list rather
    /// than one: nothing stops a language having both a general-purpose
    /// server and a specialized one, and the core decides what to start.
    pub fn language_servers_for(&self, language_id: &str) -> Vec<&LanguageServerContribution> {
        self.language_servers
            .iter()
            .filter(|s| s.language_ids.iter().any(|id| id == language_id))
            .collect()
    }

    pub fn language_servers(&self) -> &[LanguageServerContribution] {
        &self.language_servers
    }

    /// Every registered extension as a standalone handle — what a caller
    /// needs to ask a project-wide question from a background thread, where
    /// borrowing the registry is not an option.
    pub fn handles(&self) -> Vec<ExtensionHandle> {
        self.manifests
            .iter()
            .zip(&self.extensions)
            .map(|(manifest, extension)| ExtensionHandle::new(Arc::from(manifest.id.as_str()), extension.clone()))
            .collect()
    }

    /// Every kind of project that can be scaffolded, in registration order —
    /// what the new-project wizard lists.
    pub fn scaffolds(&self) -> Vec<&ScaffoldContribution> {
        self.scaffolds.iter().map(|(scaffold, _)| scaffold).collect()
    }

    /// The files a new project of `spec`'s kind needs, from whichever
    /// extension declared that target. `None` when nothing scaffolds it.
    pub fn scaffold_files(&self, spec: &ScaffoldSpec) -> Option<Vec<(std::path::PathBuf, String)>> {
        let (_, idx) = self
            .scaffolds
            .iter()
            .find(|(s, _)| s.build_tool_id == spec.build_tool_id && s.language_id == spec.language_id)?;
        self.extensions[*idx].scaffold_files(spec)
    }

    /// The runtime release the open project declares, as the first extension
    /// that recognizes the project reports it.
    pub fn project_release(&self, project_root: &Path) -> Option<ProjectRelease> {
        self.extensions.iter().find_map(|e| e.project_release(project_root))
    }

    /// Every registered build tool, in registration order — what a UI that
    /// asks the user to pick one (the new-project wizard) lists.
    pub fn build_tools(&self) -> Vec<BuildToolHandle> {
        self.build_tools
            .iter()
            .map(|(tool, static_id, idx)| {
                BuildToolHandle::new(static_id, tool.display_name.clone(), self.extensions[*idx].clone())
            })
            .collect()
    }

    pub fn build_tool(&self, tool_id: &str) -> Option<BuildToolHandle> {
        self.build_tools.iter().find(|(tool, ..)| tool.id == tool_id).map(
            |(tool, static_id, idx)| {
                BuildToolHandle::new(static_id, tool.display_name.clone(), self.extensions[*idx].clone())
            },
        )
    }

    /// Which registered tool builds the project at `project_root`, by marker
    /// file presence. Replaces the core's own hardcoded `pom.xml`/
    /// `build.gradle` check; `None` (no tool claims this directory) stays a
    /// normal answer, exactly as it was before.
    pub fn detect_build_tool(&self, project_root: &Path) -> Option<BuildToolHandle> {
        self.build_tools
            .iter()
            .find(|(tool, ..)| tool.marker_files.iter().any(|name| project_root.join(name).is_file()))
            .map(|(tool, static_id, idx)| {
                BuildToolHandle::new(static_id, tool.display_name.clone(), self.extensions[*idx].clone())
            })
    }

    /// Like [`Self::detect_build_tool`], but only among the tools
    /// `extension_id` contributes — for launching something that extension
    /// described in its own notation (a run marker's entry point), which
    /// another extension's tool has no reason to understand.
    pub fn detect_build_tool_of(&self, project_root: &Path, extension_id: &str) -> Option<BuildToolHandle> {
        self.build_tools
            .iter()
            .filter(|(_, _, idx)| self.manifests[*idx].id == extension_id)
            .find(|(tool, ..)| tool.marker_files.iter().any(|name| project_root.join(name).is_file()))
            .map(|(tool, static_id, idx)| {
                BuildToolHandle::new(static_id, tool.display_name.clone(), self.extensions[*idx].clone())
            })
    }
}

#[cfg(test)]
#[path = "registry_test.rs"]
mod registry_test;
