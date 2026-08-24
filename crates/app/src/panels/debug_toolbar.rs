//! Debug toolbar (`PLAN.md` Track 23 Phase 2): Continue/Step Over/Step
//! Into/Step Out/Stop. Not a `View`-menu-toggled dock like the build/
//! terminal panels — its whole purpose is tied 1:1 to a live debug session
//! existing at all, so `app.rs` shows it exactly while `DebugState::
//! is_running()` is true and nothing else decides its visibility.

use fg_i18n::t;

/// Which button (if any) was clicked this frame — `app.rs` dispatches each
/// to the matching `DebugState` method, the same outcome-struct-then-
/// dispatch shape `menu_bar::MenuBarOutcome` already uses.
#[derive(Default)]
pub struct DebugToolbarOutcome {
    pub continue_request: bool,
    pub step_over_request: bool,
    pub step_into_request: bool,
    pub step_out_request: bool,
    pub stop_request: bool,
}

/// `paused` gates Continue/Step Over/Step Into/Step Out — clicking any of
/// them only means something while the debuggee is actually stopped at a
/// real frame (`DebugState::is_paused`); Stop is always enabled, matching
/// `stop`'s own "harmless even when there's nothing to stop" contract.
pub fn show(ui: &mut egui::Ui, paused: bool) -> DebugToolbarOutcome {
    let mut outcome = DebugToolbarOutcome::default();
    ui.horizontal(|ui| {
        if ui.add_enabled(paused, egui::Button::new(t().common.debug_continue)).clicked() {
            outcome.continue_request = true;
        }
        if ui.add_enabled(paused, egui::Button::new(t().common.debug_step_over)).clicked() {
            outcome.step_over_request = true;
        }
        if ui.add_enabled(paused, egui::Button::new(t().common.debug_step_into)).clicked() {
            outcome.step_into_request = true;
        }
        if ui.add_enabled(paused, egui::Button::new(t().common.debug_step_out)).clicked() {
            outcome.step_out_request = true;
        }
        ui.separator();
        if ui.button(t().common.stop).clicked() {
            outcome.stop_request = true;
        }
    });
    outcome
}
