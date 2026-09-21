
use super::*;

#[test]
fn poll_returns_early_with_nothing_running() {
    let mut diff = DiffState::default();
    let mut state = EditorState::default();
    diff.poll(&mut state); // must not panic with no scans in flight
}

#[test]
fn run_then_poll_applies_a_completed_scan_to_the_matching_open_tab() {
    let (_dir, doc) = test_support::temp_document("A.java", "class A {}");
    let path = doc.path.clone();
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };

    let mut diff = DiffState::default();
    // A nonexistent root still exercises the real spawn-and-send path —
    // `git_diff_hunks`/`git_blame` both degrade to an empty result
    // rather than an error, so this exercises "a completed scan with
    // zero hunks/blame lines is still applied," not just the happy
    // path.
    diff.run(path.clone(), PathBuf::from("/nonexistent/root"));

    loop {
        diff.poll(&mut state);
        if !diff.scans.contains_key(&path) {
            break;
        }
    }
    assert!(state.open_tabs[0].diff_hunks.is_empty());
    assert!(state.open_tabs[0].blame.is_empty());
}

#[test]
fn run_then_poll_applies_real_blame_lines_to_the_matching_open_tab() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let file = root.join("A.java");
    std::fs::write(&file, "class A {}\n").unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(&root)
            .args(args)
            .output()
            .unwrap()
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "a@b.com"]);
    run(&["config", "user.name", "test"]);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);

    let doc = fg_core::Document::open(file.clone()).unwrap();
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };

    let mut diff = DiffState::default();
    diff.run(file.clone(), root);
    loop {
        diff.poll(&mut state);
        if !diff.scans.contains_key(&file) {
            break;
        }
    }
    assert_eq!(state.open_tabs[0].blame.len(), 1);
    assert_eq!(state.open_tabs[0].blame[0].summary, "init");
}

#[test]
fn poll_drops_a_result_for_a_path_thats_no_longer_open() {
    let mut diff = DiffState::default();
    diff.run(PathBuf::from("/tmp/gone.java"), PathBuf::from("."));
    let mut state = EditorState::default(); // nothing open

    loop {
        diff.poll(&mut state);
        if diff.scans.is_empty() {
            break;
        }
    }
    assert!(
        state.open_tabs.is_empty(),
        "must not panic or fabricate a tab for the dropped result"
    );
}

#[test]
fn check_for_saves_does_not_trigger_on_a_tabs_very_first_sighting() {
    let (_dir, doc) = test_support::temp_document("A.java", "class A {}");
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };
    let mut diff = DiffState::default();

    diff.check_for_saves(&state, Some(Path::new(".")));
    assert!(
        diff.scans.is_empty(),
        "a freshly-observed clean tab is not a save transition"
    );

    state.open_tabs[0].buffer.insert(0, "// x\n");
    diff.check_for_saves(&state, Some(Path::new(".")));
    assert!(diff.scans.is_empty(), "going dirty is not a save transition either");
}

#[test]
fn check_for_saves_triggers_on_a_dirty_to_clean_transition() {
    let (_dir, mut doc) = test_support::temp_document("A.java", "class A {}");
    doc.buffer.insert(0, "// x\n");
    assert!(doc.is_dirty());
    let path = doc.path.clone();
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };
    let mut diff = DiffState::default();

    diff.check_for_saves(&state, Some(Path::new("."))); // observes it already dirty

    let saved = state.open_tabs[0].saved_buffer.clone();
    state.open_tabs[0].buffer.replace(saved); // simulates a save clearing dirty
    diff.check_for_saves(&state, Some(Path::new(".")));

    assert!(
        diff.scans.contains_key(&path),
        "the dirty -> clean transition must kick off a scan"
    );
}

#[test]
fn check_for_saves_is_a_no_op_with_no_project_root() {
    let (_dir, mut doc) = test_support::temp_document("A.java", "class A {}");
    doc.buffer.insert(0, "// x\n");
    let mut state = EditorState {
        open_tabs: vec![doc],
        ..Default::default()
    };
    let mut diff = DiffState::default();

    diff.check_for_saves(&state, None);
    let saved = state.open_tabs[0].saved_buffer.clone();
    state.open_tabs[0].buffer.replace(saved);
    diff.check_for_saves(&state, None);

    assert!(diff.scans.is_empty(), "no project root means nothing to diff against");
}
