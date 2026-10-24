//! Extensions > Manage Extensions… — lists the extensions this build ships
//! with, what each contributes, and lets the user switch them off.
//!
//! The registry is built once at startup and grammars are installed into a
//! process-global store, so a change applies on the next launch. The dialog
//! says so rather than pretending to hot-reload.

use fg_extension::ExtensionInfo;
use fg_i18n::t;

use crate::widgets::modal::show_modal;

/// Storage key for the newline-joined ids of disabled extensions.
const DISABLED_KEY: &str = "disabled_extensions";

/// Read before the registry is built, so it takes the raw storage.
pub fn stored_disabled(storage: Option<&dyn eframe::Storage>) -> Vec<String> {
    storage
        .and_then(|storage| storage.get_string(DISABLED_KEY))
        .map(|stored| stored.lines().filter(|id| !id.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

pub fn persist_disabled(storage: &mut dyn eframe::Storage, disabled: &[String]) {
    storage.set_string(DISABLED_KEY, disabled.join("\n"));
}

pub struct ExtensionsState {
    open: bool,
    extensions: Vec<ExtensionInfo>,
    /// What the running registry was built without.
    started_disabled: Vec<String>,
    /// What the dialog currently has switched off.
    disabled: Vec<String>,
}

impl ExtensionsState {
    pub fn new(extensions: Vec<ExtensionInfo>, disabled: Vec<String>) -> Self {
        Self {
            open: false,
            extensions,
            started_disabled: disabled.clone(),
            disabled,
        }
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn disabled(&self) -> &[String] {
        &self.disabled
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        !self.disabled.iter().any(|d| d == id)
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        self.disabled.retain(|d| d != id);
        if !enabled {
            self.disabled.push(id.to_string());
        }
    }

    pub fn restart_required(&self) -> bool {
        let sorted = |ids: &[String]| {
            let mut ids = ids.to_vec();
            ids.sort();
            ids
        };
        sorted(&self.disabled) != sorted(&self.started_disabled)
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut ExtensionsState) {
    let outcome = show_modal(ui, "extensions_dialog", state.open.then_some(()), |ui, ()| {
        ui.heading(t().extensions.heading);
        ui.label(t().extensions.intro);
        ui.separator();
        for info in state.extensions.clone() {
            let mut enabled = state.is_enabled(&info.manifest.id);
            let label = format!("{} {}", info.manifest.name, info.manifest.version);
            if ui.checkbox(&mut enabled, label).changed() {
                state.set_enabled(&info.manifest.id, enabled);
            }
            ui.indent(("extension", &info.manifest.id), |ui| {
                for (heading, names) in [
                    (t().extensions.languages, &info.languages),
                    (t().extensions.language_servers, &info.language_servers),
                    (t().extensions.build_tools, &info.build_tools),
                    (t().extensions.analyzers, &info.analyzers),
                ] {
                    if !names.is_empty() {
                        ui.weak(format!("{heading}: {}", names.join(", ")));
                    }
                }
                if !info.capabilities.is_empty() {
                    let names: Vec<&str> = info.capabilities.iter().map(|c| c.id()).collect();
                    ui.weak(format!("{}: {}", t().extensions.permissions, names.join(", ")));
                }
            });
        }
        if state.restart_required() {
            ui.separator();
            ui.label(t().extensions.restart_required);
        }
        ui.separator();
        ui.button(t().common.close).clicked()
    });
    if let Some((close_clicked, escape_pressed)) = outcome
        && (close_clicked || escape_pressed)
    {
        state.open = false;
    }
}

#[cfg(test)]
#[path = "extensions_test.rs"]
mod extensions_test;
