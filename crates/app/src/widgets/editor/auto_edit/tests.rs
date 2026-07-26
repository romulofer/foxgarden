//! Unit tests for [`super`](auto_edit.rs), extracted verbatim from that
//! file's colocated `#[cfg(test)] mod tests` so the module file stays focused
//! on the code under test. Behavior-identical to the inline module it replaced.

use super::*;

#[test]
fn enter_matches_previous_line_indentation() {
    let old = "    int x = 1;";
    let new = "    int x = 1;\n";
    let cursor_char = new.chars().count(); // cursor right after the newline
    let (corrected, new_cursor) = apply_auto_indent(old, new, Some(cursor_char), IndentSettings::default());
    assert_eq!(corrected, "    int x = 1;\n    ");
    assert_eq!(new_cursor, Some(cursor_char + 4));
}

#[test]
fn enter_after_open_brace_adds_one_extra_indent_level() {
    let old = "public class Foo {";
    let new = "public class Foo {\n";
    let cursor_char = new.chars().count();
    let (corrected, new_cursor) = apply_auto_indent(old, new, Some(cursor_char), IndentSettings::default());
    assert_eq!(corrected, "public class Foo {\n    ");
    assert_eq!(new_cursor, Some(cursor_char + 4));
}

#[test]
fn enter_after_open_brace_stacks_on_existing_indentation() {
    let old = "    public void foo() {";
    let new = "    public void foo() {\n";
    let cursor_char = new.chars().count();
    let (corrected, new_cursor) = apply_auto_indent(old, new, Some(cursor_char), IndentSettings::default());
    assert_eq!(corrected, "    public void foo() {\n        ");
    assert_eq!(new_cursor, Some(cursor_char + 8));
}

#[test]
fn enter_on_unindented_line_with_no_brace_is_a_no_op() {
    let old = "foo();";
    let new = "foo();\n";
    let cursor_char = new.chars().count();
    let (corrected, new_cursor) = apply_auto_indent(old, new, Some(cursor_char), IndentSettings::default());
    assert_eq!(corrected, new);
    assert_eq!(new_cursor, None);
    // SPEC.md §3: a no-op returns a borrow of `new`, not a fresh clone.
    assert!(matches!(corrected, Cow::Borrowed(_)));
}

#[test]
fn non_newline_insertion_is_left_to_auto_pair() {
    let (corrected, cursor) = apply_auto_indent("foo ", "foo {", Some(5), IndentSettings::default());
    assert_eq!(corrected, "foo {");
    assert_eq!(cursor, None);
    assert!(matches!(corrected, Cow::Borrowed(_)));
}

#[test]
fn a_byte_length_delta_outside_a_single_chars_range_bails_without_char_counting() {
    // Deletion (`new` shorter than `old`): the O(1) byte-length
    // pre-filter (SPEC.md §3) rules this out before any `.chars()` pass
    // — asserting `Cow::Borrowed` here is the observable proof that no
    // owned copy was made, the same guarantee the no-op tests above
    // check for the in-range-but-not-Enter cases.
    let (corrected, cursor) = apply_auto_indent("foo bar", "foo ba", Some(6), IndentSettings::default());
    assert_eq!(corrected, "foo ba");
    assert_eq!(cursor, None);
    assert!(matches!(corrected, Cow::Borrowed(_)));
}

#[test]
fn typing_opener_inserts_matching_closer() {
    assert_eq!(apply_auto_pair("foo ", "foo {", Some(5)), "foo {}");
    assert_eq!(apply_auto_pair("", "(", Some(1)), "()");
    assert_eq!(apply_auto_pair("x", "x[", Some(2)), "x[]");
    assert_eq!(apply_auto_pair("List", "List<", Some(5)), "List<>");
}

#[test]
fn typing_closer_angle_bracket_over_existing_one_skips_duplicate() {
    assert_eq!(apply_auto_pair("List<>", "List<>>", Some(6)), "List<>");
}

