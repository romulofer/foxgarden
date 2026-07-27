//! The dockable bottom terminal panel (`SPEC.md` §8.2, `PLAN.md`'s
//! terminal-panel track Phase 5) — its own small tab strip over
//! `state.terminal_tabs`, entirely independent of the file tab strip
//! (`panels::tabs`): a terminal session's position is just its own index
//! into `terminal_tabs`, never interleaved with a file tab's. Rendered only
//! while the caller's `terminal_panel_visible` flag is set, mirroring
//! `side_panel`'s own "a dockable area with its own visibility flag" shape.

use fg_core::EditorState;

/// Draws the panel's own tab strip plus the active session's content area
/// (still a placeholder — no pty until Phase 6). Mutates `state.
/// terminal_tabs`/`active_terminal` directly for focus/close/new-session
/// requests, the same "collect requests during the loop, apply them after"
/// shape `panels::tabs::show`'s own file-tab strip already uses, to avoid
/// mutating `state` while a `for` loop still holds an immutable borrow of
/// `state.terminal_tabs`.
pub fn show(ui: &mut egui::Ui, state: &mut EditorState) {
    let mut focus_request = None;
    let mut close_request = None;
    let mut new_session_requested = false;

    ui.horizontal(|ui| {
        for (index, terminal) in state.terminal_tabs.iter().enumerate() {
            let selected = state.active_terminal == Some(index);
            ui.horizontal(|ui| {
                let label_response = ui.selectable_label(selected, &terminal.title);
                if label_response.clicked() {
                    focus_request = Some(index);
                }
                if ui.small_button("x").clicked() {
                    close_request = Some(index);
                }
            });
        }
        if ui.small_button("+").on_hover_text("New Terminal").clicked() {
            new_session_requested = true;
        }
    });

    if let Some(index) = focus_request {
        state.active_terminal = Some(index);
    }
    if let Some(index) = close_request {
        state.close_terminal_tab(index);
    }
    if new_session_requested {
        state.new_terminal_tab();
    }

    ui.separator();

    match state.active_terminal.and_then(|index| state.terminal_tabs.get(index)) {
        Some(terminal) => {
            ui.weak(format!("{} — not yet implemented (PLAN.md Phase 6)", terminal.title));
        }
        None => {
            ui.weak("No terminal session");
        }
    }
}
