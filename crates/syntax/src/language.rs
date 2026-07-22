use fg_core::Language;

pub fn ts_language(language: Language) -> tree_sitter::Language {
    match language {
        Language::Java => tree_sitter_java::LANGUAGE.into(),
        Language::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
    }
}

pub fn highlights_query_source(language: Language) -> &'static str {
    match language {
        Language::Java => tree_sitter_java::HIGHLIGHTS_QUERY,
        Language::Kotlin => include_str!("../queries/highlights_kotlin.scm"),
    }
}
