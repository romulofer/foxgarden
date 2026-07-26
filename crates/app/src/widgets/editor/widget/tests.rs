//! Unit tests for [`super`](widget.rs), extracted verbatim from that
//! file's colocated `#[cfg(test)] mod tests` so the module file stays focused
//! on the code under test. Behavior-identical to the inline module it replaced.

use super::*;
use fg_core::Language;

/// `test_support::temp_document` takes `(name, contents)`; every one of
/// this module's ~50 call sites was already written the other way
/// around (`(contents, filename)`, matching how the fixture text reads
/// as the "main" argument in a test body), so this keeps that order
/// rather than touching all of them.
fn open_fixture(contents: &str, filename: &str) -> (tempfile::TempDir, Document) {
    test_support::temp_document(filename, contents)
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
fn sticky_headers_keeps_only_scopes_above_the_top_line_outermost_first() {
    // Scopes at lines 2 (class) and 8 (method), top visible line 15: both
    // are above, both pin, outermost (line 2) first.
    assert_eq!(sticky_headers_to_pin(&[2, 8], 15, 5), vec![2, 8]);
    // Top line 5: only the class header (line 2) is above it; the method
    // header at 8 is below the viewport top and doesn't pin.
    assert_eq!(sticky_headers_to_pin(&[2, 8], 5, 5), vec![2]);
    // Nothing above the top of the file.
    assert_eq!(sticky_headers_to_pin(&[2, 8], 0, 5), Vec::<usize>::new());
}

#[test]
fn sticky_headers_caps_at_max_depth_keeping_the_outermost() {
    // Six enclosing scopes, all above the top line, cap 5 → keep the five
    // outermost (drop the innermost, line 60).
    assert_eq!(
        sticky_headers_to_pin(&[10, 20, 30, 40, 50, 60], 100, 5),
        vec![10, 20, 30, 40, 50]
    );
}

#[test]
fn sticky_scroll_enabled_renders_without_panicking() {
    let (_dir, mut doc) = open_fixture(
        "public class Hello {\n    void greet() {\n        int x = 1;\n        int y = 2;\n    }\n}\n",
        "Hello.java",
    );
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let view = ViewSettings {
        show_sticky_scroll: true,
        ..ViewSettings::default()
    };

    egui::__run_test_ui(|ui| {
        // EditorFont::Default, not JetBrainsMono: see the first test in
        // this file for why.
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            view,
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
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
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
}

#[test]
fn highlight_and_fold_caches_are_reused_across_an_idle_frame() {
    // SPEC.md §4/§5, PLAN.md Phase 3: a second frame with no edit in
    // between must hit the cache, not recompute. `Arc::ptr_eq` is the
    // direct proof — both caches' `if let Some(cached) = ... { return
    // cached.spans/folds; }` hit path returns the *same allocation*
    // without re-inserting into `ctx.data`, so a cache miss (a fresh
    // `Arc::new(..)`) is the only way the second frame's pointer could
    // differ from the first's.
    let (_dir, mut doc) = open_fixture(
        "public class Hello {\n    void greet() {\n        int x = 1;\n    }\n}\n",
        "Hello.java",
    );
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let widget_id = egui::Id::new(doc.path.to_string_lossy().into_owned());
    let highlight_cache_id = egui::Id::new(("widget_highlight_spans", widget_id));
    let fold_cache_id = egui::Id::new(("widget_foldable_ranges", widget_id));

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let run_frame = |doc: &mut Document, parser: &mut Option<IncrementalParser>| {
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(
                ui,
                doc,
                parser,
                EditorFont::Default,
                14.0,
                IndentSettings::default(),
                ViewSettings::default(),
                None,
                &mut None,
                None,
                &mut None,
                None,
                false,
                &mut None,
                None,
                false,
                false,
                false,
                false,
                &mut None,
                &mut Vec::new(),
                &mut None,
            );
        });
    };

    run_frame(&mut doc, &mut parser);
    let spans_after_first = ctx
        .data(|d| d.get_temp::<CachedHighlightSpans>(highlight_cache_id))
        .expect("highlight cache populated after first frame")
        .spans;
    let folds_after_first = ctx
        .data(|d| d.get_temp::<CachedFolds>(fold_cache_id))
        .expect("fold cache populated after first frame")
        .folds;

    run_frame(&mut doc, &mut parser); // idle: no edit, no theme change
    let spans_after_second = ctx
        .data(|d| d.get_temp::<CachedHighlightSpans>(highlight_cache_id))
        .expect("highlight cache still populated after second frame")
        .spans;
    let folds_after_second = ctx
        .data(|d| d.get_temp::<CachedFolds>(fold_cache_id))
        .expect("fold cache still populated after second frame")
        .folds;

    assert!(Arc::ptr_eq(&spans_after_first, &spans_after_second));
    assert!(Arc::ptr_eq(&folds_after_first, &folds_after_second));
}

#[test]
fn bracket_pair_highlighting_renders_without_panicking_when_cursor_is_beside_a_brace() {
    let (_dir, mut doc) = open_fixture("class Foo {\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let brace = doc.buffer.to_string().find('{').unwrap() + 1;

    // A collapsed selection right after the opening brace is exactly
    // the position `syntax::bracket_match` recognizes as "touching"
    // it — this is a smoke test for `paint_bracket_match` (painting
    // isn't otherwise assertable), the matching logic itself is
    // covered directly by `syntax::brackets`'s own tests.
    focused_frame_with_selection(&mut doc, &mut parser, brace..brace, vec![]);
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
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
}

#[test]
fn occurrence_highlighting_does_not_panic_when_the_cursor_touches_a_word() {
    // A fresh widget's default (collapsed) cursor sits at char 0, which
    // touches "abc" — this should compute and paint every occurrence
    // ("abc" appears twice) without panicking.
    let (_dir, mut doc) = open_fixture("abc def abc", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    egui::__run_test_ui(|ui| {
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
}

#[test]
fn occurrence_highlight_cache_is_reused_across_an_idle_frame() {
    // SPEC.md §6, PLAN.md Phase 4: same direct `Arc::ptr_eq` proof as
    // `highlight_and_fold_caches_are_reused_across_an_idle_frame` — a
    // second frame with the cursor resting on the same word, no edit in
    // between, must hit the cache rather than re-scan the buffer for every
    // occurrence.
    let (_dir, mut doc) = open_fixture("abc def abc", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;
    let widget_id = egui::Id::new(doc.path.to_string_lossy().into_owned());
    let cache_id = egui::Id::new(("widget_occurrence_highlights", widget_id));

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let run_frame = |doc: &mut Document, parser: &mut Option<IncrementalParser>| {
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(widget_id));
            show(
                ui,
                doc,
                parser,
                EditorFont::Default,
                14.0,
                IndentSettings::default(),
                ViewSettings::default(),
                None,
                &mut None,
                None,
                &mut None,
                None,
                false,
                &mut None,
                None,
                false,
                false,
                false,
                false,
                &mut None,
                &mut Vec::new(),
                &mut None,
            );
        });
    };

    run_frame(&mut doc, &mut parser);
    let first = ctx
        .data(|d| d.get_temp::<CachedOccurrenceHighlights>(cache_id))
        .expect("occurrence cache populated after first frame")
        .occurrences;

    run_frame(&mut doc, &mut parser); // idle: cursor stays on "abc", no edit
    let second = ctx
        .data(|d| d.get_temp::<CachedOccurrenceHighlights>(cache_id))
        .expect("occurrence cache still populated after second frame")
        .occurrences;

    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn plain_text_file_renders_without_a_parser_and_stays_free_of_diagnostics() {
    let (_dir, mut doc) = open_fixture("just some notes, no code here", "notes.txt");
    assert_eq!(doc.language, None);
    let mut parser: Option<IncrementalParser> = None;

    egui::__run_test_ui(|ui| {
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
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
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
}

#[test]
fn whitespace_and_indent_guides_render_without_panicking() {
    let (_dir, mut doc) = open_fixture("public class Hello {\n    int x = 1;\n}\n", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let view_settings = ViewSettings {
        word_wrap: true,
        show_whitespace: true,
        show_indent_guides: true,
        show_sticky_scroll: false,
        ..ViewSettings::default()
    };

    egui::__run_test_ui(|ui| {
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            view_settings,
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
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
    // `RawInput::modifiers` ("which modifier keys are down at the start
    // of the frame") is a separate top-level field from each
    // `Event::Key`'s own `modifiers` — it's what `ui.input(|i|
    // i.modifiers)` actually reads (e.g. `Ctrl+J`'s `modifiers.command`
    // check), not the per-event field. Left at its `..Default::default()`
    // value (`NONE`), a simulated `Ctrl+<key>` event would carry the
    // right modifiers on the event itself but still read as unmodified —
    // so derive it from whichever `Key` event carries it.
    let modifiers = events
        .iter()
        .find_map(|e| match e {
            egui::Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let raw_input = egui::RawInput {
        events,
        modifiers,
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        // `show` sets the widget's id via `.id(egui::Id::new(id_salt))`
        // — a pure hash of the path string, independent of which `Ui`
        // ends up calling `.show()` — so replicate that exact
        // computation here or the id won't match and `request_focus`
        // will target nothing.
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
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

/// Like `focused_frame`, but for exercising wrap-selection: `show`'s
/// interception reads the *persisted* selection from before its own
/// frame runs (see `show`'s wrap-selection block), so this pre-stores
/// one — as if some earlier, unmodeled frame were where the user
/// actually dragged/clicked to create it — before driving the frame
/// under test.
///
/// Runs one plain, unfocused-to-focused warm-up frame before injecting
/// the selection: a selection only ever set via `text_area::set_caret`
/// without the widget having actually lived through a real frame first
/// wouldn't match how a real drag-selection always happens on a frame
/// *after* the widget already has focus, so this just makes the test
/// match that.
fn focused_frame_with_selection(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    selection: std::ops::Range<usize>,
    events: Vec<egui::Event>,
) {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });

    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: selection.end,
            anchor: selection.start,
        },
    );

    // Same derivation as `focused_frame`: `RawInput::modifiers` (what
    // `ui.input(|i| i.modifiers)` actually reads) is separate from each
    // `Event::Key`'s own `modifiers` field, so a simulated Shift+Tab
    // needs it pulled up to the top level or `show`'s
    // `ui.input(|i| i.modifiers.shift)` check would read unmodified.
    let modifiers = events
        .iter()
        .find_map(|e| match e {
            egui::Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let raw_input = egui::RawInput {
        events,
        modifiers,
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
}

/// Like `focused_frame_with_selection`, but returns the resulting
/// cursor/selection range — needed for a purely cursor-moving
/// interception like Home/Shift+Home, which leaves the buffer itself
/// unchanged, so `doc.buffer` alone can't confirm anything moved.
fn focused_frame_with_selection_returning_cursor(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    selection: std::ops::Range<usize>,
    events: Vec<egui::Event>,
) -> Caret {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });

    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: selection.end,
            anchor: selection.start,
        },
    );

    let modifiers = events
        .iter()
        .find_map(|e| match e {
            egui::Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let raw_input = egui::RawInput {
        events,
        modifiers,
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });

    text_area::peek_caret(&ctx, id).expect("caret should be set after a focused frame")
}

/// Like `focused_frame`, but for exercising multi-cursor edits: `show`
/// reads the primary cursor's *persisted* `TextEditState` to apply a
/// multi-cursor edit before its own `TextEdit::show()` call runs (see
/// `show`'s multi-cursor block), so — same reasoning as
/// `focused_frame_with_selection` above — this runs one warm-up frame
/// first to establish that persisted state before driving the frame
/// under test. This isn't just a test-harness nicety: in real usage
/// `doc.extra_selections` can only ever become non-empty via an earlier
/// `Ctrl+D` frame, so a widget with multi-cursor active has necessarily
/// already lived through at least one prior frame — a single dry frame
/// with `extra_selections` pre-seeded, as `focused_frame` alone would
/// give it, is a scenario that can't happen outside a test.
fn focused_frame_with_extra_selections(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    extra_selections: Vec<std::ops::Range<usize>>,
    events: Vec<egui::Event>,
) {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });

    doc.extra_selections = extra_selections;

    let modifiers = events
        .iter()
        .find_map(|e| match e {
            egui::Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let raw_input = egui::RawInput {
        events,
        modifiers,
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
}

/// Like `focused_frame`, but runs a warm-up frame first and lets the
/// caller choose `IndentSettings` — needed to exercise the plain-
/// Tab-with-no-selection interception, which (like wrap-selection and
/// multi-cursor) reads the *persisted* selection from before
/// `TextEdit::show()` runs this frame, so a single dry frame with no
/// prior state can't reach it. See `focused_frame_with_selection`'s doc
/// comment for why a real warm-up frame, not just a stored
/// `TextEditState`, is what's needed.
fn focused_frame_with_indent_settings(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    indent_settings: IndentSettings,
    events: Vec<egui::Event>,
) {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            indent_settings,
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });

    let modifiers = events
        .iter()
        .find_map(|e| match e {
            egui::Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let raw_input = egui::RawInput {
        events,
        modifiers,
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            indent_settings,
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
}

/// Combines `focused_frame_with_indent_settings` (custom
/// `IndentSettings`) and `focused_frame_with_selection` (an injected
/// cursor/selection) — needed to exercise the Tab-with-no-selection
/// live-template path under a non-default indent mode, which needs
/// both: the cursor positioned right after a trigger word, and control
/// over `use_tabs`.
fn focused_frame_with_indent_settings_and_selection(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    indent_settings: IndentSettings,
    selection: std::ops::Range<usize>,
    events: Vec<egui::Event>,
) {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            indent_settings,
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });

    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: selection.end,
            anchor: selection.start,
        },
    );

    let modifiers = events
        .iter()
        .find_map(|e| match e {
            egui::Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let raw_input = egui::RawInput {
        events,
        modifiers,
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            indent_settings,
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
}

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

fn shift_key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    }
}

fn alt_key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            alt: true,
            ..egui::Modifiers::NONE
        },
    }
}

fn alt_shift_key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            alt: true,
            shift: true,
            ..egui::Modifiers::NONE
        },
    }
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
fn shift_tab_with_no_selection_is_left_to_egui_regardless_of_indent_mode() {
    // The new spaces-mode interception only ever fires for plain Tab —
    // Shift+Tab with no selection is (and remains) egui's own no-
    // selection dedent handling, untouched by `indent_settings`.
    let (_dir, mut doc) = open_fixture("    abc", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let spaces_mode = IndentSettings {
        use_tabs: false,
        width: 4,
    };
    focused_frame_with_indent_settings(
        &mut doc,
        &mut parser,
        spaces_mode,
        vec![shift_key_event(egui::Key::Tab)],
    );

    // Whatever egui's own no-selection Shift+Tab does, the buffer must
    // not have grown by a spaces-mode insertion — the interception must
    // not have fired.
    assert!(doc.buffer.to_string().len() <= "    abc".len());
}

#[test]
fn tab_after_a_known_java_trigger_word_expands_the_live_template() {
    let (_dir, mut doc) = open_fixture("sout", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    // Collapsed cursor (4..4) right after "sout" — not a real
    // selection, just how `focused_frame_with_selection` positions a
    // bare cursor via the persisted `TextEditState` the Tab
    // interception reads.
    focused_frame_with_selection(&mut doc, &mut parser, 4..4, vec![key_event(egui::Key::Tab)]);

    assert_eq!(doc.buffer.to_string(), "System.out.println();");
}

#[test]
fn tab_after_a_known_kotlin_trigger_word_expands_the_kotlin_template() {
    let (_dir, mut doc) = open_fixture("sout", "Hello.kt");
    let mut parser = parsed(Language::Kotlin, &doc.buffer.to_string());

    focused_frame_with_selection(&mut doc, &mut parser, 4..4, vec![key_event(egui::Key::Tab)]);

    assert_eq!(doc.buffer.to_string(), "println()");
}

#[test]
fn tab_after_an_unknown_word_falls_through_to_normal_spaces_indentation() {
    let (_dir, mut doc) = open_fixture("xyz", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    focused_frame_with_selection(&mut doc, &mut parser, 3..3, vec![key_event(egui::Key::Tab)]);

    // No template matches "xyz" — default (spaces) indentation applies
    // instead, same as plain Tab anywhere else with no selection.
    assert_eq!(doc.buffer.to_string(), "xyz    ");
}

#[test]
fn tab_after_a_trigger_word_expands_even_in_tabs_mode() {
    // Live-template expansion is orthogonal to the tabs-vs-spaces
    // setting — it must win even when `use_tabs` would otherwise leave
    // plain Tab un-intercepted.
    let (_dir, mut doc) = open_fixture("sout", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let tabs_mode = IndentSettings {
        use_tabs: true,
        width: 4,
    };
    focused_frame_with_indent_settings_and_selection(
        &mut doc,
        &mut parser,
        tabs_mode,
        4..4,
        vec![key_event(egui::Key::Tab)],
    );

    assert_eq!(doc.buffer.to_string(), "System.out.println();");
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

fn command_key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    }
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
fn read_only_doc_ignores_typed_text() {
    let (_dir, mut doc) = open_fixture("foo", "notes.txt");
    doc.read_only = true;
    let mut parser: Option<IncrementalParser> = None;

    focused_frame(&mut doc, &mut parser, vec![egui::Event::Text("x".to_string())]);

    assert_eq!(doc.buffer.to_string(), "foo");
}

#[test]
fn read_only_doc_ignores_backspace() {
    let (_dir, mut doc) = open_fixture("foo", "notes.txt");
    doc.read_only = true;
    let mut parser: Option<IncrementalParser> = None;

    focused_frame(&mut doc, &mut parser, vec![key_event(egui::Key::Backspace)]);

    assert_eq!(doc.buffer.to_string(), "foo");
}

#[test]
fn read_only_doc_ignores_ctrl_j_join_lines() {
    let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
    doc.read_only = true;
    let mut parser: Option<IncrementalParser> = None;

    focused_frame(&mut doc, &mut parser, vec![command_key_event(egui::Key::J)]);

    assert_eq!(doc.buffer.to_string(), "foo\nbar");
}

#[test]
fn read_only_doc_ignores_generate_request() {
    let (_dir, mut doc) = open_fixture("class Foo {\n    private int x;\n}\n", "Foo.java");
    doc.read_only = true;
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let original = doc.buffer.to_string();

    focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Both), &mut None, vec![]);

    assert_eq!(doc.buffer.to_string(), original);
}

#[test]
fn is_mutating_event_blocks_typed_text_paste_and_cut() {
    assert!(is_mutating_event(&egui::Event::Text("a".to_string())));
    assert!(is_mutating_event(&egui::Event::Paste("a".to_string())));
    assert!(is_mutating_event(&egui::Event::Cut));
}

#[test]
fn is_mutating_event_blocks_known_editing_shortcuts() {
    for key in [
        egui::Key::Backspace,
        egui::Key::Delete,
        egui::Key::Tab,
        egui::Key::Enter,
        egui::Key::J,
        egui::Key::G,
        egui::Key::Slash,
        egui::Key::U,
        egui::Key::L,
        egui::Key::Z,
        egui::Key::Y,
    ] {
        assert!(
            is_mutating_event(&key_event(key)),
            "{key:?} should be treated as mutating"
        );
    }
    assert!(is_mutating_event(&alt_key_event(egui::Key::ArrowUp)));
    assert!(is_mutating_event(&alt_key_event(egui::Key::ArrowDown)));
}

#[test]
fn is_mutating_event_allows_navigation_and_copy() {
    for key in [
        egui::Key::ArrowUp,
        egui::Key::ArrowDown,
        egui::Key::ArrowLeft,
        egui::Key::ArrowRight,
        egui::Key::Home,
        egui::Key::End,
        egui::Key::PageUp,
        egui::Key::PageDown,
        egui::Key::Escape,
        egui::Key::D, // Ctrl+D occurrence select — not a mutation
        egui::Key::W, // Ctrl+W/Ctrl+Shift+W expand/shrink selection — not a mutation
    ] {
        assert!(
            !is_mutating_event(&key_event(key)),
            "{key:?} should not be treated as mutating"
        );
    }
    assert!(!is_mutating_event(&egui::Event::Copy));
}

/// Runs one more frame on `ctx`/`id` (already focused and holding
/// whatever selection the previous frame left persisted — unlike
/// `focused_frame_with_selection`, this does *not* reset the selection
/// first) with `event` as the only input, and returns the resulting
/// persisted selection as a plain `Range<usize>`. Shared by the
/// Ctrl+W/Ctrl+Shift+W tests below, which need to chain several presses
/// in sequence — each reading the *previous* press's result as its own
/// starting selection — rather than the single request/response shape
/// every other helper in this file provides.
fn run_frame_reading_selection(
    ctx: &egui::Context,
    id: egui::Id,
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    event: egui::Event,
) -> std::ops::Range<usize> {
    let modifiers = match &event {
        egui::Event::Key { modifiers, .. } => *modifiers,
        _ => egui::Modifiers::NONE,
    };
    let raw_input = egui::RawInput {
        events: vec![event],
        modifiers,
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
    text_area::peek_caret(ctx, id)
        .expect("a selection should be persisted after the frame")
        .range()
}

#[test]
fn ctrl_w_expands_selection_by_syntax_node_and_ctrl_shift_w_shrinks_back() {
    let source = "class Foo {\n    void run() {\n        foo();\n    }\n}\n";
    let (_dir, mut doc) = open_fixture(source, "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
    let call_start = source.find("foo()").unwrap();

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: call_start + 3,
            anchor: call_start,
        },
    );

    let after_first_expand =
        run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_key_event(egui::Key::W));
    assert_eq!(&source[after_first_expand.clone()], "foo()");

    let after_second_expand =
        run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_key_event(egui::Key::W));
    assert!(
        after_second_expand.start <= after_first_expand.start && after_second_expand.end >= after_first_expand.end,
        "each expand must grow the selection: {after_first_expand:?} -> {after_second_expand:?}"
    );
    assert_ne!(after_second_expand, after_first_expand);

    let after_shrink =
        run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_shift_key_event(egui::Key::W));
    assert_eq!(
        after_shrink, after_first_expand,
        "shrink must restore exactly what the second expand grew out of"
    );

    // Pure selection movement — the buffer itself must never change.
    assert_eq!(doc.buffer.to_string(), source);
}

#[test]
fn ctrl_w_on_a_file_with_no_parsed_tree_is_a_no_op_that_still_consumes_the_keystroke() {
    // No language recognized for "notes.txt" — `parser` stays `None`
    // throughout, so there's no tree for `Ctrl+W` to act on. This must
    // not fall through to egui's own built-in `Ctrl+W` ("delete
    // previous word"): the buffer has to come out unchanged either way.
    let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection(&mut doc, &mut parser, 6..6, vec![command_key_event(egui::Key::W)]);

    assert_eq!(doc.buffer.to_string(), "hello world");
}

#[test]
fn ctrl_w_still_expands_selection_on_a_read_only_java_file() {
    // Non-mutating navigation, same as Ctrl+D — must keep working even
    // when `doc.read_only` blocks every actual edit path.
    let source = "class Foo {\n    int x;\n}\n";
    let (_dir, mut doc) = open_fixture(source, "Foo.java");
    doc.read_only = true;
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let x_pos = source.find('x').unwrap();

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: x_pos,
            anchor: x_pos,
        },
    );

    let expanded = run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_key_event(egui::Key::W));

    assert!(
        !expanded.is_empty(),
        "Ctrl+W should still grow the selection on a read-only file"
    );
    assert_eq!(
        doc.buffer.to_string(),
        source,
        "read-only must still block any actual edit"
    );
}

