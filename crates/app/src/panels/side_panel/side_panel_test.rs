//! Unit tests for [`super`](side_panel.rs), extracted verbatim from that
//! file's colocated `#[cfg(test)] mod tests` so the module file stays focused
//! on the code under test. Behavior-identical to the inline module it replaced.

use super::*;

#[test]
fn create_file_with_parents_creates_missing_nested_directories() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controllers/api/UserController.java");

    create_file_with_parents(&path, "class UserController {}").unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "class UserController {}");
}

#[test]
fn create_file_with_parents_works_for_a_flat_path_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Main.java");

    create_file_with_parents(&path, "class Main {}").unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "class Main {}");
}

#[test]
fn create_file_with_parents_leaves_already_existing_directories_alone() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("existing")).unwrap();
    let path = dir.path().join("existing/File.java");

    create_file_with_parents(&path, "class File {}").unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "class File {}");
}

#[test]
fn has_unsafe_path_component_accepts_a_plain_name() {
    assert!(!has_unsafe_path_component("UserController.java"));
}

#[test]
fn has_unsafe_path_component_accepts_a_nested_relative_path() {
    assert!(!has_unsafe_path_component("controllers/UserController.java"));
}

#[test]
fn has_unsafe_path_component_rejects_parent_dir_traversal() {
    assert!(has_unsafe_path_component("../../etc/passwd"));
    assert!(has_unsafe_path_component("controllers/../../outside.txt"));
}

#[test]
fn has_unsafe_path_component_rejects_an_absolute_path() {
    assert!(has_unsafe_path_component("/etc/passwd"));
}

#[test]
fn apply_tree_actions_rejects_a_traversal_rename_and_leaves_the_file_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("File.java");
    std::fs::write(&old_path, "class File {}").unwrap();

    let mut panel = SidePanelState {
        rename_draft: Some((old_path.clone(), "File.java".to_string())),
        ..Default::default()
    };
    let mut outcome = SidePanelOutcome::default();
    let actions = TreeActions {
        confirm_rename: Some("../outside.txt".to_string()),
        ..Default::default()
    };

    apply_tree_actions(&mut panel, actions, &mut outcome);

    assert!(outcome.error.is_some(), "a traversal rename must surface an error");
    assert!(old_path.exists(), "the original file must be untouched");
    assert!(!dir.path().join("../outside.txt").exists());
}

#[test]
fn apply_tree_actions_rejects_a_rename_containing_a_path_separator_even_without_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("File.java");
    std::fs::write(&old_path, "class File {}").unwrap();

    let mut panel = SidePanelState {
        rename_draft: Some((old_path.clone(), "File.java".to_string())),
        ..Default::default()
    };
    let mut outcome = SidePanelOutcome::default();
    // No `..`, but still names a different directory entirely — rename is
    // "give this exact node a new name," not "move it."
    let actions = TreeActions {
        confirm_rename: Some("sub/File.java".to_string()),
        ..Default::default()
    };

    apply_tree_actions(&mut panel, actions, &mut outcome);

    assert!(outcome.error.is_some());
    assert!(old_path.exists());
}

#[test]
fn apply_tree_actions_still_allows_an_ordinary_rename() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("File.java");
    std::fs::write(&old_path, "class File {}").unwrap();

    let mut panel = SidePanelState {
        rename_draft: Some((old_path.clone(), "File.java".to_string())),
        ..Default::default()
    };
    let mut outcome = SidePanelOutcome::default();
    let actions = TreeActions {
        confirm_rename: Some("Renamed.java".to_string()),
        ..Default::default()
    };

    apply_tree_actions(&mut panel, actions, &mut outcome);

    assert!(outcome.error.is_none());
    assert!(!old_path.exists());
    assert!(dir.path().join("Renamed.java").exists());
}

#[test]
fn delete_path_removes_a_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("File.java");
    std::fs::write(&path, "class File {}").unwrap();

    delete_path(&path).unwrap();

    assert!(!path.exists());
}

#[test]
fn delete_path_removes_a_directory_and_everything_inside_it() {
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("pkg");
    std::fs::create_dir_all(subdir.join("nested")).unwrap();
    std::fs::write(subdir.join("A.java"), "class A {}").unwrap();
    std::fs::write(subdir.join("nested/B.java"), "class B {}").unwrap();

    delete_path(&subdir).unwrap();

    assert!(!subdir.exists());
}

