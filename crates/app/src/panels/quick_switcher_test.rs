
use super::*;
use fg_core::Document;

fn doc_at(dir: &tempfile::TempDir, name: &str) -> Document {
    let path = dir.path().join(name);
    std::fs::write(&path, "").unwrap();
    Document::open(path).unwrap()
}

#[test]
fn recent_files_lists_open_tabs_before_closed_ones() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = EditorState::new();
    state.open_tabs.push(doc_at(&dir, "Open.java"));
    state.closed_tabs.push(doc_at(&dir, "Closed.java"));

    let files = recent_files(&state);

    assert_eq!(
        files,
        vec![dir.path().join("Open.java"), dir.path().join("Closed.java")]
    );
}

#[test]
fn recent_files_lists_closed_tabs_most_recently_closed_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = EditorState::new();
    state.closed_tabs.push(doc_at(&dir, "First.java"));
    state.closed_tabs.push(doc_at(&dir, "Second.java"));

    let files = recent_files(&state);

    assert_eq!(
        files,
        vec![dir.path().join("Second.java"), dir.path().join("First.java")]
    );
}

#[test]
fn recent_files_does_not_duplicate_a_path_that_is_both_open_and_in_closed_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = EditorState::new();
    state.open_tabs.push(doc_at(&dir, "Both.java"));
    state.closed_tabs.push(doc_at(&dir, "Both.java"));

    let files = recent_files(&state);

    assert_eq!(files, vec![dir.path().join("Both.java")]);
}

#[test]
fn matches_query_is_case_insensitive_on_the_file_name() {
    let path = Path::new("/project/src/HelloWorld.java");
    assert!(matches_query(path, "hello"));
    assert!(matches_query(path, "WORLD"));
    assert!(!matches_query(path, "goodbye"));
}

#[test]
fn matches_query_empty_matches_everything() {
    assert!(matches_query(Path::new("/a/b.java"), ""));
}

#[test]
fn toggle_opens_and_resets_query_and_selection() {
    let mut switcher = QuickSwitcherState {
        open: false,
        query: "leftover".to_string(),
        selected: 3,
    };

    switcher.toggle();

    assert!(switcher.open);
    assert!(switcher.query.is_empty());
    assert_eq!(switcher.selected, 0);
}

#[test]
fn toggle_twice_closes_it_again() {
    let mut switcher = QuickSwitcherState::default();
    switcher.toggle();
    switcher.toggle();
    assert!(!switcher.open);
}

/// Ctrl+E is a "go back to what I was just in" gesture: the file
/// already on screen must not be the first thing offered, or the
/// gesture does nothing.
#[test]
fn the_active_tab_is_offered_last_not_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = EditorState::new();
    state.open_tabs.push(doc_at(&dir, "First.java"));
    state.open_tabs.push(doc_at(&dir, "Second.java"));
    state.active_tab = Some(1);

    let files = recent_files(&state);

    assert_eq!(files.first().unwrap().file_name().unwrap(), "First.java");
    assert_eq!(files.last().unwrap().file_name().unwrap(), "Second.java");
}

#[test]
fn a_candidate_carries_its_directory_inside_the_project() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("service/src")).unwrap();
    let mut state = EditorState::new();
    state.open_project(dir.path().to_path_buf()).unwrap();

    let nested = dir.path().join("service/src/App.java");
    assert_eq!(location_of(&state, &nested), "service/src");
    // A file at the root has no location worth repeating.
    assert_eq!(location_of(&state, &dir.path().join("App.java")), "");
}
