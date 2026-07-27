//! Dot-completion tests: candidate resolution (`dot_completion_candidates`/
//! `java_dot_completion_candidates`/`kotlin_dot_completion_candidates`)
//! called directly, plus (below) real multi-frame trigger tests via
//! `typing_session` that catch bugs in *when* `show` decides to call that
//! resolution, which direct calls can't.

use super::super::*;
use super::common::*;
use fg_core::Language;

fn tree_of(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Java);
    parser.parse(source).clone()
}

fn labels(items: &[CompletionItem]) -> Vec<String> {
    let mut out: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    out.sort_unstable();
    out
}

#[test]
fn this_dot_offers_every_member_of_the_enclosing_class_unfiltered() {
    let source = "class Foo {\n    private int x;\n    private static final int MAX = 1;\n    private void helper() {\n    }\n    public void run() {\n    }\n}\n";
    let tree = tree_of(source);
    let cursor = source.find("helper").unwrap();

    let items =
        java_dot_completion_candidates(&tree, source, cursor, "this", None).expect("this. should resolve inside its own class");

    assert_eq!(labels(&items), vec!["MAX", "helper", "run", "x"]);
}

#[test]
fn super_dot_offers_the_superclasss_members_from_the_project_tree() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Base.java"),
        "public class Base {\n    private int baseField;\n    public void run() {\n    }\n}\n",
    )
    .unwrap();
    let foo_source = "public class Foo extends Base {\n    public void go() {\n    }\n}\n";
    let tree = tree_of(foo_source);
    let cursor = foo_source.find("go").unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let items = java_dot_completion_candidates(&tree, foo_source, cursor, "super", Some(&project))
        .expect("super. should find Base.java in the project tree");

    assert_eq!(labels(&items), vec!["baseField", "run"]);
}

#[test]
fn super_dot_reports_none_when_the_superclass_has_no_project_file() {
    let dir = tempfile::tempdir().unwrap();
    let foo_source = "public class Foo extends SomeJdkThing {\n    public void go() {\n    }\n}\n";
    let tree = tree_of(foo_source);
    let cursor = foo_source.find("go").unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    assert!(java_dot_completion_candidates(&tree, foo_source, cursor, "super", Some(&project)).is_none());
}

#[test]
fn a_local_variable_typed_as_another_project_class_offers_that_classs_public_members() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Bar.java"),
        "public class Bar {\n    public void baz() {\n    }\n    private void secret() {\n    }\n}\n",
    )
    .unwrap();
    let foo_source = "class Foo {\n    void run() {\n        Bar b = new Bar();\n        int x = 0;\n    }\n}\n";
    let tree = tree_of(foo_source);
    let cursor = foo_source.find("int x").unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let items = java_dot_completion_candidates(&tree, foo_source, cursor, "b", Some(&project))
        .expect("a local typed as an in-project class should resolve");

    // `secret` is private on `Bar` — an external receiver keeps
    // `methods_in_type`'s existing visibility filtering, unlike `this.`/
    // `super.`'s unfiltered listing.
    assert_eq!(labels(&items), vec!["baz"]);
}

#[test]
fn an_external_receivers_one_level_supertype_is_included() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Baz.java"), "public class Baz {\n    public void inherited() {\n    }\n}\n").unwrap();
    std::fs::write(
        dir.path().join("Bar.java"),
        "public class Bar extends Baz {\n    public void baz() {\n    }\n}\n",
    )
    .unwrap();
    let foo_source = "class Foo {\n    void run() {\n        Bar b = new Bar();\n        int x = 0;\n    }\n}\n";
    let tree = tree_of(foo_source);
    let cursor = foo_source.find("int x").unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let items = java_dot_completion_candidates(&tree, foo_source, cursor, "b", Some(&project))
        .expect("a local typed as an in-project class should resolve");

    assert_eq!(labels(&items), vec!["baz", "inherited"]);
}

#[test]
fn a_jdk_typed_local_produces_no_candidates() {
    let foo_source = "class Foo {\n    void run() {\n        String s = \"hi\";\n        int x = 0;\n    }\n}\n";
    let tree = tree_of(foo_source);
    let cursor = foo_source.find("int x").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    assert!(java_dot_completion_candidates(&tree, foo_source, cursor, "s", Some(&project)).is_none());
}

