
use super::*;

fn entry(path: &str, index: char, worktree: char) -> StatusEntry {
    StatusEntry {
        path: PathBuf::from(path),
        index_status: index,
        worktree_status: worktree,
    }
}

#[test]
fn poll_status_applies_a_successful_result_to_entries() {
    let mut state = GitStageState {
        status_rx: Some(spawn(|| Ok(vec![entry("f.txt", '?', '?')]))),
        ..GitStageState::default()
    };
    let result = loop {
        if let Some(result) = state.poll_status() {
            break result;
        }
    };
    assert!(result.is_ok());
    assert_eq!(&state.entries, &[entry("f.txt", '?', '?')]);
}

#[test]
fn poll_status_returns_the_error_without_touching_entries() {
    let mut state = GitStageState {
        entries: vec![entry("stale.txt", '?', '?')],
        status_rx: Some(spawn(|| Err("boom".to_string()))),
        ..GitStageState::default()
    };
    let result = loop {
        if let Some(result) = state.poll_status() {
            break result;
        }
    };
    assert_eq!(result, Err("boom".to_string()));
    assert_eq!(&state.entries, &[entry("stale.txt", '?', '?')]);
}

#[test]
fn commit_marks_running_and_clears_the_message_only_on_success() {
    let mut state = GitStageState {
        commit_message: "wip".to_string(),
        op_rx: Some(spawn(|| Ok(()))),
        committing: true,
        ..GitStageState::default()
    };
    assert!(state.op_running());

    let result = loop {
        if let Some(result) = state.poll_op() {
            break result;
        }
    };
    assert!(result.is_ok());
    assert!(!state.op_running());
    assert_eq!(state.commit_message, "");
}

#[test]
fn a_failed_commit_leaves_the_message_untouched() {
    let mut state = GitStageState {
        commit_message: "wip".to_string(),
        op_rx: Some(spawn(|| Err("nothing to commit".to_string()))),
        committing: true,
        ..GitStageState::default()
    };

    let result = loop {
        if let Some(result) = state.poll_op() {
            break result;
        }
    };
    assert!(result.is_err());
    assert_eq!(state.commit_message, "wip");
}

#[test]
fn stage_and_unstage_are_not_marked_as_committing() {
    let mut state = GitStageState {
        commit_message: "keep me".to_string(),
        op_rx: Some(spawn(|| Ok(()))),
        ..GitStageState::default()
    };
    // Not going through `stage`/`unstage` themselves (no real repo
    // needed for this assertion) — `committing` simply starts `false`
    // and only `commit` ever sets it, so a stage/unstage completing
    // must never clear the message.
    assert!(!state.committing);

    let result = loop {
        if let Some(result) = state.poll_op() {
            break result;
        }
    };
    assert!(result.is_ok());
    assert_eq!(state.commit_message, "keep me");
}

fn file_diff(headers: &[&str]) -> FileDiff {
    FileDiff {
        preamble: "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n".to_string(),
        hunks: headers
            .iter()
            .map(|h| fg_core::RawHunk {
                header: h.to_string(),
                lines: vec![" ctx".to_string()],
            })
            .collect(),
    }
}

#[test]
fn poll_expanded_applies_a_successful_result() {
    let mut state = GitStageState {
        expanded_rx: Some(spawn(|| Ok((file_diff(&["@@ -1 +1 @@"]), file_diff(&[]))))),
        ..GitStageState::default()
    };
    let result = loop {
        if let Some(result) = state.poll_expanded() {
            break result;
        }
    };
    assert!(result.is_ok());
    assert_eq!(state.expanded_diffs.unwrap().0.hunks.len(), 1);
}

#[test]
fn toggle_expand_on_the_same_path_twice_collapses_it() {
    let mut state = GitStageState::default();
    let path = PathBuf::from("f.txt");
    state.toggle_expand(PathBuf::from("/nonexistent"), path.clone());
    assert_eq!(state.expanded, Some(path.clone()));

    state.toggle_expand(PathBuf::from("/nonexistent"), path);
    assert_eq!(state.expanded, None);
    assert!(state.expanded_rx.is_none());
    assert!(state.expanded_diffs.is_none());
}

#[test]
fn stage_hunk_and_unstage_hunk_are_no_ops_with_nothing_expanded() {
    let mut state = GitStageState::default();
    state.stage_hunk(PathBuf::from("/nonexistent"), 0);
    assert!(!state.op_running(), "no expanded_diffs means no patch to apply");

    state.unstage_hunk(PathBuf::from("/nonexistent"), 0);
    assert!(!state.op_running());
}

#[test]
fn refresh_expanded_is_a_no_op_with_nothing_expanded() {
    let mut state = GitStageState::default();
    state.refresh_expanded(PathBuf::from("/nonexistent"));
    assert!(state.expanded_rx.is_none());
}

/// End-to-end: expanding a real two-hunk file, staging just its second
/// hunk, and confirming the *real* `git diff --cached` afterward shows
/// exactly that one hunk — not just that `git_apply_cached` itself
/// works (already covered in `fg_core::status`'s own tests), but that
/// this state's own `expanded_diffs`/`hunk_patch` wiring picks the
/// right hunk out of the right (unstaged) half.
#[test]
fn expand_then_stage_hunk_round_trips_through_a_real_repo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let file = root.join("f.txt");
    let lines: Vec<String> = (1..=20).map(|n| format!("l{n}")).collect();
    std::fs::write(&file, lines.join("\n") + "\n").unwrap();
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

    let mut edited = lines.clone();
    edited[1] = "CHANGED2".to_string();
    edited[17] = "CHANGED18".to_string();
    std::fs::write(&file, edited.join("\n") + "\n").unwrap();

    let mut state = GitStageState::default();
    let path = PathBuf::from("f.txt");
    state.toggle_expand(root.clone(), path);
    loop {
        if let Some(result) = state.poll_expanded() {
            result.expect("diff fetch succeeds");
            break;
        }
    }
    assert_eq!(state.expanded_diffs.as_ref().unwrap().0.hunks.len(), 2);

    state.stage_hunk(root.clone(), 1); // the CHANGED18 hunk
    loop {
        if let Some(result) = state.poll_op() {
            result.expect("apply --cached succeeds");
            break;
        }
    }

    let cached = fg_core::git_file_diff_cached(&file, &root).unwrap();
    assert_eq!(cached.hunks.len(), 1);
    assert!(fg_core::hunk_patch(&cached, 0).unwrap().contains("+CHANGED18"));
}
