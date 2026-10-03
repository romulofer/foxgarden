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
    /// newest (see `lsp_manager::Server::recommended_version`).
    latest_versions: std::collections::HashMap<Server, LatestVersionResult>,
    /// Whether the last finished JDK scan came up empty — shown beside the
    /// Java Home field, since that's the field it's about. `false` before
    /// any scan has finished, so nothing is claimed until one has.
    no_java_home_found: bool,
    /// The open project's own declared Java release, read when the dialog
    /// opens. Display only — `lsp_state` detects this itself for the session
    /// it configures; showing it here is what makes "why is this linted as
    /// Java 8?" answerable without guessing.
    project_release: Option<fg_core::JavaRelease>,
}

impl LspServersState {
    /// Opens the dialog, and — when no Java Home is configured yet — starts
    /// the background JDK scan that fills that field in, so it's already
    /// populated by the time the user reads down to it instead of sitting
    /// empty with jdt.ls' Java 21 requirement stated above it.
    pub fn open_settings(&mut self, settings: &LspSettings, project_root: Option<&std::path::Path>) {
        self.settings_open = true;
        if settings.jdtls_java_home.trim().is_empty() {
            self.manager.detect_java_home();
        }
        self.project_release = project_root.and_then(fg_core::detect_java_release);
    }

    /// Records a finished update check — called from `FoxGardenApp::ui`'s
    /// own per-frame poll of `self.lsp_servers.manager`.
    pub fn record_latest_version(&mut self, server: Server, result: LatestVersionResult) {
        self.latest_versions.insert(server, result);
    }

    /// Records a finished JDK scan — `FoxGardenApp::ui` writes the found
    /// path into `LspSettings::jdtls_java_home` itself and passes on only
    /// whether there was one.
    pub fn record_java_home_detection(&mut self, found: bool) {
        self.no_java_home_found = !found;
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

        for server in ALL_SERVERS {
            show_server_section(ui, state, settings, server);
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
fn show_server_section(ui: &mut egui::Ui, state: &mut LspServersState, settings: &mut LspSettings, server: Server) {
    ui.add_space(8.0);
    ui.strong(server.display_name());
    ui.label(egui::RichText::new(server.install_note()).weak());

    // Copied out before the rows below borrow `settings` mutably for the
    // path field — the buttons only need to *read* these.
    let installed_version = settings.fields_for(server).1.clone();

    ui.horizontal(|ui| {
        if installed_version.is_empty() {
            ui.label(egui::RichText::new(t().common.not_installed).weak());
        } else {
            ui.label(msg::installed_version(&installed_version));
        }

        let installing = state.manager.installing(server);
        let install_label = if installed_version.is_empty() {
            t().install.install
        } else {
            t().install.reinstall
        };
        if ui
            .add_enabled(!installing, egui::Button::new(install_label))
            .on_hover_text(msg::installs_from(
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
                egui::Button::new(if checking {
                    t().install.checking
                } else {
                    t().install.check_for_updates
                }),
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
    if let Some(status) = state.manager.status(server) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(status);
        });
    }

    ui.horizontal(|ui| {
        ui.label(t().lsp.binary);
        ui.add(
            egui::TextEdit::singleline(settings.fields_for(server).0)
                .id_salt(match server {
                    Server::Jdtls => "jdtls_binary_path",
                    Server::KotlinLanguageServer => "kotlin_language_server_binary_path",
                })
                .hint_text(t().lsp.binary_hint)
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
            ui.label(t().lsp.java_home).on_hover_text(t().lsp.java_home_hover);
            ui.add(
                egui::TextEdit::singleline(&mut settings.jdtls_java_home)
                    .id_salt("jdtls_java_home")
                    .desired_width(300.0),
            );
            let detecting = state.manager.detecting_java_home();
            if ui
                .add_enabled(!detecting, egui::Button::new(t().lsp.detect))
                .on_hover_text(t().lsp.detect_hint)
                .clicked()
            {
                state.manager.detect_java_home();
            }
            if detecting {
                ui.spinner();
            }
        });
        // What the project itself targets, which is a different question
        // from which JVM jdt.ls runs on: jdt.ls needs 21, the code may be
        // Java 8. `lsp_state` hands jdt.ls every installed JDK and marks
        // this one its default, so diagnostics match the project's own
        // compiler rather than jdt.ls' JVM.
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

        if state.no_java_home_found && settings.jdtls_java_home.trim().is_empty() {
            ui.label(
                egui::RichText::new(
                    "No JDK 21 or newer found on this machine — install one, or type its path above. jdt.ls won't \
                     start without it.",
                )
                .weak(),
            );
        }
    }
}

#[cfg(test)]
#[path = "lsp_servers_test.rs"]
mod lsp_servers_test;
