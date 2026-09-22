use fg_core::Language;
use fg_extension::{
    Contributions, Extension, ExtensionManifest, GrammarContribution, GrammarSource, LanguageContribution,
    Registry, CURRENT_SCHEMA_VERSION,
};

use super::{install, ts_language, GrammarError};

/// An extension contributing one language whose grammar is whatever the
/// test needs it to be. Invented ids throughout (`elvish`, never `java`):
/// the claim under test is that the store holds grammars this crate knows
/// nothing about, and reaching for the one language the editor is saturated
/// with would prove the opposite.
struct FakeExtension {
    id: &'static str,
    language_id: &'static str,
    source: GrammarSource,
}

impl Extension for FakeExtension {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            id: self.id.to_string(),
            name: self.id.to_string(),
            version: "0.0.1".to_string(),
            schema_version: CURRENT_SCHEMA_VERSION,
        }
    }

    fn contributions(&self) -> Contributions {
        Contributions {
            languages: vec![LanguageContribution {
                id: self.language_id.to_string(),
                display_name: self.language_id.to_string(),
                file_extensions: vec![self.language_id.to_string()],
                filename_patterns: Vec::new(),
            }],
            grammars: vec![GrammarContribution {
                language_id: self.language_id.to_string(),
                source: self.source.clone(),
                highlight_query: None,
            }],
            ..Default::default()
        }
    }
}

fn registry_with(extension: &FakeExtension) -> Registry {
    let mut registry = Registry::new();
    registry.register(extension).expect("fixture must register");
    registry
}

/// The store is process-wide and shared with every other test in this
/// binary, so a test that installs has to install under an id nothing else
/// uses. Tests below take their ids from their own names for that reason.
fn install_fake(id: &'static str, language_id: &'static str, source: GrammarSource) -> Vec<GrammarError> {
    install(&registry_with(&FakeExtension {
        id,
        language_id,
        source,
    }))
}

#[test]
fn a_contributed_grammar_becomes_parseable() {
    // The shipped set is installed automatically for this crate's tests
    // (see `store`), which is what makes the editor's own Java/Kotlin tests
    // work without each of them installing first.
    let java = ts_language(Language::Java).expect("the shipped Java grammar must be installed");
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&java).expect("an installed grammar must load");
    let tree = parser.parse("class A { }", None).expect("parse");
    assert_eq!(tree.root_node().kind(), "program");
}

#[test]
fn a_language_nothing_contributed_a_grammar_for_has_none() {
    assert!(ts_language(Language::new("khuzdul")).is_none());
}

#[test]
fn a_grammar_from_a_missing_shared_library_is_reported_not_fatal() {
    let errors = install_fake(
        "missing-library-ext",
        "missing-library-lang",
        GrammarSource::SharedLibrary {
            path: std::path::PathBuf::from("/nonexistent/libtree_sitter_elvish.so"),
            symbol: "tree_sitter_elvish".to_string(),
        },
    );
    assert_eq!(errors.len(), 1, "one grammar, one error");
    let GrammarError::Load {
        language_id, message, ..
    } = &errors[0]
    else {
        panic!("a missing file is a load failure, not an ABI mismatch: {:?}", errors[0]);
    };
    assert_eq!(language_id, "missing-library-lang");
    assert!(!message.is_empty(), "the loader's own message must be carried through");
    // The point of returning the error rather than raising it: the process
    // is still running and the language is simply unparseable.
    assert!(ts_language(Language::new("missing-library-lang")).is_none());
}

/// A wrong entry symbol in an otherwise real library — the second failure
/// mode Track 24 Checkpoint 0's spike found, and the one a hand-written
/// extension manifest is most likely to produce.
#[test]
fn a_shared_library_without_the_named_symbol_is_reported() {
    // Any real shared object will do; what is being tested is the symbol
    // lookup, not the file. The C library is present wherever this test
    // runs.
    let libc = ["/lib/x86_64-linux-gnu/libc.so.6", "/usr/lib/libc.so.6"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.exists());
    let Some(libc) = libc else {
        eprintln!("skipping: no libc.so.6 at either known path on this machine");
        return;
    };
    let errors = install_fake(
        "wrong-symbol-ext",
        "wrong-symbol-lang",
        GrammarSource::SharedLibrary {
            path: libc,
            symbol: "tree_sitter_definitely_not_here".to_string(),
        },
    );
    assert!(
        matches!(errors.as_slice(), [GrammarError::Load { .. }]),
        "expected one load failure, got {errors:?}"
    );
}

#[test]
fn installing_twice_keeps_the_grammar_already_in_use() {
    // Grammars are never swapped out, because a `Tree` already parsed with
    // one points into it (Track 24 Checkpoint 0). A second install of the
    // same language is therefore a no-op rather than a replacement — which
    // is also what makes it safe for several test binaries' fixtures to
    // install the shipped set independently.
    let errors = install(&fg_languages::builtin_registry());
    assert!(errors.is_empty(), "re-installing the shipped set must not error: {errors:?}");
    assert!(ts_language(Language::Java).is_some());
}

#[test]
fn every_shipped_grammar_installs_cleanly() {
    let errors = install(&fg_languages::builtin_registry());
    assert!(errors.is_empty(), "{errors:?}");
    for language in [
        Language::Java,
        Language::Kotlin,
        Language::Properties,
        Language::Yaml,
        Language::Xml,
        Language::Dockerfile,
    ] {
        assert!(ts_language(language).is_some(), "{language} has no installed grammar");
    }
}

/// Every shipped grammar's ABI is inside the range this build accepts —
/// the check `install` runs, asserted here against the real grammars so a
/// version bump that outruns the pinned tree-sitter fails a test rather
/// than silently turning highlighting off.
#[test]
fn every_shipped_grammar_is_within_this_builds_abi_range() {
    let supported = super::supported_abi();
    for language in [Language::Java, Language::Kotlin, Language::Yaml, Language::Xml] {
        let abi = ts_language(language).expect("installed").abi_version();
        assert!(supported.contains(&abi), "{language} is ABI {abi}, supported {supported:?}");
    }
}
