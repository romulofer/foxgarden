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

pub(super) fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// Auto-closes brackets/quotes: typing an opener (`{`, `(`, `[`, `"`, `'`)
/// inserts its matching closer right after the cursor, and typing a closer
/// that's already sitting right there just types over it instead of
/// duplicating it. Only fires on a pure single-character insertion (so
/// pastes, multi-char IME commits, and replacing a selection are untouched).
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
        '{' | '(' | '[' => {
            let closer = match inserted {
                '{' => '}',
                '(' => ')',
                _ => ']',
            };
            format!("{}{closer}{}", &text[..inserted_end], &text[inserted_end..])
        }
        '"' | '\'' if old_char_at_same_pos == Some(inserted) => {
            // Typing over an existing quote: drop the duplicate, cursor
            // effectively moves past the original.
            format!("{}{}", &text[..inserted_start], &text[inserted_end..])
        }
        '"' | '\'' => format!("{}{inserted}{}", &text[..inserted_end], &text[inserted_end..]),
        '}' | ')' | ']' if old_char_at_same_pos == Some(inserted) => {
            format!("{}{}", &text[..inserted_start], &text[inserted_end..])
        }
        _ => text.to_string(),
    }
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
    }

    #[test]
    fn typing_quote_inserts_matching_quote() {
        assert_eq!(apply_auto_pair("", "\"", Some(1)), "\"\"");
        assert_eq!(apply_auto_pair("", "'", Some(1)), "''");
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
}
