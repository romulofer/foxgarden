use super::*;
use crate::IncrementalParser;

fn parsed(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Java);
    parser.parse(source).clone()
}

#[test]
fn collects_enclosing_class_and_method_outermost_first() {
    let source = "class Foo {\n    void run() {\n        int x = 1;\n    }\n}\n";
    let tree = parsed(source);
    let inside = source.find("int x").unwrap();

    let starts = enclosing_scope_starts(&tree, inside, Language::Java);
    assert_eq!(starts.len(), 2, "class + method should both enclose the body");
    // Outermost first: the class declaration starts before the method.
    assert_eq!(starts[0], source.find("class Foo").unwrap());
    assert_eq!(starts[1], source.find("void run").unwrap());
}

#[test]
fn excludes_control_flow_blocks_between_the_scopes() {
    // The `if` block encloses the cursor but is deliberately not a sticky
    // scope — only the class and method declarations should come back.
    let source = "class Foo {\n    void run() {\n        if (true) {\n            go();\n        }\n    }\n}\n";
    let tree = parsed(source);
    let inside = source.find("go()").unwrap();

    let starts = enclosing_scope_starts(&tree, inside, Language::Java);
    assert_eq!(starts.len(), 2);
    assert_eq!(&source[starts[0]..starts[0] + 5], "class");
    assert_eq!(&source[starts[1]..starts[1] + 4], "void");
}

#[test]
fn top_level_offset_has_no_enclosing_scope() {
    let source = "class Foo {\n}\n";
    let tree = parsed(source);
    // Offset 0 sits on the `class` keyword itself — the class node starts
    // *at* it, so nothing strictly encloses byte 0 as an ancestor scope
    // beyond the class, which starts here too. The header-above-top
    // filter in the app layer drops a scope whose header is the current
    // line anyway; here we just confirm a top-of-file offset doesn't
    // report a phantom deeper scope.
    let starts = enclosing_scope_starts(&tree, 0, Language::Java);
    assert!(
        starts.iter().all(|&s| s == 0),
        "only the class (starting at 0) may enclose byte 0"
    );
}

#[test]
fn a_language_without_a_scope_vocabulary_returns_empty() {
    // Parsed as Java but queried as Yaml: no scope kinds, no results —
    // guards the early-out for languages folding/sticky don't cover yet.
    let source = "class Foo {\n    void run() {}\n}\n";
    let tree = parsed(source);
    let inside = source.find("run").unwrap();
    assert!(enclosing_scope_starts(&tree, inside, Language::Yaml).is_empty());
}
