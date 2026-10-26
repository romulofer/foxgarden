//! Which extensions this build ships with, and the one place that knows
//! it (`PLAN.md` Track 24 Phases 2 and 3).
//!
//! Four extensions, and they no longer sit side by side in this file: the
//! JVM half is `foxgarden-spring`, a separate project under
//! `../spring-foxgarden` that reaches the editor only through
//! `fg-extension`, `foxgarden-kmp` (`../kmp-foxgarden`) adds Kotlin
//! Multiplatform on top, `foxgarden-python` (`../python-foxgarden`) brings
//! Python, and `file-types` below is general file-type support
//! with nothing to do with the JVM. That split is the whole point of the
//! track — a YAML file is not a JVM concept, and someone running this
//! editor on a project with no JVM in it should still get YAML.
//!
//! Each language brings its own grammar and highlight query now (Phase 3):
//! `crates/syntax` no longer links a single grammar crate, so the answer to
//! "which languages can this editor parse" lives here and in the extension
//! rather than in a `match` in the middle of the editor.

use fg_extension::{
    Contributions, Extension, ExtensionInfo, FileIcon, ExtensionManifest, FilenamePattern, GrammarContribution, GrammarSource,
    LanguageContribution, NodeKinds, RegisterError, Registry, CURRENT_SCHEMA_VERSION,
};

pub use foxgarden_kmp::KmpExtension;
pub use foxgarden_python::PythonExtension;
pub use foxgarden_spring::SpringExtension;

mod installed;
mod process;
pub use installed::{InstalledExtension, LoadError, MANIFEST_FILE};

fn language(id: &str, display_name: &str, extensions: &[&str]) -> LanguageContribution {
    LanguageContribution {
        id: id.to_string(),
        display_name: display_name.to_string(),
        file_extensions: extensions.iter().map(|e| (*e).to_string()).collect(),
        filename_patterns: Vec::new(),
    }
}

/// Glyphs are Nerd Fonts codepoints, drawn from the symbols font the app embeds.
fn file_icon(language_id: &str, glyph: char) -> FileIcon {
    FileIcon {
        language_id: language_id.to_string(),
        glyph,
    }
}

/// These formats parse and highlight but declare no node-kind vocabulary:
/// sticky scroll and folding are no-ops for them, exactly as they were when
/// the core hardcoded entries for Java alone.
fn grammar(language_id: &str, source: GrammarSource, highlight_query: &str) -> GrammarContribution {
    GrammarContribution {
        language_id: language_id.to_string(),
        source,
        highlight_query: Some(highlight_query.to_string()),
        node_kinds: NodeKinds::default(),
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
            file_icons: vec![
                file_icon("properties", '\u{f013}'),
                file_icon("yaml", '\u{f0219}'),
                file_icon("xml", '\u{e619}'),
                file_icon("dockerfile", '\u{f308}'),
            ],
            ..Default::default()
        }
    }
}

/// Every extension this build ships with, in registration order. The
/// multiplatform one scaffolds on the JVM extension's Gradle and Kotlin, so it
/// drops its template when that one is among `disabled`.
fn shipped(disabled: &[String]) -> Vec<Box<dyn Extension>> {
    let kmp = if disabled.iter().any(|id| id == "spring") { KmpExtension::without_scaffold() } else { KmpExtension::new() };
    vec![Box::new(SpringExtension), Box::new(kmp), Box::new(PythonExtension), Box::new(FileTypesExtension)]
}

/// What each shipped extension brings, whether or not it is enabled: the
/// extension manager lists these.
pub fn shipped_extensions() -> Vec<ExtensionInfo> {
    shipped(&[]).iter().map(|e| ExtensionInfo::of(e.as_ref())).collect()
}

/// Registers everything this build ships with, except the extensions whose
/// id is in `disabled`.
///
/// Returns the first refusal rather than panicking: this is the exact call
/// a host will one day make over extensions it did not write, and a
/// registration path that aborts the process on a bad contribution is not
/// one that can ever be pointed at somebody else's extension.
pub fn register_builtins(registry: &mut Registry, disabled: &[String]) -> Result<(), RegisterError> {
    for extension in shipped(disabled) {
        if !disabled.contains(&extension.manifest().id) {
            registry.register(extension)?;
        }
    }
    Ok(())
}

/// Registers the extensions installed under `root` (see [`installed`]), after
/// the built-in ones so a clash is the installed extension's to lose. One that
/// cannot be loaded or registered is skipped and reported, never fatal: it is
/// somebody else's file. Returns a message per skipped extension.
pub fn register_installed(registry: &mut Registry, root: &std::path::Path, disabled: &[String]) -> Vec<String> {
    let (loaded, load_errors) = installed::load_all(root);
    let mut problems: Vec<String> = load_errors.iter().map(ToString::to_string).collect();
    for extension in loaded {
        let id = extension.manifest().id;
        if disabled.contains(&id) {
            continue;
        }
        if let Err(error) = registry.register(Box::new(extension)) {
            problems.push(error.to_string());
        }
    }
    problems
}

/// What each extension installed under `root` brings, for the extension
/// manager. Unloadable ones are left out; `register_installed` reports them.
pub fn installed_extension_infos(root: &std::path::Path) -> Vec<ExtensionInfo> {
    installed::load_all(root).0.iter().map(|e| ExtensionInfo::of(e)).collect()
}

/// A registry with exactly this build's own languages in it.
///
/// Holds the *declarations* only. Making the editor able to parse with the
/// grammars they declare is a second step (`syntax::install_grammars`),
/// kept separate because this crate deliberately does not depend on the
/// editor's tree-sitter machinery — an extension describes a grammar, it
/// does not load one.
pub fn builtin_registry() -> Registry {
    builtin_registry_without(&[])
}

/// [`builtin_registry`] minus the extensions the user switched off.
pub fn builtin_registry_without(disabled: &[String]) -> Registry {
    let mut registry = Registry::new();
    register_builtins(&mut registry, disabled).expect("this build's own extensions must not conflict");
    registry
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_test;
