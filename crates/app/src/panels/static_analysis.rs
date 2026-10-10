//! Tools > "Run <analyzer>" for every analyzer an extension contributes
//! (`PLAN.md` Track 5), plus their shared Settings > External Tools… dialog,
//! including installing/updating the tools themselves
//! (`crate::tool_manager`). Running a tool and reading its report is the
//! contributing extension's (`fg_extension::AnalyzerHandle::run`); this
//! module is the app-side wiring: persisted binary/config paths, each
//! analyzer's own background scan, and applying a completed scan's findings
//! to open documents.

use fg_i18n::{msg, t};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use fg_core::{Diagnostic, EditorState};
use fg_extension::{AnalyzerHandle, AnalyzerRun};

use crate::tool_manager::{self, ToolManagerState};
use crate::widgets::modal::show_modal;

/// One analyzer's Settings > External Tools entry: where its binary is,
/// what configuration to run it with, and which version (if any)
/// `tool_manager` last installed into its own cache directory — `""` means
/// "not installed via the in-app downloader," which is also the correct
/// default for someone who's pointed a binary field at an existing system
/// install instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnalyzerSettings {
    pub binary: String,
    /// Required (not optional convenience) for an analyzer whose
    /// contribution declares a `config_label`: Checkstyle and PMD have no
    /// usable default of their own, so "Run" does nothing without it.
    pub config: String,
    pub installed_version: String,
}

/// Settings > External Tools — the binary/config paths for every analyzer,
/// keyed by analyzer id (`SPEC.md` §5). Grouped into one struct, the same
/// "travels together through persistence as one unit" shape
/// `IndentSettings`/`ViewSettings` already use, rather than a growing list
/// of flat `menu_bar::show` parameters.
#[derive(Debug, Clone, Default)]
pub struct ExternalToolPaths {
    analyzers: HashMap<String, AnalyzerSettings>,
}

impl ExternalToolPaths {
    pub fn get(&self, analyzer_id: &str) -> Option<&AnalyzerSettings> {
        self.analyzers.get(analyzer_id)
    }

    pub fn get_mut(&mut self, analyzer_id: &str) -> &mut AnalyzerSettings {
        self.analyzers.entry(analyzer_id.to_string()).or_default()
    }

    /// Writes `installed`'s binary path (and default config, if the tool
    /// has one) into the matching entry, overwriting whatever was there —
    /// called after `tool_manager::ToolManagerState::install` completes.
    /// An explicit "Install"/"Reinstall" click is exactly the case where
    /// overwriting a hand-typed path is the right call: the user just
    /// asked this app to go get a fresh copy and use it.
    pub fn apply_installed(&mut self, installed: &tool_manager::Installed) {
        let entry = self.get_mut(&installed.analyzer_id);
        entry.binary = installed.binary.display().to_string();
        entry.config = installed.default_config.clone().unwrap_or_default();
        entry.installed_version = installed.version.clone();
    }

    /// Reads each of `analyzer_ids`' settings back out of `storage`, leaving
    /// the ones that were never saved at their defaults. The keys are
    /// `<id>_binary`, `<id>_config` and `<id>_installed_version`.
    pub fn restore(&mut self, storage: &dyn eframe::Storage, analyzer_ids: &[&str]) {
        for &id in analyzer_ids {
            let entry = self.get_mut(id);
            if let Some(binary) = storage.get_string(&storage_key(id, "binary")) {
                entry.binary = binary;
            }
            // Before analyzers were contributed by extensions, PMD's
            // configuration was stored as `pmd_ruleset`; read it when the
            // current key was never written so an upgrade keeps it.
            if let Some(config) = storage
                .get_string(&storage_key(id, "config"))
                .or_else(|| storage.get_string(&storage_key(id, "ruleset")))
            {
                entry.config = config;
            }
            if let Some(version) = storage.get_string(&storage_key(id, "installed_version")) {
                entry.installed_version = version;
            }
        }
    }

    /// Writes each of `analyzer_ids`' settings to `storage`; the inverse of
    /// [`Self::restore`].
    pub fn persist(&self, storage: &mut dyn eframe::Storage, analyzer_ids: &[&str]) {
        for &id in analyzer_ids {
            let settings = self.get(id).cloned().unwrap_or_default();
            storage.set_string(&storage_key(id, "binary"), settings.binary);
            storage.set_string(&storage_key(id, "config"), settings.config);
            storage.set_string(&storage_key(id, "installed_version"), settings.installed_version);
        }
    }
}