#[test]
fn an_undeclared_receiver_produces_no_candidates() {
    let foo_source = "class Foo {\n    void run() {\n        int x = 0;\n    }\n}\n";
    let tree = tree_of(foo_source);
    let cursor = foo_source.find("int x").unwrap();

    assert!(java_dot_completion_candidates(&tree, foo_source, cursor, "neverDeclared", None).is_none());
}

#[test]
fn this_dot_reports_none_outside_any_class() {
    let source = "// just a comment\n";
    let tree = tree_of(source);
    assert!(java_dot_completion_candidates(&tree, source, 0, "this", None).is_none());
}

#[test]
fn dispatcher_routes_java_to_java_dot_completion_candidates() {
    let source = "class Foo {\n    private void helper() {\n    }\n    void run() {\n        int x = 0;\n    }\n}\n";
    let tree = tree_of(source);
    let cursor = source.find("int x").unwrap();

    let items = dot_completion_candidates(Language::Java, &tree, source, cursor, "this", None).expect("Java should dispatch to java_dot_completion_candidates");
    assert_eq!(labels(&items), vec!["helper", "run"]);
}

fn kotlin_tree_of(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Kotlin);
    parser.parse(source).clone()
}

#[test]
fn kotlin_this_dot_offers_every_member_of_the_enclosing_class_unfiltered() {
    let source = "class Foo {\n    val x: Int = 0\n    private fun helper() {\n    }\n    fun run() {\n    }\n}\n";
    let tree = kotlin_tree_of(source);
    let cursor = source.find("helper").unwrap();

    let items =
        kotlin_dot_completion_candidates(&tree, source, cursor, "this", None).expect("this. should resolve inside its own class");

    assert_eq!(labels(&items), vec!["helper", "run", "x"]);
}

#[test]
fn kotlin_super_dot_offers_the_superclasss_members_from_the_project_tree() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Base.kt"),
        "open class Base {\n    val baseField: Int = 0\n    fun run() {\n    }\n}\n",
    )
    .unwrap();
    let foo_source = "class Foo : Base() {\n    fun go() {\n    }\n}\n";
    let tree = kotlin_tree_of(foo_source);
    let cursor = foo_source.find("go").unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let items = kotlin_dot_completion_candidates(&tree, foo_source, cursor, "super", Some(&project))
        .expect("super. should find Base.kt in the project tree");

    assert_eq!(labels(&items), vec!["baseField", "run"]);
}

#[test]
fn kotlin_super_dot_reports_none_when_the_superclass_has_no_project_file() {
    let dir = tempfile::tempdir().unwrap();
    let foo_source = "class Foo : SomeStdlibThing() {\n    fun go() {\n    }\n}\n";
    let tree = kotlin_tree_of(foo_source);
    let cursor = foo_source.find("go").unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    assert!(kotlin_dot_completion_candidates(&tree, foo_source, cursor, "super", Some(&project)).is_none());
}

#[test]
fn kotlin_a_local_variable_typed_as_another_project_class_offers_that_classs_public_members() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Bar.kt"),
        "class Bar {\n    fun baz() {\n    }\n    private fun secret() {\n    }\n}\n",
    )
    .unwrap();
    let foo_source = "class Foo {\n    fun run() {\n        val b: Bar = Bar()\n        val x = 0\n    }\n}\n";
    let tree = kotlin_tree_of(foo_source);
    let cursor = foo_source.find("val x").unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let items = kotlin_dot_completion_candidates(&tree, foo_source, cursor, "b", Some(&project))
        .expect("a local typed as an in-project class should resolve");

    // `secret` is private on `Bar` — an external receiver keeps
    // `kotlin_functions_in_type`'s existing visibility filtering, unlike
    // `this.`/`super.`'s unfiltered listing.
    assert_eq!(labels(&items), vec!["baz"]);
}

