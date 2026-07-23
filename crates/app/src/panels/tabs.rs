use fg_core::{Document, EditorState};
use syntax::IncrementalParser;

use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::widgets::editor::{self, AccessorKind};
use crate::widgets::modal::show_modal;

/// Fully reparses `doc`'s *current* buffer contents against `parser` and
/// refreshes its diagnostics from the result. Only for a brand-new parser
/// with no previous tree to diff against — there's no `InputEdit` to
/// describe "the buffer changed from nothing," so this has to reparse from
/// scratch. Once a parser already has a tree, prefer the incremental
/// `syntax::diff_edit` + `parser.reparse` path instead (see `save_tab`) —
/// even for a change that didn't originate as a normal editor keystroke, a
/// before/after text diff is still cheaper than discarding the whole tree.
fn reparse_from_scratch(doc: &mut Document, parser: &mut IncrementalParser) {
    let source = doc.buffer.to_string();
    parser.parse(&source);
    doc.diagnostics = syntax::syntax_errors(parser.tree().expect("just parsed"));
}

/// Builds a fresh parser for `doc` and populates its initial diagnostics, so
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
    reparse_from_scratch(doc, &mut parser);
    Some(parser)
}

/// Saves `index`'s document, then reparses it if it has a parser.
/// `Document::save` may itself rewrite the buffer (trimming trailing
/// whitespace) as a side effect of saving — without a reparse afterward,
/// the existing parse tree silently drifts out of sync with what's actually
/// in `doc.buffer`, which is what made highlighting (and squiggle
/// positions) look subtly wrong immediately after a save that trimmed
/// anything. Captures the buffer's text *before* saving specifically so it
/// can reparse incrementally (`syntax::diff_edit` + `parser.reparse`, the
/// same pattern every edit path in `widgets::editor::show` already uses)
/// instead of discarding the tree and reparsing from scratch — a save only
/// ever changes a handful of trailing-whitespace bytes, not the whole file,
/// so there's no reason to pay for a full reparse just because the edit
/// came from `Document::save` instead of a keystroke. Shared by `Ctrl+S`/
/// File > Save and the close-confirmation modal's "Save" button, so neither
/// can reintroduce the "saved without reparsing at all" bug by skipping
/// this.
fn save_tab(
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
    index: usize,
    last_error: &mut Option<String>,
) {
    let Some(doc) = state.open_tabs.get_mut(index) else {
        return;
    };
    let old_text = doc.buffer.to_string();
    if let Err(err) = doc.save() {
        *last_error = Some(format!("failed to save: {err}"));
        return;
    }
    if let Some(Some(parser)) = parsers.get_mut(index) {
        let new_text = doc.buffer.to_string();
        let edit = syntax::diff_edit(&old_text, &new_text);
        parser.reparse(&new_text, edit);
        doc.diagnostics = syntax::syntax_errors(parser.tree().expect("just reparsed"));
    }
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
    font_size: f32,
    indent_settings: IndentSettings,
    generate_request: Option<AccessorKind>,
    last_error: &mut Option<String>,
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
        save_active_tab(state, parsers, last_error);
    }

    let reopen_closed_tab_requested =
        ui.input(|i| i.key_pressed(egui::Key::T) && i.modifiers.command && i.modifiers.shift);
    if reopen_closed_tab_requested {
        reopen_last_closed_tab(state, parsers);
    }

    show_close_confirm(ui, state, pending_close, parsers, last_error);

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
        editor::show(ui, doc, parser, editor_font, font_size, indent_settings, generate_request, last_error);
    });
}

/// Saves the active tab's document, if any. Shared by `Ctrl+S` here and the
/// menu bar's File > Save.
pub fn save_active_tab(
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
    last_error: &mut Option<String>,
) {
    if let Some(active) = state.active_tab {
        save_tab(state, parsers, active, last_error);
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
    last_error: &mut Option<String>,
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

    show_modal(ui, "close_confirm", Some(index), |ui, &index| {
        ui.label(format!("Save changes to {name} before closing?"));
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                save_tab(state, parsers, index, last_error);
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

#[cfg(test)]
mod tests {
    use super::*;
    use fg_core::Language;

    #[test]
    fn save_tab_reparses_so_trimmed_content_is_not_highlighted_against_a_stale_tree() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Hello.java");
        // Trailing whitespace on the first line: `Document::save` trims
        // this, shortening the buffer by however many spaces there were —
        // exactly the class of side effect that leaves an un-reparsed tree
        // silently stale. Trailing whitespace on the *first* line matters
        // here — trimming it shifts the byte offset of every token after
        // it (the `private String name;` line), so a stale tree's node
        // ranges land on the wrong bytes instead of coincidentally still
        // being correct (which is exactly what happened when this test
        // first put the trimmed whitespace *after* all the highlighted
        // tokens: nothing downstream of the trim to shift meant a stale
        // tree and a fresh one produced identical spans by accident).
        std::fs::write(&path, "public class Hello {   \n    private String name;\n}\n").unwrap();

        let mut state = EditorState::new();
        state.open_tab(path).unwrap();
        let parser = open_parser_for(&mut state.open_tabs[0]);
        let mut parsers = vec![parser];
        let mut last_error = None;

        save_tab(&mut state, &mut parsers, 0, &mut last_error);

        let doc = &state.open_tabs[0];
        assert_eq!(doc.buffer.to_string(), "public class Hello {\n    private String name;\n}\n");
        assert!(!doc.is_dirty(), "buffer and saved_buffer must agree right after save");

        let tree = parsers[0].as_ref().unwrap().tree().unwrap();
        let text = doc.buffer.to_string();
        let spans = syntax::highlight_spans(tree, &text, Language::Java);

        // Cross-check against a from-scratch parse of the same (trimmed)
        // text: if `save_tab`'s reparse kept the tree in sync, the two
        // must match exactly. Before the fix, the tree still reflected the
        // pre-trim (longer) text, so node byte ranges no longer lined up
        // with `text` at all past the trimmed line.
        let mut fresh_parser = IncrementalParser::new(Language::Java);
        fresh_parser.parse(&text);
        let fresh_spans = syntax::highlight_spans(fresh_parser.tree().unwrap(), &text, Language::Java);
        assert_eq!(spans, fresh_spans);
    }
}
