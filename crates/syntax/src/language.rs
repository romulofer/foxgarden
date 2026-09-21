use fg_core::Language;

/// The tree-sitter grammar for `language`, or `None` if this build has
/// none for it.
///
/// `None` became reachable when `Language` stopped being a closed enum
/// (`PLAN.md` Track 24 Phase 2): the set of languages is whatever the
/// registered extensions contribute, while the set of grammars compiled in
/// here is fixed at build time, so the two can legitimately disagree. A
/// language with no grammar is not an error — it is a file that opens and
/// edits normally with no parse tree, which is exactly what any
/// unrecognized file has always done.
///
/// **Still a `match` on hardcoded ids, and deliberately so.** Phase 3 is
/// what replaces this with grammars supplied through the registry; leaving
/// the dispatch here until then keeps Phase 2 to one change — the identity
/// of a language — rather than two.
pub fn ts_language(language: Language) -> Option<tree_sitter::Language> {
    Some(match language {
        Language::Java => tree_sitter_java::LANGUAGE.into(),
        Language::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
        Language::Properties => tree_sitter_properties::LANGUAGE.into(),
        Language::Yaml => tree_sitter_yaml::LANGUAGE.into(),
        Language::Xml => tree_sitter_xml::LANGUAGE_XML.into(),
        Language::Dockerfile => tree_sitter_containerfile::LANGUAGE.into(),
        _ => return None,
    })
}

/// The highlight query for `language`, or `None` if this build has none.
///
/// Independent of `ts_language` returning `Some`: a grammar with no query
/// parses fine and simply paints unhighlighted.
pub fn highlights_query_source(language: Language) -> Option<&'static str> {
    Some(match language {
        Language::Java => include_str!("../queries/highlights_java.scm"),
        Language::Kotlin => include_str!("../queries/highlights_kotlin.scm"),
        Language::Properties => tree_sitter_properties::HIGHLIGHTS_QUERY,
        Language::Yaml => tree_sitter_yaml::HIGHLIGHTS_QUERY,
        Language::Xml => tree_sitter_xml::XML_HIGHLIGHT_QUERY,
        Language::Dockerfile => include_str!("../queries/highlights_dockerfile.scm"),
        _ => return None,
    })
}
