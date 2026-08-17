//! Settings > JDKs… — the modal that owns the machine-wide inventory of
//! registered JDKs (`PLAN.md` Track 29 Phase 1): add one by folder picker
//! (auto-detected via a real `java -version`), remove one, see what's
//! registered. Deliberately separate from Settings > Language Servers…
//! (`panels::lsp_servers`): `LspSettings::jdtls_java_home` is "which JVM
//! runs jdt.ls itself" (always 21+), this is "which JDKs exist on this
//! machine to *target*" (any version) — different concerns.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use crate::jdk_registry::JdkRegistry;
use crate::widgets::modal::show_modal;

/// Dialog-open flag plus the last "Add JDK" failure, if any — shown inline
/// rather than through the app-wide `last_error` banner, since it's local
/// to one field in this one dialog, the same way `run_configs.rs`'s own
/// per-config fields report their own save failures.
///
/// `picker_rx` runs the native folder-picker dialog on a background thread
/// (mirroring `git_stage.rs`'s own `spawn`/`poll` shape) rather than
/// calling `rfd::FileDialog::pick_folder()` inline on click — that call is
/// synchronous and, live-verified this session, blocks the whole UI thread
/// with no timeout for as long as the OS/portal dialog takes to resolve
/// (indefinitely, if the portal never responds) — see TECHNICAL_DEBT.md.
#[derive(Default)]
pub struct JdkRegistryState {
    settings_open: bool,
    last_add_error: Option<String>,
    picker_rx: Option<Receiver<Option<PathBuf>>>,
}

impl JdkRegistryState {
    pub fn open_settings(&mut self) {
        self.last_add_error = None;
        self.settings_open = true;
    }

    pub fn picker_running(&self) -> bool {
        self.picker_rx.is_some()
    }

    /// Drains a completed folder-picker dialog, if it finished since the
    /// last poll — called once a frame from `FoxGardenApp::ui`, same as
    /// `git_stage.rs`'s own `poll_op`. `None` covers both "still running"
    /// and "user picked nothing" — the caller only needs to know when a
    /// real folder came back.
    pub fn poll_picker(&mut self) -> Option<PathBuf> {
        let rx = self.picker_rx.as_ref()?;
        match rx.try_recv() {
            Ok(folder) => {
                self.picker_rx = None;
                folder
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.picker_rx = None;
                None
            }
        }
    }
}

pub fn show_settings(ui: &egui::Ui, state: &mut JdkRegistryState, registry: &mut JdkRegistry) {
    if let Some(folder) = state.poll_picker() {
        if let Err(error) = registry.detect_and_add(folder) {
            state.last_add_error = Some(error);
        } else {
            state.last_add_error = None;
        }
    }
    if state.picker_running() {
        ui.ctx().request_repaint();
    }

    let outcome = show_modal(ui, "jdk_registry_dialog", state.settings_open.then_some(()), |ui, ()| {
        ui.set_min_width(480.0);
        ui.heading("JDKs");
        ui.label(
            egui::RichText::new(
                "JDKs registered here are available to target when analyzing or scaffolding a project at a \
                 specific Java version — separate from Settings > Language Servers…'s Java Home, which is only \
                 the JVM jdt.ls itself runs under.",
            )
            .weak(),
        );
        ui.separator();

        if registry.jdks.is_empty() {
            ui.label(egui::RichText::new("No JDKs registered yet.").weak());
        }
        let mut remove_index = None;
        for (index, jdk) in registry.jdks.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&jdk.label).strong());
                ui.label(egui::RichText::new(jdk.home.display().to_string()).weak());
                if ui.small_button("Remove").clicked() {
                    remove_index = Some(index);
                }
            });
        }
        if let Some(index) = remove_index {
            registry.remove(index);
        }

        ui.add_space(4.0);
        if ui.add_enabled(!state.picker_running(), egui::Button::new("Add JDK…")).clicked() {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(rfd::FileDialog::new().pick_folder());
            });
            state.picker_rx = Some(rx);
        }
        if let Some(error) = &state.last_add_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }

        ui.separator();
        ui.button("Close").clicked()
    });
    if let Some((close_clicked, escape_pressed)) = outcome
        && (close_clicked || escape_pressed)
    {
        state.settings_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_settings_opens_the_dialog_and_clears_the_previous_error() {
        let mut state =
            JdkRegistryState { settings_open: false, last_add_error: Some("stale".to_string()), picker_rx: None };
        state.open_settings();
        assert!(state.settings_open);
        assert!(state.last_add_error.is_none());
    }

    #[test]
    fn poll_picker_drains_a_completed_pick_without_blocking_the_caller() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut state = JdkRegistryState::default();
        state.picker_rx = Some(rx);
        assert!(state.picker_running());

        // Nothing sent yet — still running, not a false "user picked
        // nothing" result (the bug this whole background-thread shape
        // exists to avoid: never mistake "in flight" for "resolved").
        assert!(state.poll_picker().is_none());
        assert!(state.picker_running());

        tx.send(Some(PathBuf::from("/opt/jdk21"))).unwrap();
        assert_eq!(state.poll_picker(), Some(PathBuf::from("/opt/jdk21")));
        assert!(!state.picker_running());
    }

    #[test]
    fn poll_picker_on_a_disconnected_sender_clears_the_slot_without_a_result() {
        let (tx, rx) = std::sync::mpsc::channel::<Option<PathBuf>>();
        let mut state = JdkRegistryState::default();
        state.picker_rx = Some(rx);
        drop(tx);

        assert!(state.poll_picker().is_none());
        assert!(!state.picker_running());
    }
}
