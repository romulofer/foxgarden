use fg_core::Language;
use fg_extension::{
    Contributions, Extension, ExtensionManifest, LanguageContribution, Registry, RunTarget, CURRENT_SCHEMA_VERSION,
};

use super::{install, main_entries};
use crate::IncrementalParser;

/// An extension that calls every top-level node of its own invented
/// language a runnable entry point — nonsense as an analysis, which is the
/// point: what is under test is that the editor shows whatever the
/// *extension* says, not that it agrees with any language's real rules.
struct FakeRunner;

impl Extension for FakeRunner {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            id: "runner".to_string(),
            name: "runner".to_string(),
            version: "1.0.0".to_string(),
            schema_version: CURRENT_SCHEMA_VERSION,
        }
    }

    fn contributions(&self) -> Contributions {
        Contributions {
            languages: vec![LanguageContribution {
                id: "forge".to_string(),
                display_name: "Forge".to_string(),
                file_extensions: vec!["forge".to_string()],
                filename_patterns: Vec::new(),
            }],
            ..Default::default()
        }
    }

    fn run_targets(
        &self,
        language_id: &str,
        tree: &tree_sitter::Tree,
        _source: &str,
        file_stem: &str,
    ) -> Vec<RunTarget> {
        if language_id != "forge" {
            return Vec::new();
        }
        let mut cursor = tree.root_node().walk();
        tree.root_node()
            .children(&mut cursor)
            .map(|node| RunTarget {
                line: node.start_position().row,
                entry_point: format!("{language_id}:{file_stem}"),
                label: node.kind().to_string(),
            })
            .collect()
    }
}

/// The shipped extensions *plus* the fake one. Installing a registry that
/// drops the shipped extensions would change what every other test in this
/// binary sees, since the provider set is process-wide.
fn registry_with_fake_runner() -> Registry {
    let mut registry = fg_languages::builtin_registry();
    registry.register(Box::new(FakeRunner)).expect("fixture must register");
    registry
}

fn java_tree(source: &str) -> tree_sitter::Tree {
    let mut parser = IncrementalParser::new(Language::Java).expect("the shipped Java grammar");
    parser.parse(source).clone()
}

/// The shipped set is installed for this crate's tests (see `providers`),
/// so the real Java analysis answers unless a test replaces it.
#[test]
fn the_owning_extension_is_what_finds_a_files_entry_points() {
    let source = "public class App { public static void main(String[] args) { } }\n";
    let tree = java_tree(source);

    let entries = main_entries(&tree, source, Language::Java, "App");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].entry_point, "App");
    assert_eq!(entries[0].label, "App");
}

/// A language the editor has never heard of gets run markers purely because
/// its extension said so — no analysis in this crate is involved, and the
/// shipped languages keep answering as they did.
#[test]
fn an_extensions_own_language_gets_whatever_entry_points_it_reports() {
    let source = "public class App { public static void main(String[] args) { } }\n";
    let tree = java_tree(source);
    install(&registry_with_fake_runner());

    let invented = main_entries(&tree, source, Language::new("forge"), "App");
    assert_eq!(invented.len(), 1, "{invented:?}");
    assert_eq!(invented[0].entry_point, "forge:App");
    assert_eq!(invented[0].label, "class_declaration");

    let java = main_entries(&tree, source, Language::Java, "App");
    assert_eq!(java[0].entry_point, "App", "the shipped analysis still answers");
}

#[test]
fn a_language_no_extension_claims_has_no_run_markers() {
    let source = "key: value\n";
    let tree = java_tree(source);
    assert!(main_entries(&tree, source, Language::new("khuzdul"), "notes").is_empty());
}
