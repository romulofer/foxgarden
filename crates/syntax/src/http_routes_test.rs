use fg_core::Language;
use fg_extension::{
    Contributions, Extension, ExtensionManifest, HttpRoute, LanguageContribution, Registry, CURRENT_SCHEMA_VERSION,
};

use super::http_routes;
use crate::IncrementalParser;
use crate::providers::install;

/// An extension that calls every top-level node of its own invented
/// language a `GET` route — nonsense as an analysis, which is the point:
/// what is under test is that the route map lists whatever the *extension*
/// says, not that it agrees with any framework's real rules.
struct FakeRouter;

impl Extension for FakeRouter {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            id: "router".to_string(),
            name: "router".to_string(),
            version: "1.0.0".to_string(),
            schema_version: CURRENT_SCHEMA_VERSION,
        }
    }

    fn contributions(&self) -> Contributions {
        Contributions {
            languages: vec![LanguageContribution {
                id: "quenya".to_string(),
                display_name: "Quenya".to_string(),
                file_extensions: vec!["qya".to_string()],
                filename_patterns: Vec::new(),
            }],
            http_route_languages: vec!["quenya".to_string()],
            ..Default::default()
        }
    }

    fn http_routes(&self, language_id: &str, tree: &tree_sitter::Tree, _source: &str) -> Vec<HttpRoute> {
        if language_id != "quenya" {
            return Vec::new();
        }
        let mut cursor = tree.root_node().walk();
        tree.root_node()
            .children(&mut cursor)
            .map(|node| HttpRoute {
                method: "GET".to_string(),
                path: format!("/{}", node.kind()),
                owner: "Router".to_string(),
                handler: "handle".to_string(),
                handler_byte: node.start_byte(),
            })
            .collect()
    }
}

fn java_tree(source: &str) -> tree_sitter::Tree {
    let mut parser = IncrementalParser::new(Language::Java).expect("the shipped Java grammar");
    parser.parse(source).clone()
}

const CONTROLLER: &str = "@RequestMapping(\"/api\")\nclass Users {\n    @GetMapping(\"/{id}\")\n    void find() {}\n}\n";

/// The shipped set is installed for this crate's tests (see `providers`),
/// so the Spring extension answers for Java without any setup here.
#[test]
fn the_extension_that_knows_the_framework_is_what_finds_a_files_routes() {
    let tree = java_tree(CONTROLLER);

    let routes = http_routes(&tree, CONTROLLER, Language::Java);
    assert_eq!(
        routes,
        vec![HttpRoute {
            method: "GET".to_string(),
            path: "/api/{id}".to_string(),
            owner: "Users".to_string(),
            handler: "find".to_string(),
            handler_byte: CONTROLLER.find("find").unwrap(),
        }]
    );
}

/// A language the editor has never heard of gets routes purely because its
/// extension said so, and the shipped languages keep answering beside it.
#[test]
fn an_extensions_own_language_gets_whatever_routes_it_reports() {
    let mut registry = Registry::new();
    registry.register(Box::new(FakeRouter)).expect("fixture must register");
    install(&registry);
    let tree = java_tree(CONTROLLER);

    let invented = http_routes(&tree, CONTROLLER, Language::new("quenya"));
    assert_eq!(invented.len(), 1, "{invented:?}");
    assert_eq!(invented[0].path, "/class_declaration");

    assert_eq!(http_routes(&tree, CONTROLLER, Language::Java).len(), 1, "the shipped analysis still answers");
}

#[test]
fn a_language_no_extension_finds_routes_in_has_none() {
    let tree = java_tree(CONTROLLER);
    assert!(http_routes(&tree, CONTROLLER, Language::new("khuzdul")).is_empty());
}
