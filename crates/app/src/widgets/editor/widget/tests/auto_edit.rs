//! Tests for the pure text-transform interceptions in `auto_edit.rs`: wrap-selection, Home, Tab/Shift+Tab indent, Alt+Arrow move/duplicate line, Ctrl+J join, case conversion, sort/unique lines, and auto-pair.

use super::super::*;
use super::common::*;
use fg_core::Language;

#[test]
fn typing_a_bracket_over_a_selection_wraps_it_instead_of_replacing_it() {
    let (_dir, mut doc) = open_fixture("foo bar baz", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // "bar" is chars 4..7.
    focused_frame_with_selection(&mut doc, &mut parser, 4..7, vec![egui::Event::Text("(".to_string())]);

    assert_eq!(doc.buffer.to_string(), "foo (bar) baz");
}

#[test]
fn typing_an_angle_bracket_over_a_selection_wraps_it_too() {
    let (_dir, mut doc) = open_fixture("List Item", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // "Item" is chars 5..9.
    focused_frame_with_selection(&mut doc, &mut parser, 5..9, vec![egui::Event::Text("<".to_string())]);

    assert_eq!(doc.buffer.to_string(), "List <Item>");
}

#[test]
fn home_from_mid_line_goes_to_first_non_whitespace() {
    let (_dir, mut doc) = open_fixture("    foo", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // Collapsed cursor (6..6) mid "foo".
    let range =
        focused_frame_with_selection_returning_cursor(&mut doc, &mut parser, 6..6, vec![key_event(egui::Key::Home)]);

    assert_eq!(range.primary, 4);
    assert!(
        range.is_collapsed(),
        "Home with no selection active must not create one"
    );
    assert_eq!(doc.buffer.to_string(), "    foo", "Home must never change the buffer");
}

#[test]
fn home_from_first_non_whitespace_goes_to_column_zero() {
    let (_dir, mut doc) = open_fixture("    foo", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    let range =
        focused_frame_with_selection_returning_cursor(&mut doc, &mut parser, 4..4, vec![key_event(egui::Key::Home)]);

    assert_eq!(range.primary, 0);
}

#[test]
fn shift_home_extends_the_selection_instead_of_collapsing_it() {
    let (_dir, mut doc) = open_fixture("    foo", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // Cursor at the end of "foo" (7), nothing selected yet.
    let range = focused_frame_with_selection_returning_cursor(
        &mut doc,
        &mut parser,
        7..7,
        vec![shift_key_event(egui::Key::Home)],
    );

    // Primary (the moving end) lands on first-non-whitespace; secondary
    // (the anchor) stays where Shift+Home started from.
    assert_eq!(range.primary, 4);
    assert_eq!(range.anchor, 7);
}

#[test]
fn shift_tab_dedents_every_line_a_multi_line_selection_touches() {
    let (_dir, mut doc) = open_fixture("    foo\n    bar\nbaz", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    // Selects all of "foo" and all of "bar" (chars 4..15), leaving
    // "baz" untouched.
    focused_frame_with_selection(&mut doc, &mut parser, 4..15, vec![shift_key_event(egui::Key::Tab)]);

    assert_eq!(doc.buffer.to_string(), "foo\nbar\nbaz");
}

#[test]
fn shift_tab_over_a_selection_does_not_delete_the_selected_text() {
    // Regression test for the bug this feature fixes: egui's own
    // Shift+Tab deletes the entire selection before dedenting, so a
    // multi-line selection lost all its text, not just its leading
    // whitespace.
    let (_dir, mut doc) = open_fixture("    foo\n    bar", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    // "oo\n    ba" (chars 5..14) — a selection that starts and ends
    // mid-line, not on either line's boundary. Dedent still strips each
    // touched *line's* leading whitespace in full (not just whatever
    // fell inside the selection), same as every other editor's
    // block-dedent — so both lines lose their 4-space indent, and none
    // of "foo"/"bar" is lost the way egui's own delete-then-dedent
    // default would lose it.
    focused_frame_with_selection(&mut doc, &mut parser, 5..14, vec![shift_key_event(egui::Key::Tab)]);

    assert_eq!(doc.buffer.to_string(), "foo\nbar");
}

#[test]
fn tab_over_a_selection_indents_every_line_instead_of_replacing_it() {
    let (_dir, mut doc) = open_fixture("foo\nbar", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    // All of "foo" and all of "bar" (chars 0..7).
    focused_frame_with_selection(&mut doc, &mut parser, 0..7, vec![key_event(egui::Key::Tab)]);

    assert_eq!(doc.buffer.to_string(), "    foo\n    bar");
}

#[test]
fn tab_with_no_selection_inserts_a_literal_tab_in_tabs_mode() {
    // Guards the un-intercepted path: with `use_tabs: true`, a literal
    // tab already *is* the configured indent unit, so plain Tab with a
    // collapsed cursor (no selection) must keep falling through to
    // egui's own behavior rather than being intercepted.
    let (_dir, mut doc) = open_fixture("abc", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let tabs_mode = IndentSettings {
        use_tabs: true,
        width: 4,
    };
    focused_frame_with_indent_settings(&mut doc, &mut parser, tabs_mode, vec![key_event(egui::Key::Tab)]);

    assert_eq!(doc.buffer.to_string(), "\tabc");
}

#[test]
fn tab_with_no_selection_inserts_spaces_in_spaces_mode() {
    // In "spaces" mode (the default), plain Tab with a collapsed cursor
    // must insert `width` spaces instead of the literal tab egui's own
    // `.code_editor()` handling would otherwise insert.
    let (_dir, mut doc) = open_fixture("abc", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let spaces_mode = IndentSettings {
        use_tabs: false,
        width: 4,
    };
    focused_frame_with_indent_settings(&mut doc, &mut parser, spaces_mode, vec![key_event(egui::Key::Tab)]);

    assert_eq!(doc.buffer.to_string(), "    abc");
}

#[test]
fn tab_with_no_selection_respects_a_configured_width() {
    let (_dir, mut doc) = open_fixture("abc", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let two_space_mode = IndentSettings {
        use_tabs: false,
        width: 2,
    };
    focused_frame_with_indent_settings(&mut doc, &mut parser, two_space_mode, vec![key_event(egui::Key::Tab)]);

    assert_eq!(doc.buffer.to_string(), "  abc");
}

#[test]
fn shift_tab_with_no_selection_dedents_the_current_line() {
    // Regression test: Shift+Tab with no selection used to be a silent
    // no-op — egui's `TextEdit` has no built-in dedent for a bare
    // Shift+Tab outside its `lock_focus` literal-tab-insert path.
    let (_dir, mut doc) = open_fixture("    abc", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let spaces_mode = IndentSettings {
        use_tabs: false,
        width: 4,
    };
    // Collapsed cursor mid-line (char 6, inside "abc").
    focused_frame_with_indent_settings_and_selection(
        &mut doc,
        &mut parser,
        spaces_mode,
        6..6,
        vec![shift_key_event(egui::Key::Tab)],
    );

    assert_eq!(doc.buffer.to_string(), "abc");
}

#[test]
fn shift_tab_with_no_selection_respects_tabs_mode() {
    let (_dir, mut doc) = open_fixture("\tabc", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let tabs_mode = IndentSettings {
        use_tabs: true,
        width: 4,
    };
    focused_frame_with_indent_settings_and_selection(
        &mut doc,
        &mut parser,
        tabs_mode,
        2..2,
        vec![shift_key_event(egui::Key::Tab)],
    );

    assert_eq!(doc.buffer.to_string(), "abc");
}

#[test]
fn alt_arrow_up_moves_the_current_line_up() {
    let (_dir, mut doc) = open_fixture("aaa\nbbb\nccc", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // Cursor at column 0 of "bbb" (char 4).
    focused_frame_with_selection(&mut doc, &mut parser, 4..4, vec![alt_key_event(egui::Key::ArrowUp)]);

    assert_eq!(doc.buffer.to_string(), "bbb\naaa\nccc");
}

#[test]
fn alt_arrow_down_moves_the_current_line_down() {
    let (_dir, mut doc) = open_fixture("aaa\nbbb\nccc", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // Cursor at column 0 of "aaa" (char 0).
    focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![alt_key_event(egui::Key::ArrowDown)]);

    assert_eq!(doc.buffer.to_string(), "bbb\naaa\nccc");
}

#[test]
fn alt_arrow_up_on_the_first_line_is_a_no_op() {
    let (_dir, mut doc) = open_fixture("aaa\nbbb", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![alt_key_event(egui::Key::ArrowUp)]);

    assert_eq!(doc.buffer.to_string(), "aaa\nbbb");
}

#[test]
fn alt_shift_arrow_down_duplicates_the_line_and_lands_on_the_copy() {
    let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection(
        &mut doc,
        &mut parser,
        0..0,
        vec![alt_shift_key_event(egui::Key::ArrowDown)],
    );

    assert_eq!(doc.buffer.to_string(), "foo\nfoo\nbar");
}

#[test]
fn alt_shift_arrow_up_duplicates_the_line_and_stays_on_the_original() {
    let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection(
        &mut doc,
        &mut parser,
        0..0,
        vec![alt_shift_key_event(egui::Key::ArrowUp)],
    );

    assert_eq!(doc.buffer.to_string(), "foo\nfoo\nbar");
}

#[test]
fn ctrl_j_joins_the_current_line_with_the_next_one() {
    let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // A fresh widget's default cursor sits somewhere on the first line
    // ("foo") — `join_lines` only cares which line the cursor is on,
    // not its exact column (see `join_lines_uses_cursor_position_
    // regardless_of_column_within_the_line` in `auto_edit.rs`).
    focused_frame(&mut doc, &mut parser, vec![command_key_event(egui::Key::J)]);

    assert_eq!(doc.buffer.to_string(), "foo bar");
}

#[test]
fn ctrl_j_on_the_last_line_is_a_no_op() {
    let (_dir, mut doc) = open_fixture("foo", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame(&mut doc, &mut parser, vec![command_key_event(egui::Key::J)]);

    assert_eq!(doc.buffer.to_string(), "foo");
}

#[test]
fn ctrl_shift_u_uppercases_the_selection() {
    let (_dir, mut doc) = open_fixture("foo bar baz", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // "bar" is chars 4..7.
    let last_error =
        focused_frame_with_selection_and_case_request(&mut doc, &mut parser, 4..7, None, vec![command_shift_u_event()]);

    assert_eq!(last_error, None);
    assert_eq!(doc.buffer.to_string(), "foo BAR baz");
}

#[test]
fn ctrl_shift_l_lowercases_the_selection() {
    let (_dir, mut doc) = open_fixture("foo BAR baz", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    let last_error =
        focused_frame_with_selection_and_case_request(&mut doc, &mut parser, 4..7, None, vec![command_shift_l_event()]);

    assert_eq!(last_error, None);
    assert_eq!(doc.buffer.to_string(), "foo bar baz");
}

#[test]
fn tools_menu_convert_to_title_case() {
    let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    let last_error = focused_frame_with_selection_and_case_request(
        &mut doc,
        &mut parser,
        0..11,
        Some(CaseConversion::Title),
        vec![],
    );

    assert_eq!(last_error, None);
    assert_eq!(doc.buffer.to_string(), "Hello World");
}

#[test]
fn case_conversion_with_no_selection_reports_why_instead_of_doing_nothing() {
    let (_dir, mut doc) = open_fixture("foo bar baz", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;
    let before = doc.buffer.to_string();

    // Collapsed selection (4..4) — cursor positioned, nothing selected.
    let last_error =
        focused_frame_with_selection_and_case_request(&mut doc, &mut parser, 4..4, None, vec![command_shift_u_event()]);

    assert_eq!(doc.buffer.to_string(), before);
    assert!(last_error.is_some_and(|msg| msg.contains("Select")));
}

#[test]
fn auto_pair_still_works_for_a_plain_text_file_with_no_parser() {
    let (_dir, mut doc) = open_fixture("", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame(&mut doc, &mut parser, vec![egui::Event::Text("{".to_string())]);

    assert_eq!(doc.buffer.to_string(), "{}");
}

#[test]
fn sort_lines_request_sorts_the_selected_lines() {
    let (_dir, mut doc) = open_fixture("banana\napple\ncherry", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection_and_line_op_request(&mut doc, &mut parser, 0..19, true, false);

    assert_eq!(doc.buffer.to_string(), "apple\nbanana\ncherry");
}

#[test]
fn unique_lines_request_dedupes_the_selected_lines() {
    let (_dir, mut doc) = open_fixture("foo\nbar\nfoo", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection_and_line_op_request(&mut doc, &mut parser, 0..11, false, true);

    assert_eq!(doc.buffer.to_string(), "foo\nbar");
}