#[test]
fn kotlin_an_external_receivers_one_level_supertype_is_included() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Baz.kt"), "open class Baz {\n    fun inherited() {\n    }\n}\n").unwrap();
    std::fs::write(
        dir.path().join("Bar.kt"),
        "class Bar : Baz() {\n    fun baz() {\n    }\n}\n",
    )
    .unwrap();
    let foo_source = "class Foo {\n    fun run() {\n        val b: Bar = Bar()\n        val x = 0\n    }\n}\n";
    let tree = kotlin_tree_of(foo_source);
    let cursor = foo_source.find("val x").unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let items = kotlin_dot_completion_candidates(&tree, foo_source, cursor, "b", Some(&project))
        .expect("a local typed as an in-project class should resolve");

    assert_eq!(labels(&items), vec!["baz", "inherited"]);
}

#[test]
fn kotlin_a_constructor_promoted_property_is_offered_via_this() {
    let source = "class Foo(val x: Int) {\n    fun run() {\n    }\n}\n";
    let tree = kotlin_tree_of(source);
    let cursor = source.find("run").unwrap();

    let items =
        kotlin_dot_completion_candidates(&tree, source, cursor, "this", None).expect("this. should resolve inside its own class");

    assert_eq!(labels(&items), vec!["run", "x"]);
}

#[test]
fn kotlin_a_stdlib_typed_local_produces_no_candidates() {
    let foo_source = "class Foo {\n    fun run() {\n        val s: String = \"hi\"\n        val x = 0\n    }\n}\n";
    let tree = kotlin_tree_of(foo_source);
    let cursor = foo_source.find("val x").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    assert!(kotlin_dot_completion_candidates(&tree, foo_source, cursor, "s", Some(&project)).is_none());
}

#[test]
fn kotlin_a_non_constructor_call_inferred_local_produces_no_candidates() {
    let foo_source = "class Foo {\n    fun run() {\n        val b = someFunction()\n        val x = 0\n    }\n}\n";
    let tree = kotlin_tree_of(foo_source);
    let cursor = foo_source.find("val x").unwrap();

    assert!(kotlin_dot_completion_candidates(&tree, foo_source, cursor, "b", None).is_none());
}

#[test]
fn kotlin_an_undeclared_receiver_produces_no_candidates() {
    let source = "class Foo {\n    fun run() {\n        val x = 0\n    }\n}\n";
    let tree = kotlin_tree_of(source);
    let cursor = source.find("val x").unwrap();

    assert!(kotlin_dot_completion_candidates(&tree, source, cursor, "neverDeclared", None).is_none());
}

#[test]
fn kotlin_this_dot_reports_none_outside_any_class() {
    let source = "// just a comment\n";
    let tree = kotlin_tree_of(source);
    assert!(kotlin_dot_completion_candidates(&tree, source, 0, "this", None).is_none());
}

#[test]
fn dispatcher_routes_kotlin_to_kotlin_dot_completion_candidates() {
    let source = "class Foo {\n    private fun helper() {\n    }\n    fun run() {\n        val x = 0\n    }\n}\n";
    let tree = kotlin_tree_of(source);
    let cursor = source.find("val x").unwrap();

    let items = dot_completion_candidates(Language::Kotlin, &tree, source, cursor, "this", None)
        .expect("Kotlin should dispatch to kotlin_dot_completion_candidates");
    assert_eq!(labels(&items), vec!["helper", "run"]);
}

// `visible_labels` assumes ASCII fixture text, so a char offset doubles
// as a byte offset without needing `char_to_byte`.

fn visible_labels(state: &CompletionState, text: &str, cursor_byte: usize) -> Vec<String> {
    let mut labels: Vec<String> = state.visible(text, cursor_byte).iter().map(|i| i.label.clone()).collect();
    labels.sort_unstable();
    labels
}

