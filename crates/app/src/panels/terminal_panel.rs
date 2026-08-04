//! The dockable bottom terminal panel (`SPEC.md` §8.2, `PLAN.md`'s
//! terminal-panel track Phases 5-8) — its own small tab strip over
//! `state.terminal_tabs`, entirely independent of the file tab strip
//! (`panels::tabs`): a terminal session's position is just its own index
//! into `terminal_tabs`, never interleaved with a file tab's. Rendered only
//! while the caller's `terminal_panel_visible` flag is set, mirroring
//! `side_panel`'s own "a dockable area with its own visibility flag" shape.

use fg_core::EditorState;

use crate::pty_session::PtySession;
use crate::style::fonts::EditorFont;
use crate::widgets::{terminal_input, terminal_widget};

/// The terminal grid's own keyboard-focus target — a fixed, global `Id`
/// (not derived from a `Ui`'s nested position) so `app.rs` can check the
/// same focus state *before* this module ever runs this frame, to gate its
/// own global Ctrl+letter shortcuts (`is_terminal_focused` below).
fn focus_id() -> egui::Id {
    egui::Id::new("terminal_panel_focus_target")
}

/// Whether the terminal grid currently holds keyboard focus — `app.rs`
/// calls this before checking any of its own Ctrl+letter shortcuts
/// (`Ctrl+E`/`Ctrl+P`/`Ctrl+Shift+E`/`Ctrl+N`/`Ctrl+B`), since `PLAN.md`
/// Phase 8 wires those same letters up as real terminal control bytes
/// (`Ctrl+P` recalls shell history, `Ctrl+E` moves to end-of-line, ...) —
/// without this guard, typing one of them into a focused terminal session
/// would *also* fire the app's own global popup/panel toggle, which reads
/// as a bug, not a coincidence, from inside the terminal. `Ctrl+\`` (the
/// terminal toggle itself) and `F11` (zen mode) are deliberately **not**
/// gated by this — neither collides with a Ctrl+letter control byte, and
/// `Ctrl+\`` in particular must keep working to *close* the panel while
/// it's focused, mirroring VSCode's own terminal-toggle behavior.
pub fn is_terminal_focused(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.has_focus(focus_id()))
}

/// New-session/close requests the caller (`app.rs`) applies to *both*
/// `state.terminal_tabs` and its own index-aligned `terminal_sessions`
/// together — this module only has `&mut EditorState`, not the real
/// `PtySession`s (those live on `FoxGardenApp`, same reason `parsers` isn't
/// on `EditorState` either: a live child process isn't headless-testable).
#[derive(Default)]
pub struct TerminalPanelOutcome {
    pub new_session_requested: bool,
    pub close_request: Option<usize>,
}

