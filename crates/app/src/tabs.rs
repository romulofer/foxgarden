use fg_core::EditorState;
use syntax::IncrementalParser;

use crate::editor_widget;
use crate::fonts::EditorFont;

/// Renders the tab bar and the active document's editor. `parsers` is kept
/// index-aligned with `state.open_tabs`; every close here removes the
/// matching parser in the same step.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    pending_close: &mut Option<usize>,
    parsers: &mut Vec<IncrementalParser>,
    editor_font: EditorFont,
) {
    let mut focus_request = None;
    let mut close_request = None;

    ui.horizontal_wrapped(|ui| {
        for (index, doc) in state.open_tabs.iter().enumerate() {
            let name = doc
                .path()
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let label = if doc.is_dirty() {
                format!("*{name}")
            } else {
                name
            };
            let selected = state.active_tab == Some(index);

            ui.horizontal(|ui| {
                if ui.selectable_label(selected, label).clicked() {
                    focus_request = Some(index);
                }
                if ui.small_button("x").clicked() {
                    close_request = Some(index);
                }
            });
        }
    });

    if let Some(index) = focus_request {
        state.focus_tab(index);
    }

    if let Some(index) = close_request {
        request_close_tab(state, parsers, pending_close, index);
    }

    let save_requested = ui.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command);
    if save_requested {
        save_active_tab(state);
    }

    show_close_confirm(ui, state, pending_close, parsers);

    ui.separator();

    let Some(active) = state.active_tab else {
        ui.weak("No file open");
        return;
    };
    let Some(doc) = state.open_tabs.get_mut(active) else {
        return;
    };
    let Some(parser) = parsers.get_mut(active) else {
        return;
    };

    egui::ScrollArea::vertical().show(ui, |ui| {
        editor_widget::show(ui, doc, parser, editor_font);
    });
}

/// Saves the active tab's document, if any. Shared by `Ctrl+S` here and the
/// menu bar's File > Save.
pub fn save_active_tab(state: &mut EditorState) {
    if let Some(active) = state.active_tab {
        if let Some(doc) = state.open_tabs.get_mut(active) {
            if let Err(err) = doc.save() {
                eprintln!("failed to save: {err}");
            }
        }
    }
}

/// Closes `index`, prompting first if it's dirty. Shared by each tab's `x`
/// button and the menu bar's File > Close Tab.
pub fn request_close_tab(
    state: &mut EditorState,
    parsers: &mut Vec<IncrementalParser>,
    pending_close: &mut Option<usize>,
    index: usize,
) {
    if state.open_tabs[index].is_dirty() {
        *pending_close = Some(index);
    } else {
        state.close_tab(index);
        parsers.remove(index);
    }
}

fn show_close_confirm(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    pending_close: &mut Option<usize>,
    parsers: &mut Vec<IncrementalParser>,
) {
    let Some(index) = *pending_close else {
        return;
    };
    if index >= state.open_tabs.len() {
        *pending_close = None;
        return;
    }

    let name = state.open_tabs[index]
        .path()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let ctx = ui.ctx().clone();
    egui::Modal::new(egui::Id::new("close_confirm")).show(&ctx, |ui| {
        ui.label(format!("Save changes to {name} before closing?"));
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                if let Some(doc) = state.open_tabs.get_mut(index) {
                    if let Err(err) = doc.save() {
                        eprintln!("failed to save: {err}");
                    }
                }
                state.close_tab(index);
                parsers.remove(index);
                *pending_close = None;
            }
            if ui.button("Discard").clicked() {
                state.close_tab(index);
                parsers.remove(index);
                *pending_close = None;
            }
            if ui.button("Cancel").clicked() {
                *pending_close = None;
            }
        });
    });
}
