//! Shared test fixtures/event-builders for every `widget::widget_test::*` topic
//! module below — `open_fixture`/`parsed`/the `focused_frame*` family/the
//! `*_event` builders are all reused across more than one topic, so they
//! live here once rather than being copy-pasted per file. `pub(super)` on
//! each one keeps them visible to every sibling test module without making
//! them part of `widget`'s own public surface.

use super::super::*;
use fg_core::Language;

/// `test_support::temp_document` takes `(name, contents)`; every one of
/// this module's ~50 call sites was already written the other way
/// around (`(contents, filename)`, matching how the fixture text reads
/// as the "main" argument in a test body), so this keeps that order
/// rather than touching all of them.
pub(super) fn open_fixture(contents: &str, filename: &str) -> (tempfile::TempDir, Document) {
    test_support::temp_document(filename, contents)
}

/// Builds a freshly parsed `Some(IncrementalParser)`, matching what
/// `panels::tabs::open_parser_for` produces for any file with a
/// recognized language — `show`'s tests always exercise the "has a
/// language" path unless a test says otherwise.
pub(super) fn parsed(language: Language, source: &str) -> Option<IncrementalParser> {
    // A parser needs the shipped grammars installed (Track 24
    // Phase 3); this test builds one by hand rather than through a
    // fixture that would have installed them already.
    test_support::install_grammars();
    let mut parser = IncrementalParser::new(language).expect("an installed grammar must load");
    parser.parse(source);
    Some(parser)
}

