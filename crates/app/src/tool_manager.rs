//! Downloads and installs the external static-analysis tools an extension
//! contributes (`fg_extension::AnalyzerInstall`) into a per-user cache
//! directory, so Settings > External Tools doesn't require the user to
//! have already found and installed them manually.
//!
//! What to download, which version, and what it must hash to are the
//! contributing extension's: a pinned version rather than whatever the
//! vendor calls "latest" (a newer major line can need a newer JDK than the
//! user has), and a checksum because the download becomes code this app then
//! runs. This module is the mechanism: fetch, verify, unpack, locate the
//! launcher, and do it off the UI thread.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use directories::ProjectDirs;
use fg_extension::{AnalyzerInstall, AnalyzerPackaging, LatestRelease};

/// Where a completed install landed: `binary` is what the analyzer's
/// binary-path setting should be set to (a bare jar, which the extension
/// launches itself, or a launcher script); `default_config` is the tool's
/// own bundled default configuration, when it ships one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub analyzer_id: String,
    pub version: String,
    pub binary: PathBuf,
    pub default_config: Option<String>,
}

pub type InstallResult = Result<Installed, String>;
pub type LatestVersionResult = Result<String, String>;

fn cache_dir() -> Result<PathBuf, String> {
    ProjectDirs::from("", "", "foxgarden")
        .map(|dirs| dirs.cache_dir().join("tools"))
        .ok_or_else(|| "couldn't determine a cache directory for this platform".to_string())
}

/// Lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;

    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Refuses `downloaded` unless it hashes to exactly what the extension
/// pinned for `install`.
///
/// Pinning the version alone isn't integrity: a release asset on GitHub can
/// be deleted and re-uploaded under the same tag, and the download itself is
/// a plain HTTPS GET whose only guarantee is "someone with a valid
/// certificate for this host served me some bytes". Since these downloads
/// become *executable* code this app then runs, a swapped artifact is
/// arbitrary code execution on the user's machine. Comparing against a hash
/// shipped with the extension makes a substituted asset a loud, refusable
/// failure instead of a silent one.
fn verify_download(display_name: &str, install: &AnalyzerInstall, downloaded: &[u8]) -> Result<(), String> {
    let actual = sha256_hex(downloaded);
    if actual != install.sha256.to_ascii_lowercase() {
        return Err(format!(
            "{display_name}'s download doesn't match its expected checksum (expected {}, got {actual}) — refusing to \
             install it",
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

/// Extracts `downloaded` (a zip's raw bytes) under `dir`, then locates
/// `launcher` inside whatever versioned top-level directory the archive
/// itself contains — read back from the actual extracted tree (via
/// `dir_hint`) rather than string-building the expected name, so an upstream
/// archive-layout change fails loudly here (a missing launcher) instead of
/// silently pointing at the wrong path.
fn extract_zip_and_locate_launcher(
    display_name: &str,
    dir_hint: &str,
    launcher: &str,
    dir: &Path,
    downloaded: &[u8],
) -> Result<PathBuf, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(downloaded)).map_err(|e| format!("not a valid zip: {e}"))?;
    archive
        .extract(dir)
        .map_err(|e| format!("failed to extract archive: {e}"))?;

    let top_level = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok())
        .find(|entry| {
            entry.file_type().is_ok_and(|t| t.is_dir()) && entry.file_name().to_string_lossy().contains(dir_hint)
        })
        .ok_or_else(|| format!("couldn't find {display_name}'s extracted directory under {}", dir.display()))?
        .path();

    let script = launcher.split('/').fold(top_level, |path, part| path.join(part));
    if !script.exists() {
        return Err(format!("expected launcher at {} but it doesn't exist", script.display()));
    }
    Ok(script)
}

/// Downloads and installs `install`'s pinned release into the cache
/// directory, returning where it landed. Re-running this for an
/// already-installed tool re-downloads and overwrites it — simple, and
/// cheap enough given this only ever runs on explicit user action (an
/// "Install"/"Reinstall" button click), never automatically.
fn install_sync(analyzer_id: String, display_name: String, install: AnalyzerInstall) -> InstallResult {
    let dir = cache_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let downloaded = download(&install.download_url)?;
    verify_download(&display_name, &install, &downloaded)?;

    let binary = match &install.packaging {
        AnalyzerPackaging::File { name } => {
            let dest = dir.join(name);
            std::fs::write(&dest, &downloaded).map_err(|e| e.to_string())?;
            dest
        }
        AnalyzerPackaging::Zip { dir_hint, launcher } => {
            extract_zip_and_locate_launcher(&display_name, dir_hint, launcher, &dir, &downloaded)?
        }
    };

    Ok(Installed {
        analyzer_id,
        version: install.version,
        binary,
        default_config: install.default_config,
    })
}

/// Fetches the tool's current `releases/latest` tag from the GitHub API —
/// separate from `install_sync`, which always installs the pinned version
/// regardless of what this returns. GitHub's API responds with plain JSON;
/// only the one `tag_name` field is read, via `serde_json::Value` rather
/// than a full typed response struct this code has no other use for.
fn check_latest_sync(latest: LatestRelease) -> LatestVersionResult {
    let mut response = ureq::get(&latest.api_url)
        .header("User-Agent", "foxgarden")
        .call()
        .map_err(|e| format!("update check failed: {e}"))?;
    let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "GitHub API response had no tag_name".to_string())?;
    // Strip the tool's own tag prefix so the returned string is a bare
    // version, comparable against the pinned version / an
    // `Installed::version` the same way for every tool.
    Ok(tag.strip_prefix(latest.tag_prefix.as_str()).unwrap_or(tag).to_string())
}

