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
            hunks.push(RawHunk {
                header: line.to_string(),
                lines: Vec::new(),
            });
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
#[path = "diff_test.rs"]
mod diff_test;
