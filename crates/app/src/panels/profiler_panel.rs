//! The dockable Profiler panel (`PLAN.md` Track 26 Phase 2): a header with
//! the capture's sample count and a zoom-out control, over the interactive
//! flame graph (`widgets::flame_graph`) of the most recent async-profiler
//! capture held in `ProfilerState::last_profile`.
//!
//! Kept deliberately thin — all the layout/interaction lives in the reusable
//! flame-graph widget; this only supplies the panel chrome and the three
//! states it can be in (a capture running, a captured profile to show, or
//! nothing captured yet), mirroring how `build_panel` hosts the build log.

use fg_i18n::{msg, t};

use crate::profiler_state::ProfilerState;
use crate::widgets::flame_graph::{self, FlameGraphState};

pub fn show(ui: &mut egui::Ui, profiler: &ProfilerState, flame: &mut FlameGraphState) {
    ui.horizontal(|ui| {
        ui.strong(t().common.profiler_title);
        if profiler.is_capturing() {
            ui.spinner();
            ui.label(t().common.profiling);
        } else if let Some(tree) = &profiler.last_profile {
            ui.label(msg::profile_captured(tree.total));
            if ui.button(t().common.profiler_reset_zoom).clicked() {
                flame.reset();
            }
        }
    });

    match &profiler.last_profile {
        Some(tree) if tree.total > 0 => {
            ui.label(egui::RichText::new(t().common.profiler_hint).weak().small());
            ui.separator();
            flame_graph::show(ui, tree, flame);
        }
        _ => {
            ui.add_space(8.0);
            ui.label(t().common.profiler_no_profile);
        }
    }
}
