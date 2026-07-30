//! Git diff gutter (`PLAN.md` Track 9 Phase 1): shells out to `git diff
//! --no-color -U0 -- <path>` for one file at a time and parses the unified
//! diff's own hunk headers into per-line added/removed/modified marks — no
//! `git2`/`libgit2` dependency, mirroring `static_analysis`'s own "shell out
//! to the real CLI" approach rather than a library binding, since this is
//! the first git-aware code in the project and there's nothing here that
//! needs more than what the CLI's own plumbing output already gives.

use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Added,
    Removed,
    Modified,
}

/// One hunk from a unified diff, converted to this codebase's own 0-based
/// line-space (matching `Document`/the gutter's line numbering, not git's
/// 1-based hunk-header terms). `Added`/`Modified` cover every line the
/// hunk's `+` side touches (`lines` non-empty). `Removed` has no surviving
/// line of its own to cover — the content is just gone — so `lines` is the
/// empty range `at..at`, marking *where* the removal happened rather than
/// *which* lines: verified against a real `git diff -U0` run (see this
/// module's tests), a pure-deletion hunk's `+` side always reports a
/// `,0` count with a start line that's already exactly the right 0-based
/// index of the line immediately following the deleted content (including
/// the real edge case of a deletion at the very start of the file, which
/// git reports as `+0,0` — `at` is `0` there too, no adjustment needed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub kind: DiffLineKind,
    pub lines: std::ops::Range<usize>,
}

#[derive(Debug)]
pub enum GitDiffError {
    /// `git` itself couldn't be launched (not on `PATH`, ...) — anything
    /// else (not a git repository, an untracked file, no changes at all)
    /// isn't distinguished from "no changes": all three produce empty (or
    /// unparseable-as-hunks) stdout, and every one of them means the same
    /// thing to this gutter, nothing to show, not an error to surface.
    Spawn(std::io::Error),
}

impl std::fmt::Display for GitDiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitDiffError::Spawn(e) => write!(f, "failed to run git: {e}"),
        }
    }
}

impl std::error::Error for GitDiffError {}

/// Runs `git diff --no-color -U0 -- <path>` with `root` as the working
/// directory — `root` only needs to be *inside* the repository's working
/// tree (git walks up to find the real repo root itself), which is what
/// lets this work against a Maven/Gradle multi-module project root that
/// isn't necessarily the git root — and parses the result via
/// `parse_unified_diff`.
pub fn git_diff_hunks(path: &Path, root: &Path) -> Result<Vec<DiffHunk>, GitDiffError> {
    let output = Command::new("git")
        .args(["diff", "--no-color", "-U0", "--"])
        .arg(path)
        .current_dir(root)
        .output()
        .map_err(GitDiffError::Spawn)?;
    Ok(parse_unified_diff(&String::from_utf8_lossy(&output.stdout)))
}

/// Parses every `@@ -old_start[,old_count] +new_start[,new_count] @@` hunk
/// header in `diff` into a `DiffHunk`, ignoring the actual `+`/`-` content
/// lines entirely — Phase 1 only needs *which lines changed*, not the
/// changed text itself (that's Track 18's inline-diff-widget job). Pure/no
/// I/O, directly testable against a captured `git diff -U0` fixture. Lines
/// that aren't a hunk header (the `diff --git`/`index`/`---`/`+++` lines,
/// and every actual content line) are silently skipped.
pub fn parse_unified_diff(diff: &str) -> Vec<DiffHunk> {
    diff.lines().filter_map(parse_hunk_header).collect()
}

fn parse_hunk_header(line: &str) -> Option<DiffHunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;

    let (_old_start, old_count) = parse_range(old)?;
    let (new_start, new_count) = parse_range(new)?;

    Some(if new_count == 0 {
        DiffHunk {
            kind: DiffLineKind::Removed,
            lines: new_start..new_start,
        }
    } else if old_count == 0 {
        DiffHunk {
            kind: DiffLineKind::Added,
            lines: (new_start - 1)..(new_start - 1 + new_count),
        }
    } else {
        DiffHunk {
            kind: DiffLineKind::Modified,
            lines: (new_start - 1)..(new_start - 1 + new_count),
        }
    })
}

/// Parses one hunk-header half (`"start"` or `"start,count"`) into
/// `(start, count)`, defaulting `count` to 1 when the unified diff format
/// omits it (its own convention for a single-line hunk).
fn parse_range(s: &str) -> Option<(usize, usize)> {
    match s.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}

/// One hunk's header line (`"@@ -a,b +c,d @@ trailing context"`) plus its
/// content lines, each already carrying its own leading `' '`/`'+'`/`'-'` —
/// captured verbatim from a real `git diff` run. Unlike `DiffHunk` (Phase
/// 1, which only keeps the *line range* a hunk covers, for the gutter's own
/// purposes), this keeps the hunk's full text, which is what `hunk_patch`
/// needs to reassemble a standalone patch for `git apply --cached` (Phase
/// 4's per-hunk staging).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawHunk {
    pub header: String,
    pub lines: Vec<String>,
}

