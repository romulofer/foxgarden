use super::*;
use crate::{FilenamePattern, GrammarSource, LanguageContribution};

/// A stand-in extension built entirely from its contribution set —
/// Checkpoint 1's "fake extension that registers a fake language". It is
/// deliberately not a JVM one: the whole claim this crate makes is that
/// the registry holds languages it knows nothing about, and testing it
/// exclusively with Java would prove the opposite of what is wanted.
struct FakeExtension {
    id: &'static str,
    schema_version: u32,
    contributions: fn() -> Contributions,
}

impl FakeExtension {
    fn new(id: &'static str, contributions: fn() -> Contributions) -> Self {
        Self {
            id,
            schema_version: CURRENT_SCHEMA_VERSION,
            contributions,
        }
    }
}

impl Extension for FakeExtension {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            id: self.id.to_string(),
            name: self.id.to_string(),
            version: "1.0.0".to_string(),
            schema_version: self.schema_version,
        }
    }

    fn contributions(&self) -> Contributions {
        (self.contributions)()
    }
}

fn lang(id: &str, exts: &[&str]) -> LanguageContribution {
    LanguageContribution {
        id: id.to_string(),
        display_name: id.to_uppercase(),
        file_extensions: exts.iter().map(|e| (*e).to_string()).collect(),
        filename_patterns: Vec::new(),
    }
}

fn elvish() -> Contributions {
    Contributions {
        languages: vec![lang("elvish", &["elv"])],
        ..Default::default()
    }
}

#[test]
fn a_registered_language_is_findable_by_id_and_by_extension() {
    let mut registry = Registry::new();
    registry.register(&FakeExtension::new("rivendell", elvish)).unwrap();

    assert_eq!(registry.extensions().len(), 1);
    assert_eq!(registry.language("elvish").unwrap().language.display_name, "ELVISH");
    assert_eq!(
        registry.language_for_extension("elv").map(|l| l.language.id.as_str()),
        Some("elvish")
    );
    assert_eq!(
        registry.language("elvish").unwrap().extension_id,
        "rivendell",
        "the registry must remember who contributed a language, to name in errors"
    );
}

#[test]
fn an_unregistered_extension_resolves_to_nothing_rather_than_a_default() {
    let registry = Registry::new();
    assert!(registry.language_for_extension("elv").is_none());
    assert!(registry.language_for_filename("Dockerfile").is_none());
}

#[test]
fn a_filename_pattern_resolves_a_file_with_no_usable_extension() {
    let mut registry = Registry::new();
    registry
        .register(&FakeExtension::new("containers", || Contributions {
            languages: vec![LanguageContribution {
                id: "dockerfile".to_string(),
                display_name: "Dockerfile".to_string(),
                file_extensions: Vec::new(),
                filename_patterns: vec![FilenamePattern::StemWithAffix {
                    stem: "Dockerfile".to_string(),
                }],
            }],
            ..Default::default()
        }))
        .unwrap();

    for name in ["Dockerfile", "dockerfile", "Dockerfile.dev", "dev.Dockerfile"] {
        assert_eq!(
            registry.language_for_filename(name).map(|l| l.language.id.as_str()),
            Some("dockerfile"),
            "{name}"
        );
    }
}

#[test]
fn a_grammar_attaches_to_its_language() {
    let mut registry = Registry::new();
    registry
        .register(&FakeExtension::new("rivendell", || Contributions {
            languages: vec![lang("elvish", &["elv"])],
            grammars: vec![GrammarContribution {
                language_id: "elvish".to_string(),
                source: GrammarSource::SharedLibrary {
                    path: "/tmp/libelvish.so".into(),
                    symbol: "tree_sitter_elvish".to_string(),
                },
                highlight_query: Some("(identifier) @variable".to_string()),
            }],
            ..Default::default()
        }))
        .unwrap();

    let grammar = registry.grammar("elvish").expect("grammar registered");
    assert_eq!(grammar.highlight_query.as_deref(), Some("(identifier) @variable"));
}

