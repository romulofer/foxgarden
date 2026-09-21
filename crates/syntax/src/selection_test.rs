use super::*;
use crate::IncrementalParser;
use fg_core::Language;

fn parsed(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Java).expect("a bundled grammar must load");
    parser.parse(source).clone()
}

#[test]
fn expands_from_an_identifier_to_its_enclosing_expression() {
    let source = "class Foo {\n    void run() {\n        foo();\n    }\n}\n";
    let tree = parsed(source);
    let call_start = source.find("foo()").unwrap();
    let ident_range = call_start..call_start + 3; // "foo"

    let expanded = expand_selection(&tree, ident_range.start, ident_range.end).unwrap();
    // The identifier's smallest containing named node other than
    // itself is the whole method-invocation expression "foo()".
    assert_eq!(&source[expanded.clone()], "foo()");
    assert!(expanded.start <= ident_range.start && expanded.end >= ident_range.end);
}

#[test]
fn expanding_a_range_that_already_matches_a_node_climbs_to_its_parent() {
    let source = "class Foo {\n    void run() {\n        foo();\n    }\n}\n";
    let tree = parsed(source);
    let call_start = source.find("foo()").unwrap();
    let call_end = call_start + "foo()".len();

    // "foo()" is itself a full method-invocation node — expanding
    // *from* its exact range must climb past it to the enclosing
    // statement, not return the identical range back.
    let expanded = expand_selection(&tree, call_start, call_end).unwrap();
    assert_ne!(expanded, call_start..call_end);
    assert!(expanded.start <= call_start && expanded.end >= call_end);
}

#[test]
fn repeated_expansion_strictly_grows_the_range_each_time() {
    let source = "class Foo {\n    void run() {\n        foo();\n    }\n}\n";
    let tree = parsed(source);
    let call_start = source.find("foo()").unwrap();

    let mut range = call_start..call_start + 3; // "foo"
    for _ in 0..4 {
        let next = expand_selection(&tree, range.start, range.end).unwrap();
        assert!(
            next.start <= range.start && next.end >= range.end && next != range,
            "expansion must strictly grow: {range:?} -> {next:?}"
        );
        range = next;
    }
}

#[test]
fn expanding_the_whole_file_selection_returns_none() {
    let source = "class Foo {}\n";
    let tree = parsed(source);

    // The root node's own range already covers the whole source, and
    // it has no parent left to climb to.
    assert_eq!(expand_selection(&tree, 0, source.len()), None);
}

#[test]
fn expanding_a_collapsed_cursor_finds_the_smallest_touching_named_node() {
    let source = "class Foo {\n    int x;\n}\n";
    let tree = parsed(source);
    let x_pos = source.find('x').unwrap();

    let expanded = expand_selection(&tree, x_pos, x_pos).unwrap();
    assert!(source[expanded].contains('x'));
}
