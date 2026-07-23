/// Auto-indents after Enter: matches the new line's indentation to the line
/// just ended, plus one extra level if that line ends in `{`. Only fires on
/// a pure single-character insertion of `\n` (same guard as
/// `apply_auto_pair`, for the same reasons — pastes/IME/selection-replace
/// are left alone). Returns `(text, None)` unchanged if it doesn't apply.
pub(super) fn apply_auto_indent(old_text: &str, text: &str, cursor_char: Option<usize>) -> (String, Option<usize>) {
    let old_chars = old_text.chars().count();
    let new_chars = text.chars().count();
    if new_chars != old_chars + 1 {
        return (text.to_string(), None);
    }
    let Some(cursor_char) = cursor_char.filter(|&c| c > 0 && c <= new_chars) else {
        return (text.to_string(), None);
    };

    let inserted_start = char_to_byte(text, cursor_char - 1);
    let inserted_end = char_to_byte(text, cursor_char);
    let inserted = text[inserted_start..inserted_end]
        .chars()
        .next()
        .expect("cursor_char > 0 guarantees a preceding char");
    if inserted != '\n' {
        return (text.to_string(), None);
    }

    // The old cursor position (before Enter) is where the line being ended
    // sits; find that line's start and its content up to the cursor.
    let old_cursor_char = cursor_char - 1;
    let old_cursor_byte = char_to_byte(old_text, old_cursor_char);
    let line_start_byte = old_text[..old_cursor_byte].rfind('\n').map_or(0, |i| i + 1);
    let current_line_before_cursor = &old_text[line_start_byte..old_cursor_byte];

    let leading_ws_len = current_line_before_cursor
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(current_line_before_cursor.len());
    let leading_ws = &current_line_before_cursor[..leading_ws_len];
    let extra_indent = if current_line_before_cursor.trim_end().ends_with('{') {
        "    "
    } else {
        ""
    };
    let new_indent = format!("{leading_ws}{extra_indent}");
    if new_indent.is_empty() {
        return (text.to_string(), None);
    }

    let corrected = format!("{}{new_indent}{}", &text[..inserted_end], &text[inserted_end..]);
    let new_cursor_char = cursor_char + new_indent.chars().count();
    (corrected, Some(new_cursor_char))
}

/// Merges the line below `cursor_char` onto the current line — `Ctrl+J`'s
/// "join lines" command. Returns `None` if the cursor is on the last line
/// (nothing to join). The newline and the next line's leading whitespace
/// are replaced by a single space, except when that would be redundant (the
/// current line is empty or already ends in whitespace) or pointless (the
/// next line is itself blank) — in those cases the join leaves no separator
/// at all, matching how most editors' "join lines" avoids inserting spaces
/// no one would want. Returns the joined text and where the cursor should
/// land: right at the join point, same as most editors default to.
pub(super) fn join_lines(text: &str, cursor_char: usize) -> Option<(String, usize)> {
    let cursor_byte = char_to_byte(text, cursor_char);
    let line_start = text[..cursor_byte].rfind('\n').map_or(0, |i| i + 1);
    let nl_byte = line_start + text[line_start..].find('\n')?;
    let current_line = &text[line_start..nl_byte];

    let after_nl = &text[nl_byte + 1..];
    let ws_len = after_nl.find(|c: char| c != ' ' && c != '\t').unwrap_or(after_nl.len());
    let next_line_start = nl_byte + 1 + ws_len;
    let next_line_first_char = text[next_line_start..].chars().next();

    let needs_space =
        !current_line.is_empty() && !current_line.ends_with([' ', '\t']) && !matches!(next_line_first_char, None | Some('\n'));
    let separator = if needs_space { " " } else { "" };

    let joined = format!("{}{separator}{}", &text[..nl_byte], &text[next_line_start..]);
    let new_cursor_char = text[..nl_byte].chars().count() + separator.chars().count();
    Some((joined, new_cursor_char))
}

pub(super) fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// Maps an auto-pairable opening character to its closing counterpart —
/// shared by `apply_auto_pair` (typing an opener with no selection) and
/// `wrap_selection` (typing one *over* a selection), so the two features
/// can't quietly disagree on which characters are paired or what they pair
/// with.
///
/// `<`/`>` is a deliberate tradeoff, not an oversight: in Java/Kotlin `<` is
/// also the less-than operator, so auto-closing it unconditionally means
/// typing `x < 5` inserts an unwanted `>` after the `<`. Every other paired
/// character here is unambiguous in context; `<` isn't, and this doesn't
/// attempt the type-position analysis that would be needed to tell "generic"
/// from "comparison" apart.
fn closing_char(opener: char) -> Option<char> {
    match opener {
        '{' => Some('}'),
        '(' => Some(')'),
        '[' => Some(']'),
        '<' => Some('>'),
        '"' => Some('"'),
        '\'' => Some('\''),
        _ => None,
    }
}

/// Whether `c` is a character this editor auto-pairs — used by
/// `widgets::editor::show` to decide whether a single typed character while
/// a selection is active should be intercepted for `wrap_selection` instead
/// of falling through to egui's default replace-selection behavior.
pub(super) fn is_pairable(c: char) -> bool {
    closing_char(c).is_some()
}

