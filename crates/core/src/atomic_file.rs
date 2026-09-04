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
    let name = destination.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
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
mod tests {
    use super::*;

    #[test]
    fn writes_the_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Main.java");
        write_atomically(&path, b"class Main {}\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "class Main {}\n");
    }

    #[test]
    fn overwrites_an_existing_file_and_leaves_no_temp_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Main.java");
        std::fs::write(&path, "old").unwrap();

        write_atomically(&path, b"new").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "Main.java")
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
    }

    #[cfg(unix)]
    #[test]
    fn preserves_an_existing_file_s_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.sh");
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

        write_atomically(&path, b"#!/bin/sh\necho hi\n").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_a_symlink_instead_of_replacing_it() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.java");
        let link = dir.path().join("link.java");
        std::fs::write(&target, "old").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write_atomically(&link, b"new").unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[test]
    fn a_failed_write_leaves_the_previous_contents_intact() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be: `File::create` on the temp
        // path still succeeds, but the rename over a directory fails — the
        // closest reliably-reproducible mid-save failure.
        let path = dir.path().join("Main.java");
        std::fs::create_dir(&path).unwrap();

        assert!(write_atomically(&path, b"new").is_err());
        assert!(path.is_dir());
    }
}
