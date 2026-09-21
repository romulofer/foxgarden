//! `PLAN.md` Track 24 Phase 2: the languages this build ships with, as
//! real extensions registering through the real API.
//!
//! Split into two from the start, rather than one "builtins" blob, because
//! that is the split Track 24 is heading for: the JVM half becomes the
//! `spring` extension (Phase 6), and the rest is general file-type support
//! that has nothing to do with the JVM and should not be entangled with
//! it. Having two here also means the registry is exercised by more than a
//! single extension from the first moment it is used for real — a registry
//! that has only ever held one extension has not been shown to hold two.
//!
//! What these contribute today is deliberately thin: identity only (ids,
//! display names, how to recognize a file). Grammars and highlight queries
//! stay compiled into `crates/syntax` until Phase 3 moves them here, and
//! language servers stay in `lsp_state` until Phase 4.

use fg_extension::{
    Contributions, Extension, ExtensionManifest, FilenamePattern, LanguageContribution, Registry,
    RegisterError, CURRENT_SCHEMA_VERSION,
};

fn manifest(id: &str, name: &str) -> ExtensionManifest {
    ExtensionManifest {
        id: id.to_string(),
        name: name.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: CURRENT_SCHEMA_VERSION,
    }
}

fn language(id: &str, display_name: &str, extensions: &[&str]) -> LanguageContribution {
    LanguageContribution {
        id: id.to_string(),
        display_name: display_name.to_string(),
        file_extensions: extensions.iter().map(|e| (*e).to_string()).collect(),
        filename_patterns: Vec::new(),
    }
}

/// The JVM languages. Becomes the `spring` extension proper in Phase 6,
/// once the build tooling, JDK registry, run/debug and Spring panels join
/// them here.
pub struct SpringExtension;

impl Extension for SpringExtension {
    fn manifest(&self) -> ExtensionManifest {
        manifest("spring", "Java, Kotlin and Spring")
    }

    fn contributions(&self) -> Contributions {
        Contributions {
            languages: vec![
                language("java", "Java", &["java"]),
                language("kotlin", "Kotlin", &["kt"]),
            ],
            ..Default::default()
        }
    }
}

/// Config and markup formats the editor opens but has no language tooling
/// for. Deliberately separate from `spring`: a `.yaml` file is not a JVM
/// concept, and someone running this editor on a project with no JVM in it
/// should still get YAML support.
pub struct FileTypesExtension;

impl Extension for FileTypesExtension {
    fn manifest(&self) -> ExtensionManifest {
        manifest("file-types", "Common file types")
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
pub fn builtin_registry() -> Registry {
    let mut registry = Registry::new();
    register_builtins(&mut registry).expect("this build's own extensions must not conflict");
    registry
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_test;
