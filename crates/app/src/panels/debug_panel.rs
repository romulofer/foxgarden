//! Variables/call-stack panel (`PLAN.md` Track 23 Phase 3). Shown exactly
//! while `DebugState::is_running()` is true, the same "tied 1:1 to a live
//! session, not a `View`-menu-toggled dock" choice `debug_toolbar` already
//! made — there's nothing useful to show a panel like this once no session
//! exists, so a persisted visibility flag would only ever show it empty.

use std::path::{Path, PathBuf};

use crate::debug_state::DebugState;

/// Draws the call stack and local variables for whatever `state` is
/// currently paused at (both sections read "not paused" while running but
/// not stopped, and are empty while genuinely idle/starting). Returns the
/// file/line to jump to if a call-stack frame row was clicked — same
/// deferred-conversion contract `build_panel::show`'s own click return
/// already established, since a 0-indexed `line` here still needs the
/// caller's own open-tab byte-offset conversion.
pub fn show(ui: &mut egui::Ui, state: &DebugState) -> Option<(PathBuf, usize)> {
    let mut clicked = None;
    if !state.is_paused() {
        ui.label("Not paused.");
        return None;
    }

    ui.strong("Call Stack");
    egui::ScrollArea::vertical().id_salt("debug_call_stack").max_height(200.0).show(ui, |ui| {
        for frame in state.call_stack() {
            let label = match &frame.file {
                Some(file) => format!("{}  ({}:{})", frame.name, file_label(file), frame.line + 1),
                None => frame.name.clone(),
            };
            if frame.file.is_some() {
                let response = ui
                    .push_id(frame.id, |ui| ui.add(egui::Label::new(&label).sense(egui::Sense::click())))
                    .inner;
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    clicked = Some((frame.file.clone().unwrap(), frame.line));
                }
            } else {
                ui.label(&label);
            }
        }
    });

    ui.separator();
    ui.strong("Variables");
    egui::ScrollArea::vertical().id_salt("debug_variables").show(ui, |ui| {
        let groups = state.variables();
        if groups.is_empty() {
            ui.weak("(loading…)");
        }
        for group in groups {
            egui::CollapsingHeader::new(&group.name).default_open(true).show(ui, |ui| {
                if group.variables.is_empty() {
                    ui.weak("(none)");
                }
                for variable in &group.variables {
                    ui.horizontal(|ui| {
                        ui.monospace(&variable.name);
                        ui.weak(":");
                        ui.monospace(&variable.value);
                        if !variable.kind.is_empty() {
                            ui.weak(format!("({})", variable.kind));
                        }
                    });
                }
            });
        }
    });

    clicked
}

fn file_label(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).unwrap_or("").to_string()
}
