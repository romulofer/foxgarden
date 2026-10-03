use super::*;
use std::path::Path;

#[test]
fn both_shipped_extensions_register_without_conflicting() {
    let registry = builtin_registry();
    let ids: Vec<&str> = registry.extensions().iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["spring", "file-types"]);
    assert_eq!(registry.languages().count(), 6);
}

/// The six languages the closed `fg_core::Language` enum used to hardcode,
/// resolved the way they were before Phase 2 — same extensions, same
/// display names.
#[test]
fn every_language_the_old_enum_had_still_resolves_by_extension() {
    let registry = builtin_registry();
    for (ext, id, display) in [
        ("java", "java", "Java"),
        ("kt", "kotlin", "Kotlin"),
        ("properties", "properties", "Properties"),
        ("yml", "yaml", "YAML"),
        ("yaml", "yaml", "YAML"),
        ("xml", "xml", "XML"),
        ("dockerfile", "dockerfile", "Dockerfile"),
    ] {
        let found = registry
            .language_for_extension(ext)
            .unwrap_or_else(|| panic!("no language for .{ext}"));
        assert_eq!(found.language.id, id, ".{ext}");
        assert_eq!(found.language.display_name, display, ".{ext}");
    }
}

#[test]
fn a_dockerfile_resolves_by_name_in_all_three_spellings() {
    let registry = builtin_registry();
    for name in ["Dockerfile", "dockerfile", "Dockerfile.dev", "dev.Dockerfile"] {
        assert_eq!(
            registry
                .language_for_path(Path::new(name))
                .map(|l| l.language.id.as_str()),
            Some("dockerfile"),
            "{name}"
        );
    }
}

/// The precedence rule `Document::open` used to own: a real extension wins
/// over a filename pattern, so a backup file is whatever its actual
/// extension says rather than being read as a Dockerfile.
#[test]
fn an_extension_match_beats_a_filename_pattern() {
    let registry = builtin_registry();
    assert!(
        registry.language_for_path(Path::new("notes.dockerfile.bak")).is_none(),
        "a .bak file must not be mistaken for a Dockerfile"
    );
    assert_eq!(
        registry
            .language_for_path(Path::new("src/main/java/Main.java"))
            .map(|l| l.language.id.as_str()),
        Some("java")
    );
}

#[test]
fn an_unknown_file_type_resolves_to_nothing() {
    let registry = builtin_registry();
    assert!(registry.language_for_path(Path::new("notes.txt")).is_none());
    assert!(registry.language_for_path(Path::new("archive.tar.gz")).is_none());
}

/// Names that merely sit near a Dockerfile must not be read as one — the
/// `README`/`docker-compose.yml` cases the old `Language::from_filename`
/// guarded, kept here now that the recognition moved into the registry.
#[test]
fn an_unrelated_filename_is_not_mistaken_for_a_dockerfile() {
    let registry = builtin_registry();
    assert!(registry.language_for_path(Path::new("README")).is_none());
    assert_eq!(
        registry
            .language_for_path(Path::new("docker-compose.yml"))
            .map(|l| l.language.id.as_str()),
        Some("yaml"),
        "a compose file is YAML, not a Dockerfile"
    );
}

/// Every shipped language arrives with something to parse it with. This is
/// the claim Phase 3 moved out of `crates/syntax`: before, a language's
/// grammar was a `match` arm in the editor, so the two could not disagree;
/// now they are separate declarations, and a language contributed without
/// its grammar would open files that silently never highlight or fold.
#[test]
fn every_shipped_language_contributes_a_grammar_and_a_highlight_query() {
    let registry = builtin_registry();
    for language in registry.languages() {
        let id = &language.language.id;
        let grammar = registry
            .grammar(id)
            .unwrap_or_else(|| panic!("{id} ships without a grammar"));
        assert!(
            grammar.highlight_query.as_ref().is_some_and(|q| !q.trim().is_empty()),
            "{id} ships without a highlight query"
        );
    }
}

/// The JVM languages and the general file types come from different
/// extensions — the split this crate exists to make. `spring` is no longer
/// even in this repository (`../spring-foxgarden`); what keeps it wired in
/// is the path dependency in this crate's `Cargo.toml` and nothing else.
#[test]
fn jvm_languages_and_file_types_come_from_different_extensions() {
    let registry = builtin_registry();
    assert_eq!(registry.language("java").unwrap().extension_id, "spring");
    assert_eq!(registry.language("kotlin").unwrap().extension_id, "spring");
    assert_eq!(registry.language("yaml").unwrap().extension_id, "file-types");
    assert_eq!(registry.language("dockerfile").unwrap().extension_id, "file-types");
}
