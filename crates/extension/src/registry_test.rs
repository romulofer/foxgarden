use std::path::{Path, PathBuf};

use super::*;
use crate::{BuildTask, CommandSpec, FilenamePattern, GrammarSource, LanguageContribution, NodeKinds};

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
    registry.register(Box::new(FakeExtension::new("rivendell", elvish))).unwrap();

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
        .register(Box::new(FakeExtension::new("containers", || Contributions {
            languages: vec![LanguageContribution {
                id: "dockerfile".to_string(),
                display_name: "Dockerfile".to_string(),
                file_extensions: Vec::new(),
                filename_patterns: vec![FilenamePattern::StemWithAffix {
                    stem: "Dockerfile".to_string(),
                }],
            }],
            ..Default::default()
        })))
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
        .register(Box::new(FakeExtension::new("rivendell", || Contributions {
            languages: vec![lang("elvish", &["elv"])],
            grammars: vec![GrammarContribution {
                language_id: "elvish".to_string(),
                source: GrammarSource::SharedLibrary {
                    path: "/tmp/libelvish.so".into(),
                    symbol: "tree_sitter_elvish".to_string(),
                },
                highlight_query: Some("(identifier) @variable".to_string()),
                node_kinds: NodeKinds::default(),
            }],
            ..Default::default()
        })))
        .unwrap();

    let grammar = registry.grammar("elvish").expect("grammar registered");
    assert_eq!(grammar.highlight_query.as_deref(), Some("(identifier) @variable"));
}

#[test]
fn a_language_server_is_findable_by_the_language_it_serves() {
    let mut registry = Registry::new();
    registry
        .register(Box::new(FakeExtension::new("rivendell", || Contributions {
            languages: vec![lang("elvish", &["elv"]), lang("khuzdul", &["khz"])],
            language_servers: vec![LanguageServerContribution {
                id: "elvish-ls".to_string(),
                display_name: "Elvish Language Server".to_string(),
                language_ids: vec!["elvish".to_string(), "khuzdul".to_string()],
                binary_name: "elvish-language-server".to_string(),
                args: Vec::new(),
                initialization_options: Some(r#"{"verbose":true}"#.to_string()),
            }],
            ..Default::default()
        })))
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
        registry.register(Box::new(ext)),
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
    registry.register(Box::new(FakeExtension::new("rivendell", elvish))).unwrap();

    assert_eq!(
        registry.register(Box::new(FakeExtension::new("lothlorien", elvish))),
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
    registry.register(Box::new(FakeExtension::new("rivendell", elvish))).unwrap();
    assert_eq!(
        registry.register(Box::new(FakeExtension::new("rivendell", Contributions::default))),
        Err(RegisterError::DuplicateExtension {
            extension_id: "rivendell".to_string(),
        })
    );
}

#[test]
fn a_grammar_for_an_unregistered_language_is_refused() {
    let mut registry = Registry::new();
    assert_eq!(
        registry.register(Box::new(FakeExtension::new("orphan", || Contributions {
            grammars: vec![GrammarContribution {
                language_id: "westron".to_string(),
                source: GrammarSource::SharedLibrary {
                    path: "/tmp/x.so".into(),
                    symbol: "tree_sitter_westron".to_string(),
                },
                highlight_query: None,
                node_kinds: NodeKinds::default(),
            }],
            ..Default::default()
        }))),
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
                node_kinds: NodeKinds::default(),
            },
            GrammarContribution {
                language_id: "elvish".to_string(),
                source: GrammarSource::SharedLibrary {
                    path: "/tmp/b.so".into(),
                    symbol: "tree_sitter_elvish".to_string(),
                },
                highlight_query: None,
                node_kinds: NodeKinds::default(),
            },
        ],
        ..Default::default()
    };

    assert_eq!(
        registry.register(Box::new(FakeExtension::new("rivendell", two_grammars))),
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
            node_kinds: NodeKinds::default(),
        }],
        ..Default::default()
    };

    assert!(registry.register(Box::new(FakeExtension::new("rivendell", good_language_bad_grammar))).is_err());
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
        registry.register(Box::new(FakeExtension::new("confused", || Contributions {
            languages: vec![lang("elvish", &["elv"]), lang("elvish", &["elvish"])],
            ..Default::default()
        }))),
        Err(RegisterError::DuplicateLanguage { .. })
    ));
}

