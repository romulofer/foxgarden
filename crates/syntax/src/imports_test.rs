use super::*;
use crate::IncrementalParser;

fn parsed(language: Language, source: &str) -> Tree {
    let mut parser = IncrementalParser::new(language).expect("a bundled grammar must load");
    parser.parse(source).clone()
}

#[test]
fn the_shipped_extension_offers_annotations_for_java_and_kotlin_only() {
    assert!(annotation_candidates(Language::Java).iter().any(|c| c.name == "Component"));
    assert!(!annotation_candidates(Language::Kotlin).is_empty());
    assert!(annotation_candidates(Language::Yaml).is_empty());
}

#[test]
fn import_edit_comes_from_the_extension_that_owns_the_language() {
    let source = "package com.example;\n\nclass Foo {}\n";
    let tree = parsed(Language::Java, source);
    let edit = import_edit(&tree, source, Language::Java, "a.B").expect("an edit");
    assert_eq!(edit.byte, "package com.example;".len());
    assert_eq!(edit.text, "\n\nimport a.B;");

    assert!(import_edit(&tree, source, Language::Yaml, "a.B").is_none());
}
