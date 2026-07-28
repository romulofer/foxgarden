//! Downloads and installs the external static-analysis tool binaries
//! (Checkstyle, PMD, SpotBugs — `PLAN.md` Track 5) into a per-user cache
//! directory, so Settings > External Tools doesn't require the user to
//! have already found and installed them manually. All three are plain-
//! Java releases (no OS-specific asset) fetched straight from each
//! project's own GitHub releases.
//!
//! Every version/URL/archive-layout claim in this file was verified
//! against a real download this session, not assumed — see
//! `Tool::recommended_version`'s own doc comment for the one case (a real
//! `UnsupportedClassVersionError` against a genuine Java 17 JVM) where
//! guessing "just use latest" would have been a real, silent-until-runtime
//! bug.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use directories::ProjectDirs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    Checkstyle,
    Pmd,
    SpotBugs,
}

impl Tool {
    pub fn display_name(self) -> &'static str {
        match self {
            Tool::Checkstyle => "Checkstyle",
            Tool::Pmd => "PMD",
            Tool::SpotBugs => "SpotBugs",
        }
    }

    fn github_repo(self) -> &'static str {
        match self {
            Tool::Checkstyle => "checkstyle/checkstyle",
            Tool::Pmd => "pmd/pmd",
            Tool::SpotBugs => "spotbugs/spotbugs",
        }
    }

    /// The version this tool manager installs — a specific, verified-
    /// compatible release, not whatever GitHub calls "latest" at fetch
    /// time. Checkstyle in particular needs this pin: its newest major
    /// line (13.x, the real `releases/latest` response as of this
    /// session) requires a newer JDK than many real installs have —
    /// running it produced a real `UnsupportedClassVersionError` against
    /// a genuine Java 17 JVM, not a hypothetical. 10.26.1 is the newest
    /// Checkstyle release confirmed (by actually running it) to still work
    /// under Java 17. PMD 7.26.0 and SpotBugs 4.10.3 were both verified
    /// working under the same JVM and don't have this problem, but are
    /// pinned too so "the version this installs" is reproducible for every
    /// tool, not a moving target for two of three and pinned for the
    /// third. `check_latest` separately surfaces whatever GitHub's own
    /// `releases/latest` says, for the user to opt into if they want it.
    pub fn recommended_version(self) -> &'static str {
        match self {
            Tool::Checkstyle => "10.26.1",
            Tool::Pmd => "7.26.0",
            Tool::SpotBugs => "4.10.3",
        }
    }

    /// GitHub release tag for `version` — real tag shapes, verified
    /// against each repo's own `releases/latest` response, not a guessed
    /// convention: Checkstyle prefixes with `checkstyle-`, PMD's tags live
    /// under a `pmd_releases/` path segment (its release process spans
    /// more than one Maven-published artifact), SpotBugs uses a bare
    /// version.
    fn tag_for(self, version: &str) -> String {
        match self {
            Tool::Checkstyle => format!("checkstyle-{version}"),
            Tool::Pmd => format!("pmd_releases/{version}"),
            Tool::SpotBugs => version.to_string(),
        }
    }

    /// The one release asset this tool manager downloads for `version`.
    /// Checkstyle ships a single self-contained jar; PMD/SpotBugs each
    /// ship a zip whose root extracts to a versioned directory holding a
    /// `bin/<script>` launcher — real asset names, read off real release
    /// listings, not a guessed naming convention.
    fn asset_file_name(self, version: &str) -> String {
        match self {
            Tool::Checkstyle => format!("checkstyle-{version}-all.jar"),
            Tool::Pmd => format!("pmd-dist-{version}-bin.zip"),
            Tool::SpotBugs => format!("spotbugs-{version}.zip"),
        }
    }

    fn download_url(self, version: &str) -> String {
        format!(
            "https://github.com/{}/releases/download/{}/{}",
            self.github_repo(),
            self.tag_for(version),
            self.asset_file_name(version)
        )
    }

    /// GitHub API URL for this tool's latest release — used only by
    /// `check_latest`, never by `install` (see `recommended_version`'s own
    /// doc comment on why install stays pinned).
    fn latest_release_api_url(self) -> String {
        format!("https://api.github.com/repos/{}/releases/latest", self.github_repo())
    }

    /// The `bin/<script>` launcher name inside PMD's/SpotBugs' own
    /// extracted directory. Checkstyle has no launcher script (it's a bare
    /// jar, invoked via `java -jar`) — never called for it.
    fn launcher_script_name(self) -> &'static str {
        match self {
            Tool::Pmd => "pmd",
            Tool::SpotBugs => "fb",
            Tool::Checkstyle => unreachable!("Checkstyle has no zip archive/launcher to locate"),
        }
    }

    /// A substring of the extracted top-level directory's own name, used
    /// to find it without hard-coding "pmd-bin-<version>" style naming
    /// this code would otherwise have to keep in sync with each project's
    /// own archive-layout convention.
    fn extracted_dir_hint(self) -> &'static str {
        match self {
            Tool::Pmd => "pmd",
            Tool::SpotBugs => "spotbugs",
            Tool::Checkstyle => unreachable!("Checkstyle has no zip archive to extract"),
        }
    }
}