fn storage_key(analyzer_id: &str, field: &str) -> String {
    format!("{analyzer_id}_{field}")
}

type ScanResult = Result<Vec<(PathBuf, Diagnostic)>, String>;

/// Settings > External Tools… dialog open flag, plus every still-running
/// analyzer scan — see `run` and `poll`. One slot per analyzer (not one
/// shared "a scan is running" flag): each is a separate Tools-menu action
/// with its own outcome, so one running doesn't block another from starting.
#[derive(Default)]
pub struct StaticAnalysisState {
    settings_open: bool,
    scans: HashMap<String, Receiver<ScanResult>>,
    /// Backs the Settings > External Tools… dialog's own Install/Check for
    /// Updates buttons — a separate concern from the scans above
    /// (installing a tool vs. running it).
    pub tool_manager: ToolManagerState,
    /// The last "Check for Updates" result per analyzer, for the Settings
    /// dialog to display — session-only (not persisted, and not written
    /// back into `ExternalToolPaths`): this app never auto-installs
    /// whatever GitHub calls latest (an extension pins a verified version
    /// instead), so this is purely informational, re-fetched fresh every
    /// time the user asks.
    latest_versions: HashMap<String, tool_manager::LatestVersionResult>,
}

impl StaticAnalysisState {
    pub fn open_settings(&mut self) {
        self.settings_open = true;
    }

    /// True while `analyzer_id`'s scan is running — its Tools menu entry
    /// disables itself on this so a second click can't pile up a second
    /// concurrent scan.
    pub fn running(&self, analyzer_id: &str) -> bool {
        self.scans.contains_key(analyzer_id)
    }

    /// Kicks off `analyzer`'s run on a background thread. A real project's
    /// worth of source can take long enough to analyze that running it on
    /// the UI thread would freeze the whole app for that whole stretch — the
    /// same reasoning `HttpRoutesState::toggle` already documents for its
    /// own scan.
    pub fn run(&mut self, analyzer: AnalyzerHandle, run: AnalyzerRun) {
        let (tx, rx) = channel();
        let id = analyzer.id().to_string();
        std::thread::spawn(move || {
            let _ = tx.send(analyzer.run(&run).map(fg_core::analyzer_diagnostics));
        });
        self.scans.insert(id, rx);
    }

