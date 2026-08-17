//! Settings > JDKs… — the modal that owns the machine-wide inventory of
//! registered JDKs (`PLAN.md` Track 29 Phase 1): add one by folder picker
//! (auto-detected via a real `java -version`), remove one, see what's
//! registered. Deliberately separate from Settings > Language Servers…
//! (`panels::lsp_servers`): `LspSettings::jdtls_java_home` is "which JVM
//! runs jdt.ls itself" (always 21+), this is "which JDKs exist on this
//! machine to *target*" (any version) — different concerns.

use crate::jdk_registry::JdkRegistry;
use crate::widgets::modal::show_modal;

/// Dialog-open flag plus the last "Add JDK" failure, if any — shown inline
/// rather than through the app-wide `last_error` banner, since it's local
/// to one field in this one dialog, the same way `run_configs.rs`'s own
/// per-config fields report their own save failures.
#[derive(Default)]
pub struct JdkRegistryState {
    settings_open: bool,
    last_add_error: Option<String>,
}

impl JdkRegistryState {
    pub fn open_settings(&mut self) {
        self.last_add_error = None;
        self.settings_open = true;
    }
}

pub fn show_settings(ui: &egui::Ui, state: &mut JdkRegistryState, registry: &mut JdkRegistry) {
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
        if ui.button("Add JDK…").clicked()
            && let Some(folder) = rfd::FileDialog::new().pick_folder()
        {
            if let Err(error) = registry.detect_and_add(folder) {
                state.last_add_error = Some(error);
            } else {
                state.last_add_error = None;
            }
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
        let mut state = JdkRegistryState { settings_open: false, last_add_error: Some("stale".to_string()) };
        state.open_settings();
        assert!(state.settings_open);
        assert!(state.last_add_error.is_none());
    }
}
