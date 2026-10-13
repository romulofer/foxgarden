//! Installs and updates the language servers Settings > Language
//! Servers… configures (`PLAN.md` Track 20) into a per-user cache directory
//! — so none has to be found, downloaded and unpacked by hand before
//! FoxGarden can use it.
//!
//! Which servers there are, how their archives are packed, where each
//! launcher sits inside and what the machine needs to run them is the
//! contributing extension's (`fg_extension::ServerInstall`); the archives'
//! bytes ride in the extension too, so an install needs no network, `git`
//! or build. This module only extracts them into the cache and runs the
//! update check against GitHub.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use directories::ProjectDirs;
use fg_extension::{ArchiveKind, HostRuntime, LatestVersionSource, ServerInstallHandle};

/// Where a completed install landed. `binary` is exactly what the matching
/// `LspSettings` binary field should be set to — a launcher script that
/// takes no arguments, which is what `lsp_state`'s own `LspSession::spawn`
/// call expects (jdt.ls' `bin/jdtls` wrapper defaults its `-data` workspace
/// directory from the process' working directory, which `lsp_state` already
/// sets to the project root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The registered id of the server that was installed.
    pub server: String,
    pub version: String,
    pub binary: PathBuf,
}

pub type InstallResult = Result<Installed, String>;
pub type LatestVersionResult = Result<String, String>;

/// One step of a running install, or its outcome. A single channel rather
/// than a separate progress channel per job: it keeps "the last thing that
/// happened" and "it finished" in one strictly-ordered stream, so a
/// completion can never be observed before the progress line that led to
/// it.
enum InstallEvent {
    Progress(String),
    Finished(InstallResult),
}

/// `pub(crate)` for `lsp_state`'s own real-server tests, which look here
/// for whatever this module last installed rather than hard-coding a
/// second copy of the same per-platform path.
pub(crate) fn cache_dir() -> Result<PathBuf, String> {
    ProjectDirs::from("", "", "foxgarden")
        .map(|dirs| dirs.cache_dir().join("lsp-servers"))
        .ok_or_else(|| "couldn't determine a cache directory for this platform".to_string())
}

/// This server's own directory name — one per server *per version*, so
/// installing an update never half-overwrites a working install: the new
/// version extracts somewhere else entirely, and the settings' binary path
/// only moves once it succeeds.
fn install_dir(server_id: &str, version: &str) -> Result<PathBuf, String> {
    Ok(cache_dir()?.join(format!("{server_id}-{version}")))
}

/// Marks `path` executable on Unix. Both launchers *should* already carry
/// the bit (Gradle's `distZip` stores Unix permissions; Maven's assembly
/// sets them), but an archive repacked anywhere along the way loses it
/// silently, and the failure that produces — `Permission denied` at spawn
/// time, several minutes after the install "succeeded" — is much harder to
/// read than just setting it here.
fn ensure_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).map_err(|e| e.to_string())?.permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        std::fs::set_permissions(path, permissions).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Catches the one way `include_bytes!` can embed something that isn't the
/// archive at all: the vendored archives are tracked via Git LFS, and a clone
/// made without `git-lfs` installed leaves a ~130-byte text *pointer* at
/// each of those paths instead of the real bytes. That compiles perfectly
/// happily, and only fails at install time — as an unreadable `failed to
/// iterate over archive` out of the tar/zip decoder — so name the actual
/// problem here instead.
fn reject_lfs_pointer(bytes: &[u8]) -> Result<(), String> {
    if bytes.starts_with(b"version https://git-lfs.github.com/spec/v1") {
        return Err(
            "this FoxGarden build embedded a Git LFS pointer instead of the server's archive — the \
             vendored files weren't fetched. Install git-lfs, run `git lfs pull`, and rebuild"
                .to_string(),
        );
    }
    Ok(())
}

/// Extracts a `.zip` archive's bytes (Kotlin Language Server's own
/// `server.zip`, vendored verbatim) into `dir`.
fn extract_zip(bytes: &[u8], dir: &Path) -> Result<(), String> {
    reject_lfs_pointer(bytes)?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a valid zip: {e}"))?;
    archive
        .extract(dir)
        .map_err(|e| format!("failed to extract archive: {e}"))
}

/// Extracts a gzipped tarball's bytes (jdt.ls' own milestone distribution,
/// vendored verbatim) into `dir`.
fn extract_tar_gz(bytes: &[u8], dir: &Path) -> Result<(), String> {
    reject_lfs_pointer(bytes)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    archive
        .unpack(dir)
        .map_err(|e| format!("failed to extract archive: {e}"))
}