#[test]
fn is_invalid_paste_target_rejects_pasting_a_directory_into_itself() {
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("pkg");
    std::fs::create_dir_all(&subdir).unwrap();

    assert!(is_invalid_paste_target(&subdir, &subdir));
}

#[test]
fn is_invalid_paste_target_rejects_pasting_into_a_descendant() {
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("pkg");
    let nested = subdir.join("nested");
    std::fs::create_dir_all(&nested).unwrap();

    assert!(is_invalid_paste_target(&subdir, &nested));
}

#[test]
fn is_invalid_paste_target_allows_an_unrelated_directory() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("pkg");
    let other = dir.path().join("other");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&other).unwrap();

    assert!(!is_invalid_paste_target(&source, &other));
}

#[test]
fn copy_recursive_copies_a_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("A.java");
    let dest = dir.path().join("B.java");
    std::fs::write(&source, "class A {}").unwrap();

    copy_recursive(&source, &dest).unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "class A {}");
    assert!(source.exists(), "copy must leave the original in place");
}

#[test]
fn copy_recursive_copies_a_directory_and_its_contents() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("pkg");
    std::fs::create_dir_all(source.join("nested")).unwrap();
    std::fs::write(source.join("A.java"), "class A {}").unwrap();
    std::fs::write(source.join("nested/B.java"), "class B {}").unwrap();
    let dest = dir.path().join("pkg_copy");

    copy_recursive(&source, &dest).unwrap();

    assert_eq!(std::fs::read_to_string(dest.join("A.java")).unwrap(), "class A {}");
    assert_eq!(
        std::fs::read_to_string(dest.join("nested/B.java")).unwrap(),
        "class B {}"
    );
    assert!(source.exists(), "copy must leave the original directory in place");
}

#[test]
fn paste_into_with_copy_leaves_the_source_and_creates_the_destination() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("A.java");
    let target_dir = dir.path().join("dest");
    std::fs::write(&source, "class A {}").unwrap();
    std::fs::create_dir_all(&target_dir).unwrap();

    let result = paste_into(&source, &target_dir, ClipboardOp::Copy).unwrap();

    assert_eq!(result, target_dir.join("A.java"));
    assert_eq!(std::fs::read_to_string(&result).unwrap(), "class A {}");
    assert!(source.exists());
}

#[test]
fn paste_into_with_cut_moves_the_source_to_the_destination() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("A.java");
    let target_dir = dir.path().join("dest");
    std::fs::write(&source, "class A {}").unwrap();
    std::fs::create_dir_all(&target_dir).unwrap();

    let result = paste_into(&source, &target_dir, ClipboardOp::Cut).unwrap();

    assert_eq!(result, target_dir.join("A.java"));
    assert_eq!(std::fs::read_to_string(&result).unwrap(), "class A {}");
    assert!(!source.exists(), "cut must remove the original");
}

#[test]
fn paste_into_fails_on_a_name_collision_at_the_destination() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("A.java");
    let target_dir = dir.path().join("dest");
    std::fs::write(&source, "class A {}").unwrap();
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(target_dir.join("A.java"), "already here").unwrap();

    let err = paste_into(&source, &target_dir, ClipboardOp::Copy).unwrap_err();

    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    // Neither side should have been touched by the failed attempt.
    assert_eq!(
        std::fs::read_to_string(target_dir.join("A.java")).unwrap(),
        "already here"
    );
    assert!(source.exists());
}

fn command_click() -> egui::Modifiers {
    egui::Modifiers {
        command: true,
        ..Default::default()
    }
}

fn shift_click() -> egui::Modifiers {
    egui::Modifiers {
        shift: true,
        ..Default::default()
    }
}

fn visible_order() -> Vec<PathBuf> {
    ["a", "b", "c", "d", "e"].iter().map(PathBuf::from).collect()
}

