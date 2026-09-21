use super::*;
use crate::IncrementalParser;

fn parsed(language: Language, source: &str) -> Tree {
    let mut parser = IncrementalParser::new(language).expect("a bundled grammar must load");
    parser.parse(source).clone()
}

#[test]
fn java_existing_imports_strips_the_terminator_and_reports_document_order() {
    let source = "package com.example;\n\nimport java.util.List;\nimport java.util.Map;\n\nclass Foo {}\n";
    let tree = parsed(Language::Java, source);
    let imports = existing_imports(&tree, source, Language::Java);
    assert_eq!(
        imports.iter().map(|i| i.path.as_str()).collect::<Vec<_>>(),
        vec!["java.util.List", "java.util.Map"]
    );
    assert_eq!(&source[imports[0].byte_range.clone()], "import java.util.List;");
}

#[test]
fn java_static_import_has_the_static_keyword_stripped_from_its_own_path() {
    let source = "import static org.junit.Assert.assertEquals;\n\nclass Foo {}\n";
    let tree = parsed(Language::Java, source);
    let imports = existing_imports(&tree, source, Language::Java);
    assert_eq!(imports[0].path, "org.junit.Assert.assertEquals");
}

#[test]
fn kotlin_existing_imports_has_no_terminator_to_strip() {
    let source = "package com.example\n\nimport java.util.List\n\nclass Foo\n";
    let tree = parsed(Language::Kotlin, source);
    let imports = existing_imports(&tree, source, Language::Kotlin);
    assert_eq!(imports[0].path, "java.util.List");
}

#[test]
fn a_language_with_no_import_vocabulary_reports_none() {
    let source = "key: value\n";
    let tree = parsed(Language::Yaml, source);
    assert!(existing_imports(&tree, source, Language::Yaml).is_empty());
}

#[test]
fn import_insertion_finds_the_alphabetically_correct_before_slot() {
    let existing = vec![
        ExistingImport {
            path: "java.util.List".to_string(),
            byte_range: 0..10,
        },
        ExistingImport {
            path: "java.util.Set".to_string(),
            byte_range: 20..30,
        },
    ];
    assert_eq!(
        import_insertion(&existing, "java.util.Map"),
        ImportInsertion::Before(20)
    );
}

#[test]
fn import_insertion_falls_back_to_after_the_last_import_when_new_path_sorts_last() {
    let existing = vec![
        ExistingImport {
            path: "java.util.List".to_string(),
            byte_range: 0..10,
        },
        ExistingImport {
            path: "java.util.Map".to_string(),
            byte_range: 20..30,
        },
    ];
    assert_eq!(
        import_insertion(&existing, "org.springframework.stereotype.Component"),
        ImportInsertion::AfterLast(30)
    );
}

#[test]
fn import_insertion_reports_already_imported_for_an_exact_match() {
    let existing = vec![ExistingImport {
        path: "org.springframework.stereotype.Component".to_string(),
        byte_range: 0..10,
    }];
    assert_eq!(
        import_insertion(&existing, "org.springframework.stereotype.Component"),
        ImportInsertion::AlreadyImported
    );
}

#[test]
fn import_insertion_with_no_existing_imports_reports_after_last_zero() {
    assert_eq!(
        import_insertion(&[], "org.springframework.stereotype.Component"),
        ImportInsertion::AfterLast(0)
    );
}