/// Where a completed install landed: `binary` is what `ExternalToolPaths`'
/// own binary-path field should be set to (a jar path for Checkstyle,
/// invoked via `java -jar` — see `run_tool_binary`; a `bin/<script>`
/// launcher path for PMD/SpotBugs, invoked directly); `default_config` is
/// the tool's own bundled classpath-resource ruleset reference (verified
/// working directly against the downloaded jar/archive, not just an
/// already-installed system package) — `None` for SpotBugs, which has no
/// ruleset-file concept the way Checkstyle/PMD do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub tool: Tool,
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

/// Extracts `downloaded` (a zip's raw bytes) under `dir`, then locates the
/// `bin/<script>` launcher inside whatever versioned top-level directory
/// the archive itself contains — read back from the actual extracted tree
/// (via `extracted_dir_hint`) rather than string-building the expected
/// name, so an upstream archive-layout change fails loudly here (a missing
/// launcher) instead of silently pointing at the wrong path.
fn extract_zip_and_locate_launcher(tool: Tool, dir: &Path, downloaded: &[u8]) -> Result<PathBuf, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(downloaded)).map_err(|e| format!("not a valid zip: {e}"))?;
    archive.extract(dir).map_err(|e| format!("failed to extract archive: {e}"))?;

    let hint = tool.extracted_dir_hint();
    let top_level = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok())
        .find(|entry| {
            entry.file_type().is_ok_and(|t| t.is_dir()) && entry.file_name().to_string_lossy().contains(hint)
        })
        .ok_or_else(|| format!("couldn't find {}'s extracted directory under {}", tool.display_name(), dir.display()))?
        .path();

    let script = top_level.join("bin").join(tool.launcher_script_name());
    if !script.exists() {
        return Err(format!("expected launcher at {} but it doesn't exist", script.display()));
    }
    Ok(script)
}

/// Downloads and installs `tool`'s `recommended_version` into the cache
/// directory, returning where it landed. Re-running this for an
/// already-installed tool re-downloads and overwrites it — simple, and
/// cheap enough given this only ever runs on explicit user action (an
/// "Install"/"Reinstall" button click), never automatically.
fn install_sync(tool: Tool) -> InstallResult {
    let version = tool.recommended_version().to_string();
    let dir = cache_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let downloaded = download(&tool.download_url(&version))?;

    let binary = match tool {
        Tool::Checkstyle => {
            let dest = dir.join(tool.asset_file_name(&version));
            std::fs::write(&dest, &downloaded).map_err(|e| e.to_string())?;
            dest
        }
        Tool::Pmd | Tool::SpotBugs => extract_zip_and_locate_launcher(tool, &dir, &downloaded)?,
    };

    let default_config = match tool {
        Tool::Checkstyle => Some("/sun_checks.xml".to_string()),
        Tool::Pmd => Some("rulesets/java/quickstart.xml".to_string()),
        Tool::SpotBugs => None,
    };

    Ok(Installed { tool, version, binary, default_config })
}

/// Fetches `tool`'s current `releases/latest` tag from the GitHub API —
/// separate from `install_sync`, which always installs `recommended_version`
/// regardless of what this returns (see that function's own doc comment).
/// GitHub's API responds with plain JSON; only the one `tag_name` field is
/// read, via `serde_json::Value` rather than a full typed response struct
/// this code has no other use for.
fn check_latest_sync(tool: Tool) -> LatestVersionResult {
    let mut response = ureq::get(tool.latest_release_api_url())
        .header("User-Agent", "foxgarden")
        .call()
        .map_err(|e| format!("update check failed: {e}"))?;
    let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "GitHub API response had no tag_name".to_string())?;
    // Strip Checkstyle's own "checkstyle-" tag prefix so the returned
    // string is a bare version, comparable against `recommended_version`/
    // an `Installed::version` the same way for every tool.
    Ok(tag.strip_prefix("checkstyle-").unwrap_or(tag).to_string())
}

/// Background-install/update-check state — two independent slots per tool
/// (an install and a version-check can both be in flight, e.g. checking
/// PMD while installing Checkstyle), keyed by `Tool` rather than one field
/// pair per tool: with three tools already and the same shape repeated for
/// each, a `HashMap` scales without the copy-pasted-field-set smell three
/// separate `checkstyle_install_rx`/`pmd_install_rx`/`spotbugs_install_rx`
/// fields (plus their check-latest twins) would have.
#[derive(Default)]
pub struct ToolManagerState {
    installs: std::collections::HashMap<Tool, Receiver<InstallResult>>,
    checks: std::collections::HashMap<Tool, Receiver<LatestVersionResult>>,
}

