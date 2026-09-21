//! Stage/commit panel (`PLAN.md` Track 9 Phase 3): `git status --porcelain
//! -uall` parsed into one `StatusEntry` per changed/untracked file (`-uall`
//! so an entirely-untracked directory is listed file-by-file rather than
//! collapsed to one `?? dir/` line — verified against a real run this
//! session, the default without it hides individual new files under a new
//! directory from a per-file checkbox list), plus `git add`/`git reset`/
//! `git commit -F -` for the panel's checkbox-toggle and Commit button.
//! Same "shell out to the real CLI" approach `diff`/`blame` already
//! established, for the same reason.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// One changed/untracked file from `git status --porcelain`'s two-letter
/// `XY` code — `index_status` (X) is the staged half, `worktree_status` (Y)
/// is the unstaged half; `' '` means "no change on that half." For a
/// rename (`"R  old -> new"`), `path` is the *new* name — the panel stages/
/// unstages/displays by current path, not the one it used to have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: PathBuf,
    pub index_status: char,
    pub worktree_status: char,
}

impl StatusEntry {
    /// Whether this file has *any* staged change — what the panel's
    /// checkbox reflects. `'?'` (untracked) is never staged on its own,
    /// only after an explicit `git add`.
    pub fn is_staged(&self) -> bool {
        self.index_status != ' ' && self.index_status != '?'
    }

    pub fn is_untracked(&self) -> bool {
        self.index_status == '?' && self.worktree_status == '?'
    }
}

#[derive(Debug)]
pub enum GitStatusError {
    /// `git` itself couldn't be launched. Anything else (not a git
    /// repository, no changes) degrades to an empty `Vec` the same way
    /// `git_diff_hunks`/`git_blame` already do — this panel simply has
    /// nothing to show, not an error to surface, in either case.
    Spawn(std::io::Error),
}

impl std::fmt::Display for GitStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitStatusError::Spawn(e) => write!(f, "failed to run git: {e}"),
        }
    }
}

impl std::error::Error for GitStatusError {}

/// Runs `git status --porcelain -uall` with `root` as the working
/// directory (same "only needs to be inside the working tree" contract as
/// `git_diff_hunks`/`git_blame`) and parses the result via
/// `parse_porcelain_status`.
pub fn git_status(root: &Path) -> Result<Vec<StatusEntry>, GitStatusError> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "-uall"])
        .current_dir(root)
        .output()
        .map_err(GitStatusError::Spawn)?;
    Ok(parse_porcelain_status(&String::from_utf8_lossy(&output.stdout)))
}

/// The Source Control panel's own committer-identity label: `git config
/// user.name`, split on whitespace and just the first token (`"Ada"` from
/// `"Ada Lovelace"`) — a quick, local, always-synchronous call (`git
/// config` just reads `.git/config`/the global config file, nothing like
/// `status`/`diff`/`blame`'s own working-tree scan), so unlike every other
/// function in this module it's never worth backgrounding on its own
/// thread. `None` if `git` fails to launch, isn't configured, or reports
/// something blank — purely decorative, so every failure mode just means
/// "don't show a name," matching `git_diff_hunks`/`git_blame`'s own
/// "nothing to show, not an error" convention.
pub fn git_user_first_name(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "user.name"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    first_name(&String::from_utf8_lossy(&output.stdout))
}

fn first_name(full: &str) -> Option<String> {
    full.split_whitespace().next().map(str::to_string)
}

/// Parses `git status --porcelain` output into one `StatusEntry` per line.
/// Pure/no I/O, directly testable against a captured fixture. A line
/// shorter than the `"XY "` prefix is skipped rather than panicking on the
/// slice below — real `git` output never produces one, but this is cheap
/// insurance against a malformed/truncated fixture.
pub fn parse_porcelain_status(output: &str) -> Vec<StatusEntry> {
    output.lines().filter_map(parse_status_line).collect()
}

fn parse_status_line(line: &str) -> Option<StatusEntry> {
    if line.len() < 3 {
        return None;
    }
    let mut chars = line.chars();
    let index_status = chars.next()?;
    let worktree_status = chars.next()?;
    let rest = line[2..].strip_prefix(' ')?;
    // A rename/copy reports `"old -> new"` — the panel only ever cares
    // about the path a file has *now*.
    let path = match rest.split_once(" -> ") {
        Some((_old, new)) => new,
        None => rest,
    };
    Some(StatusEntry {
        path: PathBuf::from(path),
        index_status,
        worktree_status,
    })
}

