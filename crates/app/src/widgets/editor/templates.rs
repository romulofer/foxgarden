use std::ops::Range;

use super::text_offset::char_to_byte;

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

/// A user-added live template — same trigger/body shape as the built-in
/// `Template`, just owned rather than `&'static str` since it's typed in at
/// runtime (Help > Live Templates…) instead of compiled in. Kept as its own
/// type rather than reusing `Template` with owned fields: the built-in
/// tables are `const` data specifically so they can live in a `&'static
/// [Template]`, which owned `String` fields would rule out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserTemplate {
    pub trigger: String,
    pub body: String,
}

/// A user's saved live templates, one list per language — the in-memory
/// form `menu_bar`'s Live Templates dialog edits directly and `app.rs`
/// persists/restores via `serialize_user_templates`/`parse_user_templates`.
#[derive(Debug, Clone, Default)]
pub struct UserTemplates {
    pub java: Vec<UserTemplate>,
    pub kotlin: Vec<UserTemplate>,
}

/// Built-in Java live templates. Deliberately not settings-editable yet
/// (see `FEATURES.md`'s "Live templates" entry) — the expansion mechanism
/// itself is what this covers; a user-editable template list is a
/// follow-up, not blocked by this shape.
pub const JAVA_TEMPLATES: &[Template] = &[
    Template {
        trigger: "sout",
        body: "System.out.println(${cursor});",
    },
    Template {
        trigger: "souf",
        body: "System.out.printf(${cursor});",
    },
    Template {
        trigger: "serr",
        body: "System.err.println(${cursor});",
    },
    Template {
        trigger: "psvm",
        body: "public static void main(String[] args) {\n    ${cursor}\n}",
    },
    Template {
        trigger: "fori",
        body: "for (int i = 0; i < ${cursor}; i++) {\n    \n}",
    },
    Template {
        trigger: "iter",
        body: "for (var item : ${cursor}) {\n    \n}",
    },
    Template {
        trigger: "ifn",
        body: "if (${cursor} == null) {\n    \n}",
    },
    Template {
        trigger: "inn",
        body: "if (${cursor} != null) {\n    \n}",
    },
    Template {
        trigger: "trycatch",
        body: "try {\n    ${cursor}\n} catch (Exception e) {\n    e.printStackTrace();\n}",
    },
    Template {
        trigger: "pipe",
        body: "|",
    },
];

/// Built-in Kotlin live templates.
pub const KOTLIN_TEMPLATES: &[Template] = &[
    Template {
        trigger: "sout",
        body: "println(${cursor})",
    },
    Template {
        trigger: "serr",
        body: "System.err.println(${cursor})",
    },
    Template {
        trigger: "main",
        body: "fun main() {\n    ${cursor}\n}",
    },
    Template {
        trigger: "fori",
        body: "for (i in 0 until ${cursor}) {\n    \n}",
    },
    Template {
        trigger: "ifn",
        body: "if (${cursor} == null) {\n    \n}",
    },
    Template {
        trigger: "inn",
        body: "if (${cursor} != null) {\n    \n}",
    },
    Template {
        trigger: "trycatch",
        body: "try {\n    ${cursor}\n} catch (e: Exception) {\n    e.printStackTrace()\n}",
    },
    Template {
        trigger: "pipe",
        body: "|",
    },
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
    templates
        .iter()
        .find(|template| template.trigger == word)
        .map(|template| template.body)
}

/// Same lookup as `find_template`, but checking `custom` (a user's own
/// saved templates for the active language) first — a user redefining a
/// built-in trigger like `sout` gets their own version back, not the
/// built-in one shadowed underneath it, the same "your config wins" rule
/// most editors apply to user-vs-default settings.
pub fn find_expansion<'a>(built_in: &'a [Template], custom: &'a [UserTemplate], word: &str) -> Option<&'a str> {
    custom
        .iter()
        .find(|template| template.trigger == word)
        .map(|template| template.body.as_str())
        .or_else(|| find_template(built_in, word))
}

/// Escapes a template body for `serialize_user_templates`' one-line-per-
/// template format: a literal backslash becomes `\\` and a literal newline
/// becomes `\n` (two characters), so a multi-line body like `psvm`'s can't
/// be mistaken for more than one saved template when read back line by
/// line. Inverse of `unescape_body`.
fn escape_body(body: &str) -> String {
    body.replace('\\', "\\\\").replace('\n', "\\n")
}

