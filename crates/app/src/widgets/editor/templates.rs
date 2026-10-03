use std::ops::Range;

use super::text_offset::char_to_byte;

/// Marks where the cursor should land after a template expands. Not a
/// literal character sequence a user would type, so a plain substring
/// search/replace is enough — no escaping needed.
pub(super) const CURSOR_MARKER: &str = "${cursor}";

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

/// A user's saved live templates, one list per language plus `global` (see
/// `GLOBAL_TEMPLATES`'s doc comment) — the in-memory form `menu_bar`'s Live
/// Templates dialog edits directly and `app.rs` persists/restores via
/// `serialize_user_templates`/`parse_user_templates`.
#[derive(Debug, Clone, Default)]
pub struct UserTemplates {
    pub java: Vec<UserTemplate>,
    pub kotlin: Vec<UserTemplate>,
    pub global: Vec<UserTemplate>,
}

/// Built-in Java live templates — alongside a user's own custom ones
/// (`UserTemplates::java`, editable via Help > Live Templates…), which take
/// priority when a trigger collides (see `find_expansion`).
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
];

/// Java's reserved words (`CompletionKind::Keyword` candidates, `SPEC.md`
/// §1's "Also includes") — not exhaustive of every contextual/restricted
/// identifier the JLS defines (`sealed`, `permits`, `yield` etc. are
/// context-sensitive, not reserved everywhere), just the always-reserved
/// set a word-completion popup is worth offering.
pub const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
    "true",
    "false",
    "null",
    "var",
];

/// Kotlin's hard keywords — always reserved, unlike its soft/modifier
/// keywords (`data`, `sealed`, `internal`, etc.), which are valid
/// identifiers elsewhere and so are a worse fit for an unconditional
/// candidate list.
pub const KOTLIN_KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

/// Every distinct identifier-shaped token in `text` (`SPEC.md` §1) — the
/// word-completion candidate source, first-seen order, deduplicated. Same
/// character class `word_before_cursor` already uses (letters, digits,
/// underscore), so a candidate here is always something that run could
/// actually match.
pub fn identifiers_in(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if !word.is_empty() && seen.insert(word) {
            result.push(word.to_string());
        }
    }
    result
}

/// Built-in templates that expand the same way regardless of the active
/// file's language — checked *in addition to* `JAVA_TEMPLATES`/
/// `KOTLIN_TEMPLATES` (see `find_expansion`'s call site in `widget::show`),
/// not instead of them, and unlike those two, still available in a file
/// with no recognized language at all (plain text, an unrecognized
/// extension). `pipe` is the prototypical example: a literal `|` isn't
/// Java- or Kotlin-specific, so it belongs here rather than duplicated in
/// both language tables.
pub const GLOBAL_TEMPLATES: &[Template] = &[Template {
    trigger: "pipe",
    body: "|",
}];

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

/// Looks up `word` across any number of custom-template groups (checked
/// first, in the order given) then any number of built-in groups — e.g. a
/// file's own language group plus the always-on global group (`GLOBAL_
/// TEMPLATES`/`UserTemplates::global`), letting one lookup cover both
/// "language-specific" and "applies everywhere" triggers together. A custom
/// group is always checked before any built-in one regardless of which
/// group either lives in, so a user redefining a built-in trigger like
/// `sout` (in their language group) or `pipe` (in their global group) gets
/// their own version back — the same "your config wins" rule most editors
/// apply to user-vs-default settings.
pub fn find_expansion<'a>(
    custom_groups: &[&'a [UserTemplate]],
    built_in_groups: &[&'a [Template]],
    word: &str,
) -> Option<&'a str> {
    for group in custom_groups {
        if let Some(template) = group.iter().find(|template| template.trigger == word) {
            return Some(template.body.as_str());
        }
    }
    built_in_groups.iter().find_map(|group| find_template(group, word))
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
/// with hand-edited or future-format input, same spirit as
/// `run_config::parse_run_configs`'s own tolerance for an unrecognized or
/// missing JSON field.
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
#[path = "templates_test.rs"]
mod templates_test;