/// Background-install/update-check state — two independent slots per
/// analyzer (an install and a version-check can both be in flight, e.g.
/// checking PMD while installing Checkstyle), keyed by analyzer id.
#[derive(Default)]
pub struct ToolManagerState {
    installs: HashMap<String, Receiver<InstallResult>>,
    checks: HashMap<String, Receiver<LatestVersionResult>>,
}

impl ToolManagerState {
    pub fn installing(&self, analyzer_id: &str) -> bool {
        self.installs.contains_key(analyzer_id)
    }

    pub fn checking(&self, analyzer_id: &str) -> bool {
        self.checks.contains_key(analyzer_id)
    }

    /// Kicks off the install on a background thread — downloading PMD's
    /// ~70MB archive on the UI thread would freeze the whole app for however
    /// long that takes, the same "background thread + channel, polled from
    /// `show`" shape every other long-running action in this app already
    /// uses.
    pub fn install(&mut self, analyzer_id: &str, display_name: &str, install: AnalyzerInstall) {
        let (tx, rx) = channel();
        let (id, name) = (analyzer_id.to_string(), display_name.to_string());
        std::thread::spawn(move || {
            let _ = tx.send(install_sync(id, name, install));
        });
        self.installs.insert(analyzer_id.to_string(), rx);
    }

    pub fn check_latest(&mut self, analyzer_id: &str, latest: LatestRelease) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(check_latest_sync(latest));
        });
        self.checks.insert(analyzer_id.to_string(), rx);
    }

    /// Drains every install/check that's finished since the last poll —
    /// called once per frame from `FoxGardenApp::ui`. Returns at most one
    /// result per analyzer per call, not just the first one found, so two
    /// tools installing at once don't have one's result delayed behind the
    /// other's.
    ///
    /// Each result is paired with the analyzer id it belongs to — an `Ok`
    /// names its own via `Installed::analyzer_id`, but an `Err` is a bare
    /// message, so without this the caller could only report "install
    /// failed" with no way to say *which* of several concurrently-installing
    /// tools it was.
    pub fn poll_installs(&mut self) -> Vec<(String, InstallResult)> {
        poll_all(&mut self.installs)
    }

    pub fn poll_checks(&mut self) -> Vec<(String, LatestVersionResult)> {
        poll_all(&mut self.checks)
    }
}

fn poll_all<T>(slots: &mut HashMap<String, Receiver<T>>) -> Vec<(String, T)> {
    let mut done = Vec::new();
    slots.retain(|id, rx| match rx.try_recv() {
        Ok(result) => {
            done.push((id.clone(), result));
            false
        }
        Err(TryRecvError::Empty) => true,
        Err(TryRecvError::Disconnected) => false,
    });
    done
}

#[cfg(test)]
#[path = "tool_manager_test.rs"]
mod tool_manager_test;