// The tests below drive `show` through a real, reused `egui::Context`
// (rather than the fire-and-forget `egui::__run_test_ui` used above),
// since they need to simulate focused keyboard events: `show`'s
// multi-cursor branches only run once `output.cursor_range` is `Some`,
// which egui only produces for a widget that currently has keyboard
// focus. `show`'s `.id_salt(doc.path...)` makes the widget's id
// reproducible outside of `show` itself, so a test can request focus on
// exactly that id before calling `show`.
pub(super) fn focused_frame(doc: &mut Document, parser: &mut Option<IncrementalParser>, events: Vec<egui::Event>) {
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

/// Drives `show` across a sequence of frames on one shared, focused
/// `egui::Context` — needed to reproduce a real multi-keystroke typing
/// session, where `completion`'s state from one frame affects the next
/// frame's triggers, unlike `focused_frame`'s one-shot use. A warm-up
/// frame establishes focus and lets the caret be placed via
/// `text_area::set_caret` (same shape `focused_frame_with_selection`
/// above uses); each of `frames_events` then gets its own real frame.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors `show`'s own parameter list, which carries the same allowance for the same reason — a test driver that bundled them would stop matching the call it exists to exercise"
)]
pub(super) fn typing_session(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    project: Option<&fg_core::Project>,
    completion: &mut Option<CompletionState>,
    spring_config: &mut crate::panels::spring_config::SpringConfigState,
    lsp: &mut crate::lsp_state::LspState,
    find_references: &mut FindReferencesState,
    rename_box: &mut RenameBox,
    code_action_gutter: &mut CodeActionGutter,
    initial_caret: usize,
    frames_events: Vec<Vec<egui::Event>>,
) {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let mut run_frame = |ctx: &egui::Context, raw_input: egui::RawInput| {
        let _ = ctx.run_ui(raw_input, |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
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
                project,
                false,
                &mut None,
                completion,
                &mut HoverState::default(),
                &mut GotoDefinitionState::default(),
                &mut PeekState::default(),
                EditorRequests::default(),
                &mut None,
                &mut Vec::new(),
                &mut None,
                &UserTemplates::default(),
                spring_config,
                lsp,
                find_references,
                rename_box,
                code_action_gutter,
                &crate::debug_state::DebugState::default(),
                true,
                &mut None,
            );
        });
    };

    run_frame(&ctx, egui::RawInput::default());
    text_area::set_caret(&ctx, id, Caret::at(initial_caret));

    for events in frames_events {
        let modifiers = events
            .iter()
            .find_map(|e| match e {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        run_frame(
            &ctx,
            egui::RawInput {
                events,
                modifiers,
                ..Default::default()
            },
        );
    }
}

pub(super) fn key_event(key: egui::Key) -> egui::Event {
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
pub(super) fn focused_frame_with_selection(
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

/// Like `focused_frame_with_selection`, but returns the resulting
/// cursor/selection range — needed for a purely cursor-moving
/// interception like Home/Shift+Home, which leaves the buffer itself
/// unchanged, so `doc.buffer` alone can't confirm anything moved.
pub(super) fn focused_frame_with_selection_returning_cursor(
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
pub(super) fn focused_frame_with_extra_selections(
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

/// Like `focused_frame`, but runs a warm-up frame first and lets the
/// caller choose `IndentSettings` — needed to exercise the plain-
/// Tab-with-no-selection interception, which (like wrap-selection and
/// multi-cursor) reads the *persisted* selection from before
/// `TextEdit::show()` runs this frame, so a single dry frame with no
/// prior state can't reach it. See `focused_frame_with_selection`'s doc
/// comment for why a real warm-up frame, not just a stored
/// `TextEditState`, is what's needed.
pub(super) fn focused_frame_with_indent_settings(
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
            0, // pane (Track 11)
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
            0, // pane (Track 11)
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

/// Combines `focused_frame_with_indent_settings` (custom
/// `IndentSettings`) and `focused_frame_with_selection` (an injected
/// cursor/selection) — needed to exercise the Tab-with-no-selection
/// live-template path under a non-default indent mode, which needs
/// both: the cursor positioned right after a trigger word, and control
/// over `use_tabs`.
pub(super) fn focused_frame_with_indent_settings_and_selection(
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
            0, // pane (Track 11)
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
            0, // pane (Track 11)
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

pub(super) fn shift_key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    }
}

pub(super) fn alt_key_event(key: egui::Key) -> egui::Event {
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

pub(super) fn alt_shift_key_event(key: egui::Key) -> egui::Event {
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

pub(super) fn command_key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    }
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
pub(super) fn run_frame_reading_selection(
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
    text_area::peek_caret(ctx, id)
        .expect("a selection should be persisted after the frame")
        .range()
}

pub(super) fn alt_click_events(pos: egui::Pos2) -> Vec<egui::Event> {
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

/// `count` press/release pairs at the same `pos`, all in one input batch —
/// enough to make egui's own click-counting (`PointerState::begin_pass`,
/// which compares `time` deltas between clicks to decide double/triple)
/// register the *last* release as a double/triple click: every event in one
/// `RawInput` shares that pass's single `time` value, so back-to-back pairs
/// at the same position always land inside the double-click window — no
/// need to drive separate frames with real elapsed time between them, same
/// one-event-batch shape `alt_click_events` already uses for a single click.
pub(super) fn multi_click_events(pos: egui::Pos2, count: usize) -> Vec<egui::Event> {
    let mut events = Vec::with_capacity(count * 2);
    for _ in 0..count {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
    }
    events
}

pub(super) fn command_shift_key_event(key: egui::Key) -> egui::Event {
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

/// Like `focused_frame`, but exposes the `generate_request`/`last_error`
/// parameters `Tools > Generate Getters/Setters` and `Ctrl+Shift+G`
/// feed `show`, returning whatever ends up in `last_error` — used to
/// verify every non-applicable case (wrong file type, no fields)
/// surfaces visible feedback instead of a silent no-op, which is easy
/// to mistake for "the shortcut doesn't work."
pub(super) fn focused_frame_with_generate_request(
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
            0, // pane (Track 11)
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
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut last_error,
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
    last_error
}

/// Like `focused_frame_with_selection`, but also lets the caller drive
/// `case_conversion_request` (the Tools menu path) and reads back
/// `last_error` — needed because case conversion, like Ctrl+/, reads
/// the *persisted* selection (see `show`'s comment on that
/// interception), so it needs the same warm-up-frame treatment
/// `focused_frame_with_selection` already gives wrap-selection/Tab.
pub(super) fn focused_frame_with_selection_and_case_request(
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
            EditorRequests {
                case_conversion: case_conversion_request,
                ..EditorRequests::default()
            },
            &mut last_error,
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
    last_error
}

/// Like `focused_frame_with_selection_and_case_request`, but for
/// `sort_lines_request`/`unique_lines_request` — same two-phase shape
/// (a warm-up frame to persist the selection, then a second frame that
/// actually drives the request), since sort/unique lines reads the
/// *persisted* selection the same way case conversion does.
pub(super) fn focused_frame_with_selection_and_line_op_request(
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
            EditorRequests {
                sort_lines: sort_lines_request,
                unique_lines: unique_lines_request,
                ..EditorRequests::default()
            },
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

pub(super) fn command_shift_u_event() -> egui::Event {
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

pub(super) fn command_shift_l_event() -> egui::Event {
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

/// Like `focused_frame_with_generate_request`, but for
/// `generate_method_request`/`generate_method_dialog` (Constructor/
/// toString/equals+hashCode) instead of `generate_request`/
/// `generate_dialog` (Getters/Setters).
pub(super) fn focused_frame_with_generate_method_request(
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
            0, // pane (Track 11)
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
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut last_error,
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
    last_error
}
