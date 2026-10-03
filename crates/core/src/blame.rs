//! Inline blame (`PLAN.md` Track 9 Phase 2): shells out to `git blame
//! --porcelain -- <path>` (same "shell out to the real CLI" approach as
//! `diff`, and for the same reason — no `git2` dependency needed) and
//! parses the result into one `BlameLine` per line of the file, dense and
//! 0-indexed so the editor widget can look up the cursor's current line
//! directly.

use std::path::Path;
use std::process::Command;

/// One line's worth of blame — just enough for a dimmed cursor-line
/// annotation (author + relative-enough date + commit summary), not the
/// full porcelain record (no `previous`/`boundary`/committer fields; nothing
/// here reads them).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlameLine {
    pub sha: String,
    pub author: String,
    /// Unix seconds (`author-time`), left for the caller to format.
    pub author_time: i64,
    pub summary: String,
}

#[derive(Debug)]
pub enum GitBlameError {
    /// `git` itself couldn't be launched — anything else (not a git
    /// repository, an untracked file) isn't distinguished from "no blame
    /// info": `git blame` prints nothing to stdout on that kind of failure,
    /// so `parse_porcelain_blame` on the resulting empty string already
    /// degrades to an empty `Vec`, same "nothing to show, not an error to
    /// surface" convention `git_diff_hunks` established.
    Spawn(std::io::Error),
}

impl std::fmt::Display for GitBlameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitBlameError::Spawn(e) => write!(f, "failed to run git: {e}"),
        }
    }
}

impl std::error::Error for GitBlameError {}

/// Runs `git blame --porcelain -- <path>` with `root` as the working
/// directory (same "only needs to be inside the working tree" contract as
/// `git_diff_hunks`) and parses the result via `parse_porcelain_blame`.
pub fn git_blame(path: &Path, root: &Path) -> Result<Vec<BlameLine>, GitBlameError> {
    let output = Command::new("git")
        .args(["blame", "--porcelain", "--"])
        .arg(path)
        .current_dir(root)
        .output()
        .map_err(GitBlameError::Spawn)?;
    Ok(parse_porcelain_blame(&String::from_utf8_lossy(&output.stdout)))
}

/// Parses `git blame --porcelain` output into one `BlameLine` per final-file
/// line, in order. Porcelain format (verified against several real captured
/// runs this session — a first-seen commit, a repeated commit, and an
/// uncommitted local edit, not assumed): each line's record opens with a
/// header (`<40-hex-sha> <orig-line> <final-line>[ <group-count>]`),
/// followed by zero or more metadata lines (`author ...`, `author-time ...`,
/// `summary ...`, plus others this parser doesn't need), and closes with
/// exactly one tab-prefixed content line. Metadata is only emitted the
/// *first* time a given commit appears anywhere in the output — a later
/// record for the same commit has just the header and content line — so a
/// per-commit cache fills in the gaps. The tab-prefix is what makes this
/// robust: it's the one thing that can't collide with a metadata line (git
/// never emits a bare source line without it), so a source line that
/// happens to start with the word "author" can't be mistaken for metadata.
pub fn parse_porcelain_blame(output: &str) -> Vec<BlameLine> {
    let mut cache: std::collections::HashMap<String, (String, i64, String)> = std::collections::HashMap::new();
    let mut result = Vec::new();
    let mut lines = output.lines().peekable();

    while let Some(header) = lines.next() {
        let mut parts = header.split_whitespace();
        let Some(sha) = parts.next() else { continue };
        if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        if parts.next().is_none() {
            continue; // not actually a header line
        }

        let (mut author, mut author_time, mut summary) = cache.get(sha).cloned().unwrap_or_default();

        while let Some(next) = lines.peek()
            && !next.starts_with('\t')
        {
            let next = lines.next().unwrap();
            if let Some(v) = next.strip_prefix("author ") {
                author = v.to_string();
            } else if let Some(v) = next.strip_prefix("author-time ") {
                author_time = v.parse().unwrap_or(0);
            } else if let Some(v) = next.strip_prefix("summary ") {
                summary = v.to_string();
            }
        }
        lines.next(); // the tab-prefixed content line itself, discarded

        cache.insert(sha.to_string(), (author.clone(), author_time, summary.clone()));
        result.push(BlameLine {
            sha: sha.to_string(),
            author,
            author_time,
            summary,
        });
    }

    result
}

#[cfg(test)]
#[path = "blame_test.rs"]
mod blame_test;