#[test]
fn typing_quote_inserts_matching_quote() {
    assert_eq!(apply_auto_pair("", "\"", Some(1)), "\"\"");
    assert_eq!(apply_auto_pair("", "'", Some(1)), "''");
}

#[test]
fn wrap_selection_wraps_selected_text_in_the_matching_pair() {
    let (wrapped, sel_start, sel_end) = wrap_selection("foo bar baz", 4, 7, '(').unwrap();
    assert_eq!(wrapped, "foo (bar) baz");
    // The originally selected text ("bar") now sits one char later, to
    // account for the inserted opener before it.
    assert_eq!((sel_start, sel_end), (5, 8));
    assert_eq!(&wrapped[sel_start..sel_end], "bar");
}

#[test]
fn wrap_selection_covers_every_auto_paired_character() {
    for (opener, closer) in [('{', '}'), ('(', ')'), ('[', ']'), ('<', '>'), ('"', '"'), ('\'', '\'')] {
        let (wrapped, ..) = wrap_selection("x", 0, 1, opener).unwrap();
        assert_eq!(wrapped, format!("{opener}x{closer}"));
    }
}

#[test]
fn wrap_selection_returns_none_for_a_non_pairable_character() {
    assert_eq!(wrap_selection("foo bar", 4, 7, 'x'), None);
}

#[test]
fn wrap_selection_works_at_the_start_and_end_of_the_buffer() {
    let (wrapped, sel_start, sel_end) = wrap_selection("bar", 0, 3, '[').unwrap();
    assert_eq!(wrapped, "[bar]");
    assert_eq!((sel_start, sel_end), (1, 4));
}

#[test]
fn typing_closer_over_existing_closer_skips_duplicate() {
    // Cursor sits right before the existing closer; user types the same
    // closer. This is the primary real-world case (type `(`, it
    // auto-closes to `()` with the cursor between them, then the user
    // types `)` to move past it) — and the reason this function uses
    // egui's real post-edit cursor position rather than diffing
    // `old_text`/`text`: with old="(a)" / new="(a))", a pure text diff
    // can't tell "typed `)` right before the existing one" apart from
    // "appended a new `)` at the end", since both produce the same two
    // strings. Only the cursor's actual position (3, not 4) disambiguates.
    assert_eq!(apply_auto_pair("(a)", "(a))", Some(3)), "(a)");
    assert_eq!(apply_auto_pair("{}", "{}}", Some(2)), "{}");
    assert_eq!(apply_auto_pair("[]", "[]]", Some(2)), "[]");
}

#[test]
fn typing_quote_over_existing_quote_skips_duplicate() {
    assert_eq!(apply_auto_pair("\"\"", "\"\"\"", Some(2)), "\"\"");
}

#[test]
fn typing_closer_at_end_of_buffer_with_no_existing_pair_just_inserts_it() {
    assert_eq!(apply_auto_pair("foo ", "foo )", Some(5)), "foo )");
    assert_eq!(apply_auto_pair("foo ", "foo }", Some(5)), "foo }");
}

#[test]
fn typing_closer_appended_after_an_unrelated_existing_closer_is_not_confused_for_skip_over() {
    // "(a)" with cursor at the very end (position 3, after the existing
    // `)`), typing another `)` — this should NOT be treated as
    // skip-over, since the cursor isn't sitting right before the
    // existing closer.
    assert_eq!(apply_auto_pair("(a)", "(a))", Some(4)), "(a))");
}

#[test]
fn replacing_a_selection_is_left_untouched() {
    // new_chars != old_chars + 1 -> not a pure single-char insertion.
    assert_eq!(apply_auto_pair("foo bar", "foo {", None), "foo {");
}

#[test]
fn multi_char_paste_is_left_untouched() {
    assert_eq!(apply_auto_pair("foo", "foo({", None), "foo({");
}