fn alt_click_events(pos: egui::Pos2) -> Vec<egui::Event> {
    let modifiers = egui::Modifiers {
        alt: true,
        ..egui::Modifiers::NONE
    };
    vec![
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers,
        },
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        },
    ]
}

#[test]
fn alt_click_adds_a_bare_extra_cursor_without_moving_the_primary_one() {
    let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    // First frame: establishes the widget's real on-screen rect (read
    // back via `Context::read_response`, the same cache egui itself
    // uses) and parks the primary cursor at a known, deliberately
    // *not*-start-of-buffer position — so "the primary cursor didn't
    // move" below is actually testing something.
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
    text_area::set_caret(&ctx, id, Caret { primary: 6, anchor: 6 });
    let widget_rect = ctx
        .read_response(id)
        .expect("TextEdit response cached after a frame")
        .rect;

    // Just inside the widget's own rect — exactly which character this
    // resolves to isn't asserted (that's `egui::Galley`'s own geometry,
    // not this feature's logic to re-verify); only that a click *inside
    // the widget* produces an extra bare caret without disturbing the
    // primary cursor is.
    let click_pos = widget_rect.left_top() + egui::vec2(2.0, 2.0);
    let events = alt_click_events(click_pos);
    let modifiers = egui::Modifiers {
        alt: true,
        ..egui::Modifiers::NONE
    };
    let raw_input = egui::RawInput {
        events,
        modifiers,
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });

    assert_eq!(
        doc.extra_selections.len(),
        1,
        "Alt+Click should add exactly one extra selection"
    );
    assert!(
        doc.extra_selections[0].is_empty(),
        "Alt+Click's extra selection should be a bare caret"
    );

    let primary_after = text_area::peek_caret(&ctx, id).unwrap();
    assert_eq!(
        (primary_after.primary, primary_after.anchor),
        (6, 6),
        "the primary cursor must stay exactly where it was before the Alt+Click"
    );
    assert_eq!(
        doc.buffer.to_string(),
        "hello world",
        "Alt+Click must never mutate the buffer"
    );
}

