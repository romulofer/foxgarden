//! Ctrl+/ line-comment toggling.

use super::super::*;
use super::common::*;

#[test]
fn ctrl_slash_comments_the_current_line() {
    let (_dir, mut doc) = open_fixture("foo();", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // Ctrl+/ reads the *persisted* selection (see `show`'s comment on
    // this interception), so — like wrap-selection/Tab/Alt+Arrow — it
    // needs `focused_frame_with_selection`'s warm-up frame, not the
    // single dry frame `focused_frame` gives; a collapsed 0..0
    // selection is just "cursor at the start, nothing selected".
    focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![command_key_event(egui::Key::Slash)]);

    assert_eq!(doc.buffer.to_string(), "// foo();");
}

#[test]
fn ctrl_slash_uncomments_an_already_commented_line() {
    let (_dir, mut doc) = open_fixture("// foo();", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![command_key_event(egui::Key::Slash)]);

    assert_eq!(doc.buffer.to_string(), "foo();");
}

#[test]
fn ctrl_slash_toggles_every_line_a_selection_touches() {
    let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // All of "foo" and all of "bar" (chars 0..7).
    focused_frame_with_selection(&mut doc, &mut parser, 0..7, vec![command_key_event(egui::Key::Slash)]);

    assert_eq!(doc.buffer.to_string(), "// foo\n// bar");
}
