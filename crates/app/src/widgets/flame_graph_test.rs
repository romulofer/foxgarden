
use super::*;

fn node(name: &str, total: u64, children: Vec<FlameNode>) -> FlameNode {
    FlameNode {
        name: name.to_string(),
        total,
        children,
    }
}

fn area() -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 500.0))
}

#[test]
fn layout_gives_the_focus_node_full_width_at_the_top() {
    let root = node("all", 10, vec![node("a", 6, vec![]), node("b", 4, vec![])]);
    let frames = layout_frames(&root, &[], area());

    let root_frame = &frames[0];
    assert_eq!(root_frame.name, "all");
    assert!((root_frame.rect.width() - 100.0).abs() < 0.01, "focus spans full width");
    assert!((root_frame.rect.top() - 0.0).abs() < 0.01, "focus is the top row");
}

#[test]
fn children_widths_are_proportional_to_their_sample_counts() {
    let root = node("all", 10, vec![node("a", 6, vec![]), node("b", 4, vec![])]);
    let frames = layout_frames(&root, &[], area());

    let a = frames.iter().find(|f| f.name == "a").unwrap();
    let b = frames.iter().find(|f| f.name == "b").unwrap();
    assert!((a.rect.width() - 60.0).abs() < 0.01, "6/10 of 100");
    assert!((b.rect.width() - 40.0).abs() < 0.01, "4/10 of 100");
    // b starts exactly where a ends — no gaps or overlaps.
    assert!((b.rect.left() - a.rect.right()).abs() < 0.01);
    // Children sit one row below their parent.
    assert!((a.rect.top() - ROW_HEIGHT).abs() < 0.01);
}

#[test]
fn a_click_target_path_addresses_the_whole_tree() {
    let root = node("all", 10, vec![node("a", 10, vec![node("c", 10, vec![])])]);
    let frames = layout_frames(&root, &[], area());
    let c = frames.iter().find(|f| f.name == "c").unwrap();
    assert_eq!(c.path, vec![0, 0], "path is the child-index route from the root");
}

#[test]
fn layout_prepends_the_focus_path_to_every_frame() {
    // Zoomed into node `a` (index 0): its subtree's paths must still be
    // rooted at the real tree, so a further click resolves correctly.
    let a = node("a", 10, vec![node("c", 10, vec![])]);
    let frames = layout_frames(&a, &[0], area());
    assert_eq!(frames[0].path, vec![0], "the focus node keeps its own path");
    let c = frames.iter().find(|f| f.name == "c").unwrap();
    assert_eq!(c.path, vec![0, 0]);
}

#[test]
fn tiny_frames_are_culled() {
    // A child that's 1/1000 of the width falls below MIN_FRAME_WIDTH.
    let root = node("all", 1000, vec![node("big", 999, vec![]), node("tiny", 1, vec![])]);
    let frames = layout_frames(&root, &[], area());
    assert!(frames.iter().any(|f| f.name == "big"));
    assert!(!frames.iter().any(|f| f.name == "tiny"), "sub-pixel frame dropped");
}

#[test]
fn resolve_focus_falls_back_and_flags_a_stale_path() {
    let root = node("all", 10, vec![node("a", 10, vec![])]);
    // Index 5 doesn't exist — a stale focus over a changed tree.
    let (node, valid, intact) = resolve_focus(&root, &[5]);
    assert!(!intact);
    assert!(valid.is_empty());
    assert_eq!(node.name, "all", "falls back to the deepest reachable node");
}

#[test]
fn resolve_focus_follows_a_valid_path() {
    let root = node("all", 10, vec![node("a", 10, vec![node("c", 10, vec![])])]);
    let (node, valid, intact) = resolve_focus(&root, &[0, 0]);
    assert!(intact);
    assert_eq!(valid, vec![0, 0]);
    assert_eq!(node.name, "c");
}

#[test]
fn max_depth_counts_reachable_rows() {
    let root = node("all", 10, vec![node("a", 10, vec![node("c", 10, vec![])])]);
    // all -> a -> c is two levels below the root.
    assert_eq!(max_depth(&root, 100.0), 2);
}

#[test]
fn frame_color_is_deterministic_per_name() {
    assert_eq!(frame_color("Spin.main"), frame_color("Spin.main"));
}

#[test]
fn elide_keeps_short_names_and_truncates_long_ones() {
    assert_eq!(elide("main", 200.0), "main");
    let long = elide("com.example.very.long.ClassName.method", 40.0);
    assert!(long.ends_with('…'));
    assert!(long.chars().count() < "com.example.very.long.ClassName.method".chars().count());
}
