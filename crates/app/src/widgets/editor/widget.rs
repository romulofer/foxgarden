use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Event, FontId, Key};
use fg_core::Document;
use ropey::Rope;
use syntax::IncrementalParser;

use super::auto_edit::{apply_auto_indent, apply_auto_pair, char_to_byte};
use super::multi_cursor::{self, MultiEditOp};
use super::painting::{paint_diagnostics, paint_extra_selections};
use crate::style::fonts::EditorFont;
use crate::style::theme;

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

/// Renders `doc`'s buffer as an editable text area, keeping `parser`'s
/// incremental tree in sync with edits (SPEC.md sections 5.4 and 5.5).
/// `parser` is `None` for files with no recognized language (anything other
/// than `.java`/`.kt`) — such files still open and edit normally, they just
/// get plain rendering and no diagnostics; auto-pair/auto-indent/multi-cursor
/// are language-agnostic and keep working regardless.
pub fn show(ui: &mut egui::Ui, doc: &mut Document, parser: &mut Option<IncrementalParser>, editor_font: EditorFont) {
    let old_text = doc.buffer.to_string();
    let mut text = old_text.clone();
    // A stable id (rather than the default position-based auto id) keeps
    // this widget's identity — and thus its cursor/selection state — tied
    // to the document, not to where `show` happens to be called from in the
    // ui tree; it also lets tests request focus deterministically.
    let id_salt = doc.path.to_string_lossy().into_owned();

    // While extra (Ctrl+D) cursors are active, the events that would mutate
    // the buffer must be pulled out of the queue before `TextEdit::show`
    // runs, so its own single-cursor editing logic never sees them and
    // can't double-edit the primary cursor — they're applied manually,
    // at every active cursor at once, after `show` returns.
    let multi_cursor_active_at_start = !doc.extra_selections.is_empty();
    let mut intercepted_events: Vec<Event> = Vec::new();
    if multi_cursor_active_at_start {
        ui.input_mut(|i| {
            intercepted_events = i.events.iter().filter(|e| is_multi_edit_event(e)).cloned().collect();
            i.events.retain(|e| !is_multi_edit_event(e));
        });
    }

    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let source = buf.as_str();
        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap_width;

        let font_id = FontId::new(14.0, editor_font.family());
        let dark_mode = ui.visuals().dark_mode;

        let tree_and_language = parser.as_ref().and_then(|p| p.tree().map(|tree| (tree, p.language())));
        if let Some((tree, language)) = tree_and_language {
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
        .id_salt(id_salt)
        .code_editor()
        .desired_width(f32::INFINITY)
        .layouter(&mut layouter)
        .show(ui);

    // `TextEditState::store` takes `self` by value, so it can only be
    // called once per frame — every path below that wants to override
    // egui's own post-edit cursor/selection just records the target range
    // here, and a single `set_char_range` + `store` happens at the very
    // end.
    let mut manual_cursor_range: Option<CCursorRange> = None;

    if !intercepted_events.is_empty() {
        if let Some(primary_range) = output.cursor_range {
            let op = multi_edit_op_from_events(&intercepted_events);
            let sorted = primary_range.as_sorted_char_range();
            let mut selections = Vec::with_capacity(1 + doc.extra_selections.len());
            selections.push(sorted.start.0..sorted.end.0);
            selections.extend(doc.extra_selections.iter().cloned());

            let (new_text, new_cursors) = multi_cursor::apply_multi_edit(&old_text, &selections, &op);

            doc.buffer = Rope::from_str(&new_text);
            if let Some(parser) = parser.as_mut() {
                let edit = syntax::diff_edit(&old_text, &new_text);
                parser.reparse(&new_text, edit);
                doc.diagnostics = syntax::syntax_errors(parser.tree().expect("just reparsed"));
            }

            manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursors[0])));
            doc.extra_selections = new_cursors[1..].iter().map(|&c| c..c).collect();
            // This frame's `output.galley` was laid out before the edit
            // above landed, so the paint below is stale by one frame — the
            // same class of staleness the auto-indent path already accepts
            // (see its comment below). Ask for a repaint to make it correct
            // as soon as possible.
            ui.ctx().request_repaint();
        }
    } else if output.response.changed() {
        if multi_cursor_active_at_start {
            // A mutating event that wasn't in the intercepted set (Tab,
            // undo/redo, an IME commit, ...) reached egui's own
            // single-cursor logic and edited the primary cursor alone.
            // `doc.extra_selections` is now stale relative to `text`, so
            // rather than paint/edit at wrong offsets next frame, treat
            // this as an implicit collapse back to single-cursor mode.
            doc.extra_selections.clear();
        }

        let cursor_char = output.cursor_range.map(|r| r.primary.index.0);
        let (text_after_indent, indent_cursor) = apply_auto_indent(&old_text, &text, cursor_char);
        let corrected = if indent_cursor.is_some() {
            manual_cursor_range = indent_cursor.map(|c| CCursorRange::one(CCursor::new(c)));
            text_after_indent
        } else {
            apply_auto_pair(&old_text, &text, cursor_char)
        };

        doc.buffer = Rope::from_str(&corrected);
        if let Some(parser) = parser.as_mut() {
            let edit = syntax::diff_edit(&old_text, &corrected);
            parser.reparse(&corrected, edit);
            doc.diagnostics = syntax::syntax_errors(parser.tree().expect("just reparsed"));
        }
    }

    let modifiers = ui.input(|i| i.modifiers);
    let ctrl_d_pressed = ui.input(|i| i.key_pressed(Key::D)) && modifiers.command;
    if ctrl_d_pressed && let Some(primary_range) = output.cursor_range {
        if primary_range.is_empty() {
            let word = multi_cursor::word_range_at(&doc.buffer.to_string(), primary_range.primary.index.0);
            if !word.is_empty() {
                manual_cursor_range = Some(CCursorRange::two(CCursor::new(word.start), CCursor::new(word.end)));
            }
        } else {
            let text_now = doc.buffer.to_string();
            let sorted = primary_range.as_sorted_char_range();
            let needle_range = sorted.start.0..sorted.end.0;
            let needle = text_now[char_to_byte(&text_now, needle_range.start)..char_to_byte(&text_now, needle_range.end)]
                .to_string();

            let mut claimed = doc.extra_selections.clone();
            claimed.push(needle_range.clone());

            let case_sensitive = modifiers.shift;
            if let Some(found) = multi_cursor::find_next_unclaimed_occurrence(
                &text_now,
                &needle,
                needle_range.end,
                &claimed,
                case_sensitive,
            ) {
                doc.extra_selections.push(needle_range);
                manual_cursor_range = Some(CCursorRange::two(CCursor::new(found.start), CCursor::new(found.end)));
            }
        }
    }

    if !doc.extra_selections.is_empty() && !ctrl_d_pressed {
        let should_collapse = ui.input(|i| i.events.iter().any(is_multi_cursor_collapse_event));
        if should_collapse {
            doc.extra_selections.clear();
        }
    }

    paint_diagnostics(ui, &output, &text, &doc.diagnostics);
    paint_extra_selections(ui, &output, &doc.extra_selections);

    // Auto-indent inserts content *before* where egui placed the cursor
    // (unlike auto-pair, which only ever inserts after it), so the cursor
    // needs to be pushed forward past the inserted indentation manually.
    // The multi-cursor paths above reuse the same mechanism to land the
    // primary cursor after a multi-edit or a Ctrl+D word/occurrence jump.
    if let Some(range) = manual_cursor_range {
        output.state.cursor.set_char_range(Some(range));
        let id = output.response.id;
        output.state.store(ui.ctx(), id);
    }
}