#[derive(Debug)]
pub enum GitCommandError {
    /// `git` itself couldn't be launched.
    Spawn(std::io::Error),
    /// `git` launched and exited non-zero — `add`/`reset`/`commit` (unlike
    /// `diff`/`blame`/`status`, all silent auto-refreshes) are always
    /// user-triggered, so a real failure (nothing staged to commit, no
    /// `user.name`/`user.email` configured, ...) needs to reach the user,
    /// not degrade to "nothing happened."
    Failed(String),
}

impl std::fmt::Display for GitCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitCommandError::Spawn(e) => write!(f, "failed to run git: {e}"),
            GitCommandError::Failed(stderr) => write!(f, "{stderr}"),
        }
    }
}

impl std::error::Error for GitCommandError {}

fn finish(output: Output) -> Result<(), GitCommandError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(GitCommandError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

/// `git add -- <paths>` — the panel's checkbox-checked action. A no-op
/// (never launches `git` at all) for an empty `paths`, since the panel only
/// calls this for one just-toggled path at a time but a caller batching
/// several has no real files to add either way.
pub fn git_add(root: &Path, paths: &[PathBuf]) -> Result<(), GitCommandError> {
    if paths.is_empty() {
        return Ok(());
    }
    let output = Command::new("git")
        .arg("add")
        .arg("--")
        .args(paths)
        .current_dir(root)
        .output()
        .map_err(GitCommandError::Spawn)?;
    finish(output)
}

/// `git reset -- <paths>` — the panel's checkbox-unchecked action. Same
/// empty-`paths` no-op as `git_add`.
pub fn git_reset_paths(root: &Path, paths: &[PathBuf]) -> Result<(), GitCommandError> {
    if paths.is_empty() {
        return Ok(());
    }
    let output = Command::new("git")
        .arg("reset")
        .arg("--")
        .args(paths)
        .current_dir(root)
        .output()
        .map_err(GitCommandError::Spawn)?;
    finish(output)
}

/// `git commit -F -`, with `message` piped over stdin rather than `-m` —
/// avoids every shell-escaping/argv-length concern a multi-line commit
/// message typed into the panel's own text box would otherwise raise.
pub fn git_commit(root: &Path, message: &str) -> Result<(), GitCommandError> {
    let mut child = Command::new("git")
        .args(["commit", "-F", "-"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(GitCommandError::Spawn)?;
    // `.take()` then drop at the end of this block closes the pipe once the
    // write is done, which is what lets `git commit -F -` see EOF on stdin
    // instead of hanging forever waiting for more.
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(message.as_bytes())
        .map_err(GitCommandError::Spawn)?;
    let output = child.wait_with_output().map_err(GitCommandError::Spawn)?;
    finish(output)
}

/// `git apply --cached` (`reverse` adds `--reverse`), fed `patch` over
/// stdin the same way `git_commit` feeds its own message — this is Phase
/// 4's per-hunk staging primitive: applying a standalone single-hunk patch
/// (built by `fg_core::hunk_patch` from a real `git diff`/`git diff
/// --cached` run) stages just that hunk without touching the working tree
/// at all (`--cached`, not `--index`), and `--reverse` against the *staged*
/// half's own patch un-stages it the same way `git reset` would for a whole
/// file. Verified against a real repo, including the reverse direction (see
/// this module's own tests) — `git apply --cached` needs no `index` line in
/// the patch to succeed, unlike `--index`/a plain working-tree `git apply`.
pub fn git_apply_cached(root: &Path, patch: &str, reverse: bool) -> Result<(), GitCommandError> {
    let mut args = vec!["apply", "--cached"];
    if reverse {
        args.push("--reverse");
    }
    let mut child = Command::new("git")
        .args(&args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(GitCommandError::Spawn)?;
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(patch.as_bytes())
        .map_err(GitCommandError::Spawn)?;
    let output = child.wait_with_output().map_err(GitCommandError::Spawn)?;
    finish(output)
}

/// `git push` against the current branch's configured remote/upstream — no
/// arguments beyond that, so it relies entirely on the repo's own push
/// configuration (same "just shell out, don't second-guess the user's own
/// git config" stance every other function here already takes). Real
/// failure reasons (no configured push destination, auth, a rejected
/// non-fast-forward push, ...) reach the caller as `GitCommandError::
/// Failed`'s real stderr — verified against a real "no remote configured"
/// repo and a real rejected push between two clones of the same bare repo
/// (see this module's own tests), not assumed from `git push --help`.
pub fn git_push(root: &Path) -> Result<(), GitCommandError> {
    let output = Command::new("git")
        .arg("push")
        .current_dir(root)
        .output()
        .map_err(GitCommandError::Spawn)?;
    finish(output)
}

#[cfg(test)]
#[path = "status_test.rs"]
mod status_test;
