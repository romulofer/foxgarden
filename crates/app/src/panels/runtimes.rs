//! Settings > <runtime kind> (JDKs…) — the modal that owns the machine-wide
//! inventory of one kind of runtime an extension contributes (`PLAN.md`
//! Track 29 Phase 1): add one by folder picker, or Auto-detect the whole
//! machine at once, remove one, see what's registered. What counts as an
//! install, and where to look for them, is the extension's answer
//! (`RuntimeHandle::detect`/`discover`). Deliberately separate from the
//! Language Servers dialog (`panels::lsp_servers`): a server's
//! `LspSettings::runtime_home_for` is "which install runs the server itself", where
//! this is "which installs exist on this machine to *target*" (any version)
//! — different concerns.

use std::sync::mpsc::Receiver;

use fg_extension::{RuntimeHandle, RuntimeInstall};
use fg_i18n::{msg, t};

use crate::runtime_inventory::RuntimeInventories;
use crate::widgets::modal::show_modal;

/// Which runtime kind's dialog is open, plus the last "Add" failure, if any
/// — shown inline rather than through the app-wide `last_error` banner,
/// since it's local to one field in this one dialog, the same way
/// `run_configs.rs`'s own per-config fields report their own save failures.
///
/// `picker`/`auto_detect_rx` both run off the UI thread (mirroring
/// `git_stage.rs`'s own `spawn`/`poll` shape) — `picker` because
/// `rfd::FileDialog::pick_folder()`, live-verified, can block indefinitely
/// with no timeout if the OS/portal dialog never resolves (see
/// TECHNICAL_DEBT.md #23); `auto_detect_rx` because a discovery spawns one
/// process per candidate install.
#[derive(Default)]
pub struct RuntimesState {
    open: Option<String>,
    last_add_error: Option<String>,
    picker: crate::folder_picker::FolderPicker,
    auto_detect_rx: Option<Receiver<Vec<RuntimeInstall>>>,
}

impl RuntimesState {
    /// Opens the dialog for runtime kind `kind`.
    pub fn open_settings(&mut self, kind: &str) {
        self.last_add_error = None;
        self.open = Some(kind.to_string());
    }

    pub fn picker_running(&self) -> bool {
        self.picker.is_open()
    }

    /// Drains a completed folder-picker dialog, if it finished since the
    /// last poll — called once a frame, same as `git_stage.rs`'s own
    /// `poll_op`. `None` covers both "still running" and "user picked
    /// nothing" — the caller only needs to know when a real folder came
    /// back.
    pub fn poll_picker(&mut self) -> Option<std::path::PathBuf> {
        self.picker.poll()
    }

    pub fn auto_detect_running(&self) -> bool {
        self.auto_detect_rx.is_some()
    }

    /// Drains a completed auto-detect scan, if it finished since the last
    /// poll — same shape as `poll_picker`. An empty `Vec` covers both
    /// "still running" and "found nothing new"; the caller only needs to
    /// know when there's something to merge in.
    pub fn poll_auto_detect(&mut self) -> Vec<RuntimeInstall> {
        let Some(rx) = self.auto_detect_rx.as_ref() else {
            return Vec::new();
        };
        match rx.try_recv() {
            Ok(installs) => {
                self.auto_detect_rx = None;
                installs
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Vec::new(),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.auto_detect_rx = None;
                Vec::new()
            }
        }
    }
}

pub fn show_settings(
    ui: &egui::Ui,
    state: &mut RuntimesState,
    handles: &[RuntimeHandle],
    inventories: &mut RuntimeInventories,
) {
    let Some(handle) = state
        .open
        .as_deref()
        .and_then(|kind| handles.iter().find(|handle| handle.id() == kind))
    else {
        return;
    };
    let inventory = inventories.entry(handle.id().to_string()).or_default();

    if let Some(folder) = state.poll_picker() {
        state.last_add_error = match handle.detect(&folder) {
            Ok(install) => {
                inventory.add(install);
                None
            }
            Err(error) => Some(error),
        };
    }
    for install in state.poll_auto_detect() {
        inventory.add(install);
    }
    if state.picker_running() || state.auto_detect_running() {
        ui.ctx().request_repaint();
    }

    let outcome = show_modal(ui, "runtimes_dialog", Some(()), |ui, ()| {
        ui.set_min_width(480.0);
        ui.heading(handle.display_name());
        ui.label(egui::RichText::new(&handle.contribution.description).weak());
        ui.separator();

        if inventory.runtimes.is_empty() {
            ui.label(egui::RichText::new(t().runtimes.none_registered).weak());
        }
        let mut remove_index = None;
        for (index, runtime) in inventory.runtimes.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&runtime.label).strong());
                ui.label(egui::RichText::new(runtime.home.display().to_string()).weak());
                if ui.small_button(t().common.remove).clicked() {
                    remove_index = Some(index);
                }
            });
        }
        if let Some(index) = remove_index {
            inventory.remove(index);
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !state.picker_running(),
                    egui::Button::new(msg::add_runtime(&handle.contribution.item_name)),
                )
                .clicked()
            {
                state.picker.open(None);
            }
            if ui
                .add_enabled(!state.auto_detect_running(), egui::Button::new(t().runtimes.auto_detect))
                .clicked()
            {
                let (tx, rx) = std::sync::mpsc::channel();
                let handle = handle.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(handle.discover());
                });
                state.auto_detect_rx = Some(rx);
            }
            if state.auto_detect_running() {
                ui.spinner();
            }
        });
        if let Some(error) = &state.last_add_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }

        ui.separator();
        ui.button(t().common.close).clicked()
    });
    if let Some((close_clicked, escape_pressed)) = outcome
        && (close_clicked || escape_pressed)
    {
        state.open = None;
    }
}

#[cfg(test)]
#[path = "runtimes_test.rs"]
mod runtimes_test;