#[test]
fn alt_click_on_the_same_position_twice_does_not_duplicate_the_extra_cursor() {
    let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
    let widget_rect = ctx
        .read_response(id)
        .expect("TextEdit response cached after a frame")
        .rect;
    let click_pos = widget_rect.left_top() + egui::vec2(2.0, 2.0);
    let modifiers = egui::Modifiers {
        alt: true,
        ..egui::Modifiers::NONE
    };

    for _ in 0..2 {
        let raw_input = egui::RawInput {
            events: alt_click_events(click_pos),
            modifiers,
            ..Default::default()
        };
        let _ = ctx.run_ui(raw_input, |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(
                ui,
                &mut doc,
                &mut parser,
                EditorFont::Default,
                14.0,
                IndentSettings::default(),
                ViewSettings::default(),
                None,
                &mut None,
                None,
                &mut None,
                None,
                false,
                &mut None,
                None,
                false,
                false,
                false,
                false,
                &mut None,
                &mut Vec::new(),
                &mut None,
            );
        });
    }

    assert_eq!(
        doc.extra_selections.len(),
        1,
        "clicking the identical position twice must not add a duplicate bare caret"
    );
}

#[test]
fn ctrl_slash_comments_the_current_line() {
    let (_dir, mut doc) = open_fixture("foo();", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // Ctrl+/ reads the *persisted* selection (see `show`'s comment on
    // this interception), so — like wrap-selection/Tab/Alt+Arrow — it
    // needs `focused_frame_with_selection`'s warm-up frame, not the
    // single dry frame `focused_frame` gives; a collapsed 0..0
    // selection is just "cursor at the start, nothing selected".
    focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![command_key_event(egui::Key::Slash)]);

    assert_eq!(doc.buffer.to_string(), "// foo();");
}