#[test]
fn a_plain_click_collapses_the_selection_to_just_the_clicked_node() {
    let mut panel = SidePanelState {
        selected: [PathBuf::from("a"), PathBuf::from("b")].into_iter().collect(),
        ..Default::default()
    };

    apply_selection_click(&mut panel, Path::new("c"), egui::Modifiers::NONE, &visible_order());

    assert_eq!(panel.selected, [PathBuf::from("c")].into_iter().collect());
    assert_eq!(panel.last_selected, Some(PathBuf::from("c")));
}

#[test]
fn command_click_adds_a_node_to_the_selection() {
    let mut panel = SidePanelState {
        selected: [PathBuf::from("a")].into_iter().collect(),
        last_selected: Some(PathBuf::from("a")),
        ..Default::default()
    };

    apply_selection_click(&mut panel, Path::new("c"), command_click(), &visible_order());

    assert_eq!(
        panel.selected,
        [PathBuf::from("a"), PathBuf::from("c")].into_iter().collect()
    );
    assert_eq!(panel.last_selected, Some(PathBuf::from("c")));
}

#[test]
fn command_click_on_an_already_selected_node_removes_it() {
    let mut panel = SidePanelState {
        selected: [PathBuf::from("a"), PathBuf::from("c")].into_iter().collect(),
        last_selected: Some(PathBuf::from("c")),
        ..Default::default()
    };

    apply_selection_click(&mut panel, Path::new("c"), command_click(), &visible_order());

    assert_eq!(panel.selected, [PathBuf::from("a")].into_iter().collect());
}

#[test]
fn shift_click_selects_the_contiguous_range_from_the_anchor_forward() {
    let mut panel = SidePanelState {
        last_selected: Some(PathBuf::from("b")),
        ..Default::default()
    };

    apply_selection_click(&mut panel, Path::new("d"), shift_click(), &visible_order());

    assert_eq!(
        panel.selected,
        [PathBuf::from("b"), PathBuf::from("c"), PathBuf::from("d")]
            .into_iter()
            .collect()
    );
    // The anchor itself doesn't move on a Shift+Click.
    assert_eq!(panel.last_selected, Some(PathBuf::from("b")));
}

#[test]
fn shift_click_selects_the_contiguous_range_from_the_anchor_backward() {
    let mut panel = SidePanelState {
        last_selected: Some(PathBuf::from("d")),
        ..Default::default()
    };

    apply_selection_click(&mut panel, Path::new("b"), shift_click(), &visible_order());

    assert_eq!(
        panel.selected,
        [PathBuf::from("b"), PathBuf::from("c"), PathBuf::from("d")]
            .into_iter()
            .collect()
    );
    assert_eq!(panel.last_selected, Some(PathBuf::from("d")));
}

#[test]
fn repeated_shift_clicks_recompute_from_the_same_anchor_rather_than_drifting() {
    let mut panel = SidePanelState {
        last_selected: Some(PathBuf::from("b")),
        ..Default::default()
    };

    apply_selection_click(&mut panel, Path::new("e"), shift_click(), &visible_order());
    assert_eq!(panel.last_selected, Some(PathBuf::from("b")));

    apply_selection_click(&mut panel, Path::new("c"), shift_click(), &visible_order());

    assert_eq!(
        panel.selected,
        [PathBuf::from("b"), PathBuf::from("c")].into_iter().collect()
    );
    assert_eq!(panel.last_selected, Some(PathBuf::from("b")));
}

#[test]
fn shift_click_with_no_prior_anchor_falls_back_to_a_plain_click() {
    let mut panel = SidePanelState::default();

    apply_selection_click(&mut panel, Path::new("c"), shift_click(), &visible_order());

    assert_eq!(panel.selected, [PathBuf::from("c")].into_iter().collect());
    assert_eq!(panel.last_selected, Some(PathBuf::from("c")));
}

#[test]
fn action_targets_is_just_the_clicked_path_with_no_multi_selection() {
    let panel = SidePanelState::default();

    assert_eq!(action_targets(&panel, Path::new("a")), vec![PathBuf::from("a")]);
}

#[test]
fn action_targets_is_just_the_clicked_path_when_only_it_is_selected() {
    let panel = SidePanelState {
        selected: [PathBuf::from("a")].into_iter().collect(),
        ..Default::default()
    };

    assert_eq!(action_targets(&panel, Path::new("a")), vec![PathBuf::from("a")]);
}

