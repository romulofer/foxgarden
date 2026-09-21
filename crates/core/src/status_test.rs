use super::*;

/// Captured verbatim from a real `git status --porcelain -uall` run
/// (a throwaway repo: one modified-and-then-deleted tracked file, one
/// staged-then-further-modified untracked file, a rename with an
/// uncommitted follow-up edit, and two untracked files inside an
/// otherwise-untracked directory) — grammar shape verified fresh, not
/// assumed, per this project's own discipline for external tool output.
const FIXTURE: &str =
    " D tracked.txt\nAM staged_then_edited.txt\nRM old_name.txt -> new_name.txt\n?? sub/one.txt\n?? sub/two.txt\n";

#[test]
fn parses_every_status_line_kind() {
    let entries = parse_porcelain_status(FIXTURE);
    assert_eq!(
        entries,
        vec![
            StatusEntry {
                path: PathBuf::from("tracked.txt"),
                index_status: ' ',
                worktree_status: 'D'
            },
            StatusEntry {
                path: PathBuf::from("staged_then_edited.txt"),
                index_status: 'A',
                worktree_status: 'M'
            },
            StatusEntry {
                path: PathBuf::from("new_name.txt"),
                index_status: 'R',
                worktree_status: 'M'
            },
            StatusEntry {
                path: PathBuf::from("sub/one.txt"),
                index_status: '?',
                worktree_status: '?'
            },
            StatusEntry {
                path: PathBuf::from("sub/two.txt"),
                index_status: '?',
                worktree_status: '?'
            },
        ]
    );
}

#[test]
fn empty_status_produces_no_entries() {
    assert_eq!(parse_porcelain_status(""), vec![]);
}

#[test]
fn is_staged_reflects_the_index_half_only() {
    let unstaged_delete = StatusEntry {
        path: PathBuf::from("f"),
        index_status: ' ',
        worktree_status: 'D',
    };
    let staged_add = StatusEntry {
        path: PathBuf::from("f"),
        index_status: 'A',
        worktree_status: ' ',
    };
    let untracked = StatusEntry {
        path: PathBuf::from("f"),
        index_status: '?',
        worktree_status: '?',
    };
    assert!(!unstaged_delete.is_staged());
    assert!(staged_add.is_staged());
    assert!(!untracked.is_staged());
}

#[test]
fn is_untracked_requires_both_halves_to_be_question_marks() {
    let untracked = StatusEntry {
        path: PathBuf::from("f"),
        index_status: '?',
        worktree_status: '?',
    };
    let staged_add = StatusEntry {
        path: PathBuf::from("f"),
        index_status: 'A',
        worktree_status: ' ',
    };
    assert!(untracked.is_untracked());
    assert!(!staged_add.is_untracked());
}

fn init_repo(root: &Path) {
    let run = |args: &[&str]| Command::new("git").current_dir(root).args(args).output().unwrap();
    run(&["init", "-q"]);
    run(&["config", "user.email", "a@b.com"]);
    run(&["config", "user.name", "test"]);
}

#[test]
fn git_status_runs_a_real_git_status_against_a_real_repo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);
    std::fs::write(root.join("new.txt"), "hello\n").unwrap();

    let entries = git_status(root).expect("git ran");
    assert_eq!(
        entries,
        vec![StatusEntry {
            path: PathBuf::from("new.txt"),
            index_status: '?',
            worktree_status: '?'
        }]
    );
}

#[test]
fn git_status_on_a_path_outside_any_repo_is_empty_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let entries = git_status(dir.path()).expect("git still launches fine");
    assert_eq!(entries, vec![]);
}

#[test]
fn first_name_takes_just_the_first_whitespace_separated_token() {
    assert_eq!(first_name("Ada Lovelace"), Some("Ada".to_string()));
    assert_eq!(first_name("Ada"), Some("Ada".to_string()));
    assert_eq!(first_name("  Ada   Lovelace  "), Some("Ada".to_string()));
    assert_eq!(first_name(""), None);
    assert_eq!(first_name("   "), None);
}

#[test]
fn git_user_first_name_runs_a_real_git_config_against_a_real_repo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);
    Command::new("git")
        .current_dir(root)
        .args(["config", "user.name", "Ada Lovelace"])
        .output()
        .unwrap();

    assert_eq!(git_user_first_name(root), Some("Ada".to_string()));
}

#[test]
fn stage_then_commit_round_trips_through_a_real_repo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);
    let file = root.join("f.txt");
    std::fs::write(&file, "hello\n").unwrap();

    let path = PathBuf::from("f.txt");
    git_add(root, std::slice::from_ref(&path)).expect("add succeeds");
    let staged = git_status(root).unwrap();
    assert_eq!(
        staged,
        vec![StatusEntry {
            path: path.clone(),
            index_status: 'A',
            worktree_status: ' '
        }]
    );

    git_commit(root, "add f.txt").expect("commit succeeds");
    assert_eq!(git_status(root).unwrap(), vec![]);

    let log = Command::new("git")
        .current_dir(root)
        .args(["log", "--format=%s"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "add f.txt");
}

