use super::*;

#[test]
fn reopening_same_file_focuses_existing_tab_without_duplicating() {
    let dir = tempfile::tempdir().unwrap();
    let path = test_support::placeholder_java_file(dir.path(), "A.java");
    let mut state = EditorState::new();

    let first_index = state.open_tab(path.clone()).unwrap();
    let second_index = state.open_tab(path).unwrap();

    assert_eq!(first_index, second_index);
    assert_eq!(state.open_tabs.len(), 1);
    assert_eq!(state.active_tab, Some(first_index));
}

#[test]
fn opening_a_tab_within_an_open_project_records_its_root() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "A.java");
    let mut state = EditorState::new();

    state.open_project(dir.path().to_path_buf()).unwrap();
    let index = state.open_tab(a).unwrap();

    assert_eq!(state.open_tabs[index].project_root, Some(dir.path().to_path_buf()));
}

#[test]
fn opening_a_tab_with_no_project_open_records_no_root() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "A.java");
    let mut state = EditorState::new();

    let index = state.open_tab(a).unwrap();

    assert_eq!(state.open_tabs[index].project_root, None);
}

#[test]
fn closing_active_tab_selects_sensible_neighbor() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "A.java");
    let b = test_support::placeholder_java_file(dir.path(), "B.java");
    let c = test_support::placeholder_java_file(dir.path(), "C.java");
    let mut state = EditorState::new();

    state.open_tab(a).unwrap();
    state.open_tab(b).unwrap();
    state.open_tab(c).unwrap();
    // active_tab is now Some(2) (C.java)

    state.focus_tab(1); // focus B.java
    state.close_tab(1); // close B.java: next tab (C.java, now at index 1) becomes active
    assert_eq!(state.active_tab, Some(1));
    assert_eq!(state.open_tabs[1].path().file_name().unwrap(), "C.java");

    state.close_tab(1); // close C.java, the only remaining tab is A.java at index 0
    assert_eq!(state.active_tab, Some(0));

    state.close_tab(0); // close last tab
    assert_eq!(state.active_tab, None);
    assert!(state.open_tabs.is_empty());
}

#[test]
fn reopen_last_closed_tab_restores_it_and_focuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "A.java");
    let b = test_support::placeholder_java_file(dir.path(), "B.java");
    let mut state = EditorState::new();

    state.open_tab(a).unwrap();
    state.open_tab(b).unwrap();
    state.close_tab(1); // close B.java
    assert_eq!(state.open_tabs.len(), 1);

    let index = state.reopen_last_closed_tab().unwrap();
    assert_eq!(state.open_tabs.len(), 2);
    assert_eq!(state.active_tab, Some(index));
    assert_eq!(state.open_tabs[index].path().file_name().unwrap(), "B.java");
}

#[test]
fn reopen_last_closed_tab_pops_in_lifo_order() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "A.java");
    let b = test_support::placeholder_java_file(dir.path(), "B.java");
    let mut state = EditorState::new();

    state.open_tab(a).unwrap();
    state.open_tab(b).unwrap();
    state.close_tab(0); // close A.java
    state.close_tab(0); // close B.java (now the only remaining tab)

    let first = state.reopen_last_closed_tab().unwrap();
    assert_eq!(state.open_tabs[first].path().file_name().unwrap(), "B.java");

    let second = state.reopen_last_closed_tab().unwrap();
    assert_eq!(state.open_tabs[second].path().file_name().unwrap(), "A.java");

    assert!(state.reopen_last_closed_tab().is_none());
}

#[test]
fn reopen_last_closed_tab_with_nothing_closed_is_a_no_op() {
    let mut state = EditorState::new();
    assert!(state.reopen_last_closed_tab().is_none());
}

#[test]
fn reopening_a_tab_already_open_focuses_it_instead_of_duplicating() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "A.java");
    let mut state = EditorState::new();

    state.open_tab(a.clone()).unwrap();
    state.close_tab(0);
    // Reopened manually (e.g. via the side panel) before Ctrl+Shift+T.
    let manual_index = state.open_tab(a).unwrap();

    let index = state.reopen_last_closed_tab().unwrap();
    assert_eq!(index, manual_index);
    assert_eq!(state.open_tabs.len(), 1);
}

#[test]
fn closed_tabs_is_capped_and_drops_the_oldest() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = EditorState::new();

    let paths: Vec<PathBuf> = (0..=MAX_CLOSED_TABS)
        .map(|i| test_support::placeholder_java_file(dir.path(), &format!("F{i}.java")))
        .collect();
    for path in &paths {
        state.open_tab(path.clone()).unwrap();
    }
    // Close them oldest-first (F0.java, F1.java, ...) — one more close
    // than the cap allows.
    for _ in 0..paths.len() {
        state.close_tab(0);
    }

    assert_eq!(state.closed_tabs.len(), MAX_CLOSED_TABS);
    assert!(
        state
            .closed_tabs
            .iter()
            .all(|doc| doc.path().file_name().unwrap() != "F0.java"),
        "the oldest closed tab should have been evicted to stay under the cap"
    );
    let most_recent = state
        .closed_tabs
        .last()
        .unwrap()
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(most_recent, format!("F{MAX_CLOSED_TABS}.java"));
}

#[test]
fn open_project_clears_closed_tabs_from_the_previous_project() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "A.java");
    let mut state = EditorState::new();

    state.open_tab(a).unwrap();
    state.close_tab(0);
    assert_eq!(state.closed_tabs.len(), 1);

    let other_project_dir = tempfile::tempdir().unwrap();
    state.open_project(other_project_dir.path().to_path_buf()).unwrap();

    assert!(state.closed_tabs.is_empty());
}

