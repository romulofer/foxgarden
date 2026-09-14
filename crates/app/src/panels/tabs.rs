use fg_i18n::{msg, t};
use std::collections::HashSet;
use std::path::PathBuf;

use fg_core::{Document, EditorState};
use ropey::Rope;
use syntax::IncrementalParser;

use crate::goto_definition::GotoDefinitionState;
use crate::panels::file_history::{self, FileHistoryState};
use crate::style::fonts::EditorFont;
use crate::style::icons;
use crate::style::indent::IndentSettings;
use crate::style::view::ViewSettings;
use crate::widgets::editor::{
    self, AccessorKind, CompletionState, GenerateAccessorsDialog, GenerateMethodDialog,
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
    trim_trailing_whitespace: bool,
) {
    let old_text = doc.buffer.to_string();
    if let Err(err) = doc.save(trim_trailing_whitespace) {
        crate::errors::report(last_error, msg::failed_to_save(&err.to_string()));
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
    trim_trailing_whitespace: bool,
) {
    let Some(doc) = state.open_tabs.get_mut(index) else {
        return;
    };
    let Some(parser) = parsers.get_mut(index) else {
        return;
    };
    save_document(doc, parser, last_error, trim_trailing_whitespace);
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
    pending_close: &mut Vec<PathBuf>,
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
    requests: crate::widgets::editor::EditorRequests,
    last_error: &mut Option<String>,
    pending_editor_input: &mut Vec<egui::Event>,
    cached_clipboard_text: &mut Option<String>,
    jump_to_char: Option<usize>,
    custom_templates: &UserTemplates,
    spring_config: &mut crate::panels::spring_config::SpringConfigState,
    lsp: &mut crate::lsp_state::LspState,
    find_references: &mut crate::widgets::editor::FindReferencesState,
    rename_box: &mut crate::widgets::editor::RenameBox,
    code_action_gutter: &mut crate::widgets::editor::CodeActionGutter,
    debug_state: &crate::debug_state::DebugState,
    file_history: &mut FileHistoryState,
    dark_mode: bool,
    trim_trailing_whitespace_on_save: bool,
    // `side_panel_reveal` is set to the path a tab's "Reveal in Tree"
    // names, for the caller to hand to the side panel (which owns tree
    // expansion and selection).
    side_panel_reveal: &mut Option<PathBuf>,
    // The welcome screen's own buttons, when it's the thing being shown
    // (no file open); left untouched otherwise.
    welcome: &mut crate::panels::welcome::WelcomeOutcome,
    // Set to the entry point whose run-gutter ▶ was clicked this frame, for
    // the caller to launch — see `editor::show`'s own parameter.
    run_request: &mut Option<syntax::MainEntry>,
    recent_projects: &[PathBuf],
) {
    let mut focus_request = None;
    let mut close_request = None;
    let mut toggle_read_only_request = None;
    let mut file_history_request: Option<usize> = None;

    let mut close_others_request = None;
    let mut close_right_request = None;
    let mut reveal_request: Option<PathBuf> = None;

    // Horizontally scrollable rather than wrapping: with a wrapped bar,
    // enough open files pushed the editor itself down the screen, and with
    // a plain clipped row the tabs past the right edge — the *active* one
    // included, once it was far enough along — simply couldn't be reached
    // with the mouse at all. Scrolling keeps the bar one row tall and every
    // tab reachable, and `scroll_to_me` below keeps the active one on
    // screen without the user hunting for it.
    // The overflow count sits at the right end of the same row, so it
    // never costs the editor a line of height — which means the scroll
    // area has to leave room for it rather than claiming the full width.
    let overflow_width = 64.0;
    // The count itself is drawn inside; the binding just keeps the
    // closure's own return value from being silently discarded.
    let _hidden_tabs = ui
        .horizontal(|ui| {
            let bar_width = (ui.available_width() - overflow_width).max(0.0);
            let hidden = egui::ScrollArea::horizontal()
                .id_salt("tab_bar")
                .auto_shrink([false, true])
                .max_width(bar_width)
                .show(ui, |ui| {
            let mut hidden = 0usize;
            ui.horizontal(|ui| {
                for (index, doc) in state.open_tabs.iter().enumerate() {
                    let name = doc
                        .path()
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let selected = state.active_tab == Some(index);

                    ui.horizontal(|ui| {
                        // Icon, name, then a dot for unsaved changes — the
                        // dot trails the name instead of an asterisk
                        // leading it, so a tab doesn't visibly shift left
                        // and right as it's edited and saved.
                        let mut label = format!("{} {name}", icons::for_file(doc.path()));
                        if doc.read_only {
                            label.insert(0, ' ');
                            label.insert(0, icons::LOCK);
                        }
                        if doc.is_dirty() {
                            label.push(' ');
                            label.push(icons::UNSAVED);
                        }
                        let label_response = ui.selectable_label(selected, label);
                        if !ui.is_rect_visible(label_response.rect) {
                            hidden += 1;
                        }
                        // Two files called `Application.java` in different
                        // modules are otherwise indistinguishable in the bar.
                        let label_response = label_response.on_hover_text(doc.path().display().to_string());
                        if label_response.clicked() {
                            focus_request = Some(index);
                        }
                        if label_response.middle_clicked() {
                            close_request = Some(index);
                        }
                        if selected && scroll_active_tab_into_view(ui, state.active_tab) {
                            label_response.scroll_to_me(Some(egui::Align::Center));
                        }
                        label_response.context_menu(|ui| {
                            if ui.button(t().tabs.close).clicked() {
                                close_request = Some(index);
                                ui.close();
                            }
                            if ui
                                .add_enabled(state.open_tabs.len() > 1, egui::Button::new(t().tabs.close_others))
                                .clicked()
                            {
                                close_others_request = Some(index);
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    index + 1 < state.open_tabs.len(),
                                    egui::Button::new(t().tabs.close_to_the_right),
                                )
                                .clicked()
                            {
                                close_right_request = Some(index);
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(t().tabs.copy_path).clicked() {
                                ui.ctx().copy_text(doc.path().display().to_string());
                                ui.close();
                            }
                            if ui.button(t().tabs.reveal_in_tree).clicked() {
                                reveal_request = Some(doc.path().to_path_buf());
                                ui.close();
                            }
                            ui.separator();
                            let toggle_label =
                                if doc.read_only { t().tabs.allow_editing } else { t().menu.read_only };
                            if ui.button(toggle_label).clicked() {
                                toggle_read_only_request = Some(index);
                                ui.close();
                            }
                            // Only a file living under the currently open project
                            // has anywhere to snapshot into (`Document::project_
                            // root`, `PLAN.md` Track 4 Phase 1) — no point offering
                            // a history browser that would always open empty.
                            if doc.project_root.is_some() && ui.button(t().file_history.menu_item).clicked() {
                                file_history_request = Some(index);
                                ui.close();
                            }
                        });
                        let close = egui::RichText::new(icons::CLOSE.to_string()).small().weak();
                        if ui.add(egui::Button::new(close).frame(false)).clicked() {
                            close_request = Some(index);
                        }
                    });
                }
            });
                    hidden
                })
                .inner;
            // How many tabs are scrolled out of sight, so "there are more
            // files open than you can see" is visible rather than something
            // the user has to discover by scrolling.
            if hidden > 0 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{hidden} {}", t().tabs.more_tabs))
                            .weak()
                            .small(),
                    )
                    .on_hover_text(t().tabs.more_tabs_hint);
                });
            }
            hidden
        })
        .inner;

    if let Some(index) = close_others_request {
        close_all_except(state, parsers, pending_close, index);
    }
    if let Some(index) = close_right_request {
        close_all_after(state, parsers, pending_close, index);
    }
    if let Some(path) = reveal_request {
        side_panel_reveal.replace(path);
    }

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
        code_action_gutter.clear();
    }

    if let Some(index) = close_request {
        request_close_tab(state, parsers, pending_close, index);
    }

    if let Some(index) = toggle_read_only_request
        && let Some(doc) = state.open_tabs.get_mut(index)
    {
        doc.read_only = !doc.read_only;
    }

    if let Some(index) = file_history_request
        && let Some(doc) = state.open_tabs.get(index)
        && let Some(project_root) = doc.project_root.clone()
    {
        file_history.open(project_root, doc.path.clone(), doc.buffer.to_string());
    }
    if let Some(content) = file_history::show(ui.ctx(), file_history, editor_font, font_size, dark_mode)
        && let Some(path) = file_history.open_path()
        && let Some(index) = state.find_tab(path)
    {
        let doc = &mut state.open_tabs[index];
        doc.buffer.replace(Rope::from_str(&content));
        doc.lsp_version += 1;
        doc.lsp_sync_pending = true;
        parsers[index] = open_parser_for(doc);
    }

    let save_requested = ui.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command && !i.modifiers.shift);
    if save_requested {
        save_active_tab(state, parsers, last_error, trim_trailing_whitespace_on_save);
    }
    let save_all_requested = ui.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command && i.modifiers.shift);
    if save_all_requested {
        save_all_dirty_tabs(state, parsers, last_error, &HashSet::new(), trim_trailing_whitespace_on_save);
    }

    let reopen_closed_tab_requested =
        ui.input(|i| i.key_pressed(egui::Key::T) && i.modifiers.command && i.modifiers.shift);
    if reopen_closed_tab_requested {
        reopen_last_closed_tab(state, parsers);
    }

    show_close_confirm(ui, state, pending_close, parsers, last_error, trim_trailing_whitespace_on_save);

    ui.separator();

    if state.active_tab.is_none() {
        // No file open is the app's own front door, not an error state —
        // see `panels::welcome`.
        *welcome = crate::panels::welcome::show(ui, recent_projects, state.project.is_some());
        return;
    }

    let focused_pane = state.focused_pane();
    let is_split = state.is_split();
    // Where each pane's column landed this frame, so a press inside a pane can
    // focus it below. The editor itself consumes the click, so this reads the
    // raw press position against each column rect rather than a widget response.
    let mut pane_rects: [Option<egui::Rect>; 2] = [None, None];

    // Scoped so `render_pane`'s borrows of `state`/`parsers`/the editor state
    // end before the focus check below reads `state` again.
    {
    // Renders one editor pane (`PLAN.md` Track 11). Called once when unsplit,
    // twice (side by side) when split. Both panes draw from the shared
    // `open_tabs`, so `parsers` stay index-aligned either way; only the
    // *focused* pane consumes the one-shot requests (generate getters, a
    // Spring jump-to, ...) so a getter generated for the pane the user is in
    // doesn't also fire in the other one.
    let mut render_pane = |ui: &mut egui::Ui, pane: usize, rect_out: &mut Option<egui::Rect>| {
        *rect_out = Some(ui.max_rect());
        let Some(active) = state.pane_active(pane) else {
            ui.centered_and_justified(|ui| {
                ui.weak(t().tabs.empty_pane);
            });
            return;
        };
        let focused = pane == focused_pane;
        let gen_req = if focused { generate_request } else { None };
        let gen_method_req = if focused { generate_method_request } else { None };
        let override_req = focused && override_method_request;
        let jump = if focused { jump_to_char } else { None };
        let requests_for_pane = if focused { requests } else { Default::default() };

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
        // `ScrollArea` would leave no way to reach it. A distinct `id_salt`
        // per pane keeps the two panes' own scroll offsets independent.
        egui::ScrollArea::both().id_salt(("editor_scroll", pane)).show(ui, |ui| {
            // The Spring endpoint map's jump-to-handler (PLAN.md Phase 4):
            // scrolls the picked handler's line into view. `ui.next_widget_
            // position()` is exactly where `editor::show`'s own first
            // allocation will land, so it doubles as that call's own internal
            // `content_origin`. The target row is only approximate (logical
            // line as a visual row, ignoring wrap/folds), close enough in the
            // common case.
            if let Some(char_offset) = jump {
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
                pane,
                editor_font,
                font_size,
                indent_settings,
                view_settings,
                gen_req,
                generate_dialog,
                gen_method_req,
                generate_method_dialog,
                project,
                override_req,
                override_method_dialog,
                completion,
                hover,
                goto_definition,
                peek,
                requests_for_pane,
                last_error,
                pending_editor_input,
                cached_clipboard_text,
                custom_templates,
                spring_config,
                lsp,
                find_references,
                rename_box,
                code_action_gutter,
                debug_state,
                trim_trailing_whitespace_on_save,
                run_request,
            );
        });
    };

    if is_split {
        let [rect0, rect1] = &mut pane_rects;
        ui.columns(2, |cols| {
            render_pane(&mut cols[0], 0, rect0);
            render_pane(&mut cols[1], 1, rect1);
        });
    } else {
        render_pane(ui, 0, &mut pane_rects[0]);
    }
    }

    // A press inside a split pane focuses it. The editor consumed the click
    // itself, so this checks the raw press origin against each column's rect
    // rather than a widget response. Only meaningful while split.
    if is_split
        && let Some(pos) = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.press_origin()).flatten())
    {
        for (pane, rect) in pane_rects.iter().enumerate() {
            if rect.is_some_and(|r| r.contains(pos)) {
                state.focus_pane(pane);
            }
        }
    }
}

