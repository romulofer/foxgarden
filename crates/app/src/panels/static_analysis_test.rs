
use super::*;

fn diag(msg: &str) -> Diagnostic {
    Diagnostic {
        range: 0..1,
        severity: fg_core::Severity::Warning,
        message: msg.to_string(),
    }
}

#[test]
fn apply_checkstyle_results_sets_matching_docs_and_clears_the_rest() {
    let (_dir_a, doc_a) = test_support::temp_document("A.java", "class A {}");
    let (_dir_b, doc_b) = test_support::temp_document("B.java", "class B {}");
    let path_a = doc_a.path.clone();
    let mut state = EditorState {
        open_tabs: vec![doc_a, doc_b],
        ..Default::default()
    };

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
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };

    apply_checkstyle_results(&mut state, &[(path, diag("fresh"))]);

    assert_eq!(state.open_tabs[0].checkstyle_diagnostics.len(), 1);
    assert_eq!(state.open_tabs[0].checkstyle_diagnostics[0].message, "fresh");
}

#[test]
fn checkstyle_and_pmd_results_dont_clobber_each_other() {
    let (_dir, doc) = test_support::temp_document("A.java", "class A {}");
    let path = doc.path.clone();
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };

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
    assert!(!state.spotbugs_running());
    assert!(state.poll_checkstyle().is_none());
    assert!(state.poll_pmd().is_none());
    assert!(state.poll_spotbugs().is_none());
}

#[test]
fn apply_spotbugs_results_leaves_checkstyle_and_pmd_untouched() {
    let (_dir, doc) = test_support::temp_document("A.java", "class A {}");
    let path = doc.path.clone();
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };

    apply_checkstyle_results(&mut state, &[(path.clone(), diag("from checkstyle"))]);
    apply_pmd_results(&mut state, &[(path.clone(), diag("from pmd"))]);
    apply_spotbugs_results(&mut state, &[(path, diag("from spotbugs"))]);

    assert_eq!(state.open_tabs[0].checkstyle_diagnostics[0].message, "from checkstyle");
    assert_eq!(state.open_tabs[0].pmd_diagnostics[0].message, "from pmd");
    assert_eq!(state.open_tabs[0].spotbugs_diagnostics.len(), 1);
    assert_eq!(state.open_tabs[0].spotbugs_diagnostics[0].message, "from spotbugs");
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
fn run_spotbugs_marks_running_until_polled_after_completion() {
    let mut state = StaticAnalysisState::default();
    state.run_spotbugs(
        PathBuf::from("/nonexistent/spotbugs-binary"),
        PathBuf::from("/nonexistent/classes"),
        PathBuf::from("."),
    );
    assert!(state.spotbugs_running());

    let result = loop {
        if let Some(result) = state.poll_spotbugs() {
            break result;
        }
    };
    assert!(result.is_err());
    assert!(!state.spotbugs_running());
}

#[test]
fn checkstyle_pmd_and_spotbugs_scans_run_independently() {
    let mut state = StaticAnalysisState::default();
    state.run_checkstyle(
        PathBuf::from("/nonexistent/checkstyle"),
        PathBuf::from("/nonexistent/config"),
        PathBuf::from("."),
    );
    state.run_pmd(
        PathBuf::from("/nonexistent/pmd"),
        "quickstart".to_string(),
        PathBuf::from("."),
    );
    state.run_spotbugs(
        PathBuf::from("/nonexistent/spotbugs"),
        PathBuf::from("/nonexistent/classes"),
        PathBuf::from("."),
    );
    assert!(state.checkstyle_running());
    assert!(state.pmd_running());
    assert!(state.spotbugs_running());
}
