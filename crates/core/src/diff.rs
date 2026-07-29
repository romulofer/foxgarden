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
}
