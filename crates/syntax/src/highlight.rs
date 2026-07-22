use std::ops::Range;

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

/// Runs the language's highlight query over `tree`, returning byte ranges
/// tagged with a coarse `Scope`, ordered by start position.
pub fn highlight_spans(tree: &Tree, source: &str, language: Language) -> Vec<(Range<usize>, Scope)> {
    let ts_lang = ts_language(language);
    let query = Query::new(&ts_lang, highlights_query_source(language))
        .expect("bundled highlight query must compile");
    let capture_names = query.capture_names();

    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(&query, tree.root_node(), source.as_bytes());

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