#[test]
fn ctrl_slash_uncomments_an_already_commented_line() {
    let (_dir, mut doc) = open_fixture("// foo();", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![command_key_event(egui::Key::Slash)]);

    assert_eq!(doc.buffer.to_string(), "foo();");
}

#[test]
fn ctrl_slash_toggles_every_line_a_selection_touches() {
    let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    // All of "foo" and all of "bar" (chars 0..7).
    focused_frame_with_selection(&mut doc, &mut parser, 0..7, vec![command_key_event(egui::Key::Slash)]);

    assert_eq!(doc.buffer.to_string(), "// foo\n// bar");
}

fn command_shift_key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            shift: true,
            ..egui::Modifiers::NONE
        },
    }
}

#[test]
fn ctrl_shift_g_generates_getter_and_setter_at_the_cursor() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    // A fresh widget's default cursor sits at char 0, which is still
    // "inside" the class_declaration spanning the whole file — see
    // `syntax::java_fields_in_enclosing_class`'s inclusive containment
    // check.
    focused_frame(&mut doc, &mut parser, vec![command_shift_key_event(egui::Key::G)]);

    let text = doc.buffer.to_string();
    assert!(text.contains("public int getX() {\n        return this.x;\n    }"));
    assert!(text.contains("public void setX(int x) {\n        this.x = x;\n    }"));
}