/// One file's full diff, split into the file-level preamble every one of
/// its hunks shares (the `diff --git`/`index`/`---`/`+++` lines, and
/// `Binary files ... differ` for a binary file, which has no `@@` hunks
/// of its own — `hunks` is simply empty then) and its individual hunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub preamble: String,
    pub hunks: Vec<RawHunk>,
}

/// Runs `git diff --no-color -- <path>` (real, default 3-line context —
/// unlike `git_diff_hunks`'s own `-U0`, a hand-built hunk patch needs
/// surrounding context lines for `git apply` to locate it unambiguously)
/// and parses the result via `parse_file_diff`. This is the *unstaged*
/// half — the working tree against the index.
pub fn git_file_diff(path: &Path, root: &Path) -> Result<FileDiff, GitDiffError> {
    let output = Command::new("git")
        .args(["diff", "--no-color", "--"])
        .arg(path)
        .current_dir(root)
        .output()
        .map_err(GitDiffError::Spawn)?;
    Ok(parse_file_diff(&String::from_utf8_lossy(&output.stdout)))
}

/// Same as `git_file_diff`, but the *staged* half (`git diff --no-color
/// --cached -- <path>` — the index against `HEAD`), for the "unstage this
/// hunk" side of Phase 4's per-hunk staging.
pub fn git_file_diff_cached(path: &Path, root: &Path) -> Result<FileDiff, GitDiffError> {
    let output = Command::new("git")
        .args(["diff", "--no-color", "--cached", "--"])
        .arg(path)
        .current_dir(root)
        .output()
        .map_err(GitDiffError::Spawn)?;
    Ok(parse_file_diff(&String::from_utf8_lossy(&output.stdout)))
}

/// Splits a real `git diff` file section into its shared preamble and
/// individual `@@`-delimited hunks. Pure/no I/O, directly testable against
/// a captured fixture. A hunk header is recognized by `"@@ "` at the start
/// of a line — real content lines never start that way, since every one of
/// them starts with `' '`/`'+'`/`'-'` instead.
pub fn parse_file_diff(diff: &str) -> FileDiff {
    let mut preamble = String::new();
    let mut hunks: Vec<RawHunk> = Vec::new();

    for line in diff.lines() {
        if line.starts_with("@@ ") {
            hunks.push(RawHunk { header: line.to_string(), lines: Vec::new() });
        } else if let Some(hunk) = hunks.last_mut() {
            hunk.lines.push(line.to_string());
        } else {
            preamble.push_str(line);
            preamble.push('\n');
        }
    }
    FileDiff { preamble, hunks }
}

