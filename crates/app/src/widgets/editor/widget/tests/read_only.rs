//! Read-only mode: every edit-shaped request is a no-op, plus `is_mutating_event`, the pure classifier read-only mode is built on.

use super::super::*;
use super::common::*;
use fg_core::Language;

#[test]
fn read_only_doc_ignores_typed_text() {
    let (_dir, mut doc) = open_fixture("foo", "notes.txt");
    doc.read_only = true;
    let mut parser: Option<IncrementalParser> = None;

    focused_frame(&mut doc, &mut parser, vec![egui::Event::Text("x".to_string())]);

    assert_eq!(doc.buffer.to_string(), "foo");
}

#[test]
fn read_only_doc_ignores_backspace() {
    let (_dir, mut doc) = open_fixture("foo", "notes.txt");
    doc.read_only = true;
    let mut parser: Option<IncrementalParser> = None;

    focused_frame(&mut doc, &mut parser, vec![key_event(egui::Key::Backspace)]);

    assert_eq!(doc.buffer.to_string(), "foo");
}

#[test]
fn read_only_doc_ignores_ctrl_j_join_lines() {
    let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
    doc.read_only = true;
    let mut parser: Option<IncrementalParser> = None;

    focused_frame(&mut doc, &mut parser, vec![command_key_event(egui::Key::J)]);

    assert_eq!(doc.buffer.to_string(), "foo\nbar");
}

#[test]
fn read_only_doc_ignores_generate_request() {
    let (_dir, mut doc) = open_fixture("class Foo {\n    private int x;\n}\n", "Foo.java");
    doc.read_only = true;
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let original = doc.buffer.to_string();

    focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Both), &mut None, vec![]);

    assert_eq!(doc.buffer.to_string(), original);
}

#[test]
fn is_mutating_event_blocks_typed_text_paste_and_cut() {
    assert!(is_mutating_event(&egui::Event::Text("a".to_string())));
    assert!(is_mutating_event(&egui::Event::Paste("a".to_string())));
    assert!(is_mutating_event(&egui::Event::Cut));
}

#[test]
fn is_mutating_event_blocks_known_editing_shortcuts() {
    for key in [
        egui::Key::Backspace,
        egui::Key::Delete,
        egui::Key::Tab,
        egui::Key::Enter,
        egui::Key::J,
        egui::Key::G,
        egui::Key::Slash,
        egui::Key::U,
        egui::Key::L,
        egui::Key::Z,
        egui::Key::Y,
    ] {
        assert!(
            is_mutating_event(&key_event(key)),
            "{key:?} should be treated as mutating"
        );
    }
    assert!(is_mutating_event(&alt_key_event(egui::Key::ArrowUp)));
    assert!(is_mutating_event(&alt_key_event(egui::Key::ArrowDown)));
}

#[test]
fn is_mutating_event_allows_navigation_and_copy() {
    for key in [
        egui::Key::ArrowUp,
        egui::Key::ArrowDown,
        egui::Key::ArrowLeft,
        egui::Key::ArrowRight,
        egui::Key::Home,
        egui::Key::End,
        egui::Key::PageUp,
        egui::Key::PageDown,
        egui::Key::Escape,
        egui::Key::D, // Ctrl+D occurrence select — not a mutation
        egui::Key::W, // Ctrl+W/Ctrl+Shift+W expand/shrink selection — not a mutation
    ] {
        assert!(
            !is_mutating_event(&key_event(key)),
            "{key:?} should not be treated as mutating"
        );
    }
    assert!(!is_mutating_event(&egui::Event::Copy));
}

#[test]
fn read_only_doc_ignores_sort_lines_request() {
    let (_dir, mut doc) = open_fixture("banana\napple", "notes.txt");
    doc.read_only = true;
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection_and_line_op_request(&mut doc, &mut parser, 0..12, true, false);

    assert_eq!(doc.buffer.to_string(), "banana\napple");
}

#[test]
fn read_only_doc_ignores_generate_method_request() {
    let (_dir, mut doc) = open_fixture("class Foo {\n    private int x;\n}\n", "Foo.java");
    doc.read_only = true;
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let before = doc.buffer.to_string();

    focused_frame_with_generate_method_request(&mut doc, &mut parser, Some(GenerateMethodKind::Constructor), &mut None);

    assert_eq!(doc.buffer.to_string(), before);
}
