//! Crash-safe copies of unsaved buffers, under `.foxgarden/drafts/`.
//!
//! Unsaved work only existed in memory: killing the process, a crash, or a
//! machine losing power took every edit with it, silently — the next launch
//! reopened the same tabs showing the last *saved* contents, with nothing
//! to say anything had been lost. Auto-save exists but is off by default
//! (and writing the real file on a timer is a different, more opinionated
//! thing than not losing work).
//!
//! A draft is that file's in-memory text, written periodically while it
//! differs from disk and deleted the moment it doesn't. On the next launch
//! the app finds any drafts left behind and offers to restore them —
//! "offers", because a draft is by definition text the user never
//! committed to disk, and silently resurrecting it would be its own kind of
//! surprise.

use std::path::{Path, PathBuf};

/// Where a file's draft lives: `.foxgarden/drafts/<relative path>.draft`,
/// mirroring `file_history`'s own per-project convention. `None` for a file
/// outside the project (a JDK source opened by go-to-definition), which has
/// no project-relative location to key off.
fn draft_path(project_root: &Path, file_path: &Path) -> Option<PathBuf> {
    let relative = file_path.strip_prefix(project_root).ok()?;
    let mut name = relative.file_name()?.to_os_string();
    name.push(".draft");
    Some(
        project_root
            .join(".foxgarden")
            .join("drafts")
            .join(relative.parent()?)
            .join(name),
    )
}

/// Writes `content` as `file_path`'s draft. Atomic (`crate::write_atomically`)
/// for the same reason the real save is: a draft half-written by the very
/// crash it exists to survive would be worse than none.
pub fn write_draft(project_root: &Path, file_path: &Path, content: &str) -> std::io::Result<()> {
    let Some(path) = draft_path(project_root, file_path) else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::write_atomically(&path, content.as_bytes())
}

/// Deletes `file_path`'s draft, if it has one — called when the buffer
/// matches disk again (a save, or an undo back to the saved state), so a
/// stale draft can't be offered for restore later.
pub fn discard_draft(project_root: &Path, file_path: &Path) -> std::io::Result<()> {
    let Some(path) = draft_path(project_root, file_path) else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// A draft found on disk at startup: the file it belongs to, and the text
/// that was never saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub file_path: PathBuf,
    pub content: String,
}

/// Every draft under `project_root` whose own file still exists and whose
/// content still differs from what's on disk.
///
/// The two filters are what keep a restore offer honest: a draft for a
/// deleted file has nothing to restore *into*, and one that matches disk
/// (the app was killed after a save but before the draft was cleaned up)
/// represents no lost work at all. Both are deleted as they're found.
pub fn pending_drafts(project_root: &Path) -> Vec<Draft> {
    let root = project_root.join(".foxgarden").join("drafts");
    let mut found = Vec::new();
    collect(&root, &root, project_root, &mut found);
    found
}

fn collect(dir: &Path, drafts_root: &Path, project_root: &Path, found: &mut Vec<Draft>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect(&path, drafts_root, project_root, found);
            continue;
        }
        let Some(relative) = path.strip_prefix(drafts_root).ok().map(Path::to_path_buf) else {
            continue;
        };
        let Some(file_name) = relative
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".draft"))
        else {
            continue;
        };
        let file_path = match relative.parent() {
            Some(parent) => project_root.join(parent).join(file_name),
            None => project_root.join(file_name),
        };

        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let on_disk = std::fs::read_to_string(&file_path);
        let worth_offering = match on_disk {
            Ok(saved) => saved != content,
            // The file is gone; there's nothing to restore into.
            Err(_) => false,
        };
        if worth_offering {
            found.push(Draft { file_path, content });
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
#[path = "drafts_test.rs"]
mod drafts_test;
