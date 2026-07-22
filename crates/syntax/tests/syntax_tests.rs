use fg_core::Language;
use syntax::{byte_to_point, highlight_spans, syntax_errors, IncrementalParser, InputEdit, Scope};

const VALID_JAVA: &str = include_str!("fixtures/valid.java");
const VALID_KOTLIN: &str = include_str!("fixtures/valid.kt");
const UNCLOSED_BRACE_JAVA: &str = include_str!("fixtures/unclosed_brace.java");
const MALFORMED_CLASS_KOTLIN: &str = include_str!("fixtures/malformed_class.kt");

#[test]
fn valid_java_has_no_syntax_errors() {
    let mut parser = IncrementalParser::new(Language::Java);
    let tree = parser.parse(VALID_JAVA);
    assert_eq!(syntax_errors(tree), vec![]);
}

#[test]
fn valid_kotlin_has_no_syntax_errors() {
    let mut parser = IncrementalParser::new(Language::Kotlin);
    let tree = parser.parse(VALID_KOTLIN);
    assert_eq!(syntax_errors(tree), vec![]);
}

#[test]
fn unclosed_brace_reports_diagnostic_at_expected_span() {
    let mut parser = IncrementalParser::new(Language::Java);
    let tree = parser.parse(UNCLOSED_BRACE_JAVA);
    let diagnostics = syntax_errors(tree);

    assert!(!diagnostics.is_empty());
    // The parser recovers by inserting a zero-width MISSING `}` right before
    // the file's trailing newline.
    let expected_pos = UNCLOSED_BRACE_JAVA.len() - 1;
    assert!(diagnostics
        .iter()
        .any(|d| d.range.start == expected_pos && d.range.end == expected_pos));
}

#[test]
fn malformed_class_decl_reports_diagnostic_at_expected_span() {
    let mut parser = IncrementalParser::new(Language::Kotlin);
    let tree = parser.parse(MALFORMED_CLASS_KOTLIN);
    let diagnostics = syntax_errors(tree);

    assert!(!diagnostics.is_empty());
    // The grammar wraps the malformed `class` declaration (missing name) in
    // an ERROR node spanning the `class` keyword itself.
    let class_start = MALFORMED_CLASS_KOTLIN.find("class").unwrap();
    let class_end = class_start + "class".len();
    assert!(diagnostics
        .iter()
        .any(|d| d.range.start == class_start && d.range.end == class_end));
}

#[test]
fn incremental_reparse_matches_full_reparse() {
    let original = VALID_JAVA;
    let insertion = "    // inserted line\n";
    let insert_at = original.find("private String name;").unwrap();
    let mut edited = original.to_string();
    edited.insert_str(insert_at, insertion);

    // Full reparse from scratch on the edited text.
    let mut full_parser = IncrementalParser::new(Language::Java);
    let full_tree_errors = syntax_errors(full_parser.parse(&edited));
    let full_tree_highlights = highlight_spans(full_parser.tree().unwrap(), &edited, Language::Java);

    // Incremental: parse original, then edit + reparse.
    let mut incremental_parser = IncrementalParser::new(Language::Java);
    incremental_parser.parse(original);

    let start_byte = insert_at;
    let old_end_byte = insert_at;
    let new_end_byte = insert_at + insertion.len();
    let edit = InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte,
        start_position: byte_to_point(original, start_byte),
        old_end_position: byte_to_point(original, old_end_byte),
        new_end_position: byte_to_point(&edited, new_end_byte),
    };
    let incremental_tree_errors = syntax_errors(incremental_parser.reparse(&edited, edit));
    let incremental_tree_highlights =
        highlight_spans(incremental_parser.tree().unwrap(), &edited, Language::Java);

    assert_eq!(full_tree_errors, incremental_tree_errors);
    assert_eq!(full_tree_highlights, incremental_tree_highlights);
}

#[test]
fn highlight_spans_cover_expected_keyword_string_comment_ranges() {
    let mut parser = IncrementalParser::new(Language::Java);
    let tree = parser.parse(VALID_JAVA);
    let spans = highlight_spans(tree, VALID_JAVA, Language::Java);

    let has_scope_over = |needle: &str, scope: Scope| {
        let start = VALID_JAVA.find(needle).unwrap();
        let end = start + needle.len();
        spans
            .iter()
            .any(|(range, s)| *s == scope && range.start <= start && range.end >= end)
    };

    assert!(has_scope_over("class", Scope::Keyword));
    assert!(has_scope_over("public", Scope::Keyword));
    assert!(has_scope_over("// A friendly greeting", Scope::Comment));
    assert!(has_scope_over("\"Hello, \"", Scope::String));
}

#[test]
fn kotlin_highlight_query_compiles_and_covers_expected_ranges() {
    // Regression test: the Kotlin highlight query previously listed "break",
    // "continue", and "reified" as literal keyword tokens, which panicked at
    // `Query::new` time (they don't survive as matchable node types in this
    // grammar crate's compiled parser, despite appearing in its grammar.js
    // source). This test exercises the exact call path the editor widget
    // uses on every Kotlin file, so a bad query fails a test instead of
    // panicking the first time a user opens a .kt file.
    let mut parser = IncrementalParser::new(Language::Kotlin);
    let tree = parser.parse(VALID_KOTLIN);
    let spans = highlight_spans(tree, VALID_KOTLIN, Language::Kotlin);

    let has_scope_over = |needle: &str, scope: Scope| {
        let start = VALID_KOTLIN.find(needle).unwrap();
        let end = start + needle.len();
        spans
            .iter()
            .any(|(range, s)| *s == scope && range.start <= start && range.end >= end)
    };

    assert!(has_scope_over("class", Scope::Keyword));
    assert!(has_scope_over("fun", Scope::Keyword));
    assert!(has_scope_over("// A friendly greeting", Scope::Comment));
    assert!(has_scope_over("\"world\"", Scope::String));
}
