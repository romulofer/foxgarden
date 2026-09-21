use super::*;
use crate::IncrementalParser;

fn parsed(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Java).expect("a bundled grammar must load");
    parser.parse(source).clone()
}

fn hidden<'a>(source: &'a str, r: &FoldRange) -> &'a str {
    &source[r.start_byte..r.end_byte]
}

#[test]
fn folds_a_class_body_and_its_multi_line_methods() {
    let source = "\
class Foo {
    void a() {
        step();
    }
    void b() {
        step();
    }
}
";
    let tree = parsed(source);
    let ranges = foldable_ranges(&tree, source, Language::Java);

    // One fold for the class body, one for each multi-line method body.
    assert_eq!(ranges.len(), 3, "class body + two method bodies");

    // Marker lines are the opening lines: `class Foo {` (0), `void a() {`
    // (1), `void b() {` (4).
    assert_eq!(ranges.iter().map(|r| r.marker_line).collect::<Vec<_>>(), vec![0, 1, 4]);

    // The class-body fold hides everything through the final `}`.
    assert!(hidden(source, &ranges[0]).contains("void a()"));
    assert!(hidden(source, &ranges[0]).trim_end().ends_with('}'));
    // A method-body fold hides just that method's statements.
    assert!(hidden(source, &ranges[1]).contains("step();"));
    assert!(!hidden(source, &ranges[1]).contains("void b()"));
}

#[test]
fn a_one_line_body_is_not_foldable() {
    let source = "class Foo {\n    void a() { step(); }\n}\n";
    let tree = parsed(source);
    let ranges = foldable_ranges(&tree, source, Language::Java);

    // Only the (multi-line) class body folds; the single-line method body
    // has nothing below its opening line to hide.
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].marker_line, 0);
}

#[test]
fn folds_a_multi_line_block_comment() {
    let source = "\
/*
 * docs
 */
class Foo {}
";
    let tree = parsed(source);
    let ranges = foldable_ranges(&tree, source, Language::Java);
    // The block comment folds; `class Foo {}` is one line, so it doesn't.
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].marker_line, 0);
    assert!(hidden(source, &ranges[0]).contains("docs"));
}

#[test]
fn hidden_span_never_includes_the_marker_line() {
    let source = "class Foo {\n    int x;\n}\n";
    let tree = parsed(source);
    let ranges = foldable_ranges(&tree, source, Language::Java);
    assert_eq!(ranges.len(), 1);
    // Nothing before the first newline (the end of `class Foo {`) is
    // hidden — the marker line stays fully visible.
    assert!(!hidden(source, &ranges[0]).contains("class Foo"));
    assert!(hidden(source, &ranges[0]).contains("int x;"));
}

#[test]
fn a_language_without_a_foldable_vocabulary_returns_empty() {
    let source = "class Foo {\n    void a() {\n        step();\n    }\n}\n";
    let tree = parsed(source);
    assert!(foldable_ranges(&tree, source, Language::Yaml).is_empty());
}

#[test]
fn folds_a_run_of_two_or_more_consecutive_imports_but_not_a_lone_one() {
    // Mirrors the real-world shape reported: one lone import, a blank
    // line, then two separate multi-import blocks separated by another
    // blank line.
    let source = "\
package com.example;

import static org.springframework.http.MediaType.APPLICATION_PDF;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.http.HttpStatus;
import org.springframework.web.bind.annotation.RestController;

import br.ufsc.bridge.pec.backend.app.config.security.UserPrincipal;
import br.ufsc.bridge.pec.backend.module.Resources;

class Foo {}
";
    let tree = parsed(source);
    let ranges = foldable_ranges(&tree, source, Language::Java);

    // Only the two multi-import blocks fold — the lone
    // `import static ...` line gets no marker.
    assert_eq!(ranges.len(), 2);

    assert!(hidden(source, &ranges[0]).contains("HttpStatus"));
    assert!(hidden(source, &ranges[0]).contains("RestController"));
    assert!(
        !hidden(source, &ranges[0]).contains("Autowired"),
        "marker line itself stays visible"
    );
    assert!(
        !hidden(source, &ranges[0]).contains("UserPrincipal"),
        "must not swallow the next block"
    );

    assert!(hidden(source, &ranges[1]).contains("Resources"));
    assert!(
        !hidden(source, &ranges[1]).contains("UserPrincipal"),
        "marker line itself stays visible"
    );
}