/// Auto-closes brackets/quotes: typing an opener (`{`, `(`, `[`, `<`, `"`,
/// `'`) inserts its matching closer right after the cursor, and typing a
/// closer that's already sitting right there just types over it instead of
/// duplicating it. Only fires on a pure single-character insertion (so
/// pastes, multi-char IME commits, and replacing a selection are untouched
/// — a selection is `wrap_selection`'s job instead).
///
/// Deliberately locates the just-typed character via `cursor_char` (egui's
/// own post-edit cursor position) rather than by diffing `old_text`/`text`:
/// a prefix/suffix diff of the two full-text snapshots is ambiguous exactly
/// in the case this needs to detect — typing a closer immediately before an
/// identical existing one (e.g. `(a|)` -> type `)`) is indistinguishable, by
/// pure text diffing, from appending a new `)` at the end. Only the real
/// cursor position disambiguates it.
pub(super) fn apply_auto_pair(old_text: &str, text: &str, cursor_char: Option<usize>) -> String {
    let old_chars = old_text.chars().count();
    let new_chars = text.chars().count();
    if new_chars != old_chars + 1 {
        return text.to_string();
    }
    let Some(cursor_char) = cursor_char.filter(|&c| c > 0 && c <= new_chars) else {
        return text.to_string();
    };

    let inserted_start = char_to_byte(text, cursor_char - 1);
    let inserted_end = char_to_byte(text, cursor_char);
    let inserted = text[inserted_start..inserted_end]
        .chars()
        .next()
        .expect("cursor_char > 0 guarantees a preceding char");

    let old_char_at_same_pos = old_text.chars().nth(cursor_char - 1);

    match inserted {
        '{' | '(' | '[' | '<' => {
            let closer = closing_char(inserted).expect("matched only auto-pairable openers");
            format!("{}{closer}{}", &text[..inserted_end], &text[inserted_end..])
        }
        '"' | '\'' if old_char_at_same_pos == Some(inserted) => {
            // Typing over an existing quote: drop the duplicate, cursor
            // effectively moves past the original.
            format!("{}{}", &text[..inserted_start], &text[inserted_end..])
        }
        '"' | '\'' => format!("{}{inserted}{}", &text[..inserted_end], &text[inserted_end..]),
        '}' | ')' | ']' | '>' if old_char_at_same_pos == Some(inserted) => {
            format!("{}{}", &text[..inserted_start], &text[inserted_end..])
        }
        _ => text.to_string(),
    }
}

/// Wraps `old_text[start_char..end_char]` in `opener`/its matching closer,
/// replacing egui's default "typing a bracket over a selection deletes it"
/// behavior. Used when a selection is active and the typed character is one
/// this editor auto-pairs — `widgets::editor::show` detects that *before*
/// `TextEdit::show()` runs, since by the time a normal post-edit diff would
/// see it, the selected text egui replaced is already gone; there's nothing
/// left for a diff-based approach (like `apply_auto_pair` and
/// `apply_auto_indent` use) to recover it from.
///
/// Returns the new text and the char range the originally selected text now
/// occupies — kept selected in the result, matching how most editors leave
/// a just-wrapped selection selected rather than collapsing the cursor, so
/// wrapping it again (to nest) or moving on with an arrow key both stay one
/// step away. `None` if `opener` isn't a character this editor auto-pairs.
pub(super) fn wrap_selection(old_text: &str, start_char: usize, end_char: usize, opener: char) -> Option<(String, usize, usize)> {
    let closer = closing_char(opener)?;
    let start_byte = char_to_byte(old_text, start_char);
    let end_byte = char_to_byte(old_text, end_char);
    let wrapped = format!(
        "{}{opener}{}{closer}{}",
        &old_text[..start_byte],
        &old_text[start_byte..end_byte],
        &old_text[end_byte..],
    );
    Some((wrapped, start_char + 1, end_char + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_matches_previous_line_indentation() {
        let old = "    int x = 1;";
        let new = "    int x = 1;\n";
        let cursor_char = new.chars().count(); // cursor right after the newline
        let (corrected, new_cursor) = apply_auto_indent(old, new, Some(cursor_char));
        assert_eq!(corrected, "    int x = 1;\n    ");
        assert_eq!(new_cursor, Some(cursor_char + 4));
    }

    #[test]
    fn enter_after_open_brace_adds_one_extra_indent_level() {
        let old = "public class Foo {";
        let new = "public class Foo {\n";
        let cursor_char = new.chars().count();
        let (corrected, new_cursor) = apply_auto_indent(old, new, Some(cursor_char));
        assert_eq!(corrected, "public class Foo {\n    ");
        assert_eq!(new_cursor, Some(cursor_char + 4));
    }

    #[test]
    fn enter_after_open_brace_stacks_on_existing_indentation() {
        let old = "    public void foo() {";
        let new = "    public void foo() {\n";
        let cursor_char = new.chars().count();
        let (corrected, new_cursor) = apply_auto_indent(old, new, Some(cursor_char));
        assert_eq!(corrected, "    public void foo() {\n        ");
        assert_eq!(new_cursor, Some(cursor_char + 8));
    }

    #[test]
    fn enter_on_unindented_line_with_no_brace_is_a_no_op() {
        let old = "foo();";
        let new = "foo();\n";
        let cursor_char = new.chars().count();
        let (corrected, new_cursor) = apply_auto_indent(old, new, Some(cursor_char));
        assert_eq!(corrected, new);
        assert_eq!(new_cursor, None);
    }

    #[test]
    fn non_newline_insertion_is_left_to_auto_pair() {
        let (corrected, cursor) = apply_auto_indent("foo ", "foo {", Some(5));
        assert_eq!(corrected, "foo {");
        assert_eq!(cursor, None);
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
}
