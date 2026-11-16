//! Downloads and installs a contributed profiler (`PLAN.md` Track 26 Phase 1 —
//! "Profiler integration") into a per-user cache directory, so profiling
//! doesn't require the user to have found and installed it manually.
//!
//! Which profiler there is, which release, its URL, its pinned SHA-256, how it
//! is packed and where its launcher sits inside are the extension's
//! (`fg_extension::ProfilerInstall`); this module downloads, verifies and
//! extracts that, mirroring `tool_manager`'s shape (pinned checksum,
//! `directories`-provided cache, `Receiver`-polled background thread).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use directories::ProjectDirs;
use fg_extension::{ArchiveKind, ProfilerHandle, ProfilerInstall};

/// Where a completed install landed: `launcher` is the path the extension's
/// `profiler_command` is handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The profiler's display name, for the "installed" message.
    pub name: String,
    pub version: String,
    pub launcher: PathBuf,
}

pub type InstallResult = Result<Installed, String>;

fn cache_dir(profiler_id: &str) -> Result<PathBuf, String> {
    ProjectDirs::from("", "", "foxgarden")
        .map(|dirs| dirs.cache_dir().join(profiler_id))
        .ok_or_else(|| "couldn't determine a cache directory for this platform".to_string())
}

/// The launcher of an already-extracted copy of the pinned release, if present
/// — lets a caller skip the download when the tool's cached (an "install once,
/// profile many times" flow). `None` too when the profiler has no build for
/// this platform.
pub fn installed_launcher(profiler: &ProfilerHandle) -> Option<PathBuf> {
    let install = profiler.contribution.install.as_ref()?;
    let launcher = cache_dir(profiler.id()).ok()?.join(&install.dir_name).join(&install.launcher);
    launcher.exists().then_some(launcher)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;

    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Refuses `downloaded` unless it hashes to exactly the pinned SHA-256 — same
/// integrity rationale as `tool_manager`'s own `verify_download`: the archive
/// becomes native code this app then executes and injects into a live process,
/// so a substituted artifact is arbitrary code execution, made a loud refusable
/// failure here rather than a silent one.
fn verify_download(name: &str, install: &ProfilerInstall, downloaded: &[u8]) -> Result<(), String> {
    let actual = sha256_hex(downloaded);
    if actual != install.sha256 {
        return Err(format!(
            "{name}'s download doesn't match its expected checksum (expected {}, got {actual}) — refusing to install it",
            install.sha256
        ));
    }
    Ok(())
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url).call().map_err(|e| format!("download failed: {e}"))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("download failed: {e}"))?;
    Ok(bytes)
}

/// Extracts `downloaded` under `dir` and returns the launcher inside the
/// archive's own top-level directory — read back from the actual extracted
/// tree rather than trusted, so an upstream layout change fails loudly here (a
/// missing launcher) instead of silently pointing at a path that doesn't exist.
fn extract_and_locate(install: &ProfilerInstall, dir: &Path, downloaded: &[u8]) -> Result<PathBuf, String> {
    match install.archive {
        ArchiveKind::Zip => {
            let mut archive =
                zip::ZipArchive::new(std::io::Cursor::new(downloaded)).map_err(|e| format!("not a valid zip: {e}"))?;
            archive.extract(dir).map_err(|e| format!("failed to extract archive: {e}"))?;
        }
        ArchiveKind::TarGz => {
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(downloaded));
            archive.unpack(dir).map_err(|e| format!("failed to extract archive: {e}"))?;
        }
    }

    let launcher = dir.join(&install.dir_name).join(&install.launcher);
    if !launcher.exists() {
        return Err(format!("expected the profiler's launcher at {} but it doesn't exist", launcher.display()));
    }
    Ok(launcher)
}

/// Downloads, verifies, and extracts the pinned release into the cache
/// directory, returning where its launcher landed. Re-running for an
/// already-installed copy re-downloads and overwrites — simple, and cheap given
/// this only runs on explicit user action, never automatically.
fn install_sync(profiler: &ProfilerHandle) -> InstallResult {
    let install = profiler
        .contribution
        .install
        .as_ref()
        .ok_or_else(|| format!("{} has no build for this platform", profiler.display_name()))?;
    let dir = cache_dir(profiler.id())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let downloaded = download(&install.download_url)?;
    verify_download(profiler.display_name(), install, &downloaded)?;
    let launcher = extract_and_locate(install, &dir, &downloaded)?;

    Ok(Installed {
        name: profiler.display_name().to_string(),
        version: install.version.clone(),
        launcher,
    })
}

/// Background-install state — one in-flight install at a time, the same
/// `Receiver`-polled-from-`show` shape `tool_manager::ToolManagerState` and
/// every other long-running action in this app uses so a download (plus its
/// extraction) never blocks the UI thread.
#[derive(Default)]
pub struct ProfilerManagerState {
    install: Option<Receiver<InstallResult>>,
}

impl ProfilerManagerState {
    pub fn installing(&self) -> bool {
        self.install.is_some()
    }

    pub fn install(&mut self, profiler: ProfilerHandle) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(install_sync(&profiler));
        });
        self.install = Some(rx);
    }

    /// Returns the install result once its background thread finishes, and
    /// clears the slot — called once per frame from the profiler UI.
    pub fn poll_install(&mut self) -> Option<InstallResult> {
        let rx = self.install.as_ref()?;
        match rx.try_recv() {
            Ok(result) => {
                self.install = None;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.install = None;
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "profiler_manager_test.rs"]
mod profiler_manager_test;
