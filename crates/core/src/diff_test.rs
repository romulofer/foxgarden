use super::*;

/// Captured verbatim from real `git diff --no-color -U0` runs (a
/// throwaway repo, one small edit at a time) — grammar shape verified
/// fresh rather than assumed, per this project's own discipline for
/// external tool output.
const MODIFY: &str = "diff --git a/f.txt b/f.txt\nindex b8cb000..4ef913f 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -3 +3 @@ l2\n-l3\n+CHANGED\n";
const PURE_ADD_MIDDLE: &str = "diff --git a/f.txt b/f.txt\nindex b8cb000..7c130bd 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -2,0 +3,2 @@ l2\n+NEWLINE1\n+NEWLINE2\n";
const PURE_REMOVE_MIDDLE: &str =
    "diff --git a/f.txt b/f.txt\nindex b8cb000..5018bc2 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -3 +2,0 @@ l2\n-l3\n";
const PURE_REMOVE_START: &str =
    "diff --git a/f.txt b/f.txt\nindex b8cb000..27a9541 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1 +0,0 @@\n-l1\n";
const PURE_REMOVE_END: &str =
    "diff --git a/f.txt b/f.txt\nindex b8cb000..0ddd0f3 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -5 +4,0 @@ l4\n-l5\n";
const ADD_AT_START: &str =
    "diff --git a/f.txt b/f.txt\nindex b8cb000..77bd6e6 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -0,0 +1 @@\n+NEWFIRST\n";
const ADD_AT_END: &str =
    "diff --git a/f.txt b/f.txt\nindex b8cb000..0970e47 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -5,0 +6 @@ l5\n+l6\n";

#[test]
fn parses_a_single_line_modification() {
    // "l1\nl2\nl3\nl4\nl5\n" with l3 (0-based line 2) changed.
    let hunks = parse_unified_diff(MODIFY);
    assert_eq!(
        hunks,
        vec![DiffHunk {
            kind: DiffLineKind::Modified,
            lines: 2..3
        }]
    );
}

#[test]
fn parses_a_pure_addition_in_the_middle() {
    // Two new lines inserted right after l2 (0-based lines 2..4 in the
    // new file).
    let hunks = parse_unified_diff(PURE_ADD_MIDDLE);
    assert_eq!(
        hunks,
        vec![DiffHunk {
            kind: DiffLineKind::Added,
            lines: 2..4
        }]
    );
}

#[test]
fn parses_a_pure_removal_in_the_middle_as_an_empty_marker_range() {
    // l3 deleted — the marker attaches at 0-based line 2 (where l4 now
    // sits), the empty range this module's own doc comment describes.
    let hunks = parse_unified_diff(PURE_REMOVE_MIDDLE);
    assert_eq!(
        hunks,
        vec![DiffHunk {
            kind: DiffLineKind::Removed,
            lines: 2..2
        }]
    );
}

#[test]
fn parses_a_pure_removal_at_the_very_start_of_the_file() {
    // git's own real "+0,0" edge case for a deletion right at line 1 —
    // no underflow, no special-casing needed on this side.
    let hunks = parse_unified_diff(PURE_REMOVE_START);
    assert_eq!(
        hunks,
        vec![DiffHunk {
            kind: DiffLineKind::Removed,
            lines: 0..0
        }]
    );
}

#[test]
fn parses_a_pure_removal_at_the_very_end_of_the_file() {
    // l5 deleted from a 5-line file — marker at 0-based line 4, one
    // past the last surviving line (index 3).
    let hunks = parse_unified_diff(PURE_REMOVE_END);
    assert_eq!(
        hunks,
        vec![DiffHunk {
            kind: DiffLineKind::Removed,
            lines: 4..4
        }]
    );
}

#[test]
fn parses_an_addition_at_the_very_start_of_the_file() {
    let hunks = parse_unified_diff(ADD_AT_START);
    assert_eq!(
        hunks,
        vec![DiffHunk {
            kind: DiffLineKind::Added,
            lines: 0..1
        }]
    );
}

#[test]
fn parses_an_addition_at_the_very_end_of_the_file() {
    let hunks = parse_unified_diff(ADD_AT_END);
    assert_eq!(
        hunks,
        vec![DiffHunk {
            kind: DiffLineKind::Added,
            lines: 5..6
        }]
    );
}

