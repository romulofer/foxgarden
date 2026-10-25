
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

/// A multiplatform test task that fails one test: when the process ends the
/// extension's reports are summarized, and the failing row jumps to the test.
#[cfg(unix)]
#[test]
fn a_test_task_ends_with_the_summary_of_the_reports_it_left() {
    use std::os::unix::fs::PermissionsExt;

    let dir = test_support::tempdir();
    let root = dir.path().to_path_buf();
    test_support::write_file(&root, "build.gradle.kts", "plugins { kotlin(\"multiplatform\") }\nkotlin { jvm() }\n");
    let source = test_support::write_file(&root, "src/jvmTest/kotlin/app/GreeterTest.kt", "class GreeterTest\n");
    let report = "<testsuite><testcase name=\"ok\" classname=\"app.GreeterTest\"/>\
        <testcase name=\"bad\" classname=\"app.GreeterTest\"><failure message=\"boom\">at app.GreeterTest.bad(GreeterTest.kt:7)</failure></testcase></testsuite>";
    // The stand-in wrapper writes the report the way Gradle would.
    let wrapper = test_support::write_file(
        &root,
        "gradlew",
        &format!(
            "#!/bin/sh\nmkdir -p build/test-results/jvmTest\ncat > build/test-results/jvmTest/TEST-app.GreeterTest.xml <<'EOF'\n{report}\nEOF\nexit 1\n"
        ),
    );
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();

    let registry = test_support::languages();
    let tool = registry.detect_build_tool(&root).expect("a gradle build");
    let task = registry
        .project_tasks(&root)
        .into_iter()
        .find(|t| t.task.label == "Gradle: jvmTest")
        .expect("the jvm target's test task");

    let mut state = BuildState::default();
    state.start_task(&tool, &task).unwrap();
    while state.running() {
        state.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let texts: Vec<&str> = state.rows.iter().map(|r| r.text.as_str()).collect();
    assert!(texts.contains(&"Tests: 2 total, 1 passed, 1 failed, 0 errored, 0 skipped"), "{texts:?}");
    let failed = state.rows.iter().find(|r| r.text == "FAILED  app.GreeterTest.bad").expect("the failing test is listed");
    let problem = failed.problem.as_ref().expect("and can be jumped to");
    assert_eq!((problem.path.as_path(), problem.line), (source.as_path(), 7));
    assert_eq!(state.last_success, Some(false));
}
