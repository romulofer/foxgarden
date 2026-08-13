//! Tab-trigger live-template expansion (`templates.rs`) as wired into `widget::show`'s own Tab interception.

use super::super::*;
use super::common_test::*;
use fg_core::Language;

#[test]
fn tab_after_a_known_java_trigger_word_expands_the_live_template() {
    let (_dir, mut doc) = open_fixture("sout", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    // Collapsed cursor (4..4) right after "sout" — not a real
    // selection, just how `focused_frame_with_selection` positions a
    // bare cursor via the persisted `TextEditState` the Tab
    // interception reads.
    focused_frame_with_selection(&mut doc, &mut parser, 4..4, vec![key_event(egui::Key::Tab)]);

    assert_eq!(doc.buffer.to_string(), "System.out.println();");
}

#[test]
fn tab_after_a_known_kotlin_trigger_word_expands_the_kotlin_template() {
    let (_dir, mut doc) = open_fixture("sout", "Hello.kt");
    let mut parser = parsed(Language::Kotlin, &doc.buffer.to_string());

    focused_frame_with_selection(&mut doc, &mut parser, 4..4, vec![key_event(egui::Key::Tab)]);

    assert_eq!(doc.buffer.to_string(), "println()");
}

#[test]
fn tab_after_an_unknown_word_falls_through_to_normal_spaces_indentation() {
    let (_dir, mut doc) = open_fixture("xyz", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    focused_frame_with_selection(&mut doc, &mut parser, 3..3, vec![key_event(egui::Key::Tab)]);

    // No template matches "xyz" — default (spaces) indentation applies
    // instead, same as plain Tab anywhere else with no selection.
    assert_eq!(doc.buffer.to_string(), "xyz    ");
}

#[test]
fn tab_after_a_trigger_word_expands_even_in_tabs_mode() {
    // Live-template expansion is orthogonal to the tabs-vs-spaces
    // setting — it must win even when `use_tabs` would otherwise leave
    // plain Tab un-intercepted.
    let (_dir, mut doc) = open_fixture("sout", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let tabs_mode = IndentSettings {
        use_tabs: true,
        width: 4,
    };
    focused_frame_with_indent_settings_and_selection(
        &mut doc,
        &mut parser,
        tabs_mode,
        4..4,
        vec![key_event(egui::Key::Tab)],
    );

    assert_eq!(doc.buffer.to_string(), "System.out.println();");
}
