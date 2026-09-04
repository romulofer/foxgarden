use std::ops::Range;
use std::sync::OnceLock;

use fg_core::Language;
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

use crate::language::{highlights_query_source, ts_language};

/// The checkpoint-1 fixed color theme (SPEC.md §5.4), extended with
/// `Property` and `Tag` for the markup/config languages added afterward:
/// Java/Kotlin's original five scopes have nothing that fits a YAML/
/// properties mapping key or an XML tag name, and those are the single
/// most prominent token in either format — leaving them uncolored would
/// make "syntax highlighting" for these languages mean little beyond
/// comments and string values. `Property` and `Tag` are separate variants
/// (rather than one shared scope) because they're different lexical
/// concepts that happen to both need *a* color — a future language wanting
/// tag-like styling visually distinct from property-key styling (HTML
/// alongside XML, say) is free to give `Tag` its own color without also
/// disturbing every YAML/properties key.
///
/// `Constant` was added later still, for Java's ALL-CAPS `@constant`
/// capture (`highlights_java.scm`) — a dedicated variant rather than
/// reusing `Property`, since a `static final` constant and a YAML/
/// properties mapping key are different enough lexical concepts that
/// sharing a color would read as a coincidence, not a deliberate visual
/// grouping.
///
/// `Parameter`/`Operator`/`Label`/`DocComment` are `PLAN.md` Track 2's own
/// "richer Java/Kotlin syntax highlighting" additions (`SPEC.md` §2).
/// `Parameter` is a dedicated variant rather than reusing `Property`
/// (`SPEC.md`'s own "a real design call to make at implementation time"):
/// a method parameter and a field are different enough lexical roles to
/// tell apart at a glance, even with a visually related (not identical)
/// color — see `theme.rs`'s own color choice for the reasoning there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Keyword,
    String,
    Comment,
    Type,
    Function,
    Property,
    Tag,
    Constant,
    Parameter,
    Operator,
    Label,
    DocComment,
}

fn scope_for_capture(name: &str) -> Option<Scope> {
    if name.starts_with("keyword") {
        Some(Scope::Keyword)
    } else if name.starts_with("string") {
        Some(Scope::String)
    } else if name.starts_with("comment.doc") {
        // Checked ahead of the plain "comment" branch below — `@comment.doc`
        // itself starts with "comment", so the generic branch would
        // otherwise shadow it and doc comments would render as ordinary
        // ones.
        Some(Scope::DocComment)
    } else if name.starts_with("comment") {
        Some(Scope::Comment)
    } else if name.starts_with("type") {
        Some(Scope::Type)
    } else if name.starts_with("function") {
        Some(Scope::Function)
    } else if name.starts_with("variable.parameter") {
        Some(Scope::Parameter)
    } else if name.starts_with("property") {
        Some(Scope::Property)
    } else if name.starts_with("tag") {
        Some(Scope::Tag)
    } else if name.starts_with("constant") {
        Some(Scope::Constant)
    } else if name.starts_with("operator") {
        Some(Scope::Operator)
    } else if name.starts_with("label") {
        Some(Scope::Label)
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
    static PROPERTIES: OnceLock<Query> = OnceLock::new();
    static YAML: OnceLock<Query> = OnceLock::new();
    static XML: OnceLock<Query> = OnceLock::new();
    static DOCKERFILE: OnceLock<Query> = OnceLock::new();
    let cell = match language {
        Language::Java => &JAVA,
        Language::Kotlin => &KOTLIN,
        Language::Properties => &PROPERTIES,
        Language::Yaml => &YAML,
        Language::Xml => &XML,
        Language::Dockerfile => &DOCKERFILE,
    };
    cell.get_or_init(|| {
        Query::new(&ts_language(language), highlights_query_source(language))
            .expect("bundled highlight query must compile")
    })
}

/// Runs the language's highlight query over `tree`, returning byte ranges
/// tagged with a coarse `Scope`, ordered by start position — one scope per
/// exact range, never two.
///
/// A single node can match more than one pattern in a bundled query: YAML's
/// highlights.scm captures every `(string_scalar)` node generically as
/// `@string`, and *separately, later in the file*, captures the same node
/// as `@property` specifically when it's a mapping key — so an unquoted key
/// produces two captures spanning the identical byte range. Per the
/// standard tree-sitter-highlight convention, a pattern declared later in
/// the query file takes priority over an earlier one for the same range
/// (more specific patterns are written after more general ones on purpose).
/// `QueryCursor::captures` yields same-range matches in that declaration
/// order, so keeping the *last* one seen per exact range — rather than
/// returning both and leaving resolution to whoever paints them — is what
/// makes `@property` correctly win over `@string` for a YAML/properties key
/// instead of losing to whichever one happens to sort first.
pub fn highlight_spans(tree: &Tree, source: &str, language: Language) -> Vec<(Range<usize>, Scope)> {
    highlight_spans_in(tree, source, language, 0..source.len())
}

/// `highlight_spans` restricted to `byte_range` — the query only visits
/// nodes overlapping it, and only captures inside it are returned.
///
/// This is what keeps highlighting a large file cheap: the editor paints
/// only the rows in the viewport, so running the query over the whole
/// document means matching (and sorting, and allocating) tens of thousands
/// of captures for a few dozen visible lines, on every edit. A range query
/// scales with what's on screen instead of with file size.
pub fn highlight_spans_in(
    tree: &Tree,
    source: &str,
    language: Language,
    byte_range: Range<usize>,
) -> Vec<(Range<usize>, Scope)> {
    let query = cached_query(language);
    let capture_names = query.capture_names();

    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(byte_range);
    let mut captures = cursor.captures(query, tree.root_node(), source.as_bytes());

    let mut by_range: std::collections::HashMap<Range<usize>, Scope> = std::collections::HashMap::new();
    while let Some((query_match, capture_index)) = captures.next() {
        let capture = query_match.captures[*capture_index];
        let name = capture_names[capture.index as usize];
        if let Some(scope) = scope_for_capture(name) {
            by_range.insert(capture.node.byte_range(), scope);
        }
    }

    let mut spans: Vec<(Range<usize>, Scope)> = by_range.into_iter().collect();
    spans.sort_by_key(|(range, _)| range.start);
    spans
}