// ──────────────────────────────────────────────────────────────────────────
// Build tools (`PLAN.md` Track 24 Phase 5)

/// An extension contributing two invented build tools, with real behavior
/// behind them — invented for the same reason the fake languages above are:
/// the claim is that the registry carries tooling the core knows nothing
/// about, and Maven would prove the opposite.
struct FakeToolchain;

impl Extension for FakeToolchain {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            id: "khazad".to_string(),
            name: "Khazad toolchain".to_string(),
            version: "1.0.0".to_string(),
            schema_version: CURRENT_SCHEMA_VERSION,
        }
    }

    fn contributions(&self) -> Contributions {
        Contributions {
            build_tools: vec![
                BuildToolContribution {
                    id: "delve".to_string(),
                    display_name: "Delve".to_string(),
                    marker_files: vec!["delve.toml".to_string()],
                },
                BuildToolContribution {
                    id: "mine".to_string(),
                    display_name: "Mine".to_string(),
                    marker_files: vec!["mine.toml".to_string(), "mine.yaml".to_string()],
                },
            ],
            ..Default::default()
        }
    }

    fn build_command(
        &self,
        tool_id: &str,
        project_root: &Path,
        task: BuildTask,
    ) -> Option<CommandSpec> {
        // Only `delve` digs; `mine` deliberately supports nothing, which is
        // what a tool without a coverage story looks like from here.
        (tool_id == "delve").then(|| CommandSpec::new("dig", project_root).arg(format!("{task:?}")))
    }

    fn classes_dir(&self, tool_id: &str, project_root: &Path) -> Option<PathBuf> {
        (tool_id == "delve").then(|| project_root.join("deep"))
    }
}

#[test]
fn a_contributed_build_tool_is_findable_by_id_and_dispatches_to_its_extension() {
    let mut registry = Registry::new();
    registry.register(Box::new(FakeToolchain)).unwrap();

    let delve = registry.build_tool("delve").unwrap();
    assert_eq!(delve.display_name, "Delve");
    let command = delve.command(Path::new("/project"), BuildTask::Compile).unwrap();
    assert_eq!(command.program, PathBuf::from("dig"));
    assert_eq!(command.args, vec!["Compile"]);
    assert_eq!(delve.classes_dir(Path::new("/project")).unwrap(), PathBuf::from("/project/deep"));

    // A tool that contributes nothing for a capability is a normal state,
    // not an error — the editor reports it and carries on.
    let mine = registry.build_tool("mine").unwrap();
    assert!(mine.command(Path::new("/project"), BuildTask::Compile).is_none());
    assert!(mine.classes_dir(Path::new("/project")).is_none());

    assert!(registry.build_tool("cartography").is_none());
    assert_eq!(registry.build_tools().len(), 2);
}

#[test]
fn a_build_tool_is_detected_by_any_of_its_own_marker_files() {
    let mut registry = Registry::new();
    registry.register(Box::new(FakeToolchain)).unwrap();
    let dir = tempfile::tempdir().unwrap();

    assert!(registry.detect_build_tool(dir.path()).is_none());

    std::fs::write(dir.path().join("mine.yaml"), "").unwrap();
    assert_eq!(registry.detect_build_tool(dir.path()).unwrap().id, "mine");

    // Registration order decides ties, so a project carrying both markers
    // resolves to the first-registered tool rather than to whichever
    // directory entry happened to be read first.
    std::fs::write(dir.path().join("delve.toml"), "").unwrap();
    assert_eq!(registry.detect_build_tool(dir.path()).unwrap().id, "delve");
}

#[test]
fn a_directory_whose_marker_name_is_a_directory_is_not_a_project() {
    let mut registry = Registry::new();
    registry.register(Box::new(FakeToolchain)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("delve.toml")).unwrap();
    assert!(registry.detect_build_tool(dir.path()).is_none());
}

