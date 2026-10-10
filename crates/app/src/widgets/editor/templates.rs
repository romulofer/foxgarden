use std::ops::Range;

use super::text_offset::char_to_byte;

/// Marks where the cursor should land after a template expands. Not a
/// literal character sequence a user would type, so a plain substring
/// search/replace is enough — no escaping needed.
pub(super) const CURSOR_MARKER: &str = "${cursor}";

/// One trigger-word-to-expansion live template — IntelliJ-style: type
/// `trigger`, press Tab with no selection, get `body` in its place. A
/// language's own come from its extension (`syntax::language_snippets`).
pub use syntax::Snippet as Template;

/// A user-added live template — same trigger/body shape as a built-in
/// `Template`, typed in at runtime (Help > Live Templates…).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserTemplate {
    pub trigger: String,
    pub body: String,
}

/// A user's saved live templates, one list per language id plus `global`
/// (see `global_templates`) — the in-memory form `menu_bar`'s Live Templates
/// dialog edits directly and `app.rs` persists/restores via
/// `serialize_user_templates`/`parse_user_templates`.
#[derive(Debug, Clone, Default)]
pub struct UserTemplates {
    languages: std::collections::BTreeMap<String, Vec<UserTemplate>>,
    pub global: Vec<UserTemplate>,
}

impl UserTemplates {
    /// `language_id`'s templates; none if the user never added any.
    pub fn for_language(&self, language_id: &str) -> &[UserTemplate] {
        self.languages.get(language_id).map_or(&[], Vec::as_slice)
    }

    pub fn for_language_mut(&mut self, language_id: &str) -> &mut Vec<UserTemplate> {
        self.languages.entry(language_id.to_string()).or_default()
    }
}

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
/// file's language — checked *in addition to* a language's own snippets
/// (see `find_expansion`'s call site in `widget::show`), not
/// instead of them, and unlike those, still available in a file
/// with no recognized language at all (plain text, an unrecognized
/// extension). `pipe` is the prototypical example: a literal `|` isn't
/// specific to any one language, so it belongs here rather than duplicated
/// in every language's table.
pub fn global_templates() -> Vec<Template> {
    vec![Template::new("pipe", "|")]
}

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
pub fn find_template<'a>(templates: &'a [Template], word: &str) -> Option<&'a str> {
    templates
        .iter()
        .find(|template| template.trigger == word)
        .map(|template| template.body.as_str())
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
