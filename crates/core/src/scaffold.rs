//! Writing a scaffolded project to disk. *What* files a new project needs
//! is an extension's answer (`fg_extension::Extension::scaffold_files`);
//! this is the one rule that stays the editor's: never scaffold into a
//! directory that already has something in it.

use std::path::{Path, PathBuf};

/// Writes `files` under `project_root`, creating parent directories as
/// needed. Refuses outright if `project_root` already exists and already
/// has anything in it — scaffolding into a real, non-empty directory (an
/// existing project, a home folder picked by mistake) would silently mix
/// generated files into whatever's already there; an empty or
/// not-yet-created directory is the only safe target.
pub fn write_scaffold(project_root: &Path, files: &[(PathBuf, String)]) -> Result<(), String> {
    if project_root.exists() {
        let mut entries =
            std::fs::read_dir(project_root).map_err(|e| format!("couldn't read {}: {e}", project_root.display()))?;
        if entries.next().is_some() {
            return Err(format!("{} already exists and is not empty", project_root.display()));
        }
    }

    for (relative, content) in files {
        let path = project_root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("couldn't create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, content).map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "scaffold_test.rs"]
mod scaffold_test;