#[test]
fn two_extensions_cannot_claim_the_same_build_tool_id() {
    struct Impostor;
    impl Extension for Impostor {
        fn manifest(&self) -> ExtensionManifest {
            ExtensionManifest {
                id: "moria".to_string(),
                name: "moria".to_string(),
                version: "1.0.0".to_string(),
                schema_version: CURRENT_SCHEMA_VERSION,
            }
        }
        fn contributions(&self) -> Contributions {
            Contributions {
                build_tools: vec![BuildToolContribution {
                    id: "delve".to_string(),
                    display_name: "Delve (also)".to_string(),
                    marker_files: vec!["delve.toml".to_string()],
                }],
                ..Default::default()
            }
        }
    }

    let mut registry = Registry::new();
    registry.register(Box::new(FakeToolchain)).unwrap();
    assert_eq!(
        registry.register(Box::new(Impostor)),
        Err(RegisterError::DuplicateBuildTool {
            tool_id: "delve".to_string(),
            extension_id: "moria".to_string(),
        })
    );
    // Rejected whole: the impostor left nothing behind, not even its name.
    assert_eq!(registry.extensions().len(), 1);
    assert_eq!(registry.build_tool("delve").unwrap().display_name, "Delve");
}

// ──────────────────────────────────────────────────────────────────────────
// Scaffolds, project release and config properties

/// An extension that scaffolds one invented kind of project and recognizes
/// one invented project marker — again nothing JVM, so what is being tested
/// is the registry's dispatch rather than Java.
struct FakeForge;

impl Extension for FakeForge {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            id: "forge".to_string(),
            name: "forge".to_string(),
            version: "1.0.0".to_string(),
            schema_version: CURRENT_SCHEMA_VERSION,
        }
    }

    fn contributions(&self) -> Contributions {
        Contributions {
            languages: vec![lang("khuzdul", &["khz"])],
            build_tools: vec![BuildToolContribution {
                id: "anvil".to_string(),
                display_name: "Anvil".to_string(),
                marker_files: vec!["anvil.toml".to_string()],
            }],
            scaffolds: vec![crate::ScaffoldContribution {
                build_tool_id: "anvil".to_string(),
                language_id: "khuzdul".to_string(),
                runtime_versions: vec![2, 7],
            }],
            ..Default::default()
        }
    }

    fn scaffold_files(&self, spec: &crate::ScaffoldSpec) -> Option<Vec<(PathBuf, String)>> {
        Some(vec![(
            PathBuf::from("anvil.toml"),
            format!("name = \"{}\"\nversion = {}\n", spec.name, spec.runtime_version?),
        )])
    }

    fn project_release(&self, project_root: &Path) -> Option<crate::ProjectRelease> {
        std::fs::read_to_string(project_root.join("anvil.toml"))
            .ok()?
            .trim()
            .strip_prefix("version = ")?
            .parse()
            .ok()
            .map(|major| crate::ProjectRelease {
                major,
                file: "anvil.toml".to_string(),
                setting: "version".to_string(),
            })
    }

    fn config_properties(&self, _project_root: &Path, build_tool: &BuildToolHandle) -> Vec<crate::ConfigProperty> {
        vec![crate::ConfigProperty {
            name: format!("{}.heat", build_tool.id),
            type_name: Some("int".to_string()),
            description: None,
            default_value: Some("900".to_string()),
        }]
    }
}

#[test]
fn a_contributed_scaffold_is_listed_and_generates_through_its_own_extension() {
    let mut registry = Registry::new();
    registry.register(Box::new(FakeForge)).unwrap();

    let scaffolds = registry.scaffolds();
    assert_eq!(scaffolds.len(), 1);
    assert_eq!(scaffolds[0].runtime_versions, vec![2, 7]);

    let spec = crate::ScaffoldSpec {
        build_tool_id: "anvil".to_string(),
        language_id: "khuzdul".to_string(),
        namespace: "under.the.mountain".to_string(),
        name: "forge-app".to_string(),
        runtime_version: Some(7),
    };
    let files = registry.scaffold_files(&spec).unwrap();
    assert_eq!(files[0].0, PathBuf::from("anvil.toml"));
    assert!(files[0].1.contains("name = \"forge-app\""), "{}", files[0].1);

    // A pair nobody declared is `None` rather than a wrong guess at which
    // extension might handle it.
    let unknown = crate::ScaffoldSpec {
        language_id: "elvish".to_string(),
        ..spec
    };
    assert!(registry.scaffold_files(&unknown).is_none());
}

