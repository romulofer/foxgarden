
use fg_core::CoverageStatus;

use super::*;

fn line(n: usize, status: CoverageStatus) -> LineCoverage {
    LineCoverage { line: n, status }
}

#[test]
fn apply_coverage_results_sets_matching_docs_and_clears_the_rest() {
    let (_dir_a, doc_a) = test_support::temp_document("A.java", "class A {}");
    let (_dir_b, doc_b) = test_support::temp_document("B.java", "class B {}");
    let path_a = doc_a.path.clone();
    let mut state = EditorState {
        open_tabs: vec![doc_a, doc_b],
        ..Default::default()
    };

    let results = vec![(path_a, vec![line(0, CoverageStatus::Covered)])];
    apply_coverage_results(&mut state, &results);

    assert_eq!(state.open_tabs[0].coverage_lines.len(), 1);
    assert_eq!(state.open_tabs[0].coverage_lines[0].status, CoverageStatus::Covered);
    assert!(state.open_tabs[1].coverage_lines.is_empty());
}

#[test]
fn apply_coverage_results_replaces_rather_than_accumulates() {
    let (_dir, mut doc) = test_support::temp_document("A.java", "class A {}");
    doc.coverage_lines = vec![line(9, CoverageStatus::Missed)];
    let path = doc.path.clone();
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };

    apply_coverage_results(&mut state, &[(path, vec![line(0, CoverageStatus::Covered)])]);

    assert_eq!(state.open_tabs[0].coverage_lines.len(), 1);
    assert_eq!(state.open_tabs[0].coverage_lines[0].line, 0);
}

#[test]
fn a_run_with_no_results_for_a_doc_clears_its_stale_marks() {
    let (_dir, mut doc) = test_support::temp_document("A.java", "class A {}");
    doc.coverage_lines = vec![line(0, CoverageStatus::Covered)];
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };

    apply_coverage_results(&mut state, &[]);

    assert!(state.open_tabs[0].coverage_lines.is_empty());
}

/// A launched program that closes its output but keeps running: once both
/// readers finish, the waiter must not sit on the child lock, or Stop (and
/// the per-frame `run_pid`) block the UI thread until the program exits.
#[cfg(unix)]
#[test]
fn stop_is_immediate_for_a_program_that_closed_its_output_but_keeps_running() {
    let mut state = BuildState::default();
    let mut command = std::process::Command::new("sh");
    command.args(["-c", "exec >&- 2>&-; sleep 30"]);
    state.spawn_process(command).expect("sh is always available");
    state.stage = Some(Stage::RunLaunched);
    // Long enough for both readers to hit EOF and the waiter to start.
    std::thread::sleep(std::time::Duration::from_millis(300));

    let started = std::time::Instant::now();
    assert!(state.run_pid().is_some());
    state.stop();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "stop blocked for {:?}",
        started.elapsed()
    );
}