#[test]
fn action_targets_is_the_whole_selection_once_more_than_one_node_is_selected() {
    let panel = SidePanelState {
        selected: [PathBuf::from("a"), PathBuf::from("b")].into_iter().collect(),
        ..Default::default()
    };

    let mut targets = action_targets(&panel, Path::new("a"));
    targets.sort();

    assert_eq!(targets, vec![PathBuf::from("a"), PathBuf::from("b")]);
}

#[test]
fn action_targets_uses_the_whole_selection_even_if_a_different_node_was_right_clicked() {
    // Matches `SPEC.md` §1's plain wording ("Delete... over a set"): the
    // right-clicked node needn't itself be one of the selected ones.
    let panel = SidePanelState {
        selected: [PathBuf::from("a"), PathBuf::from("b")].into_iter().collect(),
        ..Default::default()
    };

    let mut targets = action_targets(&panel, Path::new("c"));
    targets.sort();

    assert_eq!(targets, vec![PathBuf::from("a"), PathBuf::from("b")]);
}

fn dir(name: &str, children: Vec<FileNode>) -> FileNode {
    FileNode {
        path: PathBuf::from(name),
        name: name.to_string(),
        kind: FileKind::Dir,
        children,
    }
}

fn file(name: &str) -> FileNode {
    FileNode {
        path: PathBuf::from(name),
        name: name.to_string(),
        kind: FileKind::File,
        children: Vec::new(),
    }
}

#[test]
fn collapse_chain_with_no_children_returns_just_the_start_node() {
    let node = dir("empty", vec![]);
    let collapsed = collapse_chain(&node, "/");
    assert_eq!(collapsed.label, "empty");
    assert_eq!(collapsed.terminal.name, "empty");
}

#[test]
fn collapse_chain_stops_immediately_when_there_are_multiple_children() {
    let node = dir("backend", vec![dir("app", vec![]), dir("async", vec![])]);
    let collapsed = collapse_chain(&node, "/");
    assert_eq!(collapsed.label, "backend");
    assert_eq!(collapsed.terminal.path, node.path);
}

#[test]
fn collapse_chain_joins_a_run_of_single_child_directories() {
    let node = dir(
        "br",
        vec![dir(
            "ufsc",
            vec![dir(
                "bridge",
                vec![dir(
                    "pec",
                    vec![dir("backend", vec![dir("app", vec![]), dir("async", vec![])])],
                )],
            )],
        )],
    );
    let collapsed = collapse_chain(&node, "/");
    assert_eq!(collapsed.label, "br/ufsc/bridge/pec/backend");
    assert_eq!(collapsed.terminal.name, "backend");
}

#[test]
fn collapse_chain_stops_before_a_single_child_that_is_a_file() {
    let node = dir("only", vec![file("Main.java")]);
    let collapsed = collapse_chain(&node, "/");
    assert_eq!(collapsed.label, "only");
    assert_eq!(collapsed.terminal.name, "only");
}

#[test]
fn collapse_chain_never_folds_a_source_root_into_its_parent_chain() {
    // "main" has exactly one child ("java"), which would otherwise extend
    // the chain — but since that one child is a recognized source root, the
    // chain must stop at "main" instead of swallowing "java" into it.
    let node = dir("src", vec![dir("main", vec![dir("java", vec![dir("br", vec![])])])]);
    let collapsed = collapse_chain(&node, "/");
    assert_eq!(collapsed.label, "src/main");
    assert_eq!(collapsed.terminal.name, "main");
}

#[test]
fn collapse_chain_never_folds_a_source_root_itself_even_with_one_child() {
    let node = dir("java", vec![dir("br", vec![dir("ufsc", vec![])])]);
    let collapsed = collapse_chain(&node, ".");
    assert_eq!(collapsed.label, "java");
    assert_eq!(collapsed.terminal.name, "java");
}

#[test]
fn collapse_chain_uses_the_given_separator() {
    let node = dir("br", vec![dir("ufsc", vec![dir("x", vec![]), dir("y", vec![])])]);
    assert_eq!(collapse_chain(&node, ".").label, "br.ufsc");
    assert_eq!(collapse_chain(&node, "/").label, "br/ufsc");
}