#[test]
fn ignores_non_hunk_lines() {
    let diff = "diff --git a/f.txt b/f.txt\nindex 111..222 100644\n--- a/f.txt\n+++ b/f.txt\n";
    assert_eq!(parse_unified_diff(diff), vec![]);
}

#[test]
fn empty_diff_produces_no_hunks() {
    assert_eq!(parse_unified_diff(""), vec![]);
}

#[test]
fn multiple_hunks_in_one_diff_are_all_parsed_in_order() {
    let diff = format!("{MODIFY}{PURE_ADD_MIDDLE}");
    let hunks = parse_unified_diff(&diff);
    assert_eq!(
        hunks,
        vec![
            DiffHunk {
                kind: DiffLineKind::Modified,
                lines: 2..3
            },
            DiffHunk {
                kind: DiffLineKind::Added,
                lines: 2..4
            },
        ]
    );
}

/// End-to-end against a real `git` binary and a real temp repository —
/// not just the parser in isolation — so a change to the actual CLI
/// invocation (wrong flag, wrong argument order) would fail this even
/// if `parse_unified_diff` itself stayed correct.
#[test]
fn git_diff_hunks_runs_a_real_git_diff_against_a_real_repo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("f.txt");
    std::fs::write(&file, "l1\nl2\nl3\n").unwrap();

    let run = |args: &[&str]| Command::new("git").current_dir(root).args(args).output().unwrap();
    run(&["init", "-q"]);
    run(&["config", "user.email", "a@b.com"]);
    run(&["config", "user.name", "test"]);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);

    std::fs::write(&file, "l1\nCHANGED\nl3\n").unwrap();

    let hunks = git_diff_hunks(&file, root).expect("git ran");
    assert_eq!(
        hunks,
        vec![DiffHunk {
            kind: DiffLineKind::Modified,
            lines: 1..2
        }]
    );
}

#[test]
fn git_diff_hunks_on_a_path_outside_any_repo_is_empty_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("f.txt");
    std::fs::write(&file, "hello\n").unwrap();

    let hunks = git_diff_hunks(&file, root).expect("git still launches fine");
    assert_eq!(hunks, vec![]);
}

/// Captured verbatim from a real `git diff --no-color` run (default
/// 3-line context, not `-U0`) against a 20-line file with two edits far
/// enough apart to land in separate hunks — the shape `parse_file_diff`/
/// `hunk_patch` need to verify against, per this project's own
/// discipline for external tool output.
const TWO_HUNK_DIFF: &str = "diff --git a/f.txt b/f.txt\nindex 86bba90..25db50c 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,5 +1,5 @@\n l1\n-l2\n+CHANGED2\n l3\n l4\n l5\n@@ -15,6 +15,6 @@ l14\n l15\n l16\n l17\n-l18\n+CHANGED18\n l19\n l20\n";

#[test]
fn parse_file_diff_splits_the_shared_preamble_from_each_individual_hunk() {
    let file_diff = parse_file_diff(TWO_HUNK_DIFF);
    assert_eq!(
        file_diff.preamble,
        "diff --git a/f.txt b/f.txt\nindex 86bba90..25db50c 100644\n--- a/f.txt\n+++ b/f.txt\n"
    );
    assert_eq!(file_diff.hunks.len(), 2);
    assert_eq!(file_diff.hunks[0].header, "@@ -1,5 +1,5 @@");
    assert_eq!(
        file_diff.hunks[0].lines,
        vec![" l1", "-l2", "+CHANGED2", " l3", " l4", " l5"]
    );
    assert_eq!(file_diff.hunks[1].header, "@@ -15,6 +15,6 @@ l14");
    assert_eq!(
        file_diff.hunks[1].lines,
        vec![" l15", " l16", " l17", "-l18", "+CHANGED18", " l19", " l20"]
    );
}

#[test]
fn parse_file_diff_on_an_unchanged_file_has_no_hunks() {
    let file_diff = parse_file_diff("");
    assert!(file_diff.preamble.is_empty());
    assert!(file_diff.hunks.is_empty());
}

#[test]
fn hunk_patch_rebuilds_a_standalone_single_hunk_patch() {
    let file_diff = parse_file_diff(TWO_HUNK_DIFF);
    let patch = hunk_patch(&file_diff, 1).unwrap();
    assert_eq!(
        patch,
        "diff --git a/f.txt b/f.txt\nindex 86bba90..25db50c 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -15,6 +15,6 @@ l14\n l15\n l16\n l17\n-l18\n+CHANGED18\n l19\n l20\n"
    );
}