/// Like `focused_frame`, but exposes the `generate_request`/`last_error`
/// parameters `Tools > Generate Getters/Setters` and `Ctrl+Shift+G`
/// feed `show`, returning whatever ends up in `last_error` — used to
/// verify every non-applicable case (wrong file type, no fields)
/// surfaces visible feedback instead of a silent no-op, which is easy
/// to mistake for "the shortcut doesn't work."
fn focused_frame_with_generate_request(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    generate_request: Option<AccessorKind>,
    generate_dialog: &mut Option<GenerateAccessorsDialog>,
    events: Vec<egui::Event>,
) -> Option<String> {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let modifiers = events
        .iter()
        .find_map(|e| match e {
            egui::Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let raw_input = egui::RawInput {
        events,
        modifiers,
        ..Default::default()
    };
    let mut last_error = None;
    let _ = ctx.run_ui(raw_input, |ui| {
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            generate_request,
            generate_dialog,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut last_error,
            &mut Vec::new(),
            &mut None,
        );
    });
    last_error
}

/// Like `focused_frame_with_selection`, but also lets the caller drive
/// `case_conversion_request` (the Tools menu path) and reads back
/// `last_error` — needed because case conversion, like Ctrl+/, reads
/// the *persisted* selection (see `show`'s comment on that
/// interception), so it needs the same warm-up-frame treatment
/// `focused_frame_with_selection` already gives wrap-selection/Tab.
fn focused_frame_with_selection_and_case_request(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    selection: std::ops::Range<usize>,
    case_conversion_request: Option<CaseConversion>,
    events: Vec<egui::Event>,
) -> Option<String> {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });

    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: selection.end,
            anchor: selection.start,
        },
    );

    let modifiers = events
        .iter()
        .find_map(|e| match e {
            egui::Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let raw_input = egui::RawInput {
        events,
        modifiers,
        ..Default::default()
    };
    let mut last_error = None;
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            case_conversion_request,
            false,
            false,
            false,
            false,
            &mut last_error,
            &mut Vec::new(),
            &mut None,
        );
    });
    last_error
}

