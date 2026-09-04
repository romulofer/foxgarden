//! Settings > JDKs… — the modal that owns the machine-wide inventory of
//! registered JDKs (`PLAN.md` Track 29 Phase 1): add one by folder picker,
//! or Auto-detect the whole machine at once via `lsp_manager::
//! installed_runtimes` (TECHNICAL_DEBT.md #24 — reuses that scan's own
//! sdkman/asdf/jenv/macOS-bundle knowledge instead of a second copy of it),
//! remove one, see what's registered. Deliberately separate from Settings >
//! Language Servers… (`panels::lsp_servers`): `LspSettings::
//! jdtls_java_home` is "which JVM runs jdt.ls itself" (always 21+), this is
//! "which JDKs exist on this machine to *target*" (any version) —
//! different concerns.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use crate::jdk_registry::JdkRegistry;
use crate::lsp_manager::JavaRuntime;
use crate::widgets::modal::show_modal;

/// Dialog-open flag plus the last "Add JDK" failure, if any — shown inline
/// rather than through the app-wide `last_error` banner, since it's local
/// to one field in this one dialog, the same way `run_configs.rs`'s own
/// per-config fields report their own save failures.
///
/// `picker_rx`/`auto_detect_rx` both run on a background thread (mirroring
/// `git_stage.rs`'s own `spawn`/`poll` shape) rather than blocking the UI
/// thread inline — `picker_rx` because `rfd::FileDialog::pick_folder()`,
/// live-verified this session, can block indefinitely with no timeout if
/// the OS/portal dialog never resolves (see TECHNICAL_DEBT.md #23);
/// `auto_detect_rx` because `lsp_manager::installed_runtimes()` spawns one
/// `java -version` per candidate JDK, the same reason that function is
/// already documented as UI-thread-unsafe at its own call site.
#[derive(Default)]
pub struct JdkRegistryState {
    settings_open: bool,
    last_add_error: Option<String>,
    picker: crate::folder_picker::FolderPicker,
    auto_detect_rx: Option<Receiver<Vec<JavaRuntime>>>,
}

impl JdkRegistryState {
    pub fn open_settings(&mut self) {
        self.last_add_error = None;
        self.settings_open = true;
    }

    pub fn picker_running(&self) -> bool {
        self.picker.is_open()
    }

    /// Drains a completed folder-picker dialog, if it finished since the
    /// last poll — called once a frame from `FoxGardenApp::ui`, same as
    /// `git_stage.rs`'s own `poll_op`. `None` covers both "still running"
    /// and "user picked nothing" — the caller only needs to know when a
    /// real folder came back.
    pub fn poll_picker(&mut self) -> Option<PathBuf> {
        self.picker.poll()
    }

    pub fn auto_detect_running(&self) -> bool {
        self.auto_detect_rx.is_some()
    }

    /// Drains a completed auto-detect scan, if it finished since the last
    /// poll — same shape as `poll_picker`. An empty `Vec` covers both
    /// "still running" and "found nothing new"; the caller only needs to
    /// know when there's something to merge in.
    pub fn poll_auto_detect(&mut self) -> Vec<JavaRuntime> {
        let Some(rx) = self.auto_detect_rx.as_ref() else { return Vec::new() };
        match rx.try_recv() {
            Ok(runtimes) => {
                self.auto_detect_rx = None;
                runtimes
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Vec::new(),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.auto_detect_rx = None;
                Vec::new()
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
    for runtime in state.poll_auto_detect() {
        registry.add_known(runtime.path, runtime.major);
    }
    if state.picker_running() || state.auto_detect_running() {
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
        ui.horizontal(|ui| {
            if ui.add_enabled(!state.picker_running(), egui::Button::new("Add JDK…")).clicked() {
                state.picker.open(None);
            }
            if ui.add_enabled(!state.auto_detect_running(), egui::Button::new("Auto-detect")).clicked() {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::lsp_manager::installed_runtimes());
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
        let mut state = JdkRegistryState {
            settings_open: false,
            last_add_error: Some("stale".to_string()),
            picker: crate::folder_picker::FolderPicker::default(),
            auto_detect_rx: None,
        };
        state.open_settings();
        assert!(state.settings_open);
        assert!(state.last_add_error.is_none());
    }

    // The picker's own "in flight vs. resolved" mechanics are tested
    // where they now live, in `crate::folder_picker` — this dialog only
    // forwards to it.

    #[test]
    fn poll_auto_detect_drains_a_completed_scan_without_blocking_the_caller() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut state = JdkRegistryState::default();
        state.auto_detect_rx = Some(rx);
        assert!(state.auto_detect_running());

        // Nothing sent yet — still running, not a false "found nothing"
        // result (same "in flight" vs. "resolved" distinction poll_picker
        // has to make).
        assert!(state.poll_auto_detect().is_empty());
        assert!(state.auto_detect_running());

        let found = vec![JavaRuntime { major: 21, name: "JavaSE-21".to_string(), path: PathBuf::from("/jdk21") }];
        tx.send(found.clone()).unwrap();
        assert_eq!(state.poll_auto_detect(), found);
        assert!(!state.auto_detect_running());
    }

    #[test]
    fn poll_auto_detect_on_a_disconnected_sender_clears_the_slot_without_a_result() {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<JavaRuntime>>();
        let mut state = JdkRegistryState::default();
        state.auto_detect_rx = Some(rx);
        drop(tx);

        assert!(state.poll_auto_detect().is_empty());
        assert!(!state.auto_detect_running());
    }
}