#[test]
fn a_scaffold_for_an_unregistered_build_tool_is_refused() {
    struct Dwarfless;
    impl Extension for Dwarfless {
        fn manifest(&self) -> ExtensionManifest {
            ExtensionManifest {
                id: "dwarfless".to_string(),
                name: "dwarfless".to_string(),
                version: "1.0.0".to_string(),
                schema_version: CURRENT_SCHEMA_VERSION,
            }
        }
        fn contributions(&self) -> Contributions {
            Contributions {
                languages: vec![lang("khuzdul", &["khz"])],
                scaffolds: vec![crate::ScaffoldContribution {
                    build_tool_id: "anvil".to_string(),
                    language_id: "khuzdul".to_string(),
                    runtime_versions: Vec::new(),
                }],
                ..Default::default()
            }
        }
    }

    let mut registry = Registry::new();
    assert_eq!(
        registry.register(Box::new(Dwarfless)),
        Err(RegisterError::UnknownBuildTool {
            tool_id: "anvil".to_string(),
            extension_id: "dwarfless".to_string(),
        })
    );
    assert!(registry.scaffolds().is_empty());
    // Rejected whole: the language it also declared is not registered either.
    assert!(registry.language("khuzdul").is_none());
}

#[test]
fn the_project_release_is_whichever_extension_recognizes_the_project() {
    let mut registry = Registry::new();
    registry.register(Box::new(FakeForge)).unwrap();
    let dir = tempfile::tempdir().unwrap();

    assert!(registry.project_release(dir.path()).is_none());

    std::fs::write(dir.path().join("anvil.toml"), "version = 7\n").unwrap();
    let release = registry.project_release(dir.path()).unwrap();
    assert_eq!(release.major, 7);
    assert_eq!(release.file, "anvil.toml");
}

#[test]
fn an_extension_handle_answers_project_questions_away_from_the_registry() {
    let mut registry = Registry::new();
    registry.register(Box::new(FakeForge)).unwrap();
    let handle = registry.handles().into_iter().next().unwrap();
    assert_eq!(&*handle.id, "forge");
    let anvil = registry.build_tool("anvil").unwrap();

    // The point of the handle: it is owned, so a background scan can hold it
    // while the registry stays where it is.
    let scanned = std::thread::spawn(move || handle.config_properties(Path::new("/project"), &anvil))
        .join()
        .unwrap();
    // Keyed off the tool it was handed, not one it detected itself.
    assert_eq!(scanned[0].name, "anvil.heat");
}

/// A second extension scaffolding a pair someone already scaffolds would
/// never be asked — the wizard lists the pair once and generation picks the
/// first match — so it is refused at registration, like every other
/// duplicate, rather than silently shadowed.
#[test]
fn two_scaffolds_for_the_same_pair_are_refused() {
    struct Copycat;
    impl Extension for Copycat {
        fn manifest(&self) -> ExtensionManifest {
            ExtensionManifest {
                id: "copycat".to_string(),
                name: "copycat".to_string(),
                version: "1.0.0".to_string(),
                schema_version: CURRENT_SCHEMA_VERSION,
            }
        }
        fn contributions(&self) -> Contributions {
            Contributions {
                scaffolds: vec![crate::ScaffoldContribution {
                    build_tool_id: "anvil".to_string(),
                    language_id: "khuzdul".to_string(),
                    runtime_versions: Vec::new(),
                }],
                ..Default::default()
            }
        }
    }

    let mut registry = Registry::new();
    registry.register(Box::new(FakeForge)).unwrap();
    assert_eq!(
        registry.register(Box::new(Copycat)),
        Err(RegisterError::DuplicateScaffold {
            tool_id: "anvil".to_string(),
            language_id: "khuzdul".to_string(),
            extension_id: "copycat".to_string(),
        })
    );
    assert_eq!(registry.scaffolds().len(), 1);
}

