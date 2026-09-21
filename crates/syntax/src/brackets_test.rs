use super::*;
use crate::IncrementalParser;
use fg_core::Language;

fn parsed(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Java);
    parser.parse(source).clone()
}

#[test]
fn cursor_right_after_opening_brace_matches_the_closing_one() {
    let source = "class Foo {\n}\n";
    let tree = parsed(source);
    let brace_open = source.find('{').unwrap();
    let brace_close = source.find('}').unwrap();

    let (open, close) = bracket_match(&tree, source, brace_open + 1).unwrap();
    assert_eq!(open, brace_open..brace_open + 1);
    assert_eq!(close, brace_close..brace_close + 1);
}

#[test]
fn cursor_right_before_closing_brace_matches_the_opening_one() {
    let source = "class Foo {\n}\n";
    let tree = parsed(source);
    let brace_open = source.find('{').unwrap();
    let brace_close = source.find('}').unwrap();

    let (open, close) = bracket_match(&tree, source, brace_close).unwrap();
    assert_eq!(open, brace_open..brace_open + 1);
    assert_eq!(close, brace_close..brace_close + 1);
}

#[test]
fn cursor_deep_inside_the_block_body_finds_no_match() {
    let source = "class Foo {\n    int x;\n}\n";
    let tree = parsed(source);
    // Cursor in the middle of "int x;", nowhere near either brace.
    let mid = source.find("int").unwrap() + 1;

    assert_eq!(bracket_match(&tree, source, mid), None);
}

#[test]
fn matches_the_innermost_pair_for_nested_brackets() {
    let source = "class Foo {\n    int[] a = new int[3];\n}\n";
    let tree = parsed(source);
    let bracket_open = source.find('[').unwrap();
    let bracket_close = source.find(']').unwrap();

    let (open, close) = bracket_match(&tree, source, bracket_open + 1).unwrap();
    assert_eq!(open, bracket_open..bracket_open + 1);
    assert_eq!(close, bracket_close..bracket_close + 1);
}

#[test]
fn parenthesis_pair_around_a_method_call_matches() {
    let source = "class Foo {\n    void run() {\n        foo();\n    }\n}\n";
    let tree = parsed(source);
    let call_open = source.rfind('(').unwrap();
    let call_close = source.rfind(')').unwrap();

    let (open, close) = bracket_match(&tree, source, call_close).unwrap();
    assert_eq!(open, call_open..call_open + 1);
    assert_eq!(close, call_close..call_close + 1);
}

#[test]
fn unbalanced_code_finds_no_match_rather_than_panicking() {
    let source = "class Foo {\n    void run(";
    let tree = parsed(source);
    let paren = source.rfind('(').unwrap();

    assert_eq!(bracket_match(&tree, source, paren + 1), None);
}