impl ToolManagerState {
    pub fn installing(&self, tool: Tool) -> bool {
        self.installs.contains_key(&tool)
    }

    pub fn checking(&self, tool: Tool) -> bool {
        self.checks.contains_key(&tool)
    }

    /// Kicks off `tool`'s install on a background thread — downloading
    /// PMD's ~70MB archive (the largest of the three) on the UI thread
    /// would freeze the whole app for however long that takes, the same
    /// "background thread + channel, polled from `show`" shape every other
    /// long-running action in this app already uses (`SpringEndpointsState`,
    /// `StaticAnalysisState`'s own scans).
    pub fn install(&mut self, tool: Tool) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(install_sync(tool));
        });
        self.installs.insert(tool, rx);
    }

    pub fn check_latest(&mut self, tool: Tool) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(check_latest_sync(tool));
        });
        self.checks.insert(tool, rx);
    }

    /// Drains every install/check that's finished since the last poll —
    /// called once per frame from `FoxGardenApp::ui`. Returns at most one
    /// result per tool per call (matching how many background threads can
    /// realistically finish between two consecutive frames), not just the
    /// first one found, so two tools installing at once don't have one's
    /// result delayed behind the other's.
    pub fn poll_installs(&mut self) -> Vec<InstallResult> {
        poll_all(&mut self.installs)
    }

    pub fn poll_checks(&mut self) -> Vec<(Tool, LatestVersionResult)> {
        let mut done = Vec::new();
        self.checks.retain(|&tool, rx| match rx.try_recv() {
            Ok(result) => {
                done.push((tool, result));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        done
    }
}

fn poll_all(slots: &mut std::collections::HashMap<Tool, Receiver<InstallResult>>) -> Vec<InstallResult> {
    let mut done = Vec::new();
    slots.retain(|_, rx| match rx.try_recv() {
        Ok(result) => {
            done.push(result);
            false
        }
        Err(TryRecvError::Empty) => true,
        Err(TryRecvError::Disconnected) => false,
    });
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_url_matches_the_real_asset_layout_verified_this_session() {
        assert_eq!(
            Tool::Checkstyle.download_url("10.26.1"),
            "https://github.com/checkstyle/checkstyle/releases/download/checkstyle-10.26.1/checkstyle-10.26.1-all.jar"
        );
        assert_eq!(
            Tool::Pmd.download_url("7.26.0"),
            "https://github.com/pmd/pmd/releases/download/pmd_releases/7.26.0/pmd-dist-7.26.0-bin.zip"
        );
        assert_eq!(
            Tool::SpotBugs.download_url("4.10.3"),
            "https://github.com/spotbugs/spotbugs/releases/download/4.10.3/spotbugs-4.10.3.zip"
        );
    }

    #[test]
    fn latest_release_api_url_targets_each_tool_s_own_repo() {
        assert_eq!(
            Tool::Pmd.latest_release_api_url(),
            "https://api.github.com/repos/pmd/pmd/releases/latest"
        );
    }

    #[test]
    fn extract_zip_and_locate_launcher_finds_a_real_pmd_archive_s_launcher() {
        // A minimal but real zip, built in-memory, shaped exactly like
        // PMD's own release archive: a versioned top-level directory
        // holding `bin/pmd`.
        let dir = test_support::tempdir();
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            writer.start_file("pmd-bin-7.26.0/bin/pmd", options).unwrap();
            std::io::Write::write_all(&mut writer, b"#!/bin/sh\necho fake pmd\n").unwrap();
            writer.finish().unwrap();
        }

        let launcher = extract_zip_and_locate_launcher(Tool::Pmd, dir.path(), &buf).expect("locates the launcher");
        assert_eq!(launcher, dir.path().join("pmd-bin-7.26.0").join("bin").join("pmd"));
        assert!(launcher.exists());
    }

    #[test]
    fn extract_zip_and_locate_launcher_errors_when_no_matching_top_level_dir_exists() {
        let dir = test_support::tempdir();
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            writer.start_file("unrelated/file.txt", zip::write::SimpleFileOptions::default()).unwrap();
            writer.finish().unwrap();
        }

        assert!(extract_zip_and_locate_launcher(Tool::Pmd, dir.path(), &buf).is_err());
    }

    #[test]
    fn tool_manager_state_starts_with_nothing_installing_or_checking() {
        let state = ToolManagerState::default();
        for tool in [Tool::Checkstyle, Tool::Pmd, Tool::SpotBugs] {
            assert!(!state.installing(tool));
            assert!(!state.checking(tool));
        }
    }
}
