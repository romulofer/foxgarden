//! Tools > "Run Checkstyle"/"Run PMD" (`PLAN.md` Track 5, Phases 1-2 —
//! SpotBugs lands in a later phase) plus their shared Settings > External
//! Tools… dialog. Running each tool itself lives in `fg_core::
//! checkstyle_diagnostics`/`fg_core::pmd_diagnostics`; this module is the
//! app-side wiring: persisted binary/config paths, each tool's own
//! background scan, and applying a completed scan's findings to open
//! documents.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use fg_core::{Diagnostic, EditorState};

use crate::widgets::modal::show_modal;

/// Settings > External Tools — binary/config paths for the static-analysis
/// tools Tools > "Run Checkstyle"/"Run PMD"/"Run SpotBugs" shell out to
/// (`SPEC.md` §5, none of them ship bundled). Grouped into one struct, the
/// same "travels together through persistence as one unit" shape
/// `IndentSettings`/`ViewSettings` already use, rather than four flat
/// `menu_bar::show` parameters.
#[derive(Debug, Clone, Default)]
pub struct ExternalToolPaths {
    pub checkstyle_binary: String,
    /// Checkstyle has no usable default ruleset of its own — every real
    /// run needs an explicit `-c`, so this is required (not optional
    /// convenience) for "Run Checkstyle" to do anything.
    pub checkstyle_config: String,
    pub pmd_binary: String,
    /// PMD's `-R` — one ruleset path, or several comma-separated (PMD's own
    /// `-R=<rulesets>[,<rulesets>...]` shape, forwarded verbatim); required
    /// the same way `checkstyle_config` is, PMD also has no usable default.
    pub pmd_ruleset: String,
    /// Unused until `PLAN.md` Track 5 Phase 3 lands — present now so
    /// Settings > External Tools shows every tool's row up front (Phase 1's
    /// own "shared plumbing" scope) rather than growing new UI piecemeal
    /// per phase.
    pub spotbugs_binary: String,
}

type ScanResult = Result<Vec<(PathBuf, Diagnostic)>, String>;

/// Settings > External Tools… dialog open flag, plus a still-running
/// Checkstyle and/or PMD scan (if any) — see `run_checkstyle`/`run_pmd` and
/// `poll_checkstyle`/`poll_pmd`. Two independent slots (not one shared
/// "a scan is running" flag): Checkstyle and PMD are separate Tools-menu
/// actions with separate outcomes, so one running doesn't block the other
/// from starting.
#[derive(Default)]
pub struct StaticAnalysisState {
    settings_open: bool,
    checkstyle_scan_rx: Option<Receiver<ScanResult>>,
    pmd_scan_rx: Option<Receiver<ScanResult>>,
}

/// Runs `work` on a fresh background thread and returns the receiving end
/// of the channel it'll send its one result on — the thread-plus-channel
/// mechanics shared by `run_checkstyle`/`run_pmd`, which differ only in
/// which `fg_core` function `work` calls. A real project's worth of source
/// can take long enough to analyze that running either on the UI thread
/// would freeze the whole app for that whole stretch — the same reasoning
/// `SpringEndpointsState::toggle` already documents for its own scan.
fn spawn_scan(work: impl FnOnce() -> ScanResult + Send + 'static) -> Receiver<ScanResult> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx
}

/// Drains `rx_slot`'s completed result, if any finished since the last
/// poll, clearing the slot either way once it resolves (a value or a
/// disconnected sender both mean "nothing left to wait for"). Shared by
/// `poll_checkstyle`/`poll_pmd`.
fn poll_scan(rx_slot: &mut Option<Receiver<ScanResult>>) -> Option<ScanResult> {
    let rx = rx_slot.as_ref()?;
    match rx.try_recv() {
        Ok(result) => {
            *rx_slot = None;
            Some(result)
        }
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => {
            *rx_slot = None;
            None
        }
    }
}

impl StaticAnalysisState {
    pub fn open_settings(&mut self) {
        self.settings_open = true;
    }

    /// True while a Checkstyle scan is running — `Tools > Run Checkstyle`
    /// disables itself on this so a second click can't pile up a second
    /// concurrent scan.
    pub fn checkstyle_running(&self) -> bool {
        self.checkstyle_scan_rx.is_some()
    }

    /// Same as `checkstyle_running`, for PMD.
    pub fn pmd_running(&self) -> bool {
        self.pmd_scan_rx.is_some()
    }