/// The install a server should run on: the first one of its host runtime the
/// extension discovers on this machine that satisfies the server's minimum
/// version, or `None` if there is none. Runs on a background thread
/// (`LspManagerState::detect_runtime_home`) — discovery spawns a process per
/// installed runtime, far too slow for the UI thread.
fn detect_runtime_home(host: &HostRuntime, runtimes: &[fg_extension::RuntimeHandle]) -> Option<PathBuf> {
    runtimes
        .iter()
        .filter(|runtime| runtime.id() == host.runtime_id)
        .flat_map(|runtime| runtime.discover())
        .find(|install| install.major_version >= host.minimum_major_version)
        .map(|install| install.home)
}

/// Extracts the server's bundled archive into `dir` and returns the
/// launcher's path — the only install path there is: the bytes are embedded
/// in the FoxGarden binary itself, by the extension that contributes the
/// server.
fn install_bundled(server: &ServerInstallHandle, dir: &Path, report: &dyn Fn(String)) -> Result<PathBuf, String> {
    report("Checking prerequisites…".to_string());
    server.check_prerequisites()?;

    report(format!("Extracting bundled {}…", server.display_name()));
    let archive = server
        .archive()
        .ok_or_else(|| format!("{} carries no archive for this server", server.display_name()))?;
    match server.install.archive {
        ArchiveKind::TarGz => extract_tar_gz(archive, dir)?,
        ArchiveKind::Zip => extract_zip(archive, dir)?,
    }

    let launcher = dir.join(server.install.launcher.iter().collect::<PathBuf>());
    if !launcher.exists() {
        return Err(format!(
            "{}'s bundled archive didn't contain the expected launcher at {}",
            server.display_name(),
            launcher.display()
        ));
    }
    ensure_executable(&launcher)?;
    Ok(launcher)
}

/// Installs one specific `version` of `server`, reporting each step through
/// `report`. Runs entirely on a background thread (see
/// `LspManagerState::install`) so extraction never blocks the UI thread.
/// Only the bundled version can actually be installed.
fn install_sync(server: &ServerInstallHandle, version: &str, report: &dyn Fn(String)) -> InstallResult {
    if version != server.install.version {
        return Err(format!(
            "only the version bundled with this FoxGarden build ({}) can be installed — {version} isn't vendored",
            server.install.version
        ));
    }

    let dir = install_dir(server.id(), version)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;

    let binary = install_bundled(server, &dir, report)?;

    Ok(Installed {
        server: server.id().to_string(),
        version: version.to_string(),
        binary,
    })
}

/// Where `check_latest_sync` looks. A repository that cuts real GitHub
/// releases answers with the latest one; one that only tags has its newest
/// version come from the tag list.
fn latest_version_api_url(server: &ServerInstallHandle) -> String {
    let repo = &server.install.github_repo;
    match server.install.latest_version_source {
        LatestVersionSource::Tags => format!("https://api.github.com/repos/{repo}/tags?per_page=10"),
        LatestVersionSource::Releases => format!("https://api.github.com/repos/{repo}/releases/latest"),
    }
}

/// Reads the server's newest published version off the GitHub API. Two
/// response shapes (see `latest_version_api_url`): a release object with a
/// `tag_name`, or a tag array to take the first entry of. Either way the
/// returned string is a bare version, directly comparable against
/// `ServerInstall::version` and an `Installed::version`.
fn check_latest_sync(server: &ServerInstallHandle) -> LatestVersionResult {
    let mut response = ureq::get(latest_version_api_url(server))
        .header("User-Agent", "foxgarden")
        .call()
        .map_err(|e| format!("update check failed: {e}"))?;
    let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = latest_tag_from_response(&json).ok_or_else(|| "GitHub's response had no usable tag".to_string())?;
    Ok(tag)
}

/// The pure decode half of `check_latest_sync`, split out so both response
/// shapes are testable without a network call. A tag array is scanned (not
/// just indexed at 0) for the first entry that looks like a version, since
/// a repo's tag list can legitimately lead with something else entirely.
fn latest_tag_from_response(json: &serde_json::Value) -> Option<String> {
    let raw = match json {
        serde_json::Value::Array(tags) => tags
            .iter()
            .filter_map(|tag| tag.get("name").and_then(serde_json::Value::as_str))
            .find(|name| name.trim_start_matches('v').starts_with(|c: char| c.is_ascii_digit()))?,
        _ => json.get("tag_name").and_then(serde_json::Value::as_str)?,
    };
    Some(raw.trim_start_matches('v').to_string())
}

/// One running install: the event stream its background thread writes to,
/// plus the most recent progress line off it, so the dialog can show what
/// a multi-minute build is actually doing instead of an unchanging
/// "Installing…".
struct InstallJob {
    rx: Receiver<InstallEvent>,
    status: String,
}

/// Background install/update-check state — the same two-independent-slots-
/// per-item shape `tool_manager::ToolManagerState` already uses (an install
/// and a version check can be in flight at once, for either server), keyed
/// by server id.
#[derive(Default)]
pub struct LspManagerState {
    installs: HashMap<String, InstallJob>,
    checks: HashMap<String, Receiver<LatestVersionResult>>,
    /// Running scans for a server's host runtime (`detect_runtime_home`),
    /// keyed by server id — at most one per server.
    runtime_detections: HashMap<String, Receiver<Option<PathBuf>>>,
}

