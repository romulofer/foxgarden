use fg_core::Language;
use syntax::{byte_to_point, highlight_spans, syntax_errors, IncrementalParser, InputEdit, Scope};

const VALID_JAVA: &str = include_str!("fixtures/valid.java");
const VALID_KOTLIN: &str = include_str!("fixtures/valid.kt");
const UNCLOSED_BRACE_JAVA: &str = include_str!("fixtures/unclosed_brace.java");
const MALFORMED_CLASS_KOTLIN: &str = include_str!("fixtures/malformed_class.kt");
const VALID_PROPERTIES: &str = include_str!("fixtures/valid.properties");
const VALID_YAML: &str = include_str!("fixtures/valid.yml");
const VALID_XML: &str = include_str!("fixtures/valid.xml");
const VALID_DOCKERFILE: &str = include_str!("fixtures/Dockerfile");

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

    // Regression coverage for the bundled query's `@attribute` /
    // `@variable.builtin` / `@constant.builtin` captures, which the fixed
    // theme has no `Scope` for and which therefore rendered uncolored
    // before highlights_java.scm remapped them (see that file's header).
    assert!(has_scope_over("SuppressWarnings", Scope::Type));
    assert!(has_scope_over("this", Scope::Keyword));
    assert!(has_scope_over("true", Scope::Keyword));
    assert!(has_scope_over("println", Scope::Function));

    // Regression coverage for gaps found by diffing against Zed's own Java
    // extension (`../references/java`, same tree-sitter-java grammar): record
    // and custom-annotation declaration names weren't captured as `@type` at
    // all, `"@interface"` wasn't a keyword, and binary integer literals
    // weren't in the `@number` literal list.
    assert!(has_scope_over("Point", Scope::Type));
    assert!(has_scope_over("Marker", Scope::Type));
    assert!(has_scope_over("@interface", Scope::Keyword));
    assert!(has_scope_over("record", Scope::Keyword));

    // Regression coverage for TECHNICAL_DEBT.md #2: the `@constant` capture
    // in highlights_java.scm (ALL-CAPS identifiers) had no matching `Scope`
    // in `scope_for_capture` and rendered as plain text.
    assert!(has_scope_over("MAX_LENGTH", Scope::Constant));

    // First richer-highlighting increment: fields get their own
    // `Scope::Property` (both at their declaration site and at an
    // `object.field`-style access), distinct from a local variable or
    // parameter, which stay plain `@variable`/`Scope`-less.
    assert!(has_scope_over("mask", Scope::Property), "a field's own declaration site should be Scope::Property");
    assert!(has_scope_over("loud", Scope::Property), "`this.loud`'s field access should be Scope::Property");
    // `MAX_LENGTH` is *also* a field declarator, but it must still resolve
    // to Constant, not Property — Constants is the later (and so, per
    // `highlight_spans`' own same-range-conflict rule, winning) pattern in
    // the query file specifically so an ALL-CAPS field keeps reading as a
    // constant rather than just "a field."
    assert!(!has_scope_over("MAX_LENGTH", Scope::Property));

    // An enum constant that doesn't follow the ALL_CAPS convention still
    // resolves to Constant via the explicit `enum_constant` capture, not
    // just the regex heuristic (same treatment TECHNICAL_DEBT.md #3 already
    // gave Kotlin's enum entries).
    assert!(has_scope_over("Hearts", Scope::Constant));
    assert!(has_scope_over("Spades", Scope::Constant));
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

    // Regression coverage for the additions beyond the checkpoint-1 minimum:
    // call expressions, `true`/`false`/`null` (not distinct node types in
    // this grammar — matched by text), character literals, and a companion
    // object's name.
    assert!(has_scope_over("println", Scope::Function));
    assert!(has_scope_over("'w'", Scope::String));
    assert!(has_scope_over("true", Scope::Keyword));
    assert!(has_scope_over("Defaults", Scope::Type));

    // Regression coverage for TECHNICAL_DEBT.md #3: enum entries ported from
    // Zed's reference Kotlin query, adapted to this grammar's `identifier`
    // (not `simple_identifier`) node shape for `enum_entry`'s name child.
    assert!(has_scope_over("LOW", Scope::Constant));
    assert!(has_scope_over("MEDIUM", Scope::Constant));
    assert!(has_scope_over("HIGH", Scope::Constant));
}

#[test]
fn valid_properties_has_no_syntax_errors() {
    let mut parser = IncrementalParser::new(Language::Properties);
    let tree = parser.parse(VALID_PROPERTIES);
    assert_eq!(syntax_errors(tree), vec![]);
}

#[test]
fn properties_highlight_query_covers_key_value_and_comment() {
    let mut parser = IncrementalParser::new(Language::Properties);
    let tree = parser.parse(VALID_PROPERTIES);
    let spans = highlight_spans(tree, VALID_PROPERTIES, Language::Properties);

    let has_scope_over = |needle: &str, scope: Scope| {
        let start = VALID_PROPERTIES.find(needle).unwrap();
        let end = start + needle.len();
        spans
            .iter()
            .any(|(range, s)| *s == scope && range.start <= start && range.end >= end)
    };

    // The key is the most prominent token in a `.properties` file — this is
    // exactly the case `Scope::Property` was added for (see highlight.rs).
    assert!(has_scope_over("greeting.message", Scope::Property));
    assert!(has_scope_over("Hello, world!", Scope::String));
    assert!(has_scope_over("# A friendly greeting", Scope::Comment));
}