#[test]
fn join_lines_inserts_a_single_space_between_two_words() {
    let (joined, cursor) = join_lines("foo\nbar", 1).unwrap();
    assert_eq!(joined, "foo bar");
    // Cursor lands right at the join point: after "foo " (the original
    // line plus the inserted separator), at the start of what was the
    // next line's content.
    assert_eq!(cursor, 4);
}

#[test]
fn join_lines_strips_the_next_lines_leading_indentation() {
    let (joined, cursor) = join_lines("if (x) {\n    doStuff();\n}", 4).unwrap();
    assert_eq!(joined, "if (x) { doStuff();\n}");
    assert_eq!(cursor, 9);
}

#[test]
fn join_lines_on_the_last_line_is_a_no_op() {
    assert_eq!(join_lines("foo\nbar", 5), None);
}

#[test]
fn join_lines_uses_cursor_position_regardless_of_column_within_the_line() {
    // Cursor anywhere on "foo" (chars 0..=3) should join the *line*,
    // not require the cursor to sit at any particular column.
    let (joined, _) = join_lines("foo\nbar", 0).unwrap();
    assert_eq!(joined, "foo bar");
}

#[test]
fn join_lines_onto_a_blank_line_adds_no_space() {
    let (joined, cursor) = join_lines("foo\n\nbar", 1).unwrap();
    assert_eq!(joined, "foo\nbar");
    assert_eq!(cursor, 3);
}

#[test]
fn join_lines_from_an_empty_current_line_adds_no_leading_space() {
    let (joined, cursor) = join_lines("\nbar", 0).unwrap();
    assert_eq!(joined, "bar");
    assert_eq!(cursor, 0);
}

#[test]
fn join_lines_avoids_a_double_space_when_current_line_already_ends_in_whitespace() {
    let (joined, cursor) = join_lines("foo  \nbar", 1).unwrap();
    assert_eq!(joined, "foo  bar");
    assert_eq!(cursor, 5);
}

#[test]
fn indent_selected_lines_indents_every_touched_line() {
    // Selection spans all of "foo" and all of "bar" (chars 0..7),
    // starting right at column 0 of the first line.
    let (text, start, end) = indent_selected_lines("foo\nbar", 0, 7, false, IndentSettings::default());
    assert_eq!(text, "    foo\n    bar");
    // A selection that starts at column 0 stays at column 0 through an
    // indent (see the `column == 0` branch in `remap`), so it still
    // covers the entire (now-indented) two lines rather than excluding
    // the newly inserted leading spaces.
    assert_eq!(&text[start..end], "    foo\n    bar");
}

#[test]
fn indent_selected_lines_selection_ending_at_line_start_excludes_that_line() {
    // Selection from mid "foo" to the very start of "baz" (char 9) —
    // only "foo" and "bar" are touched, matching a Shift+Down drag that
    // never actually selects any of "baz".
    let (text, ..) = indent_selected_lines("foo\nbar\nbaz", 1, 8, false, IndentSettings::default());
    assert_eq!(text, "    foo\n    bar\nbaz");
}

#[test]
fn indent_selected_lines_dedent_removes_up_to_one_indent_level_of_spaces() {
    let (text, ..) = indent_selected_lines("    foo\n        bar", 0, 19, true, IndentSettings::default());
    assert_eq!(text, "foo\n    bar");
}

#[test]
fn indent_selected_lines_dedent_removes_a_single_leading_tab() {
    let (text, ..) = indent_selected_lines("\tfoo\n\tbar", 0, 9, true, IndentSettings::default());
    assert_eq!(text, "foo\nbar");
}

#[test]
fn indent_selected_lines_dedent_on_a_line_with_less_than_one_level_removes_what_exists() {
    let (text, ..) = indent_selected_lines("  foo\nbar", 0, 9, true, IndentSettings::default());
    assert_eq!(text, "foo\nbar");
}

#[test]
fn indent_selected_lines_dedent_on_an_unindented_line_is_a_no_op_for_that_line() {
    let (text, ..) = indent_selected_lines("foo\n    bar", 0, 11, true, IndentSettings::default());
    assert_eq!(text, "foo\nbar");
}