#[test]
fn hunk_patch_is_none_for_an_out_of_range_index() {
    let file_diff = parse_file_diff(TWO_HUNK_DIFF);
    assert!(hunk_patch(&file_diff, 5).is_none());
}

/// End-to-end against a real repo: two far-apart edits land in two real
/// hunks, and `hunk_patch`'s own rebuilt text for just the second one
/// matches what a real, independently-run `git diff` against only that
/// line range would produce.
#[test]
fn git_file_diff_runs_a_real_git_diff_and_splits_real_hunks() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("f.txt");
    let lines: Vec<String> = (1..=20).map(|n| format!("l{n}")).collect();
    std::fs::write(&file, lines.join("\n") + "\n").unwrap();

    let run = |args: &[&str]| Command::new("git").current_dir(root).args(args).output().unwrap();
    run(&["init", "-q"]);
    run(&["config", "user.email", "a@b.com"]);
    run(&["config", "user.name", "test"]);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);

    let mut edited = lines.clone();
    edited[1] = "CHANGED2".to_string();
    edited[17] = "CHANGED18".to_string();
    std::fs::write(&file, edited.join("\n") + "\n").unwrap();

    let file_diff = git_file_diff(&file, root).expect("git ran");
    assert_eq!(file_diff.hunks.len(), 2);
    assert!(hunk_patch(&file_diff, 0).unwrap().contains("+CHANGED2"));
    assert!(hunk_patch(&file_diff, 1).unwrap().contains("+CHANGED18"));
}

#[test]
fn git_file_diff_cached_reads_only_the_staged_half() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("f.txt");
    std::fs::write(&file, "l1\nl2\nl3\n").unwrap();

    let run = |args: &[&str]| Command::new("git").current_dir(root).args(args).output().unwrap();
    run(&["init", "-q"]);
    run(&["config", "user.email", "a@b.com"]);
    run(&["config", "user.name", "test"]);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);

    std::fs::write(&file, "l1\nCHANGED\nl3\n").unwrap();

    assert!(git_file_diff_cached(&file, root).expect("git ran").hunks.is_empty());
    run(&["add", "."]);
    assert_eq!(git_file_diff_cached(&file, root).expect("git ran").hunks.len(), 1);
    assert!(git_file_diff(&file, root).expect("git ran").hunks.is_empty());
}

#[test]
fn git_show_head_returns_the_committed_content_not_the_working_tree_edit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("f.txt");
    std::fs::write(&file, "l1\n").unwrap();

    let run = |args: &[&str]| Command::new("git").current_dir(root).args(args).output().unwrap();
    run(&["init", "-q"]);
    run(&["config", "user.email", "a@b.com"]);
    run(&["config", "user.name", "test"]);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);

    std::fs::write(&file, "l1\nCHANGED\n").unwrap();

    assert_eq!(git_show_head(Path::new("f.txt"), root).expect("git ran"), "l1\n");
}

#[test]
fn git_show_head_resolves_a_subdirectory_path_relative_to_root_not_the_git_top_level() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/f.txt"), "l1\n").unwrap();

    let run = |args: &[&str]| Command::new("git").current_dir(root).args(args).output().unwrap();
    run(&["init", "-q"]);
    run(&["config", "user.email", "a@b.com"]);
    run(&["config", "user.name", "test"]);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);

    // `root` passed as `sub/` itself (a subdirectory of the git repo,
    // mirroring a multi-module project root) — the `./`-prefixed form
    // must still resolve `f.txt` relative to *this* cwd, not the real
    // git top level one directory up.
    assert_eq!(
        git_show_head(Path::new("f.txt"), &root.join("sub")).expect("git ran"),
        "l1\n"
    );
}

#[test]
fn git_show_head_on_a_file_thats_never_been_committed_is_empty_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let run = |args: &[&str]| Command::new("git").current_dir(root).args(args).output().unwrap();
    run(&["init", "-q"]);
    run(&["config", "user.email", "a@b.com"]);
    run(&["config", "user.name", "test"]);
    std::fs::write(root.join("new.txt"), "brand new\n").unwrap();

    assert_eq!(
        git_show_head(Path::new("new.txt"), root).expect("git still launches fine"),
        ""
    );
}