/// Saves the active tab's document, if any. Shared by `Ctrl+S` here and the
/// menu bar's File > Save.
pub fn save_active_tab(
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
    last_error: &mut Option<String>,
    trim_trailing_whitespace: bool,
) {
    if let Some(active) = state.active_tab {
        save_tab(state, parsers, active, last_error, trim_trailing_whitespace);
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
    trim_trailing_whitespace: bool,
) {
    for index in 0..state.open_tabs.len() {
        if state.open_tabs[index].is_dirty() && !external_conflicts.contains(state.open_tabs[index].path()) {
            save_tab(state, parsers, index, last_error, trim_trailing_whitespace);
        }
    }
}

/// Closes `index`, prompting first if it's dirty. Shared by each tab's
/// close button and the menu bar's File > Close Tab.
///
/// A dirty tab joins `pending_close`, a *queue* rather than a single slot,
/// because "Close Others" can hit several unsaved files at once and each
/// one still deserves its own save/discard decision — asked one at a time,
/// in tab order. Queued by path, not index: closing the tabs ahead of it
/// renumbers everything after, and a stale index would confirm-and-close
/// the wrong file.
pub fn request_close_tab(
    state: &mut EditorState,
    parsers: &mut Vec<Option<IncrementalParser>>,
    pending_close: &mut Vec<PathBuf>,
    index: usize,
) {
    if state.open_tabs[index].is_dirty() {
        let path = state.open_tabs[index].path().to_path_buf();
        if !pending_close.contains(&path) {
            pending_close.push(path);
        }
    } else {
        state.close_tab(index);
        parsers.remove(index);
    }
}

/// Closes every tab except `keep`, and every tab after it — the tab bar's
/// own "Close Others"/"Close to the Right". Both walk backwards so each
/// close can't renumber a tab this loop hasn't reached yet.
fn close_all_except(
    state: &mut EditorState,
    parsers: &mut Vec<Option<IncrementalParser>>,
    pending_close: &mut Vec<PathBuf>,
    keep: usize,
) {
    let keep_path = state.open_tabs[keep].path().to_path_buf();
    for index in (0..state.open_tabs.len()).rev() {
        if state.open_tabs[index].path() != keep_path {
            request_close_tab(state, parsers, pending_close, index);
        }
    }
}

fn close_all_after(
    state: &mut EditorState,
    parsers: &mut Vec<Option<IncrementalParser>>,
    pending_close: &mut Vec<PathBuf>,
    after: usize,
) {
    for index in ((after + 1)..state.open_tabs.len()).rev() {
        request_close_tab(state, parsers, pending_close, index);
    }
}

/// Whether the active tab should be scrolled into view this frame: true
/// exactly on the frame the active tab changed, so the bar follows a tab
/// switch (including one made from the quick switcher, with the tab bar
/// scrolled somewhere else entirely) without fighting the user's own
/// scrolling on every other frame.
fn scroll_active_tab_into_view(ui: &egui::Ui, active: Option<usize>) -> bool {
    let id = egui::Id::new("tab_bar_last_active");
    let previous: Option<Option<usize>> = ui.ctx().data(|d| d.get_temp(id));
    if previous == Some(active) {
        return false;
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, active));
    true
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
    pending_close: &mut Vec<PathBuf>,
    parsers: &mut Vec<Option<IncrementalParser>>,
    last_error: &mut Option<String>,
    trim_trailing_whitespace: bool,
) {
    // The queue front is whichever unsaved tab is being asked about right
    // now; anything in it whose tab is gone (closed by another path, or
    // reloaded clean) is dropped rather than asked about.
    let index = loop {
        let Some(path) = pending_close.first() else {
            return;
        };
        match state.find_tab(path) {
            Some(index) if state.open_tabs[index].is_dirty() => break index,
            _ => {
                pending_close.remove(0);
            }
        }
    };

    let name = state.open_tabs[index]
        .path()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let remaining = pending_close.len();

    let outcome = show_modal(ui, "close_confirm", Some(index), |ui, &index| {
        ui.label(msg::save_changes_before_closing(&name));
        ui.horizontal(|ui| {
            if ui.button(t().common.save).clicked() {
                save_tab(state, parsers, index, last_error, trim_trailing_whitespace);
                state.close_tab(index);
                parsers.remove(index);
                pending_close.remove(0);
            }
            // Only worth offering when this isn't the last one being asked
            // about: a batch close ("Close Others" over several unsaved
            // files) otherwise means answering the same question once per
            // file.
            if remaining > 1 && ui.button(t().tabs.save_all).clicked() {
                for path in std::mem::take(pending_close) {
                    if let Some(index) = state.find_tab(&path) {
                        save_tab(state, parsers, index, last_error, trim_trailing_whitespace);
                        state.close_tab(index);
                        parsers.remove(index);
                    }
                }
            }
            if ui.button(t().tabs.discard).clicked() {
                state.close_tab(index);
                parsers.remove(index);
                pending_close.remove(0);
            }
            if ui.button(t().common.cancel).clicked() {
                // Cancel abandons the whole batch, not just this one file:
                // answering "cancel" to "close these 7 tabs?" and then
                // being asked about the other six is not what anyone means.
                pending_close.clear();
            }
        });
    });
    // Escape means "stop asking," the same as Cancel.
    if let Some((_, true)) = outcome {
        pending_close.clear();
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

        save_tab(&mut state, &mut parsers, 0, &mut last_error, true);

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
