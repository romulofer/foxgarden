use fg_core::Language;

pub fn ts_language(language: Language) -> tree_sitter::Language {
    match language {
        Language::Java => tree_sitter_java::LANGUAGE.into(),
        Language::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
        Language::Properties => tree_sitter_properties::LANGUAGE.into(),
        Language::Yaml => tree_sitter_yaml::LANGUAGE.into(),
        Language::Xml => tree_sitter_xml::LANGUAGE_XML.into(),
    }
}

pub fn highlights_query_source(language: Language) -> &'static str {
    match language {
        Language::Java => tree_sitter_java::HIGHLIGHTS_QUERY,
        Language::Kotlin => include_str!("../queries/highlights_kotlin.scm"),
        Language::Properties => tree_sitter_properties::HIGHLIGHTS_QUERY,
        Language::Yaml => tree_sitter_yaml::HIGHLIGHTS_QUERY,
        Language::Xml => tree_sitter_xml::XML_HIGHLIGHT_QUERY,
    }
}
