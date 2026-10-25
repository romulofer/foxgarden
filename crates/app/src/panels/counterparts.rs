//! Go to expect/actual (`PLAN.md` Track 30 Phase 2): the picker shown when a
//! declaration has several counterparts (an `expect` with an `actual` per
//! platform). One counterpart is jumped to directly by the caller; none is a
//! toast.

use std::path::{Path, PathBuf};

use fg_extension::Counterpart;
use fg_i18n::t;

use crate::widgets::modal::show_modal;

/// The counterparts on offer, and the project root their paths are shown
/// relative to. Empty when no picker is open.
#[derive(Default)]
pub struct CounterpartsState {
    choices: Vec<Counterpart>,
    root: PathBuf,
}

impl CounterpartsState {
    pub fn open(&mut self, root: &Path, choices: Vec<Counterpart>) {
        self.root = root.to_path_buf();
        self.choices = choices;
    }

    pub fn is_open(&self) -> bool {
        !self.choices.is_empty()
    }
}

/// Draws the picker. Returns where to jump when a row was chosen.
pub fn show(ui: &egui::Ui, state: &mut CounterpartsState) -> Option<(PathBuf, usize)> {
    let mut picked = None;
    let outcome = show_modal(ui, "counterparts_dialog", state.is_open().then_some(()), |ui, ()| {
        ui.heading(t().palettes.go_to_counterpart);
        ui.separator();
        for choice in &state.choices {
            let shown = choice.path.strip_prefix(&state.root).unwrap_or(&choice.path);
            let row = format!("{}  {}", choice.label, shown.display());
            if ui.selectable_label(false, row).clicked() {
                picked = Some((choice.path.clone(), choice.byte_offset));
            }
        }
        ui.separator();
        ui.button(t().common.close).clicked()
    });
    let dismissed = outcome.is_some_and(|(close_clicked, escape_pressed)| close_clicked || escape_pressed);
    if picked.is_some() || dismissed {
        state.choices.clear();
    }
    picked
}

#[cfg(test)]
#[path = "counterparts_test.rs"]
mod counterparts_test;
