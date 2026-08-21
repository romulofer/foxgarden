use fg_i18n::{msg, t};
use std::collections::HashSet;
use std::path::PathBuf;

use fg_core::{Document, EditorState};
use syntax::IncrementalParser;

use crate::goto_definition::GotoDefinitionState;
use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::style::view::ViewSettings;
use crate::widgets::editor::{
    self, AccessorKind, CaseConversion, CompletionState, GenerateAccessorsDialog, GenerateMethodDialog,
    GenerateMethodKind, HoverState, OverrideMethodDialog, PeekState, UserTemplates,
};
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

/// Saves `doc`, then reparses it against `parser` if it has one.
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
/// came from `Document::save` instead of a keystroke. Shared by `save_tab`
/// (`Ctrl+S`/File > Save/the close-confirmation modal's "Save" button, all
/// of which look the document up by tab index first) and the editor's
/// right-click "Save" (which already has `doc`/`parser` in hand, with no
/// tab index involved at all) — neither can reintroduce the "saved without
/// reparsing at all" bug by skipping this.
pub(crate) fn save_document(
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    last_error: &mut Option<String>,
) {
    let old_text = doc.buffer.to_string();
    if let Err(err) = doc.save() {
        *last_error = Some(msg::failed_to_save(&err.to_string()));
        return;
    }
    if let Some(parser) = parser.as_mut() {
        let new_text = doc.buffer.to_string();
        let edit = syntax::diff_edit(&old_text, &new_text);
        parser.reparse(&new_text, edit);
        doc.diagnostics = syntax::syntax_errors(parser.tree().expect("just reparsed"));
    }
}

/// Saves `index`'s document by tab index — the lookup `Ctrl+S`/File > Save/
/// the close-confirmation modal all need before they can call
/// `save_document`.
fn save_tab(
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
    index: usize,
    last_error: &mut Option<String>,
) {
    let Some(doc) = state.open_tabs.get_mut(index) else {
        return;
    };
    let Some(parser) = parsers.get_mut(index) else {
        return;
    };
    save_document(doc, parser, last_error);
}