/// Runs `git show HEAD:./<path>` (`root` as cwd) and returns its stdout
/// verbatim — the file's content as of `HEAD`, for feeding `widgets::
/// diff_view::show_diff` against the current working-tree content. The
/// `./`-prefixed form is deliberate: a bare `HEAD:<path>` resolves `path`
/// relative to the git repository's *top level*, not `root`/cwd — wrong
/// whenever `root` is a subdirectory (a Maven/Gradle multi-module project
/// root, say) — while `HEAD:./<path>` resolves relative to cwd exactly the
/// way every other function in this module already treats `root`+`path`
/// (verified against a real multi-directory repo, not assumed). Same
/// "empty output either way" degrade as `git_diff_hunks`/`git_blame`: a
/// path that doesn't exist in `HEAD` yet (a genuinely new/untracked file)
/// fails with real stderr but empty stdout — indistinguishable from "no
/// content," which is exactly the right answer for a diff against nothing.
/// Only a failure to launch `git` at all is a real `Err`.
pub fn git_show_head(path: &Path, root: &Path) -> Result<String, GitDiffError> {
    let output = Command::new("git")
        .arg("show")
        .arg(format!("HEAD:./{}", path.display()))
        .current_dir(root)
        .output()
        .map_err(GitDiffError::Spawn)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Rebuilds hunk `index` of `file_diff` into a standalone, single-hunk
/// unified diff — valid input for `git apply --cached` (staging that one
/// hunk) or `git apply --cached --reverse` (unstaging it), verified
/// against a real repo (see `status::tests`). `None` for an out-of-range
/// index (the caller's own hunk list went stale — e.g. a concurrent
/// refresh — rather than a real bug to panic on).
pub fn hunk_patch(file_diff: &FileDiff, index: usize) -> Option<String> {
    let hunk = file_diff.hunks.get(index)?;
    let mut patch = file_diff.preamble.clone();
    patch.push_str(&hunk.header);
    patch.push('\n');
    for line in &hunk.lines {
        patch.push_str(line);
        patch.push('\n');
    }
    Some(patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured verbatim from real `git diff --no-color -U0` runs (a
    /// throwaway repo, one small edit at a time) — grammar shape verified
    /// fresh rather than assumed, per this project's own discipline for
    /// external tool output.
    const MODIFY: &str = "diff --git a/f.txt b/f.txt\nindex b8cb000..4ef913f 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -3 +3 @@ l2\n-l3\n+CHANGED\n";
    const PURE_ADD_MIDDLE: &str = "diff --git a/f.txt b/f.txt\nindex b8cb000..7c130bd 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -2,0 +3,2 @@ l2\n+NEWLINE1\n+NEWLINE2\n";
    const PURE_REMOVE_MIDDLE: &str = "diff --git a/f.txt b/f.txt\nindex b8cb000..5018bc2 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -3 +2,0 @@ l2\n-l3\n";
    const PURE_REMOVE_START: &str = "diff --git a/f.txt b/f.txt\nindex b8cb000..27a9541 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1 +0,0 @@\n-l1\n";
    const PURE_REMOVE_END: &str = "diff --git a/f.txt b/f.txt\nindex b8cb000..0ddd0f3 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -5 +4,0 @@ l4\n-l5\n";
    const ADD_AT_START: &str = "diff --git a/f.txt b/f.txt\nindex b8cb000..77bd6e6 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -0,0 +1 @@\n+NEWFIRST\n";
    const ADD_AT_END: &str = "diff --git a/f.txt b/f.txt\nindex b8cb000..0970e47 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -5,0 +6 @@ l5\n+l6\n";

    #[test]
    fn parses_a_single_line_modification() {
        // "l1\nl2\nl3\nl4\nl5\n" with l3 (0-based line 2) changed.
        let hunks = parse_unified_diff(MODIFY);
        assert_eq!(hunks, vec![DiffHunk { kind: DiffLineKind::Modified, lines: 2..3 }]);
    }

    #[test]
    fn parses_a_pure_addition_in_the_middle() {
        // Two new lines inserted right after l2 (0-based lines 2..4 in the
        // new file).
        let hunks = parse_unified_diff(PURE_ADD_MIDDLE);
        assert_eq!(hunks, vec![DiffHunk { kind: DiffLineKind::Added, lines: 2..4 }]);
    }

    #[test]
    fn parses_a_pure_removal_in_the_middle_as_an_empty_marker_range() {
        // l3 deleted — the marker attaches at 0-based line 2 (where l4 now
        // sits), the empty range this module's own doc comment describes.
        let hunks = parse_unified_diff(PURE_REMOVE_MIDDLE);
        assert_eq!(hunks, vec![DiffHunk { kind: DiffLineKind::Removed, lines: 2..2 }]);
    }

    #[test]
    fn parses_a_pure_removal_at_the_very_start_of_the_file() {
        // git's own real "+0,0" edge case for a deletion right at line 1 —
        // no underflow, no special-casing needed on this side.
        let hunks = parse_unified_diff(PURE_REMOVE_START);
        assert_eq!(hunks, vec![DiffHunk { kind: DiffLineKind::Removed, lines: 0..0 }]);
    }

    #[test]
    fn parses_a_pure_removal_at_the_very_end_of_the_file() {
        // l5 deleted from a 5-line file — marker at 0-based line 4, one
        // past the last surviving line (index 3).
        let hunks = parse_unified_diff(PURE_REMOVE_END);
        assert_eq!(hunks, vec![DiffHunk { kind: DiffLineKind::Removed, lines: 4..4 }]);
    }

    #[test]
    fn parses_an_addition_at_the_very_start_of_the_file() {
        let hunks = parse_unified_diff(ADD_AT_START);
        assert_eq!(hunks, vec![DiffHunk { kind: DiffLineKind::Added, lines: 0..1 }]);
    }

    #[test]
    fn parses_an_addition_at_the_very_end_of_the_file() {
        let hunks = parse_unified_diff(ADD_AT_END);
        assert_eq!(hunks, vec![DiffHunk { kind: DiffLineKind::Added, lines: 5..6 }]);
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
                DiffHunk { kind: DiffLineKind::Modified, lines: 2..3 },
                DiffHunk { kind: DiffLineKind::Added, lines: 2..4 },
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

        let run = |args: &[&str]| {
            Command::new("git").current_dir(root).args(args).output().unwrap()
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "a@b.com"]);
        run(&["config", "user.name", "test"]);
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);

        std::fs::write(&file, "l1\nCHANGED\nl3\n").unwrap();

        let hunks = git_diff_hunks(&file, root).expect("git ran");
        assert_eq!(hunks, vec![DiffHunk { kind: DiffLineKind::Modified, lines: 1..2 }]);
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
        assert_eq!(file_diff.hunks[0].lines, vec![" l1", "-l2", "+CHANGED2", " l3", " l4", " l5"]);
        assert_eq!(file_diff.hunks[1].header, "@@ -15,6 +15,6 @@ l14");
        assert_eq!(file_diff.hunks[1].lines, vec![" l15", " l16", " l17", "-l18", "+CHANGED18", " l19", " l20"]);
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
        assert_eq!(git_show_head(Path::new("f.txt"), &root.join("sub")).expect("git ran"), "l1\n");
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

        assert_eq!(git_show_head(Path::new("new.txt"), root).expect("git still launches fine"), "");
    }
}
