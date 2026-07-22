use fg_core::{Document, EditorState};
use syntax::IncrementalParser;

use crate::style::fonts::EditorFont;
use crate::widgets::editor;

/// Parses `doc`'s current contents and populates its initial diagnostics, so
/// a file with a pre-existing syntax error shows its squiggle immediately on
/// open rather than only after the first edit. `None` if `doc` has no
/// recognized language — such files still open and edit fine, they just get
/// no parser (no highlighting, no diagnostics). Shared by every path that
/// adds or retags a tab: opening a file, reopening a closed one, and a
/// rename that changes (or clears) a file's language.
pub(crate) fn open_parser_for(doc: &mut Document) -> Option<IncrementalParser> {
    let Some(language) = doc.language else {
        doc.diagnostics.clear();
        return None;
    };
    let mut parser = IncrementalParser::new(language);
    let source = doc.buffer.to_string();
    parser.parse(&source);
    doc.diagnostics = syntax::syntax_errors(parser.tree().expect("just parsed"));
    Some(parser)
}

/// Renders the tab bar and the active document's editor. `parsers` is kept
/// index-aligned with `state.open_tabs`; every close here removes the
/// matching parser in the same step.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    pending_close: &mut Option<usize>,
    parsers: &mut Vec<Option<IncrementalParser>>,
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
                let label_response = ui.selectable_label(selected, label);
                if label_response.clicked() {
                    focus_request = Some(index);
                }
                if label_response.middle_clicked() {
                    close_request = Some(index);
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

    let reopen_closed_tab_requested =
        ui.input(|i| i.key_pressed(egui::Key::T) && i.modifiers.command && i.modifiers.shift);
    if reopen_closed_tab_requested {
        reopen_last_closed_tab(state, parsers);
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
        editor::show(ui, doc, parser, editor_font);
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
    parsers: &mut Vec<Option<IncrementalParser>>,
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

/// Restores the most recently closed tab (`Ctrl+Shift+T`), giving it a
/// fresh parser if it wasn't already open elsewhere — mirrors how a newly
/// opened file gets its parser in `app.rs`. A no-op if there's nothing left
/// to reopen, or if that tab is already open (in which case `EditorState`
/// just focuses it, so `parsers` needs no change).
pub fn reopen_last_closed_tab(state: &mut EditorState, parsers: &mut Vec<Option<IncrementalParser>>) {
    let Some(index) = state.reopen_last_closed_tab() else {
        return;
    };
    if index == parsers.len() {
        let parser = open_parser_for(&mut state.open_tabs[index]);
        parsers.push(parser);
    }
}

fn show_close_confirm(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    pending_close: &mut Option<usize>,
    parsers: &mut Vec<Option<IncrementalParser>>,
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