#[test]
fn unstage_reverts_a_staged_file_back_to_untracked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);
    let file = root.join("f.txt");
    std::fs::write(&file, "hello\n").unwrap();
    let path = PathBuf::from("f.txt");
    git_add(root, std::slice::from_ref(&path)).unwrap();

    git_reset_paths(root, std::slice::from_ref(&path)).expect("reset succeeds");

    assert_eq!(
        git_status(root).unwrap(),
        vec![StatusEntry {
            path,
            index_status: '?',
            worktree_status: '?'
        }]
    );
}

#[test]
fn commit_with_nothing_staged_fails_with_a_real_git_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);
    std::fs::write(root.join("f.txt"), "hello\n").unwrap();

    let err = git_commit(root, "nothing to commit").unwrap_err();
    assert!(matches!(err, GitCommandError::Failed(_)));
}

#[test]
fn git_apply_cached_stages_a_single_hunk_leaving_the_other_unstaged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);
    let file = root.join("f.txt");
    let lines: Vec<String> = (1..=20).map(|n| format!("l{n}")).collect();
    std::fs::write(&file, lines.join("\n") + "\n").unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "."])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-q", "-m", "init"])
        .output()
        .unwrap();

    let mut edited = lines.clone();
    edited[1] = "CHANGED2".to_string();
    edited[17] = "CHANGED18".to_string();
    std::fs::write(&file, edited.join("\n") + "\n").unwrap();

    let file_diff = crate::diff::git_file_diff(&file, root).expect("git ran");
    assert_eq!(file_diff.hunks.len(), 2);
    let patch = crate::diff::hunk_patch(&file_diff, 0).unwrap();

    git_apply_cached(root, &patch, false).expect("apply --cached succeeds");

    let cached = crate::diff::git_file_diff_cached(&file, root).expect("git ran");
    assert_eq!(cached.hunks.len(), 1);
    assert!(crate::diff::hunk_patch(&cached, 0).unwrap().contains("+CHANGED2"));

    let remaining = crate::diff::git_file_diff(&file, root).expect("git ran");
    assert_eq!(remaining.hunks.len(), 1);
    assert!(crate::diff::hunk_patch(&remaining, 0).unwrap().contains("+CHANGED18"));
}

#[test]
fn git_apply_cached_reverse_unstages_a_single_hunk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);
    let file = root.join("f.txt");
    std::fs::write(&file, "l1\nl2\nl3\n").unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "."])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-q", "-m", "init"])
        .output()
        .unwrap();

    std::fs::write(&file, "l1\nCHANGED\nl3\n").unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "."])
        .output()
        .unwrap();

    let cached = crate::diff::git_file_diff_cached(&file, root).expect("git ran");
    let patch = crate::diff::hunk_patch(&cached, 0).unwrap();

    git_apply_cached(root, &patch, true).expect("apply --cached --reverse succeeds");

    assert!(
        crate::diff::git_file_diff_cached(&file, root)
            .expect("git ran")
            .hunks
            .is_empty()
    );
    assert_eq!(crate::diff::git_file_diff(&file, root).expect("git ran").hunks.len(), 1);
}

#[test]
fn git_push_with_no_configured_remote_fails_with_a_real_git_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);
    std::fs::write(root.join("f.txt"), "hello\n").unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "."])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-q", "-m", "init"])
        .output()
        .unwrap();

    let err = git_push(root).unwrap_err();
    assert!(matches!(err, GitCommandError::Failed(_)));
}

#[test]
fn git_push_succeeds_against_a_real_bare_remote_then_fails_once_diverged() {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("bare.git");
    Command::new("git")
        .args(["init", "-q", "--bare"])
        .arg(&bare)
        .output()
        .unwrap();

    let a = dir.path().join("a");
    let b = dir.path().join("b");
    for clone_dir in [&a, &b] {
        Command::new("git")
            .args(["clone", "-q"])
            .arg(&bare)
            .arg(clone_dir)
            .output()
            .unwrap();
        init_repo(clone_dir);
    }

    std::fs::write(a.join("one.txt"), "one\n").unwrap();
    Command::new("git").current_dir(&a).args(["add", "."]).output().unwrap();
    Command::new("git")
        .current_dir(&a)
        .args(["commit", "-q", "-m", "one"])
        .output()
        .unwrap();
    git_push(&a).expect("first push to an empty bare remote succeeds");

    std::fs::write(b.join("two.txt"), "two\n").unwrap();
    Command::new("git").current_dir(&b).args(["add", "."]).output().unwrap();
    Command::new("git")
        .current_dir(&b)
        .args(["commit", "-q", "-m", "two"])
        .output()
        .unwrap();
    let err = git_push(&b).unwrap_err();
    assert!(matches!(err, GitCommandError::Failed(_)));
}
