use super::*;

#[test]
fn parse_collapsed_builds_an_inclusive_sample_tree() {
    // Two stacks sharing a common a;b prefix, plus a standalone one.
    let text = "a;b;c 5\na;b;d 3\ne 2\n";
    let root = parse_collapsed(text);

    assert_eq!(root.name, "all");
    assert_eq!(root.total, 10);

    let a = root.children.iter().find(|c| c.name == "a").expect("a present");
    assert_eq!(a.total, 8, "a is inclusive of both a;b;c and a;b;d");
    let b = a.children.iter().find(|c| c.name == "b").expect("b present");
    assert_eq!(b.total, 8);
    assert_eq!(b.children.iter().find(|c| c.name == "c").unwrap().total, 5);
    assert_eq!(b.children.iter().find(|c| c.name == "d").unwrap().total, 3);

    assert_eq!(root.children.iter().find(|c| c.name == "e").unwrap().total, 2);
}

#[test]
fn parse_collapsed_preserves_first_seen_child_order() {
    // Deterministic left-to-right order matters for a stable paint.
    let root = parse_collapsed("z 1\nm 1\na 1\n");
    let names: Vec<&str> = root.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["z", "m", "a"]);
}

#[test]
fn parse_collapsed_accumulates_repeated_identical_stacks() {
    let root = parse_collapsed("a;b 2\na;b 3\n");
    let a = &root.children[0];
    assert_eq!(a.total, 5);
    assert_eq!(a.children[0].total, 5);
    assert_eq!(a.children.len(), 1, "the second a;b folds into the same node");
}

#[test]
fn parse_collapsed_keeps_frame_labels_that_contain_spaces() {
    // Only the trailing count is split off; a spaced label survives whole.
    let root = parse_collapsed("java/lang/Thread.run [unknown Java] 4\n");
    let outer = &root.children[0];
    assert_eq!(outer.name, "java/lang/Thread.run [unknown Java]");
    assert_eq!(outer.total, 4);
}

#[test]
fn parse_collapsed_skips_malformed_lines() {
    let root = parse_collapsed("\nno_count_here\na;b notanumber\n; 5\nvalid 7\n");
    // Only `valid 7` is well-formed; the empty-stack `; 5` is dropped too.
    assert_eq!(root.total, 7);
    assert_eq!(root.children.len(), 1);
    assert_eq!(root.children[0].name, "valid");
}
