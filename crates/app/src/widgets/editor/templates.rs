use std::ops::Range;

use super::auto_edit::char_to_byte;

/// Marks where the cursor should land after a template expands. Not a
/// literal character sequence a user would type, so a plain substring
/// search/replace is enough — no escaping needed.
const CURSOR_MARKER: &str = "${cursor}";

/// One trigger-word-to-expansion live template — IntelliJ-style: type
/// `trigger`, press Tab with no selection, get `body` in its place.
pub struct Template {
    pub trigger: &'static str,
    pub body: &'static str,
}

/// Built-in Java live templates. Deliberately not settings-editable yet
/// (see `FEATURES.md`'s "Live templates" entry) — the expansion mechanism
/// itself is what this covers; a user-editable template list is a
/// follow-up, not blocked by this shape.
pub const JAVA_TEMPLATES: &[Template] = &[
    Template { trigger: "sout", body: "System.out.println(${cursor});" },
    Template { trigger: "psvm", body: "public static void main(String[] args) {\n    ${cursor}\n}" },
    Template { trigger: "fori", body: "for (int i = 0; i < ${cursor}; i++) {\n    \n}" },
];

/// Built-in Kotlin live templates.
pub const KOTLIN_TEMPLATES: &[Template] = &[
    Template { trigger: "sout", body: "println(${cursor})" },
    Template { trigger: "main", body: "fun main() {\n    ${cursor}\n}" },
];

/// The maximal run of identifier characters (letters, digits, underscore)
/// ending exactly at `cursor_char` — the word a just-pressed Tab would be
/// completing. Empty (an empty range at `cursor_char`) if the cursor isn't
/// right after such a run, e.g. after whitespace or punctuation.
pub fn word_before_cursor(text: &str, cursor_char: usize) -> Range<usize> {
    let chars: Vec<char> = text.chars().collect();
    let end = cursor_char.min(chars.len());
    let mut start = end;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    start..end
}

/// Looks up `word` among `templates` by exact match — triggers are short,
/// deliberately memorable mnemonics, not a fuzzy-search target — returning
/// its body if found.
pub fn find_template(templates: &[Template], word: &str) -> Option<&'static str> {
    templates.iter().find(|template| template.trigger == word).map(|template| template.body)
}

/// Replaces `text[word_range]` (the just-typed trigger) with `template_body`
/// (its `${cursor}` marker stripped out), returning the new text and where
/// the cursor should land: at the marker's position if present, otherwise
/// right after the whole inserted expansion.
pub fn expand(text: &str, word_range: Range<usize>, template_body: &str) -> (String, usize) {
    let word_start_byte = char_to_byte(text, word_range.start);
    let word_end_byte = char_to_byte(text, word_range.end);

    let cursor_offset = template_body
        .find(CURSOR_MARKER)
        .map_or(template_body.chars().count(), |byte_pos| template_body[..byte_pos].chars().count());
    let expansion = template_body.replace(CURSOR_MARKER, "");

    let new_text = format!("{}{expansion}{}", &text[..word_start_byte], &text[word_end_byte..]);
    let new_cursor = word_range.start + cursor_offset;
    (new_text, new_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_before_cursor_finds_the_trailing_identifier() {
        let text = "System.out.println(sout";
        let expected_start = text.find("sout").unwrap();
        assert_eq!(word_before_cursor(text, text.chars().count()), expected_start..text.chars().count());
    }

    #[test]
    fn word_before_cursor_is_empty_right_after_whitespace() {
        assert_eq!(word_before_cursor("foo ", 4), 4..4);
    }

    #[test]
    fn word_before_cursor_at_start_of_buffer_is_empty() {
        assert_eq!(word_before_cursor("", 0), 0..0);
    }

    #[test]
    fn find_template_matches_by_exact_trigger() {
        assert_eq!(find_template(JAVA_TEMPLATES, "sout"), Some("System.out.println(${cursor});"));
        assert_eq!(find_template(JAVA_TEMPLATES, "so"), None);
        assert_eq!(find_template(JAVA_TEMPLATES, ""), None);
    }

    #[test]
    fn expand_replaces_the_trigger_and_places_cursor_at_the_marker() {
        let before = "System.out.println(sout);";
        let word_start = before.find("sout").unwrap();
        let word_range = word_start..word_start + "sout".chars().count();

        let (text, cursor) = expand(before, word_range, "System.out.println(${cursor});");
        assert_eq!(text, "System.out.println(System.out.println(););");
        // Cursor lands right where "${cursor}" was, i.e. right after the
        // expansion's own opening paren.
        assert_eq!(&text[..cursor], "System.out.println(System.out.println(");
    }

    #[test]
    fn expand_with_no_marker_places_cursor_after_the_whole_expansion() {
        let (text, cursor) = expand("psvm", 0..4, "public static void main() {}");
        assert_eq!(text, "public static void main() {}");
        assert_eq!(cursor, text.chars().count());
    }

    #[test]
    fn expand_multiline_template_lands_cursor_on_the_indented_body_line() {
        let (text, cursor) = expand("psvm", 0..4, "public static void main(String[] args) {\n    ${cursor}\n}");
        assert_eq!(text, "public static void main(String[] args) {\n    \n}");
        assert_eq!(&text[cursor..], "\n}");
    }
}
