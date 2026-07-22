use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Color32, FontId, Shape, Stroke};
use fg_core::{Diagnostic, Document};
use ropey::Rope;
use syntax::IncrementalParser;

use crate::fonts::EditorFont;
use crate::theme;

fn plain_format(font_id: FontId, dark_mode: bool) -> TextFormat {
    TextFormat {
        font_id,
        color: theme::default_text(dark_mode),
        ..Default::default()
    }
}

fn scope_format(font_id: FontId, scope: syntax::Scope, dark_mode: bool) -> TextFormat {
    TextFormat {
        font_id,
        color: theme::color_for_scope(scope, dark_mode),
        ..Default::default()
    }
}

/// Renders `doc`'s buffer as an editable, syntax-highlighted text area with
/// squiggly underlines under its syntax errors, keeping `parser`'s
/// incremental tree in sync with edits (SPEC.md sections 5.4 and 5.5).
pub fn show(ui: &mut egui::Ui, doc: &mut Document, parser: &mut IncrementalParser, editor_font: EditorFont) {
    let language = doc.language;
    let old_text = doc.buffer.to_string();
    let mut text = old_text.clone();

    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let source = buf.as_str();
        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap_width;

        let font_id = FontId::new(14.0, editor_font.family());
        let dark_mode = ui.visuals().dark_mode;

        if let Some(tree) = parser.tree() {
            let spans = syntax::highlight_spans(tree, &old_text, language);
            let mut cursor = 0usize;
            for (range, scope) in spans {
                if range.start > range.end
                    || range.end > source.len()
                    || !source.is_char_boundary(range.start)
                    || !source.is_char_boundary(range.end)
                    || range.start < cursor
                {
                    continue;
                }
                if range.start > cursor {
                    job.append(&source[cursor..range.start], 0.0, plain_format(font_id.clone(), dark_mode));
                }
                job.append(
                    &source[range.start..range.end],
                    0.0,
                    scope_format(font_id.clone(), scope, dark_mode),
                );
                cursor = range.end;
            }
            if cursor < source.len() {
                job.append(&source[cursor..], 0.0, plain_format(font_id.clone(), dark_mode));
            }
        } else if !source.is_empty() {
            job.append(source, 0.0, plain_format(font_id, dark_mode));
        }

        ui.fonts_mut(|f| f.layout_job(job))
    };

    let mut output = egui::TextEdit::multiline(&mut text)
        .code_editor()
        .desired_width(f32::INFINITY)
        .layouter(&mut layouter)
        .show(ui);

    let mut manual_cursor: Option<usize> = None;
    if output.response.changed() {
        let cursor_char = output.cursor_range.map(|r| r.primary.index.0);
        let (text_after_indent, indent_cursor) = apply_auto_indent(&old_text, &text, cursor_char);
        let corrected = if indent_cursor.is_some() {
            manual_cursor = indent_cursor;
            text_after_indent
        } else {
            apply_auto_pair(&old_text, &text, cursor_char)
        };

        let edit = syntax::diff_edit(&old_text, &corrected);
        parser.reparse(&corrected, edit);
        doc.buffer = Rope::from_str(&corrected);
        doc.diagnostics = syntax::syntax_errors(parser.tree().expect("just reparsed"));
    }

    paint_diagnostics(ui, &output, &text, &doc.diagnostics);

    // Auto-indent inserts content *before* where egui placed the cursor
    // (unlike auto-pair, which only ever inserts after it), so the cursor
    // needs to be pushed forward past the inserted indentation manually.
    if let Some(new_cursor_char) = manual_cursor {
        output
            .state
            .cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(new_cursor_char))));
        let id = output.response.id;
        output.state.store(ui.ctx(), id);
    }
}