/// Inverse of `escape_body`. An unrecognized escape (a lone trailing `\`,
/// or `\` followed by anything other than `n`/`\`) is passed through
/// literally rather than treated as a parse error — forward-compatible
/// with hand-edited or future-format input the same way `run_config::
/// parse_block`'s unrecognized-key handling is.
fn unescape_body(escaped: &str) -> String {
    let mut result = String::with_capacity(escaped.len());
    let mut chars = escaped.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => result.push('\n'),
            Some('\\') => result.push('\\'),
            Some(other) => {
                result.push('\\');
                result.push(other);
            }
            None => result.push('\\'),
        }
    }
    result
}

/// Serializes `templates` as one `trigger\tescaped_body` line each — a tab
/// separator is safe here since `word_before_cursor`'s identifier-character
/// rule means a real trigger never contains one, and `escape_body` already
/// rules out a raw newline splitting one template's line into two.
pub fn serialize_user_templates(templates: &[UserTemplate]) -> String {
    templates
        .iter()
        .map(|t| format!("{}\t{}", t.trigger, escape_body(&t.body)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Inverse of `serialize_user_templates`. A line with no tab (hand-edited
/// noise, or a blank trailing line) is skipped rather than treated as a
/// parse error.
pub fn parse_user_templates(input: &str) -> Vec<UserTemplate> {
    input
        .lines()
        .filter_map(|line| {
            let (trigger, body) = line.split_once('\t')?;
            Some(UserTemplate {
                trigger: trigger.to_string(),
                body: unescape_body(body),
            })
        })
        .collect()
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
        .map_or(template_body.chars().count(), |byte_pos| {
            template_body[..byte_pos].chars().count()
        });
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
        assert_eq!(
            word_before_cursor(text, text.chars().count()),
            expected_start..text.chars().count()
        );
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
        assert_eq!(
            find_template(JAVA_TEMPLATES, "sout"),
            Some("System.out.println(${cursor});")
        );
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
        let (text, cursor) = expand(
            "psvm",
            0..4,
            "public static void main(String[] args) {\n    ${cursor}\n}",
        );
        assert_eq!(text, "public static void main(String[] args) {\n    \n}");
        assert_eq!(&text[cursor..], "\n}");
    }

    #[test]
    fn find_expansion_falls_back_to_the_built_in_table_when_custom_has_no_match() {
        assert_eq!(
            find_expansion(JAVA_TEMPLATES, &[], "sout"),
            Some("System.out.println(${cursor});")
        );
    }

    #[test]
    fn find_expansion_prefers_a_custom_template_over_a_built_in_of_the_same_trigger() {
        let custom = [UserTemplate {
            trigger: "sout".to_string(),
            body: "my.own.println(${cursor});".to_string(),
        }];
        assert_eq!(
            find_expansion(JAVA_TEMPLATES, &custom, "sout"),
            Some("my.own.println(${cursor});")
        );
    }

    #[test]
    fn find_expansion_finds_a_custom_only_trigger() {
        let custom = [UserTemplate {
            trigger: "myown".to_string(),
            body: "custom body".to_string(),
        }];
        assert_eq!(find_expansion(JAVA_TEMPLATES, &custom, "myown"), Some("custom body"));
        assert_eq!(find_expansion(JAVA_TEMPLATES, &custom, "nope"), None);
    }

    #[test]
    fn user_templates_round_trip_through_serialization() {
        let templates = vec![
            UserTemplate {
                trigger: "sout2".to_string(),
                body: "System.out.println(${cursor});".to_string(),
            },
            UserTemplate {
                trigger: "psvm2".to_string(),
                body: "public static void main(String[] args) {\n    ${cursor}\n}".to_string(),
            },
        ];
        let serialized = serialize_user_templates(&templates);
        assert_eq!(parse_user_templates(&serialized), templates);
    }

    #[test]
    fn a_body_containing_a_literal_backslash_round_trips() {
        let templates = vec![UserTemplate {
            trigger: "path".to_string(),
            body: "C:\\Users\\${cursor}".to_string(),
        }];
        let serialized = serialize_user_templates(&templates);
        assert_eq!(parse_user_templates(&serialized), templates);
    }

    #[test]
    fn empty_input_parses_to_no_user_templates() {
        assert_eq!(parse_user_templates(""), vec![]);
    }
}
