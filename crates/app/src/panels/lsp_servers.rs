//! Settings > Language Servers… — the modal that owns everything about the
//! opt-in language servers: the master on/off switch, each server's
//! binary path, and installing/updating the servers themselves through
//! `crate::lsp_manager`.
//!
//! This used to be a nested submenu under Settings, which stopped fitting
//! the moment installs entered the picture: a menu closes the instant focus
//! leaves it, so it can't host a multi-minute build's progress, its own
//! error text, or an update-available row. A modal (the same
//! `widgets::modal::show_modal` skeleton Settings > External Tools… already
//! uses for exactly this shape of dialog) can.

use crate::lsp_manager::{LatestVersionResult, LspManagerState};
use crate::lsp_settings::LspSettings;
use crate::widgets::modal::show_modal;
use fg_extension::ServerInstallHandle;
use fg_i18n::{msg, t};

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
    /// newest (see `ServerInstall::version`).
    latest_versions: std::collections::HashMap<String, LatestVersionResult>,
    /// Every server with an installer, kept from `open_settings` so the
    /// dialog and the status bar can name them without the registry.
    installers: Vec<ServerInstallHandle>,
    /// Servers whose last finished runtime scan came up empty — shown beside
    /// the runtime home field, since that's the field it's about. A server
    /// is absent before any scan has finished, so nothing is claimed until
    /// one has.
    no_runtime_found: std::collections::HashSet<String>,
    /// The open project's own declared Java release, read when the dialog
    /// opens. Display only — `lsp_state` detects this itself for the session
    /// it configures; showing it here is what makes "why is this linted as
    /// Java 8?" answerable without guessing.
    project_release: Option<fg_core::ProjectRelease>,
    /// The registry's runtime inventories, kept from `open_settings` so the
    /// "Detect" button can scan on a background thread without the registry.
    runtimes: Vec<fg_extension::RuntimeHandle>,
}

impl LspServersState {
    /// Opens the dialog, and — for each server whose runtime home isn't
    /// configured yet — starts the background scan that fills that field in,
    /// so it's already populated by the time the user reads down to it
    /// instead of sitting empty with the server's requirement stated above
    /// it.
    pub fn open_settings(
        &mut self,
        settings: &LspSettings,
        project_root: Option<&std::path::Path>,
        languages: &fg_extension::Registry,
    ) {
        self.settings_open = true;
        self.runtimes = languages.runtimes();
        self.installers = languages.server_installs();
        for installer in &self.installers {
            if let Some(host) = &installer.server.host_runtime
                && settings.runtime_home_for(installer.id()).trim().is_empty()
            {
                self.manager.detect_runtime_home(installer.id(), host.clone(), self.runtimes.clone());
            }
        }
        self.project_release = project_root.and_then(|root| languages.project_release(root));
    }

    /// Records a finished update check — called from `FoxGardenApp::ui`'s
    /// own per-frame poll of `self.lsp_servers.manager`.
    pub fn record_latest_version(&mut self, server_id: String, result: LatestVersionResult) {
        self.latest_versions.insert(server_id, result);
    }

    /// The servers the dialog offers to install; empty until it has been
    /// opened once.
    pub fn installers(&self) -> &[ServerInstallHandle] {
        &self.installers
    }

    /// Records a finished runtime scan — `FoxGardenApp::ui` writes the found
    /// path into the server's `LspSettings` runtime home itself and passes
    /// on only whether there was one.
    pub fn record_runtime_detection(&mut self, server_id: String, found: bool) {
        if found {
            self.no_runtime_found.remove(&server_id);
        } else {
            self.no_runtime_found.insert(server_id);
        }
    }

    fn latest_version(&self, server_id: &str) -> Option<&LatestVersionResult> {
        self.latest_versions.get(server_id)
    }
}