    /// Kicks off a Checkstyle run against `project_root` on a background
    /// thread.
    pub fn run_checkstyle(&mut self, binary: PathBuf, config: PathBuf, project_root: PathBuf) {
        self.checkstyle_scan_rx = Some(spawn_scan(move || {
            fg_core::checkstyle_diagnostics(&binary, &config, &project_root).map_err(|e| e.to_string())
        }));
    }

    /// Kicks off a PMD run against `project_root` on a background thread.
    pub fn run_pmd(&mut self, binary: PathBuf, ruleset: String, project_root: PathBuf) {
        self.pmd_scan_rx = Some(spawn_scan(move || {
            fg_core::pmd_diagnostics(&binary, &ruleset, &project_root).map_err(|e| e.to_string())
        }));
    }

    /// Drains a completed Checkstyle scan's result, if any finished since
    /// the last poll — called once per frame from `FoxGardenApp::ui`.
    pub fn poll_checkstyle(&mut self) -> Option<ScanResult> {
        poll_scan(&mut self.checkstyle_scan_rx)
    }

    /// Same as `poll_checkstyle`, for PMD.
    pub fn poll_pmd(&mut self) -> Option<ScanResult> {
        poll_scan(&mut self.pmd_scan_rx)
    }
}

/// Applies a completed scan's findings to every currently open tab, keyed
/// by path, via `field` (`|doc| &mut doc.checkstyle_diagnostics` or `|doc|
/// &mut doc.pmd_diagnostics`) — shared by `apply_checkstyle_results`/
/// `apply_pmd_results` below, since "replace this one field with whatever
/// matches this path, per open doc" is identical between tools; only
/// *which* field differs. Replaces the target field wholesale rather than
/// merging into it — a fresh run's results always supersede whatever the
/// previous run left, including a document with no findings this time
/// correctly ending up with none rather than stale squiggles from before.
/// Only open tabs get findings applied — this is a tool's results reaching
/// the same squiggle pipeline as syntax errors already does, and that
/// pipeline only exists for open documents.
fn apply_results(
    state: &mut EditorState,
    results: &[(PathBuf, Diagnostic)],
    field: impl Fn(&mut fg_core::Document) -> &mut Vec<Diagnostic>,
) {
    for doc in &mut state.open_tabs {
        *field(doc) = results
            .iter()
            .filter(|(path, _)| *path == doc.path)
            .map(|(_, diag)| diag.clone())
            .collect();
    }
}

pub fn apply_checkstyle_results(state: &mut EditorState, results: &[(PathBuf, Diagnostic)]) {
    apply_results(state, results, |doc| &mut doc.checkstyle_diagnostics);
}

pub fn apply_pmd_results(state: &mut EditorState, results: &[(PathBuf, Diagnostic)]) {
    apply_results(state, results, |doc| &mut doc.pmd_diagnostics);
}

/// Settings > External Tools… dialog — one binary/config path field per
/// tool. SpotBugs' row is still shown even though its own phase hasn't
/// landed yet, per this track's own Phase 1 "shared plumbing" scope, so a
/// user can pre-fill it; "Run SpotBugs" just doesn't exist in the Tools
/// menu yet to consume it.
pub fn show_settings(ui: &egui::Ui, state: &mut StaticAnalysisState, tools: &mut ExternalToolPaths) {
    let outcome = show_modal(ui, "external_tools_dialog", state.settings_open.then_some(()), |ui, ()| {
        ui.set_min_width(420.0);
        ui.heading("External Tools");
        ui.label(egui::RichText::new("None of these ship bundled — point each field at an already-installed binary.").weak());
        ui.separator();

        ui.strong("Checkstyle");
        labeled_path_field(ui, "checkstyle_binary_path", "Binary", &mut tools.checkstyle_binary);
        labeled_path_field(ui, "checkstyle_config_path", "Config (-c)", &mut tools.checkstyle_config);

        ui.add_space(8.0);
        ui.strong("PMD");
        labeled_path_field(ui, "pmd_binary_path", "Binary", &mut tools.pmd_binary);
        labeled_path_field(ui, "pmd_ruleset_path", "Ruleset (-R)", &mut tools.pmd_ruleset);

        ui.add_space(8.0);
        ui.strong("SpotBugs");
        labeled_path_field(ui, "spotbugs_binary_path", "Binary", &mut tools.spotbugs_binary);

        ui.separator();
        ui.button("Close").clicked()
    });
    if let Some((close_clicked, escape_pressed)) = outcome
        && (close_clicked || escape_pressed)
    {
        state.settings_open = false;
    }
}

