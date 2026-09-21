//! Crash-safe file writing: write a sibling temp file, flush it to the
//! device, then `rename` it over the destination.
//!
//! `std::fs::write` truncates the destination *first* and only then starts
//! writing, so a crash, a kill, or a power loss in between leaves the file
//! truncated or empty — for an editor, that means the user's source file is
//! destroyed by the very operation meant to preserve it. `rename` within a
//! directory is atomic on every platform this app targets: a reader either
//! sees the whole old file or the whole new one, never a half-written one.
//!
//! The temp file is deliberately a *sibling* (same directory), not one in
//! the system temp dir: `rename` across filesystems fails, and a project on
//! a different mount than `/tmp` is the normal case, not the exotic one.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Name of the temp file `write_atomically` writes before renaming — dot-
/// prefixed (hidden, and skipped by the project tree's own conventions),
/// carrying the pid and a nanosecond timestamp so two processes (or two
/// saves racing within one) never pick the same name and clobber each
/// other's in-flight write.
fn temp_path(destination: &Path) -> PathBuf {
    let dir = destination.parent().unwrap_or_else(|| Path::new("."));
    let name = destination
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    dir.join(format!(".{name}.foxgarden-{}-{nanos}.tmp", std::process::id()))
}

/// Writes `contents` to `path` atomically: no partially-written state is
/// ever observable at `path`, and an interrupted write leaves the previous
/// contents fully intact.
///
/// A `path` that is a symlink is resolved first, so saving an editor tab
/// whose file is symlinked writes *through* the link the way every other
/// editor does, rather than replacing the link itself with a regular file.
/// The temp file inherits the destination's own permissions when it already
/// exists (a `chmod +x` script stays executable across a save); a brand-new
/// file just gets the process default, exactly as `std::fs::write` would.
pub fn write_atomically(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let destination = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let temp = temp_path(&destination);

    let write_result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(contents)?;
        // Flushing the *data* before the rename is what makes the guarantee
        // real rather than nominal: without it, a crash right after the
        // rename can leave the renamed-into-place file with its metadata
        // committed but its blocks not, i.e. the same truncated file this
        // whole module exists to prevent.
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }

    if let Ok(metadata) = std::fs::metadata(&destination)
        && let Err(error) = std::fs::set_permissions(&temp, metadata.permissions())
    {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }

    match std::fs::rename(&temp, &destination) {
        Ok(()) => {
            sync_directory(&destination);
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// Flushes the *directory entry* the rename just created, so the rename
/// itself (not just the file's data) survives a power loss. Best-effort and
/// deliberately not error-returning: the write has already succeeded by
/// this point, and some platforms/filesystems (Windows, notably) don't
/// support opening a directory for this at all, which is not a save
/// failure.
#[cfg(unix)]
fn sync_directory(destination: &Path) {
    if let Some(dir) = destination.parent()
        && let Ok(handle) = std::fs::File::open(dir)
    {
        let _ = handle.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_directory(_destination: &Path) {}

#[cfg(test)]
#[path = "atomic_file_test.rs"]
mod atomic_file_test;