/// Settings > Language Servers… — the master switch, then one section per
/// server: install/update controls, live install progress, and the binary
/// path the running server is launched from.
pub fn show_settings(ui: &egui::Ui, state: &mut LspServersState, settings: &mut LspSettings) {
    let outcome = show_modal(ui, "lsp_servers_dialog", state.settings_open.then_some(()), |ui, ()| {
        ui.set_min_width(560.0);
        ui.heading(t().lsp.heading);
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

        for server in state.installers.clone() {
            show_server_section(ui, state, settings, &server);
        }

        ui.separator();
        ui.button(t().common.close).clicked()
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
fn show_server_section(ui: &mut egui::Ui, state: &mut LspServersState, settings: &mut LspSettings, server: &ServerInstallHandle) {
    ui.add_space(8.0);
    ui.strong(server.display_name());
    ui.label(egui::RichText::new(&server.install.note).weak());

    // Copied out before the rows below borrow `settings` mutably for the
    // path field — the buttons only need to *read* these.
    let installed_version = settings.fields_for(server.id()).1.clone();

    ui.horizontal(|ui| {
        if installed_version.is_empty() {
            ui.label(egui::RichText::new(t().common.not_installed).weak());
        } else {
            ui.label(msg::installed_version(&installed_version));
        }

        let installing = state.manager.installing(server.id());
        let install_label = if installed_version.is_empty() {
            t().install.install
        } else {
            t().install.reinstall
        };
        if ui
            .add_enabled(!installing, egui::Button::new(install_label))
            .on_hover_text(msg::installs_from(
                server.display_name(),
                &server.install.version,
                &server.install.github_repo,
            ))
            .clicked()
        {
            state.manager.install(server.clone(), server.install.version.clone());
        }

        let checking = state.manager.checking(server.id());
        if ui
            .add_enabled(
                !checking,
                egui::Button::new(if checking {
                    t().install.checking
                } else {
                    t().install.check_for_updates
                }),
            )
            .clicked()
        {
            state.manager.check_latest(server.clone());
        }

        // `install` can only ever install the version bundled with this
        // FoxGarden build (`ServerInstall::version`'s own
        // doc comment) — so a newer upstream version is reported for
        // awareness only, never as a clickable update that would just fail.
        match state.latest_version(server.id()) {
            Some(Ok(latest)) if *latest == server.install.version => {
                ui.label(egui::RichText::new(t().lsp.up_to_date).weak());
            }
            Some(Ok(latest)) => {
                ui.label(egui::RichText::new(msg::available_upstream(latest)).weak());
            }
            Some(Err(error)) => {
                ui.label(egui::RichText::new(msg::update_check_failed(&error.to_string())).weak());
            }
            None => {}
        }
    });

    // A jdt.ls build runs for minutes; a bare "Installing…" would look
    // indistinguishable from a hang, so whatever step it's on gets its own
    // line, with a spinner beside it as the "still moving" signal.
    if let Some(status) = state.manager.status(server.id()) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(status);
        });
    }

    ui.horizontal(|ui| {
        ui.label(t().lsp.binary);
        ui.add(
            egui::TextEdit::singleline(settings.fields_for(server.id()).0)
                .id_salt(("lsp_binary_path", server.id()))
                .hint_text(t().lsp.binary_hint)
                .desired_width(380.0),
        );
    });

    // A server that runs on a runtime of its own (a JVM, say) may need
    // pointing at one that isn't the machine's default — an sdkman/asdf-
    // managed install that isn't the active one. Empty auto-detects, same as
    // leaving it unset.
    if let Some(host) = &server.server.host_runtime {
        ui.horizontal(|ui| {
            ui.label(&host.home_label).on_hover_text(&host.note);
            ui.add(
                egui::TextEdit::singleline(settings.runtime_home_mut(server.id()))
                    .id_salt(("lsp_runtime_home", server.id()))
                    .desired_width(300.0),
            );
            let detecting = state.manager.detecting_runtime_home(server.id());
            if ui
                .add_enabled(!detecting, egui::Button::new(t().lsp.detect))
                .on_hover_text(t().lsp.detect_hint)
                .clicked()
            {
                state
                    .manager
                    .detect_runtime_home(server.id(), host.clone(), state.runtimes.clone());
            }
            if detecting {
                ui.spinner();
            }
        });
        // What the project itself targets, which is a different question
        // from which runtime the server runs on: the server may need 21
        // while the code targets 8. `lsp_state` hands the server every
        // installed JDK and marks this one its default, so diagnostics
        // match the project's own compiler rather than the server's runtime.
        ui.horizontal(|ui| {
            ui.label(t().lsp.project_java).on_hover_text(t().lsp.project_java_hover);
            match &state.project_release {
                Some(release) => {
                    ui.label(format!(
                        "{} — from {} ({})",
                        release.major, release.file, release.setting
                    ));
                }
                None => {
                    ui.label(egui::RichText::new(t().lsp.project_java_undeclared).weak());
                }
            }
        });

        if state.no_runtime_found.contains(server.id()) && settings.runtime_home_for(server.id()).trim().is_empty() {
            ui.label(egui::RichText::new(&host.none_found).weak());
        }
    }
}

#[cfg(test)]
#[path = "lsp_servers_test.rs"]
mod lsp_servers_test;
