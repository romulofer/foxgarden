//! The right-click context menu's `pending_input` queueing mechanism (see `context_menu.rs`'s own doc comment for why Undo/Redo/Select All are queued rather than applied directly).

use super::super::*;
use super::common_test::*;

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
                &mut None,
                &mut HoverState::default(),
                &mut GotoDefinitionState::default(),
                &mut PeekState::default(),
                EditorRequests::default(),
                &mut None,
                pending_input,
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
