use super::*;

#[test]
fn exact_pattern_matches_only_that_name_ignoring_case() {
    let p = FilenamePattern::Exact {
        stem: "Makefile".to_string(),
    };
    assert!(p.matches("Makefile"));
    assert!(p.matches("makefile"));
    assert!(!p.matches("Makefile.dev"));
    assert!(!p.matches("dev.Makefile"));
}

/// The three spellings a real Dockerfile appears as — the case
/// `fg_core::Language::from_filename` exists to handle today, and the
/// reason `FilenamePattern` has a second variant at all.
#[test]
fn stem_with_affix_matches_all_three_dockerfile_spellings() {
    let p = FilenamePattern::StemWithAffix {
        stem: "Dockerfile".to_string(),
    };
    assert!(p.matches("Dockerfile"));
    assert!(p.matches("dockerfile"));
    assert!(p.matches("Dockerfile.dev"));
    assert!(p.matches("dev.Dockerfile"));
}

/// The stem is matched case-insensitively but must still be the *whole*
/// affixed part — a name that merely contains it is a different file.
#[test]
fn stem_with_affix_does_not_match_a_mere_substring() {
    let p = FilenamePattern::StemWithAffix {
        stem: "Dockerfile".to_string(),
    };
    assert!(!p.matches("NotADockerfileReally"));
    assert!(!p.matches("Dockerfileish"));
}

#[test]
fn index_resolves_extensions_case_insensitively() {
    let mut index = LanguageIndex::default();
    index.insert(&LanguageContribution {
        id: "yaml".to_string(),
        display_name: "YAML".to_string(),
        file_extensions: vec!["yml".to_string(), "yaml".to_string()],
        filename_patterns: Vec::new(),
    });

    assert_eq!(index.by_extension("yml").map(String::as_str), Some("yaml"));
    assert_eq!(index.by_extension("YAML").map(String::as_str), Some("yaml"));
    assert_eq!(index.by_extension("java"), None);
}

struct Probe;

impl Extension for Probe {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            id: "probe".to_string(),
            name: "Probe".to_string(),
            version: "1.2.3".to_string(),
            schema_version: CURRENT_SCHEMA_VERSION,
        }
    }

    fn contributions(&self) -> Contributions {
        Contributions {
            languages: vec![LanguageContribution {
                id: "zz".to_string(),
                display_name: "ZZ".to_string(),
                file_extensions: vec!["zz".to_string()],
                filename_patterns: Vec::new(),
            }],
            ..Default::default()
        }
    }
}

#[test]
fn info_names_what_an_extension_contributes_by_display_name() {
    let info = ExtensionInfo::of(&Probe);
    assert_eq!(info.manifest.id, "probe");
    assert_eq!(info.languages, vec!["ZZ"]);
    assert!(info.language_servers.is_empty());
    assert!(info.build_tools.is_empty());
    assert!(info.analyzers.is_empty());
}