/// Like `focused_frame_with_selection_and_case_request`, but for
/// `sort_lines_request`/`unique_lines_request` — same two-phase shape
/// (a warm-up frame to persist the selection, then a second frame that
/// actually drives the request), since sort/unique lines reads the
/// *persisted* selection the same way case conversion does.
fn focused_frame_with_selection_and_line_op_request(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    selection: std::ops::Range<usize>,
    sort_lines_request: bool,
    unique_lines_request: bool,
) {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });

    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: selection.end,
            anchor: selection.start,
        },
    );

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            None,
            sort_lines_request,
            unique_lines_request,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
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

#[test]
fn read_only_doc_ignores_sort_lines_request() {
    let (_dir, mut doc) = open_fixture("banana\napple", "notes.txt");
    doc.read_only = true;
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection_and_line_op_request(&mut doc, &mut parser, 0..12, true, false);

    assert_eq!(doc.buffer.to_string(), "banana\napple");
}

fn command_shift_u_event() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::U,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            shift: true,
            ..egui::Modifiers::NONE
        },
    }
}

fn command_shift_l_event() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::L,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            shift: true,
            ..egui::Modifiers::NONE
        },
    }
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
fn ctrl_shift_g_is_a_no_op_for_kotlin_files_but_reports_why() {
    // Kotlin's `val`/`var` properties already are getters/setters;
    // generating explicit Java-shaped ones for them isn't idiomatic
    // (see `widget::show`'s comment on this shortcut), so the command
    // does nothing for a non-Java file — but must say so via
    // `last_error` rather than silently doing nothing.
    let (_dir, mut doc) = open_fixture("class Foo(val x: Int)\n", "Foo.kt");
    let mut parser = parsed(Language::Kotlin, &doc.buffer.to_string());
    let before = doc.buffer.to_string();

    let last_error = focused_frame_with_generate_request(
        &mut doc,
        &mut parser,
        None,
        &mut None,
        vec![command_shift_key_event(egui::Key::G)],
    );

    assert_eq!(doc.buffer.to_string(), before);
    assert!(last_error.is_some_and(|msg| msg.contains("Java")));
}

#[test]
fn ctrl_shift_g_on_a_fieldless_class_is_a_no_op_but_reports_why() {
    let (_dir, mut doc) = open_fixture("public class Empty {\n}\n", "Empty.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let before = doc.buffer.to_string();

    let last_error = focused_frame_with_generate_request(
        &mut doc,
        &mut parser,
        None,
        &mut None,
        vec![command_shift_key_event(egui::Key::G)],
    );

    assert_eq!(doc.buffer.to_string(), before);
    assert!(last_error.is_some_and(|msg| msg.contains("fields")));
}

#[test]
fn tools_menu_generate_getters_inserts_only_a_getter() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let last_error =
        focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Getters), &mut None, vec![]);

    assert_eq!(last_error, None);
    let text = doc.buffer.to_string();
    assert!(text.contains("getX"));
    assert!(!text.contains("setX"));
}

#[test]
fn tools_menu_generate_setters_inserts_only_a_setter() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let last_error =
        focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Setters), &mut None, vec![]);

    assert_eq!(last_error, None);
    let text = doc.buffer.to_string();
    assert!(!text.contains("getX"));
    assert!(text.contains("setX"));
}

#[test]
fn tools_menu_generate_setters_on_an_all_final_class_reports_why() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private final int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let before = doc.buffer.to_string();

    let last_error =
        focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Setters), &mut None, vec![]);

    assert_eq!(doc.buffer.to_string(), before);
    assert!(last_error.is_some_and(|msg| msg.contains("final")));
}

#[test]
fn tools_menu_generate_getters_on_a_multi_class_file_opens_the_picker_instead_of_generating() {
    let (_dir, mut doc) = open_fixture(
        "class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n",
        "Foo.java",
    );
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let before = doc.buffer.to_string();
    let mut generate_dialog = None;

    let last_error = focused_frame_with_generate_request(
        &mut doc,
        &mut parser,
        Some(AccessorKind::Getters),
        &mut generate_dialog,
        vec![],
    );

    assert_eq!(last_error, None);
    assert_eq!(
        doc.buffer.to_string(),
        before,
        "nothing should be inserted until the picker's Generate is clicked"
    );
    let dialog = generate_dialog.expect("multiple eligible classes should open the picker");
    assert_eq!(
        dialog.classes().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["Foo", "Bar"]
    );
}