fn is_multi_edit_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Text(_)
            | Event::Paste(_)
            | Event::Key {
                key: Key::Backspace | Key::Delete | Key::Enter,
                pressed: true,
                ..
            }
    )
}

fn multi_edit_op_from_events(events: &[Event]) -> MultiEditOp {
    let mut inserted = String::new();
    for event in events {
        match event {
            Event::Text(s) => inserted.push_str(s),
            Event::Paste(s) => inserted.push_str(s),
            Event::Key { key: Key::Enter, .. } => inserted.push('\n'),
            Event::Key { key: Key::Backspace, .. } => return MultiEditOp::Backspace,
            Event::Key { key: Key::Delete, .. } => return MultiEditOp::Delete,
            _ => {}
        }
    }
    MultiEditOp::Insert(inserted)
}

fn is_multi_cursor_collapse_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Key {
            key: Key::ArrowLeft
                | Key::ArrowRight
                | Key::ArrowUp
                | Key::ArrowDown
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown
                | Key::Escape,
            pressed: true,
            ..
        } | Event::PointerButton {
            pressed: true,
            button: egui::PointerButton::Primary,
            ..
        }
    )
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

    /// Builds a freshly parsed `Some(IncrementalParser)`, matching what
    /// `panels::tabs::open_parser_for` produces for any file with a
    /// recognized language — `show`'s tests always exercise the "has a
    /// language" path unless a test says otherwise.
    fn parsed(language: Language, source: &str) -> Option<IncrementalParser> {
        let mut parser = IncrementalParser::new(language);
        parser.parse(source);
        Some(parser)
    }

    #[test]
    fn renders_highlighted_valid_file_without_panicking() {
        let (_dir, mut doc) = open_fixture(
            "public class Hello {\n    // greeting\n    String greet() { return \"hi\"; }\n}\n",
            "Hello.java",
        );
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

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
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        doc.diagnostics = syntax::syntax_errors(parser.as_ref().unwrap().tree().unwrap());
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
    fn plain_text_file_renders_without_a_parser_and_stays_free_of_diagnostics() {
        let (_dir, mut doc) = open_fixture("just some notes, no code here", "notes.txt");
        assert_eq!(doc.language, None);
        let mut parser: Option<IncrementalParser> = None;

        egui::__run_test_ui(|ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default);
        });

        assert!(doc.diagnostics.is_empty());
    }

    #[test]
    fn auto_pair_still_works_for_a_plain_text_file_with_no_parser() {
        let (_dir, mut doc) = open_fixture("", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        focused_frame(&mut doc, &mut parser, vec![egui::Event::Text("{".to_string())]);

        assert_eq!(doc.buffer.to_string(), "{}");
    }

    #[test]
    fn simulated_edit_updates_diagnostics_and_dirty_state() {
        let (_dir, mut doc) = open_fixture("public class Hello {}\n", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        assert!(!doc.is_dirty());

        // Directly exercise the same edit -> reparse -> diagnostics path
        // that `show`'s `response.changed()` branch runs, without needing a
        // simulated keystroke through egui's input queue.
        let old_text = doc.buffer.to_string();
        let new_text = "public class Hello {\n".to_string(); // drop the closing brace
        let edit = syntax::diff_edit(&old_text, &new_text);
        let inner_parser = parser.as_mut().unwrap();
        inner_parser.reparse(&new_text, edit);
        doc.buffer = Rope::from_str(&new_text);
        doc.diagnostics = syntax::syntax_errors(inner_parser.tree().unwrap());

        assert!(doc.is_dirty());
        assert!(!doc.diagnostics.is_empty());

        egui::__run_test_ui(|ui| {
            // EditorFont::Default, not JetBrainsMono: see comment on the
            // first test in this file for why.
            show(ui, &mut doc, &mut parser, EditorFont::Default);
        });
    }

    // The tests below drive `show` through a real, reused `egui::Context`
    // (rather than the fire-and-forget `egui::__run_test_ui` used above),
    // since they need to simulate focused keyboard events: `show`'s
    // multi-cursor branches only run once `output.cursor_range` is `Some`,
    // which egui only produces for a widget that currently has keyboard
    // focus. `show`'s `.id_salt(doc.path...)` makes the widget's id
    // reproducible outside of `show` itself, so a test can request focus on
    // exactly that id before calling `show`.
    fn focused_frame(doc: &mut Document, parser: &mut Option<IncrementalParser>, events: Vec<egui::Event>) {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let raw_input = egui::RawInput { events, ..Default::default() };
        let _ = ctx.run_ui(raw_input, |ui| {
            // `TextEdit::id_salt(salt)` doesn't hash `salt` directly into the
            // widget id — it first wraps it in an `egui::IdSalt` (see
            // `builder.rs`'s `ui.make_persistent_id(id_salt)` where
            // `id_salt: IdSalt`), so replicate that same wrapping here or
            // the id won't match and `request_focus` will target nothing.
            let salt = egui::IdSalt::new(doc.path.to_string_lossy().into_owned());
            let id = ui.make_persistent_id(salt);
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default);
        });
    }

    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn multi_cursor_typed_edit_applies_at_every_active_cursor() {
        let (_dir, mut doc) = open_fixture("abcde", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        // Primary cursor starts at 0 (a fresh widget's default cursor);
        // these two extras sit at char indices 2 and 4.
        doc.extra_selections = vec![2..2, 4..4];

        focused_frame(&mut doc, &mut parser, vec![egui::Event::Text("Y".to_string())]);

        assert_eq!(doc.buffer.to_string(), "YabYcdYe");
        assert_eq!(doc.extra_selections, vec![4..4, 7..7]);
    }

    #[test]
    fn arrow_key_collapses_extra_selections() {
        let (_dir, mut doc) = open_fixture("abcde", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        doc.extra_selections = vec![2..2, 4..4];

        focused_frame(&mut doc, &mut parser, vec![key_event(egui::Key::ArrowLeft)]);

        assert!(doc.extra_selections.is_empty());
        // Arrow keys just navigate — the buffer itself is untouched.
        assert_eq!(doc.buffer.to_string(), "abcde");
    }

    #[test]
    fn non_intercepted_mutating_key_collapses_extra_selections_via_safety_net() {
        let (_dir, mut doc) = open_fixture("abc", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        // a genuine one-caret Vec<Range<usize>>, not a range of a Vec
        #[allow(clippy::single_range_in_vec_init)]
        let one_caret = vec![1..1];
        doc.extra_selections = one_caret;

        // Tab isn't in the intercepted-event set, so it reaches egui's own
        // single-cursor logic (code editors call `.lock_focus(true)`, which
        // makes Tab insert a literal tab character instead of moving focus)
        // and edits the primary cursor alone — the safety net must then
        // notice `extra_selections` is now stale and clear it.
        focused_frame(&mut doc, &mut parser, vec![key_event(egui::Key::Tab)]);

        assert!(doc.extra_selections.is_empty());
        assert_eq!(doc.buffer.to_string(), "\tabc");
    }
}
