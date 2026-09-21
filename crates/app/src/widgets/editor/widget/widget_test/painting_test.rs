//! Rendering/highlighting/diagnostics smoke tests: sticky scroll, syntax highlighting, bracket-match, squiggles, occurrence highlighting, whitespace/indent guides, and the layout/highlight caches behind them. Mirrors `painting.rs`'s own scope.

use super::super::*;
use super::common_test::*;
use fg_core::Language;

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
            0, // pane (Track 11)
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
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
            true,
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
            0, // pane (Track 11)
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
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
            true,
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
                0, // pane (Track 11)
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
                &mut None,
                &mut HoverState::default(),
                &mut GotoDefinitionState::default(),
                &mut PeekState::default(),
                EditorRequests::default(),
                &mut None,
                &mut Vec::new(),
                &mut None,
                &UserTemplates::default(),
                &mut crate::panels::spring_config::SpringConfigState::default(),
                &mut crate::lsp_state::LspState::default(),
                &mut FindReferencesState::default(),
                &mut RenameBox::default(),
                &mut CodeActionGutter::default(),
                &crate::debug_state::DebugState::default(),
                true,
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
            0, // pane (Track 11)
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
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
            true,
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
            0, // pane (Track 11)
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
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
            true,
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
                0, // pane (Track 11)
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
                &mut None,
                &mut HoverState::default(),
                &mut GotoDefinitionState::default(),
                &mut PeekState::default(),
                EditorRequests::default(),
                &mut None,
                &mut Vec::new(),
                &mut None,
                &UserTemplates::default(),
                &mut crate::panels::spring_config::SpringConfigState::default(),
                &mut crate::lsp_state::LspState::default(),
                &mut FindReferencesState::default(),
                &mut RenameBox::default(),
                &mut CodeActionGutter::default(),
                &crate::debug_state::DebugState::default(),
                true,
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
            0, // pane (Track 11)
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
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
            true,
            &mut None,
        );
    });

    assert!(doc.diagnostics.is_empty());
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
    doc.buffer.replace(Rope::from_str(&new_text));
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
            0, // pane (Track 11)
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
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
            true,
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
            0, // pane (Track 11)
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
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
            true,
            &mut None,
        );
    });
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

    let mut parser = IncrementalParser::new(Language::Java).expect("a bundled grammar must load");
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
    let mut full_parser = IncrementalParser::new(Language::Java).expect("a bundled grammar must load");
    full_parser.parse(&new_text);
    let full_spans = syntax::highlight_spans(full_parser.tree().unwrap(), &new_text, Language::Java);
    assert_eq!(spans, full_spans);
}