impl LspManagerState {
    pub fn installing(&self, server_id: &str) -> bool {
        self.installs.contains_key(server_id)
    }

    pub fn checking(&self, server_id: &str) -> bool {
        self.checks.contains_key(server_id)
    }

    /// The latest progress line for a running install, for the dialog to
    /// display — `None` when nothing is running for `server`.
    pub fn status(&self, server_id: &str) -> Option<&str> {
        self.installs.get(server_id).map(|job| job.status.as_str())
    }

    /// Whether any install is still running — the app's own update loop
    /// uses this to keep requesting repaints, since progress lands on a
    /// background thread with no input event to piggyback on.
    pub fn busy(&self) -> bool {
        !self.installs.is_empty() || !self.checks.is_empty() || !self.runtime_detections.is_empty()
    }

    /// Starts scanning this machine for an install `server` can run on, on a
    /// background thread — the dialog fires this when it opens with an empty
    /// runtime home, so the field arrives already filled in rather than
    /// leaving the user to go find a path by hand. A scan already in flight
    /// for `server_id` is left alone.
    pub fn detect_runtime_home(
        &mut self,
        server_id: &str,
        host: HostRuntime,
        runtimes: Vec<fg_extension::RuntimeHandle>,
    ) {
        if self.runtime_detections.contains_key(server_id) {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(detect_runtime_home(&host, &runtimes));
        });
        self.runtime_detections.insert(server_id.to_string(), rx);
    }

    pub fn detecting_runtime_home(&self, server_id: &str) -> bool {
        self.runtime_detections.contains_key(server_id)
    }

    /// Whether any runtime scan is running, for the status bar.
    pub fn detecting_any_runtime_home(&self) -> bool {
        !self.runtime_detections.is_empty()
    }

    /// The finished scans' answers, once each: `(server id, Some(home))` when
    /// a qualifying install was found, `(server id, None)` when the machine
    /// has none. Called once a frame alongside the other polls.
    pub fn poll_runtime_detections(&mut self) -> Vec<(String, Option<PathBuf>)> {
        let mut finished = Vec::new();
        self.runtime_detections.retain(|server_id, detection| match detection.try_recv() {
            Ok(found) => {
                finished.push((server_id.clone(), found));
                false
            }
            Err(TryRecvError::Empty) => true,
            // The scanning thread died without answering — nothing left to
            // wait for, and no install to report either.
            Err(TryRecvError::Disconnected) => {
                finished.push((server_id.clone(), None));
                false
            }
        });
        finished
    }

    /// Starts installing `version` of `server` on a background thread.
    /// A second click while one is already running is ignored rather than
    /// starting a competing job over the same directory.
    pub fn install(&mut self, server: ServerInstallHandle, version: String) {
        if self.installing(server.id()) {
            return;
        }
        let (tx, rx) = channel();
        let status = format!("Starting {} install…", server.display_name());
        let id = server.id().to_string();
        std::thread::spawn(move || {
            let report_tx = tx.clone();
            let report = move |message: String| {
                let _ = report_tx.send(InstallEvent::Progress(message));
            };
            let result = install_sync(&server, &version, &report);
            let _ = tx.send(InstallEvent::Finished(result));
        });
        self.installs.insert(id, InstallJob { rx, status });
    }

    pub fn check_latest(&mut self, server: ServerInstallHandle) {
        if self.checking(server.id()) {
            return;
        }
        let (tx, rx) = channel();
        let id = server.id().to_string();
        std::thread::spawn(move || {
            let _ = tx.send(check_latest_sync(&server));
        });
        self.checks.insert(id, rx);
    }

    /// Drains every install's event stream: progress lines update that
    /// job's status in place, and a finished job is removed and its result
    /// returned. Called once a frame.
    pub fn poll_installs(&mut self) -> Vec<(String, InstallResult)> {
        let mut done = Vec::new();
        self.installs.retain(|server, job| {
            loop {
                match job.rx.try_recv() {
                    Ok(InstallEvent::Progress(message)) => job.status = message,
                    Ok(InstallEvent::Finished(result)) => {
                        done.push((server.clone(), result));
                        return false;
                    }
                    Err(TryRecvError::Empty) => return true,
                    // The worker thread died without sending a result —
                    // there's nothing left to wait for, and no outcome to
                    // report either.
                    Err(TryRecvError::Disconnected) => return false,
                }
            }
        });
        done
    }

    pub fn poll_checks(&mut self) -> Vec<(String, LatestVersionResult)> {
        let mut done = Vec::new();
        self.checks.retain(|server, rx| match rx.try_recv() {
            Ok(result) => {
                done.push((server.clone(), result));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        done
    }
}

#[cfg(test)]
#[path = "lsp_manager_test.rs"]
mod lsp_manager_test;
