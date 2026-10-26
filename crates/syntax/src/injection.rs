//! Highlighting text that one language embeds in another: the `<script>` and
//! `<style>` of a Svelte or HTML file.
//!
//! The host language's injection query (see [`fg_extension::InjectionContribution`])
//! says which text is another language's and which; that text is parsed on its
//! own, with the other grammar over just that range, and highlighted with the
//! other language's query.
//!
//! The embedded parse is not incremental: it is redone for every embedded
//! range that overlaps what is being painted, on each recompute. Only ranges in
//! view are parsed, and an embedded script is small next to the file around it.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Mutex, OnceLock};

use fg_core::Language;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::grammars::{injection_query_source, installed_language, ts_language};
use crate::highlight::{Scope, host_spans};

/// How deep embedded languages may nest. A language that injects itself would
/// otherwise never finish.
const MAX_DEPTH: usize = 3;

/// The compiled injection query of `language`, cached like the highlight query:
/// it is compiled once, and a failure is remembered rather than retried each
/// frame. `None` when the language embeds nothing or its query does not compile
/// against its grammar.
fn cached_query(language: Language) -> Option<&'static Query> {
    static CACHE: OnceLock<Mutex<HashMap<Language, Option<&'static Query>>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().expect("injection query cache lock");
    if let Some(query) = cache.get(&language) {
        return *query;
    }
    let source = injection_query_source(language)?;
    let ts = ts_language(language)?;
    let query = Query::new(&ts, source).ok().map(|q| &*Box::leak(Box::new(q)));
    cache.insert(language, query);
    query
}

/// Every range of `source` inside `byte_range` that holds another language's
/// text, with that language. Where several patterns capture the same range, the
/// last pattern wins.
fn embedded_ranges(tree: &Tree, source: &str, language: Language, byte_range: Range<usize>) -> Vec<(tree_sitter::Range, Language)> {
    let Some(query) = cached_query(language) else { return Vec::new() };
    let Some(content) = query.capture_index_for_name("injection.content") else { return Vec::new() };

    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(byte_range);
    let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());
    let mut found: HashMap<Range<usize>, (usize, tree_sitter::Range, Language)> = HashMap::new();
    while let Some(m) = matches.next() {
        let Some(node) = m.nodes_for_capture_index(content).next() else { continue };
        let Some(name) = query
            .property_settings(m.pattern_index)
            .iter()
            .find(|property| &*property.key == "injection.language")
            .and_then(|property| property.value.as_deref())
        else {
            continue;
        };
        let Some(embedded) = installed_language(name) else { continue };
        let range = node.byte_range();
        if range.is_empty() || found.get(&range).is_some_and(|(pattern, ..)| *pattern > m.pattern_index) {
            continue;
        }
        found.insert(range, (m.pattern_index, node.range(), embedded));
    }
    let mut ranges: Vec<_> = found.into_values().map(|(_, range, language)| (range, language)).collect();
    ranges.sort_by_key(|(range, _)| range.start_byte);
    ranges
}

/// `spans`, the host language's own highlights, with the embedded languages'
/// highlights laid over the ranges they occupy.
pub(crate) fn with_embedded(
    mut spans: Vec<(Range<usize>, Scope)>,
    tree: &Tree,
    source: &str,
    language: Language,
    byte_range: Range<usize>,
    depth: usize,
) -> Vec<(Range<usize>, Scope)> {
    if depth >= MAX_DEPTH {
        return spans;
    }
    for (range, embedded) in embedded_ranges(tree, source, language, byte_range.clone()) {
        let Some(ts) = ts_language(embedded) else { continue };
        let mut parser = Parser::new();
        if parser.set_language(&ts).is_err() || parser.set_included_ranges(&[range]).is_err() {
            continue;
        }
        let Some(embedded_tree) = parser.parse(source, None) else { continue };
        let content = range.start_byte..range.end_byte;
        let visible = content.start.max(byte_range.start)..content.end.min(byte_range.end);
        let inner = host_spans(&embedded_tree, source, embedded, visible, depth + 1);
        spans.retain(|(span, _)| !(content.start <= span.start && span.end <= content.end));
        spans.extend(inner);
    }
    spans.sort_by_key(|(range, _)| (range.start, range.end));
    spans
}