#[test]
fn indent_selected_lines_preserves_all_selected_text_unlike_eguis_default() {
    // Regression guard for the bug this function exists to fix: egui's
    // own Tab/Shift+Tab deletes the whole selection first. Selecting
    // "oo\nba" (chars 1..6, a genuine cross-line selection that doesn't
    // start/end on a line boundary) and indenting must not lose any of
    // the original characters.
    let (text, ..) = indent_selected_lines("foo\nbar", 1, 6, false, IndentSettings::default());
    assert_eq!(text, "    foo\n    bar");
    for ch in ['f', 'o', 'o', 'b', 'a', 'r'] {
        assert!(text.contains(ch), "lost character {ch:?} from the selection");
    }
}

#[test]
fn indent_selected_lines_keeps_selection_anchors_aligned_with_the_original_text() {
    // Selecting "oo\nba" (1..6) out of "foo\nbar" and indenting both
    // lines: the selection should still point at the very same
    // characters ("oo\nba"), just shifted by the two lines' worth of
    // inserted indentation.
    let (text, start, end) = indent_selected_lines("foo\nbar", 1, 6, false, IndentSettings::default());
    assert_eq!(&text[start..end], "oo\n    ba");
}

#[test]
fn indent_selected_lines_single_line_selection_only_touches_that_line() {
    let (text, ..) = indent_selected_lines("foo\nbar\nbaz", 4, 7, false, IndentSettings::default());
    assert_eq!(text, "foo\n    bar\nbaz");
}

#[test]
fn duplicate_line_inserts_the_copy_directly_below() {
    let (text, cursor) = duplicate_line("foo\nbar\nbaz", 5); // cursor on "bar"
    assert_eq!(text, "foo\nbar\nbar\nbaz");
    // Cursor lands on the duplicate, same column as it started at.
    assert_eq!(&text[cursor - 1..cursor + 2], "bar");
}

#[test]
fn duplicate_line_on_the_last_line_with_no_trailing_newline_works() {
    let (text, cursor) = duplicate_line("foo\nbar", 5); // column 1 of "bar"
    assert_eq!(text, "foo\nbar\nbar");
    // Cursor lands at column 1 of the duplicate "bar" (the second one).
    assert_eq!(&text[cursor..cursor + 2], "ar");
}

#[test]
fn duplicate_line_preserves_the_cursors_column() {
    let (_, cursor) = duplicate_line("abcdef", 3);
    // "abcdef" duplicated is "abcdef\nabcdef" (13 chars); column 3 on
    // the duplicate is char 7 + 3 = 10.
    assert_eq!(cursor, 10);
}

#[test]
fn move_line_up_swaps_with_the_previous_line() {
    let (text, cursor) = move_line_up("aaa\nbbb\nccc", 4).unwrap(); // cursor at column 0 of "bbb"
    assert_eq!(text, "bbb\naaa\nccc");
    assert_eq!(&text[cursor..cursor + 3], "bbb");
}

#[test]
fn move_line_up_on_the_first_line_is_none() {
    assert_eq!(move_line_up("aaa\nbbb", 1), None);
}

#[test]
fn move_line_up_keeps_the_cursors_column() {
    let (_, cursor) = move_line_up("aaa\nbbb", 5).unwrap(); // column 1 of "bbb"
    assert_eq!(cursor, 1); // column 1 of "bbb", which now starts at 0
}

#[test]
fn move_line_down_swaps_with_the_next_line() {
    let (text, cursor) = move_line_down("aaa\nbbb\nccc", 0).unwrap(); // cursor at column 0 of "aaa"
    assert_eq!(text, "bbb\naaa\nccc");
    assert_eq!(&text[cursor..cursor + 3], "aaa");
}

#[test]
fn move_line_down_on_the_last_line_is_none() {
    assert_eq!(move_line_down("aaa\nbbb", 5), None);
}

