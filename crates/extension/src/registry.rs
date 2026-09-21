use std::collections::HashMap;

use crate::{
    Contributions, Extension, ExtensionManifest, GrammarContribution, LanguageId, LanguageIndex,
    LanguageServerContribution, RegisteredLanguage, CURRENT_SCHEMA_VERSION,
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
#[derive(Debug, Default)]
pub struct Registry {
    extensions: Vec<ExtensionManifest>,
    languages: HashMap<LanguageId, RegisteredLanguage>,
    language_servers: Vec<LanguageServerContribution>,
    index: LanguageIndex,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates and registers everything `extension` contributes.
    ///
    /// On `Err` the registry is untouched.
    pub fn register(&mut self, extension: &dyn Extension) -> Result<(), RegisterError> {
        let manifest = extension.manifest();
        let contributions = extension.contributions();
        self.validate(&manifest, &contributions)?;
        self.commit(manifest, contributions);
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
        if self.extensions.iter().any(|e| e.id == manifest.id) {
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

        Ok(())
    }

    /// Infallible by construction — `validate` has already rejected every
    /// case this could otherwise have to handle.
    fn commit(&mut self, manifest: ExtensionManifest, contributions: Contributions) {
        let extension_id = manifest.id.clone();
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
        self.extensions.push(manifest);
    }

    pub fn extensions(&self) -> &[ExtensionManifest] {
        &self.extensions
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
}

#[cfg(test)]
#[path = "registry_test.rs"]
mod registry_test;
