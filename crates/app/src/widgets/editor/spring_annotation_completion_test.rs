
use super::*;
use syntax::IncrementalParser;

fn parsed(language: Language, source: &str) -> Tree {
    // A parser needs the shipped grammars installed (Track 24
    // Phase 3); this test builds one by hand rather than through a
    // fixture that would have installed them already.
    test_support::install_grammars();
    let mut parser = IncrementalParser::new(language).expect("an installed grammar must load");
    parser.parse(source).clone()
}

#[test]
fn spring_annotation_candidates_are_labeled_and_carry_their_own_import_path_as_detail() {
    let candidates = spring_annotation_candidates();
    let component = candidates.iter().find(|c| c.label == "Component").unwrap();
    assert_eq!(component.kind, CompletionKind::Annotation);
    assert_eq!(
        component.detail.as_deref(),
        Some("org.springframework.stereotype.Component")
    );
}

#[test]
fn an_unrecognized_name_yields_no_insertion() {
    let source = "class Foo {}\n";
    let tree = parsed(Language::Java, source);
    assert!(spring_import_insertion(&tree, source, Language::Java, "NotASpringAnnotation").is_none());
}

#[test]
fn already_imported_yields_no_insertion() {
    let source = "import org.springframework.stereotype.Component;\n\nclass Foo {}\n";
    let tree = parsed(Language::Java, source);
    assert!(spring_import_insertion(&tree, source, Language::Java, "Component").is_none());
}

#[test]
fn java_with_no_imports_at_all_inserts_after_the_package_declaration() {
    let source = "package com.example;\n\nclass Foo {}\n";
    let tree = parsed(Language::Java, source);
    let (byte, insertion) = spring_import_insertion(&tree, source, Language::Java, "Component").unwrap();
    assert_eq!(byte, "package com.example;".len());
    assert_eq!(insertion, "\n\nimport org.springframework.stereotype.Component;");

    let mut result = source.to_string();
    result.insert_str(byte, &insertion);
    assert_eq!(
        result,
        "package com.example;\n\nimport org.springframework.stereotype.Component;\n\nclass Foo {}\n"
    );
}

#[test]
fn java_with_no_package_and_no_imports_inserts_at_the_very_start() {
    let source = "class Foo {}\n";
    let tree = parsed(Language::Java, source);
    let (byte, insertion) = spring_import_insertion(&tree, source, Language::Java, "Component").unwrap();
    assert_eq!(byte, 0);
    assert_eq!(insertion, "import org.springframework.stereotype.Component;\n\n");
}

#[test]
fn java_inserts_at_the_correct_alphabetical_slot_among_existing_imports() {
    // "org.springframework.stereotype.Component" sorts after
    // "java.util.List" (`j` < `o`) but before "org.springframework.
    // web.bind.annotation.RestController" (`stereotype` < `web`) —
    // genuinely bracketing the insertion point, unlike two plain
    // `java.util.*` imports (which both sort *before* any
    // `org.springframework.*` one).
    let source =
        "import java.util.List;\nimport org.springframework.web.bind.annotation.RestController;\n\nclass Foo {}\n";
    let tree = parsed(Language::Java, source);
    let (byte, insertion) = spring_import_insertion(&tree, source, Language::Java, "Component").unwrap();

    let mut result = source.to_string();
    result.insert_str(byte, &insertion);
    assert_eq!(
        result,
        "import java.util.List;\nimport org.springframework.stereotype.Component;\nimport org.springframework.web.bind.annotation.RestController;\n\nclass Foo {}\n"
    );
}

#[test]
fn java_appends_after_the_last_import_when_the_new_one_sorts_last() {
    let source = "import java.util.List;\nimport java.util.Map;\n\nclass Foo {}\n";
    let tree = parsed(Language::Java, source);
    let (byte, insertion) = spring_import_insertion(&tree, source, Language::Java, "Value").unwrap();

    let mut result = source.to_string();
    result.insert_str(byte, &insertion);
    assert_eq!(
        result,
        "import java.util.List;\nimport java.util.Map;\nimport org.springframework.beans.factory.annotation.Value;\n\nclass Foo {}\n"
    );
}

#[test]
fn kotlin_import_has_no_semicolon() {
    let source = "package com.example\n\nclass Foo\n";
    let tree = parsed(Language::Kotlin, source);
    let (_byte, insertion) = spring_import_insertion(&tree, source, Language::Kotlin, "Component").unwrap();
    assert_eq!(insertion, "\n\nimport org.springframework.stereotype.Component");
}
