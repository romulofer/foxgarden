//! Mouse-driven selection: Alt+Click's bare extra cursor, double-click word select, triple-click line select.
//!
//! Also covers the Ctrl+hover cursor-icon affordance for the Ctrl+Click
//! go-to-definition gesture (`widget.rs`, right before `goto_definition`
//! itself is fired) — not a selection change, but driven by the same
//! pointer-position-plus-modifier mechanics as everything else in this
//! file.

use super::super::*;
use super::common_test::*;

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
fn double_click_selects_the_whole_word_under_the_click() {
    // A single word with no spaces, tabs, or newlines anywhere in it: in
    // this headless test context (`FontDefinitions::empty()`, same as the
    // Alt+Click tests above) glyph metrics for the fallback font aren't
    // something this test can predict — a real run against this same
    // buffer showed a click 2px from the widget's top-left corner
    // resolving well into the *second* word of a "hello world" buffer, so
    // pixel-to-character hit-testing genuinely isn't reliable here (see the
    // Alt+Click tests' own weaker "don't assume exactly which character"
    // caveat — this needed to go further than that). A buffer with no word
    // boundary anywhere sidesteps it entirely: *no matter* which character
    // the click resolves to, that character is always part of the one word
    // spanning the whole buffer, so the assertion below only depends on
    // double-click selecting the whole word touching wherever the click
    // landed, never on predicting the landing spot itself.
    let (_dir, mut doc) = open_fixture(&"word".repeat(20), "notes.txt");
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
        );
    });
    let widget_rect = ctx
        .read_response(id)
        .expect("TextEdit response cached after a frame")
        .rect;
    let click_pos = widget_rect.left_top() + egui::vec2(2.0, 2.0);

    let raw_input = egui::RawInput {
        events: multi_click_events(click_pos, 2),
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
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
        );
    });

    let caret = text_area::peek_caret(&ctx, id).expect("a caret exists after the double-click");
    let (start, end) = (caret.primary.min(caret.anchor), caret.primary.max(caret.anchor));
    let full_text = doc.buffer.to_string();
    assert_eq!(
        &full_text[start..end],
        full_text.as_str(),
        "double-click should select the whole word under the click, not just move the cursor into it"
    );
}

#[test]
fn triple_click_selects_the_whole_line() {
    // Same "no dependency on exactly which character a click resolves to"
    // reasoning as the double-click test above, taken one step further: a
    // real run showed even the *row* a click lands on is unpredictable in
    // this headless environment (a click 2px from the top-left corner of a
    // "hello world\nsecond line" buffer resolved onto the second line, not
    // the first — glyph metrics for the fallback font under `FontDefinitions
    // ::empty()` don't produce anything resembling real row/column sizes).
    // A buffer with no `\n` anywhere has exactly one logical line no matter
    // how many *visual* rows word-wrap splits it into, so `current_line_
    // range` returns the same whole-buffer range regardless of which row/
    // column the click actually resolves to. `current_line_range`'s own
    // trailing-`\n`-exclusion behavior is already covered directly by
    // `auto_edit`'s tests; this only needs to prove a triple-click applies
    // it as a selection at all.
    let (_dir, mut doc) = open_fixture(&"word".repeat(20), "notes.txt");
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
        );
    });
    let widget_rect = ctx
        .read_response(id)
        .expect("TextEdit response cached after a frame")
        .rect;
    let click_pos = widget_rect.left_top() + egui::vec2(2.0, 2.0);

    let raw_input = egui::RawInput {
        events: multi_click_events(click_pos, 3),
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
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
        );
    });

    let caret = text_area::peek_caret(&ctx, id).expect("a caret exists after the triple-click");
    let (start, end) = (caret.primary.min(caret.anchor), caret.primary.max(caret.anchor));
    let full_text = doc.buffer.to_string();
    assert_eq!(
        &full_text[start..end],
        full_text.as_str(),
        "triple-click should select the whole current line"
    );
}

#[test]
fn ctrl_hover_over_an_identifier_sets_the_pointing_hand_cursor() {
    let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    let ctx = egui::Context::default();
    // Unlike the other tests in this file, `hovered_span`'s own geometry
    // check (`pointer_is_on_span`) needs real glyph widths to make a
    // pixel offset actually land on (or off) a specific identifier —
    // `FontDefinitions::empty()` degenerates every glyph to zero width,
    // which the coarser click-to-caret math the sibling tests exercise
    // tolerates, but this test's `hover_pos` math can't.
    ctx.set_fonts(egui::FontDefinitions::default());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
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
        );
    });
    let widget_rect = ctx
        .read_response(id)
        .expect("TextEdit response cached after a frame")
        .rect;

    // Inside the first identifier ("hello") — same offset the sibling
    // Alt+Click test above lands its own click at, just resting there
    // rather than clicking.
    let hover_pos = widget_rect.left_top() + egui::vec2(2.0, 2.0);
    let raw_input = egui::RawInput {
        events: vec![egui::Event::PointerMoved(hover_pos)],
        modifiers: egui::Modifiers {
            command: true,
            ..egui::Modifiers::NONE
        },
        ..Default::default()
    };
    let full_output = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
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
        );
    });

    // `ctx.output()` (unlike `run_ui`'s own return value) reflects
    // whatever the *next* frame has accumulated so far — effectively
    // empty right after `run_ui` returns, since `run_ui` itself already
    // took this frame's output into the `FullOutput` it hands back. The
    // cursor icon this frame actually set has to be read from there.
    assert_eq!(
        full_output.platform_output.cursor_icon,
        egui::CursorIcon::PointingHand,
        "Ctrl+hover over an identifier should show the pointing-hand cursor"
    );
}

#[test]
fn hover_without_ctrl_leaves_the_default_cursor() {
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
        );
    });
    let widget_rect = ctx
        .read_response(id)
        .expect("TextEdit response cached after a frame")
        .rect;

    let hover_pos = widget_rect.left_top() + egui::vec2(2.0, 2.0);
    let raw_input = egui::RawInput {
        events: vec![egui::Event::PointerMoved(hover_pos)],
        ..Default::default()
    };
    let full_output = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
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
        );
    });

    assert_ne!(
        full_output.platform_output.cursor_icon,
        egui::CursorIcon::PointingHand,
        "without Ctrl held, hovering an identifier must not change the cursor"
    );
}