#[test]
fn one_extension_cannot_scaffold_the_same_pair_twice() {
    struct Stutter;
    impl Extension for Stutter {
        fn manifest(&self) -> ExtensionManifest {
            ExtensionManifest {
                id: "stutter".to_string(),
                name: "stutter".to_string(),
                version: "1.0.0".to_string(),
                schema_version: CURRENT_SCHEMA_VERSION,
            }
        }
        fn contributions(&self) -> Contributions {
            let scaffold = crate::ScaffoldContribution {
                build_tool_id: "kiln".to_string(),
                language_id: "elvish".to_string(),
                runtime_versions: Vec::new(),
            };
            Contributions {
                languages: vec![lang("elvish", &["elv"])],
                build_tools: vec![BuildToolContribution {
                    id: "kiln".to_string(),
                    display_name: "Kiln".to_string(),
                    marker_files: vec!["kiln.toml".to_string()],
                }],
                scaffolds: vec![scaffold.clone(), scaffold],
                ..Default::default()
            }
        }
    }

    let mut registry = Registry::new();
    assert!(matches!(
        registry.register(Box::new(Stutter)),
        Err(RegisterError::DuplicateScaffold { .. })
    ));
    assert!(registry.language("elvish").is_none());
}

/// A run marker's entry point is in its own extension's notation, so the
/// tool that launches it must come from that extension even when another
/// extension's tool claims the directory first.
#[test]
fn build_tool_detection_can_be_limited_to_one_extension() {
    let mut registry = Registry::new();
    registry.register(Box::new(FakeToolchain)).unwrap();
    registry.register(Box::new(FakeForge)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("delve.toml"), "").unwrap();
    std::fs::write(dir.path().join("anvil.toml"), "").unwrap();

    assert_eq!(registry.detect_build_tool(dir.path()).unwrap().id, "delve");
    assert_eq!(registry.detect_build_tool_of(dir.path(), "forge").unwrap().id, "anvil");

    std::fs::remove_file(dir.path().join("anvil.toml")).unwrap();
    assert!(registry.detect_build_tool_of(dir.path(), "forge").is_none());
    assert!(registry.detect_build_tool_of(dir.path(), "nobody").is_none());
}

// ──────────────────────────────────────────────────────────────────────────
// File extensions, language order, grammar attribution, server starts

fn dwarvish() -> Contributions {
    Contributions {
        languages: vec![lang("dwarvish", &["ELV", "dw"])],
        ..Default::default()
    }
}

/// Which language a file is comes from its extension, so a second claim
/// must be refused rather than silently take over `.elv` files.
#[test]
fn a_file_extension_another_language_claims_is_refused() {
    let mut registry = Registry::new();
    registry.register(Box::new(FakeExtension::new("rivendell", elvish))).unwrap();

    assert_eq!(
        registry.register(Box::new(FakeExtension::new("moria", dwarvish))),
        Err(RegisterError::DuplicateFileExtension {
            file_extension: "elv".to_string(),
            language_id: "dwarvish".to_string(),
            extension_id: "moria".to_string(),
            already_claimed_by: "elvish".to_string(),
        })
    );
    assert_eq!(registry.language_for_extension("elv").unwrap().language.id, "elvish");
    assert!(registry.language("dwarvish").is_none());
}

#[test]
fn languages_are_listed_in_registration_order() {
    let many = || Contributions {
        languages: ["quenya", "sindarin", "khuzdul", "adunaic", "westron"]
            .iter()
            .map(|id| lang(id, &[id]))
            .collect(),
        ..Default::default()
    };
    let mut registry = Registry::new();
    registry.register(Box::new(FakeExtension::new("tongues", many))).unwrap();

    let ids: Vec<&str> = registry.languages().map(|l| l.language.id.as_str()).collect();
    assert_eq!(ids, ["quenya", "sindarin", "khuzdul", "adunaic", "westron"]);
}

