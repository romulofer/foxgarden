//! `PLAN.md` Track 24 Phase 6: the HTTP routes a file declares, found by
//! whichever extension knows the framework rather than by a `match` here.
//!
//! What this replaces is a Spring MVC analysis in this crate — mapping
//! annotations, class-level base paths, `RequestMethod` overrides, in both
//! JVM languages. That knowledge belongs to the extension that understands
//! the framework; what is general is "list a project's routes and jump to
//! their handlers", which every web framework has.
//!
//! Asks the extensions `providers` holds, handing over the tree the caller
//! already parsed.

use fg_core::Language;
use fg_extension::HttpRoute;
use tree_sitter::Tree;

/// Every HTTP route declared in `source`, in source order, from every
/// installed extension that finds routes in `language`. A language nobody
/// declared routes for simply has none.
pub fn http_routes(tree: &Tree, source: &str, language: Language) -> Vec<HttpRoute> {
    crate::providers::providers()
        .extensions
        .iter()
        .flat_map(|extension| extension.http_routes(language.id(), tree, source))
        .collect()
}

#[cfg(test)]
#[path = "http_routes_test.rs"]
mod http_routes_test;