/// Auto-indents after Enter: matches the new line's indentation to the line
/// just ended, plus one extra level if that line ends in `{`. Only fires on
/// a pure single-character insertion of `\n` (same guard as
/// `apply_auto_pair`, for the same reasons — pastes/IME/selection-replace
/// are left alone). Returns `(text, None)` unchanged if it doesn't apply.
fn apply_auto_indent(old_text: &str, text: &str, cursor_char: Option<usize>) -> (String, Option<usize>) {
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

fn char_to_byte(text: &str, char_idx: usize) -> usize {
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
fn apply_auto_pair(old_text: &str, text: &str, cursor_char: Option<usize>) -> String {
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

fn paint_diagnostics(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    text: &str,
    diagnostics: &[Diagnostic],
) {
    let painter = ui.painter();
    let squiggle_color = theme::error_squiggle(ui.visuals().dark_mode);
    for diag in diagnostics {
        let start = diag.range.start.min(text.len());
        let end = diag.range.end.min(text.len()).max(start);
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }

        let char_start = text[..start].chars().count();
        let char_end = text[..end].chars().count().max(char_start + 1);

        let start_rect = output.galley.pos_from_cursor(CCursor::new(char_start));
        let end_rect = output.galley.pos_from_cursor(CCursor::new(char_end));

        let y = output.galley_pos.y + start_rect.bottom();
        let x_start = output.galley_pos.x + start_rect.left();
        let x_end = (output.galley_pos.x + end_rect.left()).max(x_start + 4.0);

        paint_squiggle(painter, y, x_start, x_end, theme::ERROR_SQUIGGLE);
    }
}

fn paint_squiggle(painter: &egui::Painter, y: f32, x_start: f32, x_end: f32, color: Color32) {
    let amplitude = 2.0;
    let step = 3.0;
    let mut points = vec![egui::pos2(x_start, y)];
    let mut x = x_start;
    let mut up = true;
    while x < x_end {
        x = (x + step).min(x_end);
        let yy = if up { y - amplitude } else { y + amplitude };
        points.push(egui::pos2(x, yy));
        up = !up;
    }
    painter.add(Shape::line(points, Stroke::new(1.5, color)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use fg_core::Language;

    fn open_fixture(contents: &str, filename: &str) -> (tempfile::TempDir, Document) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(filename);
        std::fs::write(&path, contents).unwrap();
        let doc = Document::open(path).unwrap();
        (dir, doc)
    }

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

    #[test]
    fn renders_highlighted_valid_file_without_panicking() {
        let (_dir, mut doc) = open_fixture(
            "public class Hello {\n    // greeting\n    String greet() { return \"hi\"; }\n}\n",
            "Hello.java",
        );
        let mut parser = IncrementalParser::new(Language::Java);
        parser.parse(&doc.buffer.to_string());

        egui::__run_test_ui(|ui| {
            // egui::__run_test_ui uses an empty FontDefinitions with no
            // registered families beyond the built-in Monospace/Proportional
            // ones, so EditorFont::JetBrainsMono (a custom Name() family
            // only registered by fonts::install in real main()) would panic
            // here with "is not bound to any fonts". Use the built-in family
            // instead — this test exercises the widget's rendering logic,
            // not font registration.
            show(ui, &mut doc, &mut parser, EditorFont::Default);
        });
    }

    #[test]
    fn renders_squiggles_for_real_syntax_error_without_panicking() {
        let (_dir, mut doc) = open_fixture(
            "public class Hello {\n    public String greet() {\n        return \"Hello!\";\n    }\n",
            "Broken.java",
        );
        let mut parser = IncrementalParser::new(Language::Java);
        parser.parse(&doc.buffer.to_string());
        doc.diagnostics = syntax::syntax_errors(parser.tree().unwrap());
        assert!(
            !doc.diagnostics.is_empty(),
            "fixture should contain a deliberate syntax error"
        );

        egui::__run_test_ui(|ui| {
            // EditorFont::Default, not JetBrainsMono: see comment on the
            // first test in this file for why.
            show(ui, &mut doc, &mut parser, EditorFont::Default);
        });
    }

    #[test]
    fn simulated_edit_updates_diagnostics_and_dirty_state() {
        let (_dir, mut doc) = open_fixture("public class Hello {}\n", "Hello.java");
        let mut parser = IncrementalParser::new(Language::Java);
        parser.parse(&doc.buffer.to_string());
        assert!(!doc.is_dirty());

        // Directly exercise the same edit -> reparse -> diagnostics path
        // that `show`'s `response.changed()` branch runs, without needing a
        // simulated keystroke through egui's input queue.
        let old_text = doc.buffer.to_string();
        let new_text = "public class Hello {\n".to_string(); // drop the closing brace
        let edit = syntax::diff_edit(&old_text, &new_text);
        parser.reparse(&new_text, edit);
        doc.buffer = Rope::from_str(&new_text);
        doc.diagnostics = syntax::syntax_errors(parser.tree().unwrap());

        assert!(doc.is_dirty());
        assert!(!doc.diagnostics.is_empty());

        egui::__run_test_ui(|ui| {
            // EditorFont::Default, not JetBrainsMono: see comment on the
            // first test in this file for why.
            show(ui, &mut doc, &mut parser, EditorFont::Default);
        });
    }
}
