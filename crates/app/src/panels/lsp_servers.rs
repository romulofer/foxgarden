//! Settings > Language Servers… — the modal that owns everything about the
//! two opt-in language servers: the master on/off switch, each server's
//! binary path, and installing/updating the servers themselves through
//! `crate::lsp_manager`.
//!
//! This used to be a nested submenu under Settings, which stopped fitting
//! the moment installs entered the picture: a menu closes the instant focus
//! leaves it, so it can't host a multi-minute build's progress, its own
//! error text, or an update-available row. A modal (the same
//! `widgets::modal::show_modal` skeleton Settings > External Tools… already
//! uses for exactly this shape of dialog) can.

use crate::lsp_manager::{ALL_SERVERS, LatestVersionResult, LspManagerState, Server};
use crate::lsp_settings::LspSettings;
use crate::widgets::modal::show_modal;

/// Dialog-open flag plus the background install/update-check machinery it
/// drives. Lives on `FoxGardenApp` (not in `MenuBarState`) for the same
/// reason `StaticAnalysisState` does: it's the feature's own state, and the
/// menu only asks for it to be opened.
#[derive(Default)]
pub struct LspServersState {
    settings_open: bool,
    pub manager: LspManagerState,
    /// The last "Check for Updates" answer per server, for display only —
    /// session-only and never persisted, since Install always installs a
    /// specific chosen version rather than silently tracking whatever is
    /// newest (see `lsp_manager::Server::recommended_version`).
    latest_versions: std::collections::HashMap<Server, LatestVersionResult>,
}

impl LspServersState {
    pub fn open_settings(&mut self) {
        self.settings_open = true;
    }

    /// Records a finished update check — called from `FoxGardenApp::ui`'s
    /// own per-frame poll of `self.lsp_servers.manager`.
    pub fn record_latest_version(&mut self, server: Server, result: LatestVersionResult) {
        self.latest_versions.insert(server, result);
    }

    fn latest_version(&self, server: Server) -> Option<&LatestVersionResult> {
        self.latest_versions.get(&server)
    }
}

