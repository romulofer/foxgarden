use std::ops::Range;
use std::sync::OnceLock;

use fg_core::Language;
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

use crate::language::{highlights_query_source, ts_language};

/// The checkpoint-1 fixed color theme (SPEC.md §5.4): every tree-sitter
/// capture name is coarsened down to one of these five scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Keyword,
    String,
    Comment,
    Type,
    Function,
}

fn scope_for_capture(name: &str) -> Option<Scope> {
    if name.starts_with("keyword") {
        Some(Scope::Keyword)
    } else if name.starts_with("string") {
        Some(Scope::String)
    } else if name.starts_with("comment") {
        Some(Scope::Comment)
    } else if name.starts_with("type") {
        Some(Scope::Type)
    } else if name.starts_with("function") {
        Some(Scope::Function)
    } else {
        None
    }
}

/// The compiled query is a pure function of `language` (one of two possible
/// values), but compiling it — parsing the query DSL and validating every
/// pattern against the grammar — is real work, not a cheap lookup. Cache it
/// per language instead of recompiling it on every call: `highlight_spans`
/// is invoked from the editor's layouter closure, which egui runs every
/// frame the editor is shown (not just on edits), so an uncached call here
/// would redo this work dozens of times a second even while idle.
fn cached_query(language: Language) -> &'static Query {
    static JAVA: OnceLock<Query> = OnceLock::new();
    static KOTLIN: OnceLock<Query> = OnceLock::new();
    let cell = match language {
        Language::Java => &JAVA,
        Language::Kotlin => &KOTLIN,
    };
    cell.get_or_init(|| {
        Query::new(&ts_language(language), highlights_query_source(language))
            .expect("bundled highlight query must compile")
    })
}

/// Runs the language's highlight query over `tree`, returning byte ranges
/// tagged with a coarse `Scope`, ordered by start position.
pub fn highlight_spans(tree: &Tree, source: &str, language: Language) -> Vec<(Range<usize>, Scope)> {
    let query = cached_query(language);
    let capture_names = query.capture_names();

    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(query, tree.root_node(), source.as_bytes());

    let mut spans = Vec::new();
    while let Some((query_match, capture_index)) = captures.next() {
        let capture = query_match.captures[*capture_index];
        let name = capture_names[capture.index as usize];
        if let Some(scope) = scope_for_capture(name) {
            spans.push((capture.node.byte_range(), scope));
        }
    }

    spans.sort_by_key(|(range, _)| range.start);
    spans
}