#[test]
fn new_terminal_tab_appends_and_focuses_it_independently_of_file_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "A.java");
    let mut state = EditorState::new();

    state.open_tab(a).unwrap();
    let index = state.new_terminal_tab();

    assert_eq!(state.terminal_tabs.len(), 1);
    assert_eq!(state.terminal_tabs[index].title, "Terminal 1");
    assert_eq!(state.active_terminal, Some(0));
    // A terminal session never touches the file-tab fields at all.
    assert_eq!(state.active_tab, Some(0));
    assert_eq!(state.open_tabs.len(), 1);
}

#[test]
fn close_terminal_tab_selects_sensible_neighbor() {
    let mut state = EditorState::new();
    state.new_terminal_tab(); // 0
    state.new_terminal_tab(); // 1
    state.new_terminal_tab(); // 2, active

    state.close_terminal_tab(1); // close the middle one, not the active one
    assert_eq!(state.active_terminal, Some(1), "index 2 shifted down to 1");

    state.close_terminal_tab(1); // now closes the (shifted) active one
    assert_eq!(state.active_terminal, Some(0));

    state.close_terminal_tab(0);
    assert_eq!(state.active_terminal, None);
    assert!(state.terminal_tabs.is_empty());
}

#[test]
fn closing_a_terminal_tab_never_pushes_onto_closed_tabs() {
    let mut state = EditorState::new();
    state.new_terminal_tab();
    state.close_terminal_tab(0);

    assert!(
        state.closed_tabs.is_empty(),
        "a closed terminal session has nothing to reopen, unlike a file tab"
    );
}

fn state_with_tabs(names: &[&str]) -> (tempfile::TempDir, EditorState) {
    let dir = tempfile::tempdir().unwrap();
    let mut state = EditorState::new();
    for name in names {
        let path = test_support::placeholder_java_file(dir.path(), name);
        state.open_tab(path).unwrap();
    }
    (dir, state)
}

#[test]
fn splitting_opens_a_second_pane_on_the_same_tab_and_focuses_it() {
    let (_dir, mut state) = state_with_tabs(&["A.java", "B.java"]);
    state.focus_tab(0); // pane 0 (and the whole app) on A.java

    state.split_editor();

    assert!(state.is_split());
    assert_eq!(state.focused_pane(), 1, "the new right pane takes focus");
    assert_eq!(state.pane_active(0), Some(0), "left pane still shows A.java");
    assert_eq!(state.pane_active(1), Some(0), "right pane opens on the same tab");
    assert_eq!(state.active_tab, Some(0), "active_tab mirrors the focused (right) pane");
}

#[test]
fn each_pane_switches_tabs_independently() {
    let (_dir, mut state) = state_with_tabs(&["A.java", "B.java", "C.java"]);
    state.focus_tab(0);
    state.split_editor(); // right pane focused, both on A.java

    state.focus_tab(2); // right pane -> C.java
    assert_eq!(state.pane_active(1), Some(2));
    assert_eq!(state.pane_active(0), Some(0), "left pane is untouched");

    state.focus_pane(0);
    assert_eq!(
        state.active_tab,
        Some(0),
        "focusing the left pane restores its own active tab"
    );
    state.focus_tab(1); // left pane -> B.java
    assert_eq!(state.pane_active(0), Some(1));
    assert_eq!(state.pane_active(1), Some(2), "right pane is untouched");
}

#[test]
fn closing_a_tab_reindexes_both_panes() {
    let (_dir, mut state) = state_with_tabs(&["A.java", "B.java", "C.java"]);
    state.focus_tab(0);
    state.split_editor();
    state.focus_tab(2); // right pane on C.java (index 2)
    state.focus_pane(0); // focus left pane, on A.java (index 0)

    state.close_tab(1); // remove B.java; C.java shifts 2 -> 1

    assert_eq!(state.pane_active(0), Some(0), "left pane still on A.java");
    assert_eq!(state.pane_active(1), Some(1), "right pane's C.java followed the shift");
    assert_eq!(state.active_tab, Some(0), "focused (left) pane's active is mirrored");
}

#[test]
fn closing_a_panes_own_active_tab_moves_it_to_a_neighbor() {
    let (_dir, mut state) = state_with_tabs(&["A.java", "B.java"]);
    state.focus_tab(0);
    state.split_editor();
    state.focus_tab(1); // right pane on B.java (index 1)

    state.close_tab(1); // close the right pane's own active tab

    assert_eq!(
        state.pane_active(1),
        Some(0),
        "right pane falls back to the remaining tab"
    );
    assert_eq!(state.pane_active(0), Some(0), "left pane still on A.java");
}

#[test]
fn unsplitting_keeps_the_focused_panes_active_tab() {
    let (_dir, mut state) = state_with_tabs(&["A.java", "B.java"]);
    state.focus_tab(0);
    state.split_editor();
    state.focus_tab(1); // right pane on B.java, focused

    state.unsplit();

    assert!(!state.is_split());
    assert_eq!(state.focused_pane(), 0);
    assert_eq!(
        state.active_tab,
        Some(1),
        "the focused pane's B.java survives the collapse"
    );
    assert_eq!(state.pane_active(1), None, "there is no second pane once collapsed");
}

#[test]
fn split_editor_is_a_no_op_when_already_split() {
    let (_dir, mut state) = state_with_tabs(&["A.java", "B.java"]);
    state.focus_tab(0);
    state.split_editor();
    state.focus_tab(1); // right pane -> B.java

    state.split_editor(); // second call must not reset the panes

    assert_eq!(state.pane_active(1), Some(1), "an already-split editor is left as-is");
}