/// Like `focused_frame_with_generate_request`, but for
/// `generate_method_request`/`generate_method_dialog` (Constructor/
/// toString/equals+hashCode) instead of `generate_request`/
/// `generate_dialog` (Getters/Setters).
fn focused_frame_with_generate_method_request(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    generate_method_request: Option<GenerateMethodKind>,
    generate_method_dialog: &mut Option<GenerateMethodDialog>,
) -> Option<String> {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut last_error = None;
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            doc,
            parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            generate_method_request,
            generate_method_dialog,
            None,
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut last_error,
            &mut Vec::new(),
            &mut None,
        );
    });
    last_error
}

#[test]
fn tools_menu_generate_constructor_inserts_immediately_for_a_single_eligible_class() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let last_error = focused_frame_with_generate_method_request(
        &mut doc,
        &mut parser,
        Some(GenerateMethodKind::Constructor),
        &mut None,
    );

    assert_eq!(last_error, None);
    assert!(doc.buffer.to_string().contains("public Foo(int x)"));
}

#[test]
fn tools_menu_generate_to_string_inserts_immediately_for_a_single_eligible_class() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let last_error = focused_frame_with_generate_method_request(
        &mut doc,
        &mut parser,
        Some(GenerateMethodKind::ToString),
        &mut None,
    );

    assert_eq!(last_error, None);
    assert!(doc.buffer.to_string().contains("public String toString()"));
}

#[test]
fn tools_menu_generate_equals_and_hash_code_inserts_both_together() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let last_error = focused_frame_with_generate_method_request(
        &mut doc,
        &mut parser,
        Some(GenerateMethodKind::EqualsAndHashCode),
        &mut None,
    );

    assert_eq!(last_error, None);
    let text = doc.buffer.to_string();
    assert!(text.contains("public boolean equals(Object o)"));
    assert!(text.contains("public int hashCode()"));
}

#[test]
fn generate_method_request_on_a_kotlin_file_reports_why() {
    let (_dir, mut doc) = open_fixture("class Foo(val x: Int)\n", "Foo.kt");
    let mut parser = parsed(Language::Kotlin, &doc.buffer.to_string());
    let before = doc.buffer.to_string();

    let last_error = focused_frame_with_generate_method_request(
        &mut doc,
        &mut parser,
        Some(GenerateMethodKind::Constructor),
        &mut None,
    );

    assert_eq!(doc.buffer.to_string(), before);
    assert!(last_error.is_some_and(|msg| msg.contains("Java")));
}

#[test]
fn generate_method_request_on_a_multi_class_file_opens_the_picker_instead_of_generating() {
    let (_dir, mut doc) = open_fixture(
        "class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n",
        "Foo.java",
    );
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let before = doc.buffer.to_string();
    let mut generate_method_dialog = None;

    let last_error = focused_frame_with_generate_method_request(
        &mut doc,
        &mut parser,
        Some(GenerateMethodKind::ToString),
        &mut generate_method_dialog,
    );

    assert_eq!(last_error, None);
    assert_eq!(
        doc.buffer.to_string(),
        before,
        "nothing should be inserted until the picker's Generate is clicked"
    );
    let dialog = generate_method_dialog.expect("multiple eligible classes should open the picker");
    assert_eq!(
        dialog.classes().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["Foo", "Bar"]
    );
}

#[test]
fn read_only_doc_ignores_generate_method_request() {
    let (_dir, mut doc) = open_fixture("class Foo {\n    private int x;\n}\n", "Foo.java");
    doc.read_only = true;
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let before = doc.buffer.to_string();

    focused_frame_with_generate_method_request(&mut doc, &mut parser, Some(GenerateMethodKind::Constructor), &mut None);

    assert_eq!(doc.buffer.to_string(), before);
}

#[test]
fn override_method_finds_an_inherited_method_via_the_project_tree() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Base.java"),
        "public class Base {\n    public void run() {\n    }\n}\n",
    )
    .unwrap();
    let foo_path = dir.path().join("Foo.java");
    std::fs::write(&foo_path, "public class Foo extends Base {\n}\n").unwrap();

    let mut doc = Document::open(foo_path).unwrap();
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
    let cursor = doc.buffer.to_string().find('{').unwrap() + 1; // inside Foo's body

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: cursor,
            anchor: cursor,
        },
    );

    let mut override_method_dialog = None;
    let mut last_error = None;
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            true,
            &mut override_method_dialog,
            None,
            false,
            false,
            false,
            false,
            &mut last_error,
            &mut Vec::new(),
            &mut None,
        );
    });

    assert_eq!(last_error, None);
    let dialog = override_method_dialog.expect("Base.run() should be found as an overridable method");
    assert_eq!(dialog.methods().len(), 1);
    assert_eq!(dialog.methods()[0].name, "run");
}

#[test]
fn override_method_excludes_a_method_the_current_class_already_overrides() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Base.java"),
        "public class Base {\n    public void run() {\n    }\n    public void stop() {\n    }\n}\n",
    )
    .unwrap();
    let foo_path = dir.path().join("Foo.java");
    let foo_source = "public class Foo extends Base {\n    public void run() {\n    }\n}\n";
    std::fs::write(&foo_path, foo_source).unwrap();

    let mut doc = Document::open(foo_path).unwrap();
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
    let cursor = foo_source.find('{').unwrap() + 1;

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: cursor,
            anchor: cursor,
        },
    );

    let mut override_method_dialog = None;
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            true,
            &mut override_method_dialog,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });

    let dialog = override_method_dialog.expect("stop() should still be offered");
    assert_eq!(dialog.methods().len(), 1);
    assert_eq!(dialog.methods()[0].name, "stop");
}