/// Renders the tab bar and the active document's editor. `parsers` is kept
/// index-aligned with `state.open_tabs`; every close here removes the
/// matching parser in the same step.
#[expect(
    clippy::too_many_arguments,
    reason = "each parameter is independently threaded per-frame state (editor settings, dialog/request state, error/input plumbing) passed straight through to widgets::editor::show, not a bundle waiting to be a struct — same shape and reasoning as that function's own allowance"
)]
pub fn show(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    pending_close: &mut Option<usize>,
    parsers: &mut Vec<Option<IncrementalParser>>,
    editor_font: EditorFont,
    font_size: f32,
    indent_settings: IndentSettings,
    view_settings: ViewSettings,
    generate_request: Option<AccessorKind>,
    generate_dialog: &mut Option<GenerateAccessorsDialog>,
    generate_method_request: Option<GenerateMethodKind>,
    generate_method_dialog: &mut Option<GenerateMethodDialog>,
    override_method_request: bool,
    override_method_dialog: &mut Option<OverrideMethodDialog>,
    completion: &mut Option<CompletionState>,
    hover: &mut HoverState,
    goto_definition: &mut GotoDefinitionState,
    peek: &mut PeekState,
    case_conversion_request: Option<CaseConversion>,
    sort_lines_request: bool,
    unique_lines_request: bool,
    fold_all_request: bool,
    expand_all_request: bool,
    last_error: &mut Option<String>,
    pending_editor_input: &mut Vec<egui::Event>,
    cached_clipboard_text: &mut Option<String>,
    jump_to_char: Option<usize>,
    custom_templates: &UserTemplates,
    spring_config: &mut crate::panels::spring_config::SpringConfigState,
    lsp: &mut crate::lsp_state::LspState,
    find_references: &mut crate::widgets::editor::FindReferencesState,
    rename_box: &mut crate::widgets::editor::RenameBox,
) {
    let mut focus_request = None;
    let mut close_request = None;
    let mut toggle_read_only_request = None;

    ui.horizontal_wrapped(|ui| {
        for (index, doc) in state.open_tabs.iter().enumerate() {
            let name = doc
                .path()
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut label = String::new();
            if doc.read_only {
                label.push('🔒');
            }
            if doc.is_dirty() {
                label.push('*');
            }
            label.push_str(&name);
            let selected = state.active_tab == Some(index);

            ui.horizontal(|ui| {
                let label_response = ui.selectable_label(selected, label);
                if label_response.clicked() {
                    focus_request = Some(index);
                }
                if label_response.middle_clicked() {
                    close_request = Some(index);
                }
                label_response.context_menu(|ui| {
                    let toggle_label = if doc.read_only { t().tabs.allow_editing } else { t().menu.read_only };
                    if ui.button(toggle_label).clicked() {
                        toggle_read_only_request = Some(index);
                        ui.close();
                    }
                });
                if ui.small_button("x").clicked() {
                    close_request = Some(index);
                }
            });
        }
    });

    if let Some(index) = focus_request {
        state.focus_tab(index);
        // `anchor_byte` is a byte offset into whichever document was
        // focused when the popup opened — meaningless (or, worse,
        // coincidentally in-bounds but wrong) once a different tab's
        // buffer is what's on screen, so switching tabs always closes it,
        // the same way it would if the file itself changed out from under
        // an open dialog.
        *completion = None;
        hover.clear();
        peek.clear();
        find_references.clear();
        rename_box.clear();
    }

    if let Some(index) = close_request {
        request_close_tab(state, parsers, pending_close, index);
    }

    if let Some(index) = toggle_read_only_request
        && let Some(doc) = state.open_tabs.get_mut(index)
    {
        doc.read_only = !doc.read_only;
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
        ui.weak(t().tabs.no_file_open);
        return;
    };
    let project = state.project.as_ref();
    let Some(doc) = state.open_tabs.get_mut(active) else {
        return;
    };
    let Some(parser) = parsers.get_mut(active) else {
        return;
    };

    // `.both()`, not `.vertical()`: with `ViewSettings::word_wrap` off, a
    // long line can run past the viewport width (the editor's own
    // `desired_width(f32::INFINITY)` lets it), and a vertical-only
    // `ScrollArea` would leave no way to reach it. Wrapped content never
    // overflows horizontally by construction, so this is a no-op — no
    // horizontal scrollbar appears — whenever wrapping is on.
    egui::ScrollArea::both().show(ui, |ui| {
        // The Spring endpoint map's jump-to-handler (PLAN.md Phase 4):
        // scrolls the picked handler's line into view. `ui.next_widget_
        // position()` is exactly where `editor::show`'s own first
        // allocation will land (nothing's been drawn in this `ui` yet this
        // frame), so it doubles as that call's own internal `content_
        // origin` without needing anything back out of it. The target row
        // is only approximate — the logical line treated as a visual row,
        // ignoring word-wrap and any currently-collapsed folds before it —
        // rather than reproducing `render.rs`'s own (internal-only)
        // wrapped/folded row accounting; close enough in the common case
        // (most source lines are short, wrap is the exception not the
        // rule), and cheap to verify live rather than assume needs the
        // fuller treatment.
        if let Some(char_offset) = jump_to_char {
            let font_id = egui::FontId::new(font_size, editor_font.family());
            let row_height = ui.fonts_mut(|f| f.row_height(&font_id));
            let origin = ui.next_widget_position();
            let approx_row = doc.buffer.char_to_line(char_offset.min(doc.buffer.len_chars()));
            let rect = egui::Rect::from_min_size(
                egui::pos2(origin.x, origin.y + approx_row as f32 * row_height),
                egui::vec2(1.0, row_height),
            );
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
        }

        editor::show(
            ui,
            doc,
            parser,
            editor_font,
            font_size,
            indent_settings,
            view_settings,
            generate_request,
            generate_dialog,
            generate_method_request,
            generate_method_dialog,
            project,
            override_method_request,
            override_method_dialog,
            completion,
            hover,
            goto_definition,
            peek,
            case_conversion_request,
            sort_lines_request,
            unique_lines_request,
            fold_all_request,
            expand_all_request,
            last_error,
            pending_editor_input,
            cached_clipboard_text,
            custom_templates,
            spring_config,
            lsp,
            find_references,
            rename_box,
        );
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

/// Saves every dirty open tab, in tab order, except one currently showing
/// the "changed on disk" conflict banner (`external_conflicts`) — auto-
/// saving over an unresolved conflict would silently pick "my version
/// wins" for the user instead of leaving that choice to the banner's own
/// Reload/Keep Mine (`PLAN.md` Track 6 Phase 2). Unlike `save_active_tab`,
/// not scoped to just the focused tab — auto-save's focus-loss/idle
/// triggers fire at the app level, not the tab level, so any unsaved
/// change anywhere (other than a conflicted one) should be caught.
pub fn save_all_dirty_tabs(
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
    last_error: &mut Option<String>,
    external_conflicts: &HashSet<PathBuf>,
) {
    for index in 0..state.open_tabs.len() {
        if state.open_tabs[index].is_dirty() && !external_conflicts.contains(state.open_tabs[index].path()) {
            save_tab(state, parsers, index, last_error);
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

    let outcome = show_modal(ui, "close_confirm", Some(index), |ui, &index| {
        ui.label(msg::save_changes_before_closing(&name));
        ui.horizontal(|ui| {
            if ui.button(t().common.save).clicked() {
                save_tab(state, parsers, index, last_error);
                state.close_tab(index);
                parsers.remove(index);
                *pending_close = None;
            }
            if ui.button(t().tabs.discard).clicked() {
                state.close_tab(index);
                parsers.remove(index);
                *pending_close = None;
            }
            if ui.button(t().common.cancel).clicked() {
                *pending_close = None;
            }
        });
    });
    // Escape means "stop asking," the same as Cancel — neither saves nor
    // discards the tab, it's still open with `pending_close` cleared.
    if let Some((_, true)) = outcome {
        *pending_close = None;
    }
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
        assert_eq!(
            doc.buffer.to_string(),
            "public class Hello {\n    private String name;\n}\n"
        );
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
