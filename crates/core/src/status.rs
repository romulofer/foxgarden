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
    Some(StatusEntry { path: PathBuf::from(path), index_status, worktree_status })
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
        Err(GitCommandError::Failed(String::from_utf8_lossy(&output.stderr).trim().to_string()))
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

#[cfg(test)]
mod tests {
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
                StatusEntry { path: PathBuf::from("tracked.txt"), index_status: ' ', worktree_status: 'D' },
                StatusEntry {
                    path: PathBuf::from("staged_then_edited.txt"),
                    index_status: 'A',
                    worktree_status: 'M'
                },
                StatusEntry { path: PathBuf::from("new_name.txt"), index_status: 'R', worktree_status: 'M' },
                StatusEntry { path: PathBuf::from("sub/one.txt"), index_status: '?', worktree_status: '?' },
                StatusEntry { path: PathBuf::from("sub/two.txt"), index_status: '?', worktree_status: '?' },
            ]
        );
    }

    #[test]
    fn empty_status_produces_no_entries() {
        assert_eq!(parse_porcelain_status(""), vec![]);
    }

    #[test]
    fn is_staged_reflects_the_index_half_only() {
        let unstaged_delete = StatusEntry { path: PathBuf::from("f"), index_status: ' ', worktree_status: 'D' };
        let staged_add = StatusEntry { path: PathBuf::from("f"), index_status: 'A', worktree_status: ' ' };
        let untracked = StatusEntry { path: PathBuf::from("f"), index_status: '?', worktree_status: '?' };
        assert!(!unstaged_delete.is_staged());
        assert!(staged_add.is_staged());
        assert!(!untracked.is_staged());
    }

    #[test]
    fn is_untracked_requires_both_halves_to_be_question_marks() {
        let untracked = StatusEntry { path: PathBuf::from("f"), index_status: '?', worktree_status: '?' };
        let staged_add = StatusEntry { path: PathBuf::from("f"), index_status: 'A', worktree_status: ' ' };
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
        assert_eq!(entries, vec![StatusEntry { path: PathBuf::from("new.txt"), index_status: '?', worktree_status: '?' }]);
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
        Command::new("git").current_dir(root).args(["config", "user.name", "Ada Lovelace"]).output().unwrap();

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
        assert_eq!(staged, vec![StatusEntry { path: path.clone(), index_status: 'A', worktree_status: ' ' }]);

        git_commit(root, "add f.txt").expect("commit succeeds");
        assert_eq!(git_status(root).unwrap(), vec![]);

        let log = Command::new("git").current_dir(root).args(["log", "--format=%s"]).output().unwrap();
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

        assert_eq!(git_status(root).unwrap(), vec![StatusEntry { path, index_status: '?', worktree_status: '?' }]);
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
}