#[test]
fn override_method_on_a_superclass_not_found_in_the_project_reports_why() {
    let dir = tempfile::tempdir().unwrap();
    let foo_path = dir.path().join("Foo.java");
    let foo_source = "public class Foo extends SomeLibraryClass {\n}\n";
    std::fs::write(&foo_path, foo_source).unwrap();

    let mut doc = Document::open(foo_path).unwrap();
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
    let cursor = foo_source.find('{').unwrap() + 1;

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            false,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
        );
    });
    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: cursor,
            anchor: cursor,
        },
    );

    let mut override_method_dialog = None;
    let mut last_error = None;
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            true,
            &mut override_method_dialog,
            None,
            false,
            false,
            false,
            false,
            &mut last_error,
            &mut Vec::new(),
            &mut None,
        );
    });

    assert!(override_method_dialog.is_none());
    assert!(last_error.is_some_and(|msg| msg.contains("this project")));
}

#[test]
fn realistic_paste_reparses_and_highlights_correctly() {
    // A realistic paste: inserting a whole new, syntactically valid
    // method into a class body at a clean line boundary.
    let old_text = "public class Hello {\n}\n";
    let pasted = "    public String greet() {\n        return \"hi\";\n    }\n";
    let insert_at = old_text.find('}').unwrap();
    let mut new_text = old_text.to_string();
    new_text.insert_str(insert_at, pasted);

    let mut parser = IncrementalParser::new(Language::Java);
    parser.parse(old_text);
    let edit = syntax::diff_edit(old_text, &new_text);
    parser.reparse(&new_text, edit);

    let tree = parser.tree().unwrap();
    let spans = syntax::highlight_spans(tree, &new_text, Language::Java);

    let has_scope_over = |needle: &str, scope: syntax::Scope| {
        let start = new_text.find(needle).unwrap();
        let end = start + needle.len();
        spans
            .iter()
            .any(|(range, s)| *s == scope && range.start <= start && range.end >= end)
    };
    assert!(has_scope_over("class", syntax::Scope::Keyword));
    assert!(has_scope_over("greet", syntax::Scope::Function));
    assert!(has_scope_over("return", syntax::Scope::Keyword));
    assert!(has_scope_over("\"hi\"", syntax::Scope::String));

    // Cross-check against a from-scratch full parse of the same final
    // text: if incremental reparse after this paste produced the same
    // tree a fresh parse would, the highlighting can't be stale.
    let mut full_parser = IncrementalParser::new(Language::Java);
    full_parser.parse(&new_text);
    let full_spans = syntax::highlight_spans(full_parser.tree().unwrap(), &new_text, Language::Java);
    assert_eq!(spans, full_spans);
}

#[test]
fn multi_cursor_typed_edit_applies_at_every_active_cursor() {
    let (_dir, mut doc) = open_fixture("abcde", "Hello.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    // Primary cursor starts at 0 (a fresh widget's default cursor, from
    // the warm-up frame `focused_frame_with_extra_selections` runs
    // before setting these); the two extras sit at char indices 2 and 4.
    focused_frame_with_extra_selections(
        &mut doc,
        &mut parser,
        vec![2..2, 4..4],
        vec![egui::Event::Text("Y".to_string())],
    );

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

#[test]
fn queued_pending_input_is_drained_as_real_input_before_text_edit_runs() {
    // Proves the mechanism the right-click menu's Undo/Redo/Select All
    // items depend on: a synthetic event pushed into `pending_input`
    // (exactly as those menu items do — see `synthetic_shortcut`) is
    // drained into real input *before* `TextEdit::show()` runs, so
    // egui's own handling for it fires as if the user had actually
    // pressed the key. Undo is the proof here specifically because
    // it's the one whose entire reason for existing behind this queue
    // is that it's driven by egui's own private per-widget undo
    // history — nothing about it is reimplemented on our side, so
    // watching it actually revert text proves the queued event reached
    // real egui event handling, not just our own code.
    //
    // Three frames, with `time` advanced past `Undoer`'s stable-time
    // window (1s) between the second and third: frame 1 (idle) seeds
    // the initial undo point; frame 2 types a character via a real
    // `Event::Text`, the same path any keystroke takes; frame 3, a
    // full second later, both lets that edit's undo point actually
    // commit *and* queues Ctrl+Z via `pending_input`.
    let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let run_frame = |doc: &mut Document,
                     parser: &mut Option<IncrementalParser>,
                     time: f64,
                     events: Vec<egui::Event>,
                     pending_input: &mut Vec<egui::Event>| {
        let raw_input = egui::RawInput {
            events,
            time: Some(time),
            ..Default::default()
        };
        let _ = ctx.run_ui(raw_input, |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(
                ui,
                doc,
                parser,
                EditorFont::Default,
                14.0,
                IndentSettings::default(),
                ViewSettings::default(),
                None,
                &mut None,
                None,
                &mut None,
                None,
                false,
                &mut None,
                None,
                false,
                false,
                false,
                false,
                &mut None,
                pending_input,
                &mut None,
            );
        });
    };

    run_frame(&mut doc, &mut parser, 0.0, vec![], &mut Vec::new());
    run_frame(
        &mut doc,
        &mut parser,
        0.0,
        vec![egui::Event::Text("X".to_string())],
        &mut Vec::new(),
    );
    assert_eq!(doc.buffer.to_string(), "Xhello world");

    let mut pending_input = vec![synthetic_shortcut(egui::Key::Z, false)];
    run_frame(&mut doc, &mut parser, 1.5, vec![], &mut pending_input);

    assert!(pending_input.is_empty(), "the queue should be drained once used");
    assert_eq!(
        doc.buffer.to_string(),
        "hello world",
        "the queued Ctrl+Z should have reverted the typed character"
    );
}