    /// Drains every scan that finished since the last poll — called once
    /// per frame from `FoxGardenApp::ui`. A slot is cleared once it resolves
    /// (a value or a disconnected sender both mean "nothing left to wait
    /// for").
    pub fn poll(&mut self) -> Vec<(String, ScanResult)> {
        let mut done = Vec::new();
        self.scans.retain(|id, rx| match rx.try_recv() {
            Ok(result) => {
                done.push((id.clone(), result));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        done
    }

    /// Records a completed "Check for Updates" result for the Settings
    /// dialog to read back via `latest_version` — called from
    /// `FoxGardenApp::ui`'s per-frame poll of `self.tool_manager`.
    pub fn record_latest_version(&mut self, analyzer_id: String, result: tool_manager::LatestVersionResult) {
        self.latest_versions.insert(analyzer_id, result);
    }

    fn latest_version(&self, analyzer_id: &str) -> Option<&tool_manager::LatestVersionResult> {
        self.latest_versions.get(analyzer_id)
    }
}

/// Applies a completed scan's findings for `analyzer_id` to every currently
/// open tab, keyed by path. Replaces that analyzer's findings wholesale
/// rather than merging into them — a fresh run's results always supersede
/// whatever the previous run left, including a document with no findings
/// this time correctly ending up with none rather than stale squiggles from
/// before. Only open tabs get findings applied — this is a tool's results
/// reaching the same squiggle pipeline as syntax errors already does, and
/// that pipeline only exists for open documents.
pub fn apply_results(state: &mut EditorState, analyzer_id: &str, results: &[(PathBuf, Diagnostic)]) {
    for doc in &mut state.open_tabs {
        let findings = results
            .iter()
            .filter(|(path, _)| *path == doc.path)
            .map(|(_, diag)| diag.clone())
            .collect();
        doc.analyzer_diagnostics.insert(analyzer_id.to_string(), findings);
    }
}

/// Settings > External Tools… dialog — one section per analyzer: a binary
/// path field, a configuration field when the analyzer takes one, and an
/// Install/Update row backed by `crate::tool_manager` when the extension
/// says how to fetch it. A tool doesn't have to already be on the user's
/// system for these fields to get filled in.
pub fn show_settings(
    ui: &egui::Ui,
    state: &mut StaticAnalysisState,
    tools: &mut ExternalToolPaths,
    analyzers: &[AnalyzerHandle],
) {
    let outcome = show_modal(
        ui,
        "external_tools_dialog",
        state.settings_open.then_some(()),
        |ui, ()| {
            ui.set_min_width(460.0);
            ui.heading(t().external_tools.heading);
            ui.label(
                egui::RichText::new(
                    "Install fetches a specific, verified-compatible release into a local cache — not necessarily \
                 GitHub's newest (hover \"GitHub's latest\" below for why). Still needs a JVM already on PATH. \
                 Or point the fields at a binary you've already installed yourself.",
                )
                .weak(),
            );
            ui.separator();

            for (index, analyzer) in analyzers.iter().enumerate() {
                if index > 0 {
                    ui.add_space(8.0);
                }
                let id = analyzer.id();
                ui.strong(analyzer.display_name());
                let installed_version = tools.get(id).map(|s| s.installed_version.clone()).unwrap_or_default();
                show_install_row(ui, state, analyzer, &installed_version);
                let settings = tools.get_mut(id);
                labeled_path_field(ui, &format!("{id}_binary_path"), "Binary", &mut settings.binary);
                if let Some(label) = &analyzer.contribution.config_label {
                    labeled_path_field(ui, &format!("{id}_config_path"), label, &mut settings.config);
                }
            }

            ui.separator();
            ui.button(t().common.close).clicked()
        },
    );
    if let Some((close_clicked, escape_pressed)) = outcome
        && (close_clicked || escape_pressed)
    {
        state.settings_open = false;
    }
}

/// One analyzer's Install/Update row: installed-version status, an Install
/// (or Reinstall) button, and a Check for Updates button + whatever its last
/// result was. Kicks off `state.tool_manager`'s background work directly
/// from the button click — this dialog already owns `&mut
/// StaticAnalysisState`, so there's no need to bubble a `menu_bar`-style
/// outcome flag up through `FoxGardenApp::ui` just to start it one frame
/// later. Nothing is drawn for an analyzer the extension can't fetch.
fn show_install_row(ui: &mut egui::Ui, state: &mut StaticAnalysisState, analyzer: &AnalyzerHandle, installed_version: &str) {
    let Some(install) = &analyzer.contribution.install else {
        return;
    };
    let id = analyzer.id();
    ui.horizontal(|ui| {
        if installed_version.is_empty() {
            ui.label(egui::RichText::new(t().common.not_installed).weak());
        } else {
            ui.label(msg::installed_version(installed_version));
        }

        let installing = state.tool_manager.installing(id);
        let install_label = if installing {
            t().install.installing
        } else if installed_version.is_empty() {
            t().install.install
        } else {
            t().install.reinstall
        };
        if ui.add_enabled(!installing, egui::Button::new(install_label)).clicked() {
            state
                .tool_manager
                .install(id, analyzer.display_name(), install.clone());
        }

        if let Some(latest) = &install.latest_release {
            let checking = state.tool_manager.checking(id);
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
                state.tool_manager.check_latest(id, latest.clone());
            }
        }

        match state.latest_version(id) {
            Some(Ok(version)) if version == installed_version => {
                ui.label(egui::RichText::new(msg::up_to_date_at(version)).weak());
            }
            // Deliberately not "you're behind" phrasing — Install never
            // auto-upgrades to this, so a version mismatch here is the
            // normal steady state, not a warning. See the hover text.
            Some(Ok(version)) => {
                ui.label(egui::RichText::new(msg::githubs_latest(version)).weak())
                    .on_hover_text(msg::install_pin_explanation(&install.version));
            }
            Some(Err(err)) => {
                ui.label(egui::RichText::new(msg::update_check_failed(&err.to_string())).weak());
            }
            None => {}
        }
    });
}

fn labeled_path_field(ui: &mut egui::Ui, id_salt: &str, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::TextEdit::singleline(value).id_salt(id_salt).desired_width(280.0));
    });
}

#[cfg(test)]
#[path = "static_analysis_test.rs"]
mod static_analysis_test;
