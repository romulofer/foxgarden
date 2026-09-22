//! Which extensions this build ships with, and the one place that knows
//! it (`PLAN.md` Track 24 Phases 2 and 3).
//!
//! Two extensions, and they no longer sit side by side in this file: the
//! JVM half is `foxgarden-spring`, a separate project under
//! `../spring-foxgarden` that reaches the editor only through
//! `fg-extension`, while `file-types` below is general file-type support
//! with nothing to do with the JVM. That split is the whole point of the
//! track — a YAML file is not a JVM concept, and someone running this
//! editor on a project with no JVM in it should still get YAML.
//!
//! Each language brings its own grammar and highlight query now (Phase 3):
//! `crates/syntax` no longer links a single grammar crate, so the answer to
//! "which languages can this editor parse" lives here and in the add-on
//! rather than in a `match` in the middle of the editor.

use fg_extension::{
    Contributions, Extension, ExtensionManifest, FilenamePattern, GrammarContribution, GrammarSource,
    LanguageContribution, RegisterError, Registry, CURRENT_SCHEMA_VERSION,
};

pub use foxgarden_spring::SpringExtension;

fn language(id: &str, display_name: &str, extensions: &[&str]) -> LanguageContribution {
    LanguageContribution {
        id: id.to_string(),
        display_name: display_name.to_string(),
        file_extensions: extensions.iter().map(|e| (*e).to_string()).collect(),
        filename_patterns: Vec::new(),
    }
}

fn grammar(language_id: &str, source: GrammarSource, highlight_query: &str) -> GrammarContribution {
    GrammarContribution {
        language_id: language_id.to_string(),
        source,
        highlight_query: Some(highlight_query.to_string()),
    }
}

/// Config and markup formats the editor opens but has no language tooling
/// for.
pub struct FileTypesExtension;

impl Extension for FileTypesExtension {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            id: "file-types".to_string(),
            name: "Common file types".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            schema_version: CURRENT_SCHEMA_VERSION,
        }
    }

    fn contributions(&self) -> Contributions {
        Contributions {
            languages: vec![
                language("properties", "Properties", &["properties"]),
                language("yaml", "YAML", &["yml", "yaml"]),
                language("xml", "XML", &["xml"]),
                LanguageContribution {
                    // A Dockerfile is the one shipped language that cannot
                    // be recognized by extension: the canonical spelling
                    // has none at all. Real projects also write it with a
                    // stage name on either side (`Dockerfile.dev`,
                    // `dev.Dockerfile`), so both affix orders are matched
                    // rather than picking one convention and being wrong
                    // about half the repositories in the world.
                    filename_patterns: vec![FilenamePattern::StemWithAffix {
                        stem: "Dockerfile".to_string(),
                    }],
                    ..language("dockerfile", "Dockerfile", &["dockerfile"])
                },
            ],
            grammars: vec![
                // Three of these four queries ship inside their own grammar
                // crate, so they travel with the grammar they were written
                // against automatically. Dockerfile's is this repository's
                // own (`queries/highlights_dockerfile.scm`) — see its header
                // for what it does differently.
                grammar(
                    "properties",
                    GrammarSource::Builtin(tree_sitter_properties::LANGUAGE),
                    tree_sitter_properties::HIGHLIGHTS_QUERY,
                ),
                grammar(
                    "yaml",
                    GrammarSource::Builtin(tree_sitter_yaml::LANGUAGE),
                    tree_sitter_yaml::HIGHLIGHTS_QUERY,
                ),
                grammar(
                    "xml",
                    GrammarSource::Builtin(tree_sitter_xml::LANGUAGE_XML),
                    tree_sitter_xml::XML_HIGHLIGHT_QUERY,
                ),
                grammar(
                    "dockerfile",
                    GrammarSource::Builtin(tree_sitter_containerfile::LANGUAGE),
                    include_str!("../queries/highlights_dockerfile.scm"),
                ),
            ],
            ..Default::default()
        }
    }
}

/// Registers everything this build ships with.
///
/// Returns the first refusal rather than panicking: this is the exact call
/// a host will one day make over extensions it did not write, and a
/// registration path that aborts the process on a bad contribution is not
/// one that can ever be pointed at somebody else's extension.
pub fn register_builtins(registry: &mut Registry) -> Result<(), RegisterError> {
    registry.register(&SpringExtension)?;
    registry.register(&FileTypesExtension)?;
    Ok(())
}

/// A registry with exactly this build's own languages in it.
///
/// Holds the *declarations* only. Making the editor able to parse with the
/// grammars they declare is a second step (`syntax::install_grammars`),
/// kept separate because this crate deliberately does not depend on the
/// editor's tree-sitter machinery — an extension describes a grammar, it
/// does not load one.
pub fn builtin_registry() -> Registry {
    let mut registry = Registry::new();
    register_builtins(&mut registry).expect("this build's own extensions must not conflict");
    registry
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_test;