/// Draws the panel's own tab strip plus the active session's content area:
/// `terminal_widget::show`'s real `vt100`-rendered grid (`PLAN.md` Phase 7)
/// plus every keyboard event translated to real terminal bytes and written
/// straight to the pty — plain text via `Event::Text`, paste via
/// `Event::Paste`, everything else (arrows, Enter, Backspace, Ctrl+letter,
/// ...) via `terminal_input::key_event_to_bytes` (`PLAN.md` Phase 8).
/// `sessions` is index-aligned with `state.terminal_tabs`, kept in sync by
/// the caller. `editor_font`/`font_size`/`dark_mode` are the same settings
/// the file-tab editor uses (`SPEC.md` §8.4: "rather than a second,
/// separate terminal font setting"), threaded down from `app.rs`.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    sessions: &mut [PtySession],
    editor_font: EditorFont,
    font_size: f32,
    dark_mode: bool,
    cursor_blink: bool,
) -> TerminalPanelOutcome {
    let mut outcome = TerminalPanelOutcome::default();
    let mut focus_request = None;

    ui.horizontal(|ui| {
        for (index, terminal) in state.terminal_tabs.iter().enumerate() {
            let selected = state.active_terminal == Some(index);
            ui.horizontal(|ui| {
                let label_response = ui.selectable_label(selected, &terminal.title);
                if label_response.clicked() {
                    focus_request = Some(index);
                }
                if ui.small_button("x").clicked() {
                    outcome.close_request = Some(index);
                }
            });
        }
        if ui.small_button("+").on_hover_text("New Terminal").clicked() {
            outcome.new_session_requested = true;
        }
    });

    if let Some(index) = focus_request {
        state.active_terminal = Some(index);
    }

    ui.separator();

    let active_index = state.active_terminal;
    match active_index.and_then(|index| sessions.get_mut(index).map(|session| (index, session))) {
        Some((index, session)) => {
            session.drain();

            let focus_id = focus_id();
            // Checked *before* `terminal_widget::show` runs: whether to
            // paint the cursor this frame depends on already-known focus,
            // not the focus state `show`'s own response only updates below.
            let focused = ui.memory(|m| m.has_focus(focus_id));

            let grid_response = terminal_widget::show(
                ui,
                focus_id,
                session,
                editor_font,
                font_size,
                dark_mode,
                state.terminal_tabs[index].last_interaction,
                cursor_blink,
                focused,
            );

            // Claim keyboard focus on click so typing lands in this
            // session rather than (say) a still-focused editor tab above
            // the panel — same "click to focus" model every terminal
            // emulator already uses.
            if grid_response.clicked() {
                grid_response.request_focus();
            }
            if grid_response.has_focus() {
                // Without this, egui's own default keyboard-navigation
                // treats Tab and the arrow keys as "move focus to the next/
                // directional widget" and never delivers them as a `Key`
                // event at all — so shell-completion Tab, command-history
                // Up/Down, or in-line cursor movement via Left/Right would
                // silently defocus the terminal instead of reaching
                // `key_event_to_bytes` (which already maps all of them to
                // their real terminal byte sequences, see `terminal_input`).
                // `Memory::set_focus_lock_filter`'s own doc comment requires
                // focus to already be held as of *last* frame, so this only
                // takes effect one frame after `request_focus()` above — the
                // same frame the event loop below would otherwise start
                // losing these keys anyway.
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        focus_id,
                        egui::EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            ..Default::default()
                        },
                    )
                });
                ui.painter().rect_stroke(
                    grid_response.rect,
                    0.0,
                    ui.visuals().selection.stroke,
                    egui::StrokeKind::Inside,
                );

                let mut interacted = false;
                for event in ui.input(|i| i.events.clone()) {
                    match event {
                        egui::Event::Text(text) => {
                            let _ = session.write(text.as_bytes());
                            interacted = true;
                        }
                        // Paste writes the clipboard text's raw bytes the
                        // same way typing does (`SPEC.md` §8.5) — no
                        // bracketed-paste-mode wrapping, which is out of
                        // this phase's scope (the byte-sequence table).
                        egui::Event::Paste(text) => {
                            let _ = session.write(text.as_bytes());
                            interacted = true;
                        }
                        // egui-winit translates a bare Ctrl+C (`modifiers.
                        // command`, matched before it ever becomes a real
                        // `Key` event — see `is_copy_command` in egui-winit's
                        // own `lib.rs`) straight into this semantic `Copy`
                        // event instead, so `key_event_to_bytes`'s own
                        // `Ctrl+C -> 0x03` mapping below is never actually
                        // reachable from a real keypress; this is the only
                        // place that ever sees it. `terminal_widget::show`
                        // does now support drag-to-select — deliberately
                        // *not* wired to this event: it already copies the
                        // selection to the clipboard itself on mouse-up (the
                        // xterm/most-Linux-terminals convention), so plain
                        // Ctrl+C stays the interrupt byte unconditionally —
                        // every real terminal emulator's own convention, and
                        // the one this app's users already expect a running
                        // program in the shell to receive.
                        egui::Event::Copy => {
                            let _ = session.write(&[0x03]);
                            interacted = true;
                        }
                        egui::Event::Key {
                            key, pressed: true, modifiers, ..
                        } => {
                            if let Some(bytes) = terminal_input::key_event_to_bytes(key, modifiers) {
                                let _ = session.write(&bytes);
                                interacted = true;
                            }
                        }
                        _ => {}
                    }
                }
                if interacted {
                    state.terminal_tabs[index].last_interaction = ui.input(|i| i.time);
                }
            }
        }
        None => {
            ui.weak("No terminal session");
        }
    }

    outcome
}
