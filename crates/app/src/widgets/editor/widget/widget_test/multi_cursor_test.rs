//! Ctrl+D multi-cursor: applying an edit at every active cursor, and the various ways extra selections collapse back to one.

use super::common_test::*;
use fg_core::Language;

#[test]
fn multi_cursor_typed_edit_applies_at_every_active_cursor() {
    let (_dir, mut doc) = open_fixture("abcde", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    // Primary cursor starts at 0 (a fresh widget's default cursor, from
    // the warm-up frame `focused_frame_with_extra_selections` runs
    // before setting these); the two extras sit at char indices 2 and 4.
    focused_frame_with_extra_selections(
        &mut doc,
        &mut parser,
        vec![2..2, 4..4],
        vec![egui::Event::Text("Y".to_string())],
    );

    assert_eq!(doc.buffer.to_string(), "YabYcdYe");
    assert_eq!(doc.extra_selections, vec![4..4, 7..7]);
}

#[test]
fn arrow_key_collapses_extra_selections() {
    let (_dir, mut doc) = open_fixture("abcde", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    doc.extra_selections = vec![2..2, 4..4];

    focused_frame(&mut doc, &mut parser, vec![key_event(egui::Key::ArrowLeft)]);

    assert!(doc.extra_selections.is_empty());
    // Arrow keys just navigate — the buffer itself is untouched.
    assert_eq!(doc.buffer.to_string(), "abcde");
}

#[test]
fn non_intercepted_mutating_key_collapses_extra_selections_via_safety_net() {
    let (_dir, mut doc) = open_fixture("abc", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    // a genuine one-caret Vec<Range<usize>>, not a range of a Vec
    #[allow(clippy::single_range_in_vec_init)]
    let one_caret = vec![1..1];
    doc.extra_selections = one_caret;

    // Tab isn't in the intercepted-event set, so it reaches egui's own
    // single-cursor logic (code editors call `.lock_focus(true)`, which
    // makes Tab insert a literal tab character instead of moving focus)
    // and edits the primary cursor alone — the safety net must then
    // notice `extra_selections` is now stale and clear it.
    focused_frame(&mut doc, &mut parser, vec![key_event(egui::Key::Tab)]);

    assert!(doc.extra_selections.is_empty());
    assert_eq!(doc.buffer.to_string(), "\tabc");
}