/// A grammar one extension supplies for another's language is attributed
/// to the extension that shipped the grammar.
#[test]
fn a_grammar_remembers_which_extension_contributed_it() {
    let grammar_only = || Contributions {
        grammars: vec![GrammarContribution {
            language_id: "elvish".to_string(),
            source: GrammarSource::SharedLibrary {
                path: "/tmp/elvish.so".into(),
                symbol: "tree_sitter_elvish".to_string(),
            },
            highlight_query: None,
            node_kinds: NodeKinds::default(),
        }],
        ..Default::default()
    };
    let mut registry = Registry::new();
    registry.register(Box::new(FakeExtension::new("rivendell", elvish))).unwrap();
    registry.register(Box::new(FakeExtension::new("lorien", grammar_only))).unwrap();

    let elvish = registry.language("elvish").unwrap();
    assert_eq!(elvish.extension_id, "rivendell");
    assert_eq!(elvish.grammar_extension_id.as_deref(), Some("lorien"));
}

/// Registering the same ids again (every test that builds the shipped
/// registry does) hands back the very same leaked strings.
#[test]
fn static_ids_are_shared_between_registries() {
    let first = {
        let mut registry = Registry::new();
        registry.register(Box::new(FakeExtension::new("rivendell", elvish))).unwrap();
        registry.language("elvish").unwrap().static_id
    };
    let mut registry = Registry::new();
    registry.register(Box::new(FakeExtension::new("rivendell", elvish))).unwrap();
    assert!(std::ptr::eq(first, registry.language("elvish").unwrap().static_id));
}

/// A server whose extension does not override `resolve_server_start`
/// starts exactly as declared, from the configured path or, with none,
/// from its declared binary name on `PATH`.
#[test]
fn a_server_with_no_custom_start_starts_as_declared() {
    let with_server = || Contributions {
        languages: vec![lang("elvish", &["elv"])],
        language_servers: vec![LanguageServerContribution {
            id: "elvish-ls".to_string(),
            display_name: "Elvish LS".to_string(),
            language_ids: vec!["elvish".to_string()],
            binary_name: "elvish-ls".to_string(),
            args: vec!["--stdio".to_string()],
            initialization_options: Some("{}".to_string()),
        }],
        ..Default::default()
    };
    let mut registry = Registry::new();
    registry.register(Box::new(FakeExtension::new("rivendell", with_server))).unwrap();

    let unconfigured = registry
        .resolve_server_start("elvish-ls", &ServerStartContext::default())
        .unwrap()
        .unwrap();
    assert_eq!(unconfigured.binary, PathBuf::from("elvish-ls"));
    assert_eq!(unconfigured.args, ["--stdio"]);

    let context = ServerStartContext {
        configured_binary: " /opt/elvish-ls ".to_string(),
        ..Default::default()
    };
    let configured = registry.resolve_server_start("elvish-ls", &context).unwrap().unwrap();
    assert_eq!(configured.binary, PathBuf::from("/opt/elvish-ls"));
    assert!(registry.resolve_server_start("nobody", &context).is_none());
}

#[test]
fn http_route_languages_are_answerable_once_registered_and_shared_between_extensions() {
    let mut registry = Registry::new();
    registry
        .register(Box::new(FakeExtension::new("rivendell", || Contributions {
            languages: vec![lang("elvish", &["elv"])],
            http_route_languages: vec!["elvish".to_string()],
            ..Default::default()
        })))
        .unwrap();
    registry
        .register(Box::new(FakeExtension::new("lothlorien", || Contributions {
            http_route_languages: vec!["elvish".to_string()],
            ..Default::default()
        })))
        .expect("a second web framework on the same language is ordinary, not a conflict");

    assert!(registry.has_http_routes("elvish"));
    assert!(!registry.has_http_routes("westron"));
}

#[test]
fn http_routes_for_an_unregistered_language_are_refused() {
    let mut registry = Registry::new();
    assert_eq!(
        registry.register(Box::new(FakeExtension::new("orphan", || Contributions {
            http_route_languages: vec!["westron".to_string()],
            ..Default::default()
        }))),
        Err(RegisterError::UnknownLanguage {
            language_id: "westron".to_string(),
            extension_id: "orphan".to_string(),
        })
    );
    assert!(!registry.has_http_routes("westron"));
}
