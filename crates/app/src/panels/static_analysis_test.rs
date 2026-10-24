use super::*;

fn diag(msg: &str) -> Diagnostic {
    Diagnostic {
        range: 0..1,
        severity: fg_core::Severity::Warning,
        message: msg.to_string(),
        server: None,
    }
}

/// The shipped analyzers: running one with a binary that doesn't exist still
/// exercises the real spawn path.
fn analyzer(id: &str) -> AnalyzerHandle {
    fg_languages::builtin_registry()
        .analyzers()
        .into_iter()
        .find(|a| a.id() == id)
        .expect("a shipped analyzer")
}

fn run_of(binary: &str, config: &str, classes_dir: Option<&str>) -> AnalyzerRun {
    AnalyzerRun {
        binary: PathBuf::from(binary),
        config: config.to_string(),
        project_root: PathBuf::from("."),
        classes_dir: classes_dir.map(PathBuf::from),
    }
}

fn wait_for(state: &mut StaticAnalysisState, id: &str) -> ScanResult {
    loop {
        if let Some((_, result)) = state.poll().into_iter().find(|(done, _)| done == id) {
            return result;
        }
    }
}

#[test]
fn apply_results_sets_matching_docs_and_clears_the_rest() {
    let (_dir_a, doc_a) = test_support::temp_document("A.java", "class A {}");
    let (_dir_b, doc_b) = test_support::temp_document("B.java", "class B {}");
    let path_a = doc_a.path.clone();
    let mut state = EditorState {
        open_tabs: vec![doc_a, doc_b],
        ..Default::default()
    };

    let results = vec![(path_a.clone(), diag("A has a problem"))];
    apply_results(&mut state, "checkstyle", &results);

    assert_eq!(state.open_tabs[0].analyzer_diagnostics["checkstyle"].len(), 1);
    assert_eq!(
        state.open_tabs[0].analyzer_diagnostics["checkstyle"][0].message,
        "A has a problem"
    );
    assert!(state.open_tabs[1].analyzer_diagnostics["checkstyle"].is_empty());
}

#[test]
fn apply_results_replaces_rather_than_accumulates() {
    let (_dir, mut doc) = test_support::temp_document("A.java", "class A {}");
    doc.analyzer_diagnostics
        .insert("checkstyle".to_string(), vec![diag("stale from a previous run")]);
    let path = doc.path.clone();
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };

    apply_results(&mut state, "checkstyle", &[(path, diag("fresh"))]);

    assert_eq!(state.open_tabs[0].analyzer_diagnostics["checkstyle"].len(), 1);
    assert_eq!(state.open_tabs[0].analyzer_diagnostics["checkstyle"][0].message, "fresh");
}

#[test]
fn one_analyzers_results_dont_clobber_another_s() {
    let (_dir, doc) = test_support::temp_document("A.java", "class A {}");
    let path = doc.path.clone();
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };

    apply_results(&mut state, "checkstyle", &[(path.clone(), diag("from checkstyle"))]);
    apply_results(&mut state, "pmd", &[(path.clone(), diag("from pmd"))]);
    apply_results(&mut state, "spotbugs", &[(path, diag("from spotbugs"))]);

    let findings = &state.open_tabs[0].analyzer_diagnostics;
    assert_eq!(findings["checkstyle"][0].message, "from checkstyle");
    assert_eq!(findings["pmd"][0].message, "from pmd");
    assert_eq!(findings["spotbugs"][0].message, "from spotbugs");
}

#[test]
fn poll_returns_nothing_while_no_scan_is_running() {
    let mut state = StaticAnalysisState::default();
    assert!(!state.running("checkstyle"));
    assert!(state.poll().is_empty());
}

#[test]
fn a_scan_is_running_until_polled_after_completion() {
    let mut state = StaticAnalysisState::default();
    // `checkstyle` reports `Err` for a binary that doesn't exist, not a
    // panic, and that `Err` should reach `poll` exactly like a real failure
    // would.
    state.run(
        analyzer("checkstyle"),
        run_of("/nonexistent/checkstyle-binary", "/nonexistent/config.xml", None),
    );
    assert!(state.running("checkstyle"));

    assert!(wait_for(&mut state, "checkstyle").is_err());
    assert!(!state.running("checkstyle"));
}

#[test]
fn a_pmd_scan_with_a_missing_binary_fails_cleanly() {
    let mut state = StaticAnalysisState::default();
    state.run(
        analyzer("pmd"),
        run_of("/nonexistent/pmd-binary", "rulesets/java/quickstart.xml", None),
    );
    assert!(wait_for(&mut state, "pmd").is_err());
}

#[test]
fn a_spotbugs_scan_with_a_missing_binary_fails_cleanly() {
    let mut state = StaticAnalysisState::default();
    state.run(
        analyzer("spotbugs"),
        run_of("/nonexistent/spotbugs-binary", "", Some("/nonexistent/classes")),
    );
    assert!(wait_for(&mut state, "spotbugs").is_err());
}

#[test]
fn analyzer_scans_run_independently() {
    let mut state = StaticAnalysisState::default();
    state.run(analyzer("checkstyle"), run_of("/nonexistent/checkstyle", "/nonexistent/config", None));
    state.run(analyzer("pmd"), run_of("/nonexistent/pmd", "quickstart", None));
    state.run(
        analyzer("spotbugs"),
        run_of("/nonexistent/spotbugs", "", Some("/nonexistent/classes")),
    );
    assert!(state.running("checkstyle"));
    assert!(state.running("pmd"));
    assert!(state.running("spotbugs"));
}

#[test]
fn installing_writes_the_binary_config_and_version_into_that_analyzers_entry() {
    let mut tools = ExternalToolPaths::default();
    tools.get_mut("pmd").binary = "/hand/typed".to_string();

    tools.apply_installed(&tool_manager::Installed {
        analyzer_id: "pmd".to_string(),
        version: "7.26.0".to_string(),
        binary: PathBuf::from("/cache/pmd/bin/pmd"),
        default_config: Some("rulesets/java/quickstart.xml".to_string()),
    });

    let pmd = tools.get("pmd").unwrap();
    assert_eq!(pmd.binary, "/cache/pmd/bin/pmd");
    assert_eq!(pmd.config, "rulesets/java/quickstart.xml");
    assert_eq!(pmd.installed_version, "7.26.0");
    assert!(tools.get("checkstyle").is_none());
}