fn labeled_path_field(ui: &mut egui::Ui, id_salt: &str, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::TextEdit::singleline(value).id_salt(id_salt).desired_width(280.0));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(msg: &str) -> Diagnostic {
        Diagnostic { range: 0..1, severity: fg_core::Severity::Warning, message: msg.to_string() }
    }

    #[test]
    fn apply_checkstyle_results_sets_matching_docs_and_clears_the_rest() {
        let (_dir_a, doc_a) = test_support::temp_document("A.java", "class A {}");
        let (_dir_b, doc_b) = test_support::temp_document("B.java", "class B {}");
        let path_a = doc_a.path.clone();
        let mut state = EditorState { open_tabs: vec![doc_a, doc_b], ..Default::default() };

        let results = vec![(path_a.clone(), diag("A has a problem"))];
        apply_checkstyle_results(&mut state, &results);

        assert_eq!(state.open_tabs[0].checkstyle_diagnostics.len(), 1);
        assert_eq!(state.open_tabs[0].checkstyle_diagnostics[0].message, "A has a problem");
        assert!(state.open_tabs[1].checkstyle_diagnostics.is_empty());
    }

    #[test]
    fn apply_checkstyle_results_replaces_rather_than_accumulates() {
        let (_dir, mut doc) = test_support::temp_document("A.java", "class A {}");
        doc.checkstyle_diagnostics = vec![diag("stale from a previous run")];
        let path = doc.path.clone();
        let mut state = EditorState { open_tabs: vec![doc], ..Default::default() };

        apply_checkstyle_results(&mut state, &[(path, diag("fresh"))]);

        assert_eq!(state.open_tabs[0].checkstyle_diagnostics.len(), 1);
        assert_eq!(state.open_tabs[0].checkstyle_diagnostics[0].message, "fresh");
    }

    #[test]
    fn checkstyle_and_pmd_results_dont_clobber_each_other() {
        let (_dir, doc) = test_support::temp_document("A.java", "class A {}");
        let path = doc.path.clone();
        let mut state = EditorState { open_tabs: vec![doc], ..Default::default() };

        apply_checkstyle_results(&mut state, &[(path.clone(), diag("from checkstyle"))]);
        apply_pmd_results(&mut state, &[(path, diag("from pmd"))]);

        assert_eq!(state.open_tabs[0].checkstyle_diagnostics.len(), 1);
        assert_eq!(state.open_tabs[0].checkstyle_diagnostics[0].message, "from checkstyle");
        assert_eq!(state.open_tabs[0].pmd_diagnostics.len(), 1);
        assert_eq!(state.open_tabs[0].pmd_diagnostics[0].message, "from pmd");
    }

    #[test]
    fn poll_returns_none_while_no_scan_is_running() {
        let mut state = StaticAnalysisState::default();
        assert!(!state.checkstyle_running());
        assert!(!state.pmd_running());
        assert!(state.poll_checkstyle().is_none());
        assert!(state.poll_pmd().is_none());
    }

    #[test]
    fn run_checkstyle_marks_running_until_polled_after_completion() {
        let mut state = StaticAnalysisState::default();
        // A binary that doesn't exist still exercises the real spawn path —
        // `checkstyle_diagnostics` reports `Err`, not a panic, and that
        // `Err` should reach `poll_checkstyle` exactly like a real failure
        // would.
        state.run_checkstyle(
            PathBuf::from("/nonexistent/checkstyle-binary"),
            PathBuf::from("/nonexistent/config.xml"),
            PathBuf::from("."),
        );
        assert!(state.checkstyle_running());

        let result = loop {
            if let Some(result) = state.poll_checkstyle() {
                break result;
            }
        };
        assert!(result.is_err());
        assert!(!state.checkstyle_running());
    }

    #[test]
    fn run_pmd_marks_running_until_polled_after_completion() {
        let mut state = StaticAnalysisState::default();
        state.run_pmd(
            PathBuf::from("/nonexistent/pmd-binary"),
            "rulesets/java/quickstart.xml".to_string(),
            PathBuf::from("."),
        );
        assert!(state.pmd_running());

        let result = loop {
            if let Some(result) = state.poll_pmd() {
                break result;
            }
        };
        assert!(result.is_err());
        assert!(!state.pmd_running());
    }

    #[test]
    fn checkstyle_and_pmd_scans_run_independently() {
        let mut state = StaticAnalysisState::default();
        state.run_checkstyle(PathBuf::from("/nonexistent/checkstyle"), PathBuf::from("/nonexistent/config"), PathBuf::from("."));
        state.run_pmd(PathBuf::from("/nonexistent/pmd"), "quickstart".to_string(), PathBuf::from("."));
        assert!(state.checkstyle_running());
        assert!(state.pmd_running());
    }
}
