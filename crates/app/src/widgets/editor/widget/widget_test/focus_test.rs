//! Keyboard input reaches the editor only while it has focus.

use super::super::*;
use super::common_test::*;

/// The editor has been shown before (so it has a persisted caret), then
/// focus moves to another widget — the terminal, a search field. A Tab typed
/// there must neither indent the open file nor vanish from the frame's
/// input before the focused widget gets it.
#[test]
fn keys_typed_into_another_widget_neither_edit_the_file_nor_disappear() {
    let (_dir, mut doc) = open_fixture("foo", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let editor = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(editor));
        show_with_defaults(ui, &mut doc, &mut parser);
    });

    let mut tab_still_queued = false;
    let raw_input = egui::RawInput {
        events: vec![key_event(egui::Key::Tab)],
        ..Default::default()
    };
    let _ = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|mem| mem.request_focus(egui::Id::new("terminal")));
        show_with_defaults(ui, &mut doc, &mut parser);
        tab_still_queued = ui.input(|i| i.key_pressed(egui::Key::Tab));
    });

    assert_eq!(doc.buffer.to_string(), "foo", "the unfocused editor must not indent");
    assert!(tab_still_queued, "the Tab must still be there for the focused widget");
}