#[test]
fn a_single_import_with_no_neighbors_is_not_foldable() {
    let source = "import java.util.List;\n\nclass Foo {}\n";
    let tree = parsed(source);
    assert!(foldable_ranges(&tree, source, Language::Java).is_empty());
}

#[test]
fn exactly_two_consecutive_imports_is_the_minimum_foldable_block() {
    let source = "import java.util.List;\nimport java.util.Map;\n\nclass Foo {}\n";
    let tree = parsed(source);
    let ranges = foldable_ranges(&tree, source, Language::Java);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].marker_line, 0);
    assert!(hidden(source, &ranges[0]).contains("Map"));
}

#[test]
fn a_comment_between_two_imports_breaks_the_run() {
    let source = "\
import java.util.List;
// why this next one exists
import java.util.Map;

class Foo {}
";
    let tree = parsed(source);
    // Neither import has an unbroken run of 2+ with the comment between
    // them, so nothing folds.
    assert!(foldable_ranges(&tree, source, Language::Java).is_empty());
}

#[test]
fn kotlin_folds_a_run_of_consecutive_imports() {
    let mut parser = IncrementalParser::new(fg_core::Language::Kotlin).expect("a bundled grammar must load");
    // `class Foo {}` is a single-line (empty) body, same "not foldable on
    // its own" convention the Java import-run tests use above — keeps
    // this test focused on import-block folding, not class-body folding
    // (covered separately by `kotlin_folds_a_class_body_a_method_body_
    // and_a_control_flow_block` below).
    let source = "\
package com.example

import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PostMapping

class Foo {}
";
    let tree = parser.parse(source).clone();
    let ranges = foldable_ranges(&tree, source, fg_core::Language::Kotlin);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].marker_line, 2);
    assert!(hidden(source, &ranges[0]).contains("PostMapping"));
}

#[test]
fn kotlin_a_lone_import_is_not_foldable() {
    let mut parser = IncrementalParser::new(fg_core::Language::Kotlin).expect("a bundled grammar must load");
    let source = "import org.springframework.web.bind.annotation.GetMapping\n\nclass Foo {}\n";
    let tree = parser.parse(source).clone();
    assert!(foldable_ranges(&tree, source, fg_core::Language::Kotlin).is_empty());
}

#[test]
fn kotlin_folds_a_class_body_a_method_body_and_a_control_flow_block() {
    let mut parser = IncrementalParser::new(fg_core::Language::Kotlin).expect("a bundled grammar must load");
    let source = "\
class Foo {
    fun bar(): Int {
        if (true) {
            return 1
        }
        return 0
    }
}
";
    let tree = parser.parse(source).clone();
    let ranges = foldable_ranges(&tree, source, fg_core::Language::Kotlin);

    // class body (0), method body (1), if-block (2) — three nested folds.
    assert_eq!(ranges.iter().map(|r| r.marker_line).collect::<Vec<_>>(), vec![0, 1, 2]);
    assert!(hidden(source, &ranges[0]).contains("fun bar"));
    assert!(hidden(source, &ranges[1]).contains("if (true)"));
    assert!(hidden(source, &ranges[2]).contains("return 1"));
    assert!(!hidden(source, &ranges[2]).contains("return 0"));
}

#[test]
fn kotlin_folds_an_enum_class_body() {
    let mut parser = IncrementalParser::new(fg_core::Language::Kotlin).expect("a bundled grammar must load");
    let source = "enum class Color {\n    RED, GREEN\n}\n";
    let tree = parser.parse(source).clone();
    let ranges = foldable_ranges(&tree, source, fg_core::Language::Kotlin);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].marker_line, 0);
    assert!(hidden(source, &ranges[0]).contains("RED"));
}

#[test]
fn kotlin_folds_a_multi_line_block_comment() {
    let mut parser = IncrementalParser::new(fg_core::Language::Kotlin).expect("a bundled grammar must load");
    let source = "/*\n * docs\n */\nclass Foo {}\n";
    let tree = parser.parse(source).clone();
    let ranges = foldable_ranges(&tree, source, fg_core::Language::Kotlin);
    // The block comment folds; `class Foo {}` is one line, so it doesn't.
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].marker_line, 0);
    assert!(hidden(source, &ranges[0]).contains("docs"));
}