#[test]
fn a_language_server_is_findable_by_the_language_it_serves() {
    let mut registry = Registry::new();
    registry
        .register(&FakeExtension::new("rivendell", || Contributions {
            languages: vec![lang("elvish", &["elv"]), lang("khuzdul", &["khz"])],
            language_servers: vec![LanguageServerContribution {
                id: "elvish-ls".to_string(),
                language_ids: vec!["elvish".to_string(), "khuzdul".to_string()],
                binary_name: "elvish-language-server".to_string(),
                args: Vec::new(),
                initialization_options: Some(r#"{"verbose":true}"#.to_string()),
            }],
            ..Default::default()
        }))
        .unwrap();

    // One server serving two languages is found under both — the case a
    // per-language slot (what `lsp_state` has today) cannot express.
    assert_eq!(registry.language_servers_for("elvish").len(), 1);
    assert_eq!(registry.language_servers_for("khuzdul").len(), 1);
    assert!(registry.language_servers_for("westron").is_empty());
}

#[test]
fn an_extension_built_for_another_schema_version_is_refused() {
    let mut registry = Registry::new();
    let mut ext = FakeExtension::new("from-the-future", elvish);
    ext.schema_version = CURRENT_SCHEMA_VERSION + 7;

    assert_eq!(
        registry.register(&ext),
        Err(RegisterError::IncompatibleSchema {
            extension_id: "from-the-future".to_string(),
            found: CURRENT_SCHEMA_VERSION + 7,
            expected: CURRENT_SCHEMA_VERSION,
        })
    );
    assert!(registry.language("elvish").is_none());
}

#[test]
fn two_extensions_claiming_one_language_is_refused_naming_the_incumbent() {
    let mut registry = Registry::new();
    registry.register(&FakeExtension::new("rivendell", elvish)).unwrap();

    assert_eq!(
        registry.register(&FakeExtension::new("lothlorien", elvish)),
        Err(RegisterError::DuplicateLanguage {
            language_id: "elvish".to_string(),
            extension_id: "lothlorien".to_string(),
            already_owned_by: "rivendell".to_string(),
        })
    );
}

#[test]
fn registering_the_same_extension_twice_is_refused() {
    let mut registry = Registry::new();
    registry.register(&FakeExtension::new("rivendell", elvish)).unwrap();
    assert_eq!(
        registry.register(&FakeExtension::new("rivendell", Contributions::default)),
        Err(RegisterError::DuplicateExtension {
            extension_id: "rivendell".to_string(),
        })
    );
}

#[test]
fn a_grammar_for_an_unregistered_language_is_refused() {
    let mut registry = Registry::new();
    assert_eq!(
        registry.register(&FakeExtension::new("orphan", || Contributions {
            grammars: vec![GrammarContribution {
                language_id: "westron".to_string(),
                source: GrammarSource::SharedLibrary {
                    path: "/tmp/x.so".into(),
                    symbol: "tree_sitter_westron".to_string(),
                },
                highlight_query: None,
            }],
            ..Default::default()
        })),
        Err(RegisterError::UnknownLanguage {
            language_id: "westron".to_string(),
            extension_id: "orphan".to_string(),
        })
    );
}

#[test]
fn a_second_grammar_for_one_language_is_refused() {
    let mut registry = Registry::new();
    let two_grammars = || Contributions {
        languages: vec![lang("elvish", &["elv"])],
        grammars: vec![
            GrammarContribution {
                language_id: "elvish".to_string(),
                source: GrammarSource::SharedLibrary {
                    path: "/tmp/a.so".into(),
                    symbol: "tree_sitter_elvish".to_string(),
                },
                highlight_query: None,
            },
            GrammarContribution {
                language_id: "elvish".to_string(),
                source: GrammarSource::SharedLibrary {
                    path: "/tmp/b.so".into(),
                    symbol: "tree_sitter_elvish".to_string(),
                },
                highlight_query: None,
            },
        ],
        ..Default::default()
    };

    assert_eq!(
        registry.register(&FakeExtension::new("rivendell", two_grammars)),
        Err(RegisterError::DuplicateGrammar {
            language_id: "elvish".to_string(),
            extension_id: "rivendell".to_string(),
        })
    );
}

/// The all-or-nothing guarantee: a contribution set that fails validation
/// partway leaves *none* of itself behind, including the parts that were
/// individually fine.
#[test]
fn a_refused_extension_leaves_nothing_registered() {
    let mut registry = Registry::new();
    let good_language_bad_grammar = || Contributions {
        languages: vec![lang("elvish", &["elv"])],
        grammars: vec![GrammarContribution {
            language_id: "westron".to_string(),
            source: GrammarSource::SharedLibrary {
                path: "/tmp/x.so".into(),
                symbol: "tree_sitter_westron".to_string(),
            },
            highlight_query: None,
        }],
        ..Default::default()
    };

    assert!(registry.register(&FakeExtension::new("rivendell", good_language_bad_grammar)).is_err());
    assert!(
        registry.language("elvish").is_none(),
        "the valid language must not survive its extension being refused"
    );
    assert!(registry.language_for_extension("elv").is_none());
    assert!(registry.extensions().is_empty());
}

#[test]
fn one_extension_declaring_a_language_twice_is_refused() {
    let mut registry = Registry::new();
    assert!(matches!(
        registry.register(&FakeExtension::new("confused", || Contributions {
            languages: vec![lang("elvish", &["elv"]), lang("elvish", &["elvish"])],
            ..Default::default()
        })),
        Err(RegisterError::DuplicateLanguage { .. })
    ));
}