#[test]
fn move_line_up_then_down_is_the_identity() {
    let original = "one\ntwo\nthree\nfour";
    let cursor = original.find("three").unwrap();
    let (moved, new_cursor) = move_line_up(original, cursor).unwrap();
    let (restored, _) = move_line_down(&moved, new_cursor).unwrap();
    assert_eq!(restored, original);
}

#[test]
fn toggle_line_comments_comments_a_single_uncommented_line() {
    let (text, ..) = toggle_line_comments("foo();", 0, 0);
    assert_eq!(text, "// foo();");
}

#[test]
fn toggle_line_comments_uncomments_an_already_commented_line() {
    let (text, ..) = toggle_line_comments("// foo();", 0, 0);
    assert_eq!(text, "foo();");
}

#[test]
fn toggle_line_comments_uncomment_tolerates_no_space_after_the_marker() {
    let (text, ..) = toggle_line_comments("//foo();", 0, 0);
    assert_eq!(text, "foo();");
}

#[test]
fn toggle_line_comments_comments_every_touched_line() {
    let (text, ..) = toggle_line_comments("foo\nbar\nbaz", 0, 7); // touches "foo" and "bar"
    assert_eq!(text, "// foo\n// bar\nbaz");
}

#[test]
fn toggle_line_comments_uncomments_only_when_every_touched_line_is_commented() {
    // "foo" is commented, "bar" isn't — mixed, so the whole selection
    // is treated as "not fully commented" and gets commented further
    // rather than uncommenting just the one that qualifies.
    let (text, ..) = toggle_line_comments("// foo\nbar", 0, 10);
    assert_eq!(text, "// // foo\n// bar");
}

#[test]
fn toggle_line_comments_uncomments_every_touched_line_when_all_are_commented() {
    let (text, ..) = toggle_line_comments("// foo\n// bar", 0, 13);
    assert_eq!(text, "foo\nbar");
}

#[test]
fn toggle_line_comments_comments_a_blank_touched_line_too() {
    let (text, ..) = toggle_line_comments("foo\n\nbar", 0, 8);
    assert_eq!(text, "// foo\n// \n// bar");
}

#[test]
fn toggle_line_comments_ignores_blank_lines_when_deciding_to_uncomment() {
    // A blank line among otherwise-fully-commented lines shouldn't
    // block recognizing the selection as "commented".
    let (text, ..) = toggle_line_comments("// foo\n\n// bar", 0, 14);
    assert_eq!(text, "foo\n\nbar");
}

#[test]
fn toggle_line_comments_round_trips() {
    let original = "if (x) {\nfoo();\n}";
    let (commented, ..) = toggle_line_comments(original, 0, original.chars().count());
    let (restored, ..) = toggle_line_comments(&commented, 0, commented.chars().count());
    assert_eq!(restored, original);
}

#[test]
fn sort_lines_sorts_the_touched_lines_alphabetically() {
    let (text, ..) = sort_lines("banana\napple\ncherry", 0, 19);
    assert_eq!(text, "apple\nbanana\ncherry");
}

#[test]
fn sort_lines_with_no_selection_only_touches_the_cursors_line() {
    // Collapsed selection on "banana" (the first line) — sorting a
    // single line is a no-op, and the other lines must stay untouched
    // (and in particular not get pulled into the "sort" at all).
    let (text, ..) = sort_lines("banana\napple\ncherry", 2, 2);
    assert_eq!(text, "banana\napple\ncherry");
}

#[test]
fn sort_lines_selection_ending_at_a_line_start_excludes_that_line() {
    // Selection from column 0 of "banana" to column 0 of "cherry" (char
    // 14) touches only "banana" and "apple", matching
    // `indent_selected_lines`'s own boundary rule.
    let (text, ..) = sort_lines("banana\napple\ncherry", 0, 14);
    assert_eq!(text, "apple\nbanana\ncherry");
}

#[test]
fn sort_lines_returns_a_selection_covering_the_reordered_block() {
    let (text, start, end) = sort_lines("banana\napple", 0, 12);
    assert_eq!(&text[start..end], "apple\nbanana");
}