#[test]
fn java_typing_super_dot_one_character_at_a_time_opens_dot_completion_immediately() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Base.java"),
        "public class Base {\n    public void run() {\n    }\n}\n",
    )
    .unwrap();
    let before = "public class Foo extends Base {\n    void go() {\n        ";
    let after = "\n    }\n}\n";
    let source = format!("{before}{after}");
    let (_dir2, mut doc) = open_fixture(&source, "Foo.java");
    let mut parser = parsed(Language::Java, &source);
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();
    let mut completion = None;

    let initial_caret = before.len();
    let frames: Vec<Vec<egui::Event>> = "super.".chars().map(|c| vec![egui::Event::Text(c.to_string())]).collect();

    typing_session(&mut doc, &mut parser, Some(&project), &mut completion, initial_caret, frames);

    let state = completion
        .expect("typing \"super.\" one character at a time, with nothing else in between, should leave dot-completion open");
    let text = doc.buffer.to_string();
    let cursor_byte = initial_caret + "super.".len();
    assert_eq!(visible_labels(&state, &text, cursor_byte), vec!["run"]);
}

#[test]
fn kotlin_typing_super_dot_one_character_at_a_time_opens_dot_completion_immediately() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Base.kt"), "open class Base {\n    fun run() {\n    }\n}\n").unwrap();
    let before = "class Foo : Base() {\n    fun go() {\n        ";
    let after = "\n    }\n}\n";
    let source = format!("{before}{after}");
    let (_dir2, mut doc) = open_fixture(&source, "Foo.kt");
    let mut parser = parsed(Language::Kotlin, &source);
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();
    let mut completion = None;

    let initial_caret = before.len();
    let frames: Vec<Vec<egui::Event>> = "super.".chars().map(|c| vec![egui::Event::Text(c.to_string())]).collect();

    typing_session(&mut doc, &mut parser, Some(&project), &mut completion, initial_caret, frames);

    let state = completion
        .expect("typing \"super.\" one character at a time, with nothing else in between, should leave dot-completion open");
    let text = doc.buffer.to_string();
    let cursor_byte = initial_caret + "super.".len();
    assert_eq!(visible_labels(&state, &text, cursor_byte), vec!["run"]);
}

#[test]
fn kotlin_typing_a_single_char_receiver_then_dot_opens_dot_completion() {
    // `b` is one character, so word-completion's own 2+-char trigger
    // never opens while typing it — isolates whether a bare `b.` needs
    // the same fix as `super.` above, or fails for an unrelated reason.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Bar.kt"), "class Bar {\n    fun baz() {\n    }\n}\n").unwrap();
    let before = "class Foo {\n    fun go() {\n        val b: Bar = Bar()\n        b";
    let after = "\n    }\n}\n";
    let source = format!("{before}{after}");
    let (_dir2, mut doc) = open_fixture(&source, "Foo.kt");
    let mut parser = parsed(Language::Kotlin, &source);
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();
    let mut completion = None;

    let initial_caret = before.len();
    let frames = vec![vec![egui::Event::Text(".".to_string())]];

    typing_session(&mut doc, &mut parser, Some(&project), &mut completion, initial_caret, frames);

    let state = completion.expect("typing \".\" right after an already-present \"b\" should open dot-completion");
    let text = doc.buffer.to_string();
    let cursor_byte = initial_caret + 1;
    assert_eq!(visible_labels(&state, &text, cursor_byte), vec!["baz"]);
}

#[test]
fn java_typing_a_single_char_receiver_then_dot_opens_dot_completion() {
    // Java counterpart of the Kotlin test above.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Bar.java"),
        "public class Bar {\n    public void baz() {\n    }\n}\n",
    )
    .unwrap();
    let before = "class Foo {\n    void go() {\n        Bar b = new Bar();\n        b";
    let after = "\n    }\n}\n";
    let source = format!("{before}{after}");
    let (_dir2, mut doc) = open_fixture(&source, "Foo.java");
    let mut parser = parsed(Language::Java, &source);
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();
    let mut completion = None;

    let initial_caret = before.len();
    let frames = vec![vec![egui::Event::Text(".".to_string())]];

    typing_session(&mut doc, &mut parser, Some(&project), &mut completion, initial_caret, frames);

    let state = completion.expect("typing \".\" right after an already-present \"b\" should open dot-completion");
    let text = doc.buffer.to_string();
    let cursor_byte = initial_caret + 1;
    assert_eq!(visible_labels(&state, &text, cursor_byte), vec!["baz"]);
}
