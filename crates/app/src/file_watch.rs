use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// What should happen to an open tab after `path` changed on disk — a
/// `notify` filesystem event, already narrowed by the caller to *an
/// already-open tab's* path (anything else is uninteresting and never
/// reaches this function). Pure decision logic, deliberately separated
/// from the actual `notify::Watcher` wiring (needs a real filesystem
/// watcher to exercise, so it's left to manual/integration testing) and
/// the disk read (`app.rs` does both) so this decision — reload
/// transparently, flag a conflict, or note a deletion — can be unit
/// tested directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// The file on disk already matches the open buffer — nothing to do.
    /// Covers both "no real change happened" and this app's *own* save
    /// having triggered the very event being reconciled right now.
    Unchanged,
    /// No local unsaved edits — safe to reload the buffer from disk with
    /// no risk of losing anything.
    ReloadTransparently,
    /// Local unsaved edits exist *and* the file changed on disk under
    /// them — needs a user decision (the conflict banner), not a silently
    /// picked side.
    Conflict,
    /// The file no longer exists on disk.
    Deleted,
}

/// Decides what to do about an external change to a tab whose buffer
/// currently holds `buffer_content`, dirty or not (`is_dirty`).
/// `disk_content` is `None` for a deletion (or a file that became
/// unreadable) — there's nothing on disk to reconcile against either way,
/// so both collapse to the same `Deleted` outcome.
///
/// Comparing *content*, not a save timestamp, is what makes this
/// naturally immune to reacting to this app's own `Document::save()` —
/// the very disk write that triggered the filesystem event being
/// reconciled here — without needing a separate "was this my own recent
/// save" timer to filter it out: a self-triggered event's `disk_content`
/// is, by definition, identical to `buffer_content`, since `save()` just
/// wrote exactly that.
pub fn reconcile(is_dirty: bool, buffer_content: &str, disk_content: Option<&str>) -> ReconcileOutcome {
    match disk_content {
        None => ReconcileOutcome::Deleted,
        Some(disk) if disk == buffer_content => ReconcileOutcome::Unchanged,
        Some(_) if !is_dirty => ReconcileOutcome::ReloadTransparently,
        Some(_) => ReconcileOutcome::Conflict,
    }
}

/// Every directory that needs to be watched for `open_paths` — each open
/// tab's own parent directory, deduped. `notify` watches directories, not
/// individual files; watching each tab's specific parent (rather than the
/// whole project tree) keeps the watch set to exactly the surface that's
/// actually relevant, no matter how large the rest of the project is —
/// and, unlike the project tree, needs no `.git`/`target`/`node_modules`
/// skip-list of its own, since it's never watching anything but a handful
/// of directories actual open tabs live in.
pub fn watched_dirs_for<'a>(open_paths: impl Iterator<Item = &'a Path>) -> HashSet<PathBuf> {
    open_paths.filter_map(|p| p.parent().map(Path::to_path_buf)).collect()
}

#[cfg(test)]
#[path = "file_watch_test.rs"]
mod file_watch_test;