#[test]
fn unique_lines_drops_duplicates_keeping_the_first_occurrence() {
    let (text, ..) = unique_lines("foo\nbar\nfoo\nbaz\nbar", 0, 19);
    assert_eq!(text, "foo\nbar\nbaz");
}

#[test]
fn unique_lines_preserves_order_rather_than_also_sorting() {
    let (text, ..) = unique_lines("zebra\napple\nzebra", 0, 17);
    assert_eq!(text, "zebra\napple");
}

#[test]
fn unique_lines_with_no_duplicates_is_unchanged() {
    let (text, ..) = unique_lines("foo\nbar\nbaz", 0, 11);
    assert_eq!(text, "foo\nbar\nbaz");
}

#[test]
fn unique_lines_with_no_selection_only_touches_the_cursors_line() {
    let (text, ..) = unique_lines("foo\nfoo\nfoo", 1, 1);
    assert_eq!(text, "foo\nfoo\nfoo");
}

#[test]
fn convert_selection_case_uppercases_only_the_selected_range() {
    let (text, start, end) = convert_selection_case("foo bar baz", 4, 7, CaseConversion::Upper).unwrap();
    assert_eq!(text, "foo BAR baz");
    assert_eq!((start, end), (4, 7));
}

#[test]
fn convert_selection_case_lowercases_only_the_selected_range() {
    let (text, ..) = convert_selection_case("FOO BAR BAZ", 4, 7, CaseConversion::Lower).unwrap();
    assert_eq!(text, "FOO bar BAZ");
}

#[test]
fn convert_selection_case_title_cases_every_word_in_the_range() {
    // "_" isn't alphanumeric, so it ends a word the same way a space
    // does — "World" and "2day" are separate words, each capitalized
    // at its own start; "2" is already its own "capital", so the
    // digit-led word's letter stays lowercase (mid-word).
    let (text, ..) = convert_selection_case("hello WORLD_2day now", 0, 16, CaseConversion::Title).unwrap();
    assert_eq!(text, "Hello World_2day now");
}

#[test]
fn convert_selection_case_returns_none_for_an_empty_selection() {
    assert_eq!(convert_selection_case("foo", 1, 1, CaseConversion::Upper), None);
}

#[test]
fn convert_selection_case_tracks_a_growing_conversion() {
    // German ß uppercases to "SS" — two chars from one — so the
    // returned end must reflect the actual converted length, not just
    // assume the selection stays the same size.
    let (text, start, end) = convert_selection_case("straße", 0, 6, CaseConversion::Upper).unwrap();
    assert_eq!(text, "STRASSE");
    assert_eq!((start, end), (0, 7));
}

#[test]
fn smart_home_from_mid_line_goes_to_first_non_whitespace() {
    assert_eq!(smart_home_target("    foo", 6), 4);
}

#[test]
fn smart_home_from_first_non_whitespace_goes_to_column_zero() {
    assert_eq!(smart_home_target("    foo", 4), 0);
}

#[test]
fn smart_home_from_column_zero_goes_back_to_first_non_whitespace() {
    assert_eq!(smart_home_target("    foo", 0), 4);
}

#[test]
fn smart_home_on_an_unindented_line_always_goes_to_column_zero() {
    // First-non-whitespace *is* column 0 here, so the toggle condition
    // ("already at first-non-whitespace") is met immediately.
    assert_eq!(smart_home_target("foo", 2), 0);
}

#[test]
fn smart_home_on_a_blank_line_goes_to_column_zero() {
    assert_eq!(smart_home_target("    ", 2), 0);
}

#[test]
fn smart_home_operates_on_the_cursors_own_line_in_a_multiline_buffer() {
    let text = "foo\n    bar\nbaz";
    let cursor = text.find("bar").unwrap() + 1; // mid "bar"
    assert_eq!(smart_home_target(text, cursor), text.find("bar").unwrap());
}