#[test]
fn valid_yaml_has_no_syntax_errors() {
    let mut parser = IncrementalParser::new(Language::Yaml);
    let tree = parser.parse(VALID_YAML);
    assert_eq!(syntax_errors(tree), vec![]);
}

#[test]
fn yaml_highlight_query_covers_mapping_key_string_and_comment() {
    let mut parser = IncrementalParser::new(Language::Yaml);
    let tree = parser.parse(VALID_YAML);
    let spans = highlight_spans(tree, VALID_YAML, Language::Yaml);

    let has_scope_over = |needle: &str, scope: Scope| {
        let start = VALID_YAML.find(needle).unwrap();
        let end = start + needle.len();
        spans
            .iter()
            .any(|(range, s)| *s == scope && range.start <= start && range.end >= end)
    };

    // Mapping keys ("greeting", "message") are the most prominent token in
    // a YAML file — same `Scope::Property` case as `.properties` keys.
    assert!(has_scope_over("greeting", Scope::Property));
    assert!(has_scope_over("message", Scope::Property));
    assert!(has_scope_over("\"Hello, world!\"", Scope::String));
    assert!(has_scope_over("# A friendly note", Scope::Comment));

    // Regression guard: the bundled query also matches every plain-scalar
    // key generically as `@string` (the same rule that colors *values*),
    // in addition to the more specific `@property` capture that applies
    // only to keys. Both captures cover the identical byte range, so
    // `highlight_spans` must resolve that down to exactly one scope —
    // `Property`, since the more specific pattern is declared later in the
    // query file — not silently return (and let a caller mis-paint) both.
    assert!(
        !has_scope_over("greeting", Scope::String),
        "a mapping key must not carry both Property and String for the same range"
    );
    let key_start = VALID_YAML.find("greeting").unwrap();
    let key_end = key_start + "greeting".len();
    let spans_over_key: Vec<_> = spans
        .iter()
        .filter(|(range, _)| range.start == key_start && range.end == key_end)
        .collect();
    assert_eq!(
        spans_over_key.len(),
        1,
        "expected exactly one span for the key's exact range, got {spans_over_key:?}"
    );
}

#[test]
fn valid_xml_has_no_syntax_errors() {
    let mut parser = IncrementalParser::new(Language::Xml);
    let tree = parser.parse(VALID_XML);
    assert_eq!(syntax_errors(tree), vec![]);
}

#[test]
fn valid_dockerfile_has_no_syntax_errors() {
    let mut parser = IncrementalParser::new(Language::Dockerfile);
    let tree = parser.parse(VALID_DOCKERFILE);
    assert_eq!(syntax_errors(tree), vec![]);
}

#[test]
fn dockerfile_highlight_query_covers_instructions_strings_and_comments() {
    let mut parser = IncrementalParser::new(Language::Dockerfile);
    let tree = parser.parse(VALID_DOCKERFILE);
    let spans = highlight_spans(tree, VALID_DOCKERFILE, Language::Dockerfile);

    let has_scope_over = |needle: &str, scope: Scope| {
        let start = VALID_DOCKERFILE.find(needle).unwrap();
        let end = start + needle.len();
        spans
            .iter()
            .any(|(range, s)| *s == scope && range.start <= start && range.end >= end)
    };

    assert!(has_scope_over("FROM", Scope::Keyword));
    assert!(has_scope_over("ENTRYPOINT", Scope::Keyword));
    assert!(has_scope_over("# A friendly comment", Scope::Comment));
    assert!(has_scope_over("\"java\"", Scope::String));

    // `ARG`/`ENV` keys are this format's closest equivalent to a YAML/
    // properties mapping key — same `Scope::Property` treatment.
    assert!(has_scope_over("APP_VERSION", Scope::Property));
    assert!(has_scope_over("APP_HOME", Scope::Property));
}

#[test]
fn xml_highlight_query_covers_tag_names_and_comment() {
    let mut parser = IncrementalParser::new(Language::Xml);
    let tree = parser.parse(VALID_XML);
    let spans = highlight_spans(tree, VALID_XML, Language::Xml);

    let has_scope_over = |needle: &str, scope: Scope| {
        let start = VALID_XML.find(needle).unwrap();
        let end = start + needle.len();
        spans
            .iter()
            .any(|(range, s)| *s == scope && range.start <= start && range.end >= end)
    };

    // Element names are the most prominent token in an XML file — tag
    // captures map onto their own `Scope::Tag`, visually distinct from
    // YAML/properties keys' `Scope::Property` (see TECHNICAL_DEBT.md's
    // now-resolved entry on this conflation).
    assert!(has_scope_over("greeting", Scope::Tag));
    assert!(has_scope_over("message", Scope::Tag));
    assert!(!has_scope_over("greeting", Scope::Property), "an XML tag name must not carry Scope::Property");
    assert!(has_scope_over("A friendly note", Scope::Comment));
}