/// Settings > Language Servers… — the master switch, then one section per
/// server: install/update controls, live install progress, and the binary
/// path the running server is launched from.
pub fn show_settings(ui: &egui::Ui, state: &mut LspServersState, settings: &mut LspSettings) {
    let outcome = show_modal(ui, "lsp_servers_dialog", state.settings_open.then_some(()), |ui, ()| {
        ui.set_min_width(560.0);
        ui.heading("Language Servers");
        ui.label(
            egui::RichText::new(
                "Semantic diagnostics, completion and hover docs for Java and Kotlin come from an external \
                 language server. Install one below, or point a field at a server you already have. Nothing \
                 launches until Enabled is on and a matching file is open.",
            )
            .weak(),
        );
        ui.separator();

        ui.checkbox(&mut settings.enabled, "Enabled");
        ui.add_space(4.0);

        for server in ALL_SERVERS {
            show_server_section(ui, state, settings, server);
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

/// One server's whole section. Kept as a single function rather than split
/// per-row because the pieces are genuinely coupled: which install button
/// to show depends on the installed version, which update button to show
/// depends on the last check's result, and both are disabled by the same
/// in-flight job.
fn show_server_section(ui: &mut egui::Ui, state: &mut LspServersState, settings: &mut LspSettings, server: Server) {
    ui.add_space(8.0);
    ui.strong(server.display_name());
    ui.label(egui::RichText::new(server.install_note()).weak());

    // Copied out before the rows below borrow `settings` mutably for the
    // path field — the buttons only need to *read* these.
    let installed_version = settings.fields_for(server).1.clone();

    ui.horizontal(|ui| {
        if installed_version.is_empty() {
            ui.label(egui::RichText::new("Not installed").weak());
        } else {
            ui.label(format!("Installed: {installed_version}"));
        }

        let installing = state.manager.installing(server);
        let install_label = if installed_version.is_empty() {
            "Install"
        } else {
            "Reinstall"
        };
        if ui
            .add_enabled(!installing, egui::Button::new(install_label))
            .on_hover_text(format!(
                "Installs {} {}, from {}",
                server.display_name(),
                server.recommended_version(),
                server.github_repo(),
            ))
            .clicked()
        {
            state.manager.install(server, server.recommended_version().to_string());
        }

        let checking = state.manager.checking(server);
        if ui
            .add_enabled(
                !checking,
                egui::Button::new(if checking { "Checking…" } else { "Check for Updates" }),
            )
            .clicked()
        {
            state.manager.check_latest(server);
        }

        // `install` can only ever install the version bundled with this
        // FoxGarden build (`lsp_manager::Server::recommended_version`'s own
        // doc comment) — so a newer upstream version is reported for
        // awareness only, never as a clickable update that would just fail.
        match state.latest_version(server) {
            Some(Ok(latest)) if *latest == server.recommended_version() => {
                ui.label(egui::RichText::new("Up to date").weak());
            }
            Some(Ok(latest)) => {
                ui.label(egui::RichText::new(format!("{latest} available upstream (not yet bundled)")).weak());
            }
            Some(Err(error)) => {
                ui.label(egui::RichText::new(format!("Update check failed: {error}")).weak());
            }
            None => {}
        }
    });

    // A jdt.ls build runs for minutes; a bare "Installing…" would look
    // indistinguishable from a hang, so whatever step it's on gets its own
    // line, with a spinner beside it as the "still moving" signal.
    if let Some(status) = state.manager.status(server) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(status);
        });
    }

    ui.horizontal(|ui| {
        ui.label("Binary");
        ui.add(
            egui::TextEdit::singleline(settings.fields_for(server).0)
                .id_salt(match server {
                    Server::Jdtls => "jdtls_binary_path",
                    Server::KotlinLanguageServer => "kotlin_language_server_binary_path",
                })
                .desired_width(380.0),
        );
    });

    // jdt.ls hard-requires Java 21 *to run* (not just to build) — on a
    // machine whose default `java` isn't 21 (an sdkman/asdf-managed install
    // that isn't the active one, say), this is how to point jdt.ls at one
    // without changing the system default. Empty auto-detects from
    // `JAVA_HOME`/`PATH`, same as leaving it unset.
    if let Server::Jdtls = server {
        ui.horizontal(|ui| {
            ui.label("Java Home").on_hover_text(
                "jdt.ls requires a JDK 21+ to run. Leave blank to use JAVA_HOME/PATH, or point this at a specific \
                 JDK 21 install (e.g. ~/.sdkman/candidates/java/21.0.11-zulu).",
            );
            ui.add(
                egui::TextEdit::singleline(&mut settings.jdtls_java_home)
                    .id_salt("jdtls_java_home")
                    .desired_width(380.0),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_settings_opens_the_dialog() {
        let mut state = LspServersState::default();
        assert!(!state.settings_open);
        state.open_settings();
        assert!(state.settings_open);
    }

    #[test]
    fn recording_a_check_result_keeps_the_two_servers_separate() {
        let mut state = LspServersState::default();
        state.record_latest_version(Server::Jdtls, Ok("1.60.0".to_string()));
        assert_eq!(state.latest_version(Server::Jdtls), Some(&Ok("1.60.0".to_string())));
        assert!(state.latest_version(Server::KotlinLanguageServer).is_none());
    }

    /// A later check replaces the earlier answer rather than accumulating
    /// stale ones — the dialog only ever shows the most recent.
    #[test]
    fn a_later_check_replaces_the_previous_result() {
        let mut state = LspServersState::default();
        state.record_latest_version(Server::Jdtls, Err("offline".to_string()));
        state.record_latest_version(Server::Jdtls, Ok("1.60.0".to_string()));
        assert_eq!(state.latest_version(Server::Jdtls), Some(&Ok("1.60.0".to_string())));
    }
}
