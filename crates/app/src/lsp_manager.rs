//! Installs and updates the two language servers Settings > Language
//! Servers… configures (`PLAN.md` Track 20), straight from each project's
//! own upstream GitHub repository, into a per-user cache directory — so
//! neither has to be found, downloaded and unpacked by hand before
//! FoxGarden can use it.
//!
//! The two servers need genuinely different treatment, and that difference
//! is upstream's, not a choice made here:
//!
//! * **Kotlin Language Server** (`fwcd/kotlin-language-server`) publishes a
//!   ready-to-run `server.zip` on every GitHub release. Download, extract,
//!   point at `server/bin/kotlin-language-server`. Same shape as
//!   `tool_manager`'s own PMD/SpotBugs installs.
//! * **Eclipse JDT Language Server** (`eclipse-jdtls/eclipse.jdt.ls`) does
//!   **not** publish build artifacts on GitHub at all — its releases page
//!   holds exactly one unrelated 2016 hackathon `.vsix` (verified against
//!   the real API this session), with the actual milestone tarballs living
//!   on `download.eclipse.org` instead. Installing it from its own repo
//!   therefore means building it: shallow-clone the release tag and run the
//!   project's own bundled Maven wrapper, exactly as its README documents
//!   (`JAVA_HOME=/path/to/java/21 ./mvnw clean verify`, producing
//!   `org.eclipse.jdt.ls.product/target/repository`). That build takes
//!   several minutes and needs `git` plus a JDK 21+, which is why installs
//!   report progress rather than just a spinner.
//!
//! Every URL, tag shape, archive layout and build command in this file was
//! checked against the real repositories/release assets this session rather
//! than assumed — see each function's own doc comment.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use directories::ProjectDirs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Server {
    Jdtls,
    KotlinLanguageServer,
}

/// Every server this manager knows how to install, in the order the
/// settings dialog lists them.
pub const ALL_SERVERS: [Server; 2] = [Server::Jdtls, Server::KotlinLanguageServer];

/// The JDK jdt.ls' own build requires — its README's build instructions
/// name Java 21 explicitly (`JAVA_HOME=/path/to/java/21 ./mvnw clean
/// verify`).
const JDTLS_MINIMUM_JDK: u32 = 21;

impl Server {
    pub fn display_name(self) -> &'static str {
        match self {
            Server::Jdtls => "Eclipse JDT Language Server",
            Server::KotlinLanguageServer => "Kotlin Language Server",
        }
    }

    pub fn github_repo(self) -> &'static str {
        match self {
            Server::Jdtls => "eclipse-jdtls/eclipse.jdt.ls",
            Server::KotlinLanguageServer => "fwcd/kotlin-language-server",
        }
    }

    /// The version "Install" uses when the user hasn't picked a specific
    /// one — a pinned release verified to exist this session, not whatever
    /// is newest at click time, for the same reproducibility reason
    /// `tool_manager::Tool::recommended_version` documents for the static-
    /// analysis tools. `check_latest` separately reports what upstream
    /// currently publishes, and the dialog offers *that* version as an
    /// explicit one-click update.
    pub fn recommended_version(self) -> &'static str {
        match self {
            Server::Jdtls => "1.60.0",
            Server::KotlinLanguageServer => "1.3.13",
        }
    }

    /// What this server costs to install, for the dialog to warn about
    /// before the user commits to it.
    pub fn install_note(self) -> &'static str {
        match self {
            // Not a guess at build times: this is a full Tycho/Maven build
            // of the whole Eclipse product.
            Server::Jdtls => {
                "Built from source (upstream publishes no binaries on GitHub). Needs git, a JDK 21+ and Python 3.9+ \
                 on PATH, and takes several minutes."
            }
            Server::KotlinLanguageServer => "Prebuilt release download (~87 MB). Needs a JDK on PATH to run.",
        }
    }

    /// The GitHub tag for `version`. Real tag shapes, read off each repo's
    /// own tag/release listing: jdt.ls prefixes with `v`, kotlin-language-
    /// server uses a bare version.
    fn tag_for(self, version: &str) -> String {
        match self {
            Server::Jdtls => format!("v{version}"),
            Server::KotlinLanguageServer => version.to_string(),
        }
    }

    fn clone_url(self) -> String {
        format!("https://github.com/{}.git", self.github_repo())
    }

    /// The prebuilt release asset to download, for servers that publish
    /// one — `None` for jdt.ls, which is what sends it down the
    /// build-from-source path instead.
    fn release_asset(self, version: &str) -> Option<String> {
        match self {
            Server::Jdtls => None,
            Server::KotlinLanguageServer => Some(format!(
                "https://github.com/{}/releases/download/{}/server.zip",
                self.github_repo(),
                self.tag_for(version)
            )),
        }
    }

    /// Where the installed launcher ends up, relative to this server's own
    /// directory under the cache: the path inside the extracted archive
    /// (Kotlin) or inside the built Maven product (jdt.ls). Both are real
    /// paths verified against the actual artifact — `server/bin/kotlin-
    /// language-server` read out of the release zip's own central
    /// directory, `bin/jdtls` from the layout jdt.ls' README documents its
    /// build producing.
    fn launcher_path(self) -> PathBuf {
        match self {
            Server::Jdtls => [
                "source",
                "org.eclipse.jdt.ls.product",
                "target",
                "repository",
                "bin",
                "jdtls",
            ]
            .iter()
            .collect(),
            Server::KotlinLanguageServer => ["server", "bin", "kotlin-language-server"].iter().collect(),
        }
    }

    /// Where `check_latest` looks. kotlin-language-server publishes real
    /// GitHub releases; jdt.ls publishes none at all (see this module's own
    /// header), so its newest version has to come from the tag list.
    fn latest_version_api_url(self) -> String {
        match self {
            Server::Jdtls => format!("https://api.github.com/repos/{}/tags?per_page=10", self.github_repo()),
            Server::KotlinLanguageServer => {
                format!("https://api.github.com/repos/{}/releases/latest", self.github_repo())
            }
        }
    }
}

/// Where a completed install landed. `binary` is exactly what the matching
/// `LspSettings` binary field should be set to — a launcher script that
/// takes no arguments, which is what `lsp_state`'s own `LspSession::spawn`
/// call expects (jdt.ls' `bin/jdtls` wrapper defaults its `-data` workspace
/// directory from the process' working directory, which `lsp_state` already
/// sets to the project root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub server: Server,
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

fn cache_dir() -> Result<PathBuf, String> {
    ProjectDirs::from("", "", "foxgarden")
        .map(|dirs| dirs.cache_dir().join("lsp-servers"))
        .ok_or_else(|| "couldn't determine a cache directory for this platform".to_string())
}

/// This server's own directory name — one per server *per version*, so
/// installing an update never half-overwrites a working install: the new
/// version builds/extracts somewhere else entirely, and the settings'
/// binary path only moves once it succeeds.
fn install_dir_name(server: Server, version: &str) -> String {
    match server {
        Server::Jdtls => format!("jdtls-{version}"),
        Server::KotlinLanguageServer => format!("kotlin-language-server-{version}"),
    }
}

fn install_dir(server: Server, version: &str) -> Result<PathBuf, String> {
    Ok(cache_dir()?.join(install_dir_name(server, version)))
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

/// Runs `command` to completion, mapping a non-zero exit into an error that
/// actually says what went wrong: the tail of the combined output rather
/// than a bare status code, since a failed Maven build's real cause is
/// always in its last few lines and nowhere else this app can show it.
fn run_command(command: &mut Command, what: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("couldn't run {what}: {e} — is it installed and on PATH?"))?;
    if output.status.success() {
        return Ok(());
    }
    let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    let tail: Vec<&str> = combined.lines().rev().take(15).collect();
    let tail: Vec<&str> = tail.into_iter().rev().collect();
    Err(format!("{what} failed ({}):\n{}", output.status, tail.join("\n")))
}

/// Downloads and unpacks a prebuilt release archive (the Kotlin Language
/// Server's own `server.zip`). The launcher's location inside the archive
/// is checked rather than assumed — if upstream ever restructures it, this
/// fails loudly here instead of writing a path that doesn't exist into the
/// settings.
fn install_prebuilt(
    server: Server,
    version: &str,
    dir: &Path,
    url: &str,
    report: &dyn Fn(String),
) -> Result<PathBuf, String> {
    report(format!("Downloading {} {version}…", server.display_name()));
    let downloaded = download(url)?;

    report("Extracting…".to_string());
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(&downloaded)).map_err(|e| format!("not a valid zip: {e}"))?;
    archive
        .extract(dir)
        .map_err(|e| format!("failed to extract archive: {e}"))?;

    let launcher = dir.join(server.launcher_path());
    if !launcher.exists() {
        return Err(format!(
            "{}'s archive didn't contain the expected launcher at {}",
            server.display_name(),
            launcher.display()
        ));
    }
    ensure_executable(&launcher)?;
    Ok(launcher)
}

/// Which `java` a source build will actually use: `JAVA_HOME`'s if set
/// (Maven's own rule, so this reports on the same JVM the build will run
/// under), otherwise whatever is on `PATH`.
fn java_command() -> PathBuf {
    match std::env::var_os("JAVA_HOME") {
        Some(home) => PathBuf::from(home).join("bin").join("java"),
        None => PathBuf::from("java"),
    }
}

/// The major version out of `java -version`'s own output. Handles both
/// shapes a real JVM prints: modern `openjdk version "21.0.2"` and the
/// legacy `java version "1.8.0_292"`, where the major version is the
/// *second* component.
fn java_major_version(version_output: &str) -> Option<u32> {
    let quoted = version_output.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '_', '-', '+']);
    let first = parts.next()?;
    if first == "1" {
        parts.next()?.parse().ok()
    } else {
        first.parse().ok()
    }
}

/// Fails early — before a clone and a multi-minute build — when the JVM
/// that build would use is too old. Without this the same problem still
/// surfaces, but as a Maven stack trace several minutes in, which is a much
/// worse thing to hand a user than one sentence naming the actual version
/// they have.
fn check_java(minimum_major: u32) -> Result<(), String> {
    let java = java_command();
    let output = Command::new(&java).arg("-version").output().map_err(|e| {
        format!(
            "couldn't run {}: {e} — install a JDK {minimum_major}+ or set JAVA_HOME",
            java.display()
        )
    })?;
    // Every JVM prints its version banner on stderr, not stdout.
    let banner = String::from_utf8_lossy(&output.stderr);
    match java_major_version(&banner) {
        Some(major) if major >= minimum_major => Ok(()),
        Some(major) => Err(format!(
            "this build needs a JDK {minimum_major} or newer, but {} is Java {major} — install a newer JDK or point \
             JAVA_HOME at one",
            java.display()
        )),
        None => Err(format!("couldn't read a version out of `{} -version`", java.display())),
    }
}

/// Clones `server`'s release tag and builds it with the project's own
/// bundled Maven wrapper — the install path for a server that publishes no
/// binaries on GitHub (jdt.ls). The command is the one its README
/// documents, with `-DskipTests=true` (also the README's own suggestion):
/// this is building a known-good release tag, not validating a change, and
/// its full test suite roughly doubles an already long build.
fn install_from_source(server: Server, version: &str, dir: &Path, report: &dyn Fn(String)) -> Result<PathBuf, String> {
    report("Checking prerequisites…".to_string());
    check_java(JDTLS_MINIMUM_JDK)?;

    let source = dir.join("source");
    if source.exists() {
        std::fs::remove_dir_all(&source)
            .map_err(|e| format!("couldn't clear a previous build at {}: {e}", source.display()))?;
    }

    report(format!("Cloning {} {version}…", server.github_repo()));
    run_command(
        Command::new("git").args([
            "clone",
            "--depth",
            "1",
            "--branch",
            &server.tag_for(version),
            &server.clone_url(),
            &source.display().to_string(),
        ]),
        "git clone",
    )?;

    report("Building with Maven — this takes several minutes…".to_string());
    // An absolute path, not `./mvnw`: `std::process::Command` explicitly
    // documents a relative program path as ambiguous (parent's working
    // directory or the `current_dir` set below, platform-dependent), and
    // getting that wrong here is a confusing "not found" for a file that's
    // plainly right there.
    let wrapper = source.join(if cfg!(windows) { "mvnw.cmd" } else { "mvnw" });
    run_command(
        Command::new(&wrapper)
            .args(["clean", "verify", "-DskipTests=true"])
            .current_dir(&source),
        "the Maven build",
    )?;

    let launcher = dir.join(server.launcher_path());
    if !launcher.exists() {
        return Err(format!(
            "the build finished but produced no launcher at {} — upstream's build layout may have changed",
            launcher.display()
        ));
    }
    ensure_executable(&launcher)?;
    Ok(launcher)
}

/// Installs one specific `version` of `server`, reporting each step through
/// `report`. Runs entirely on a background thread (see
/// `LspManagerState::install`) — nothing here is safe to do on the UI
/// thread, least of all a multi-minute Maven build.
fn install_sync(server: Server, version: &str, report: &dyn Fn(String)) -> InstallResult {
    let dir = install_dir(server, version)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;

    let binary = match server.release_asset(version) {
        Some(url) => install_prebuilt(server, version, &dir, &url, report)?,
        None => install_from_source(server, version, &dir, report)?,
    };

    Ok(Installed {
        server,
        version: version.to_string(),
        binary,
    })
}

/// Reads `server`'s newest published version off the GitHub API. Two
/// response shapes, because the two repos genuinely differ (see
/// `Server::latest_version_api_url`): a release object with a `tag_name`,
/// or a tag array to take the first entry of. Either way the returned
/// string is a bare version, directly comparable against
/// `recommended_version` and an `Installed::version`.
fn check_latest_sync(server: Server) -> LatestVersionResult {
    let mut response = ureq::get(server.latest_version_api_url())
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
/// by `Server`.
#[derive(Default)]
pub struct LspManagerState {
    installs: HashMap<Server, InstallJob>,
    checks: HashMap<Server, Receiver<LatestVersionResult>>,
}

impl LspManagerState {
    pub fn installing(&self, server: Server) -> bool {
        self.installs.contains_key(&server)
    }

    pub fn checking(&self, server: Server) -> bool {
        self.checks.contains_key(&server)
    }

    /// The latest progress line for a running install, for the dialog to
    /// display — `None` when nothing is running for `server`.
    pub fn status(&self, server: Server) -> Option<&str> {
        self.installs.get(&server).map(|job| job.status.as_str())
    }

    /// Whether any install is still running — the app's own update loop
    /// uses this to keep requesting repaints, since progress lands on a
    /// background thread with no input event to piggyback on.
    pub fn busy(&self) -> bool {
        !self.installs.is_empty() || !self.checks.is_empty()
    }

    /// Starts installing `version` of `server` on a background thread.
    /// A second click while one is already running is ignored rather than
    /// starting a competing job over the same directory.
    pub fn install(&mut self, server: Server, version: String) {
        if self.installing(server) {
            return;
        }
        let (tx, rx) = channel();
        let status = format!("Starting {} install…", server.display_name());
        std::thread::spawn(move || {
            let report_tx = tx.clone();
            let report = move |message: String| {
                let _ = report_tx.send(InstallEvent::Progress(message));
            };
            let result = install_sync(server, &version, &report);
            let _ = tx.send(InstallEvent::Finished(result));
        });
        self.installs.insert(server, InstallJob { rx, status });
    }

    pub fn check_latest(&mut self, server: Server) {
        if self.checking(server) {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(check_latest_sync(server));
        });
        self.checks.insert(server, rx);
    }

    /// Drains every install's event stream: progress lines update that
    /// job's status in place, and a finished job is removed and its result
    /// returned. Called once a frame.
    pub fn poll_installs(&mut self) -> Vec<InstallResult> {
        let mut done = Vec::new();
        self.installs.retain(|_, job| {
            loop {
                match job.rx.try_recv() {
                    Ok(InstallEvent::Progress(message)) => job.status = message,
                    Ok(InstallEvent::Finished(result)) => {
                        done.push(result);
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

    pub fn poll_checks(&mut self) -> Vec<(Server, LatestVersionResult)> {
        let mut done = Vec::new();
        self.checks.retain(|&server, rx| match rx.try_recv() {
            Ok(result) => {
                done.push((server, result));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kotlin_language_server_downloads_the_real_release_asset_verified_this_session() {
        assert_eq!(
            Server::KotlinLanguageServer.release_asset("1.3.13").as_deref(),
            Some("https://github.com/fwcd/kotlin-language-server/releases/download/1.3.13/server.zip")
        );
    }

    /// jdt.ls publishes no build artifacts on GitHub at all (its releases
    /// page holds one unrelated 2016 `.vsix`), which is exactly what routes
    /// it to the build-from-source path — so "no asset" is a load-bearing
    /// fact here, not an omission.
    #[test]
    fn jdtls_has_no_prebuilt_asset_and_so_builds_from_source() {
        assert!(Server::Jdtls.release_asset("1.60.0").is_none());
    }

    #[test]
    fn tags_match_each_repos_real_convention() {
        assert_eq!(Server::Jdtls.tag_for("1.60.0"), "v1.60.0");
        assert_eq!(Server::KotlinLanguageServer.tag_for("1.3.13"), "1.3.13");
    }

    #[test]
    fn launcher_paths_match_each_artifacts_real_layout() {
        assert_eq!(
            Server::KotlinLanguageServer.launcher_path(),
            Path::new("server").join("bin").join("kotlin-language-server")
        );
        assert!(
            Server::Jdtls
                .launcher_path()
                .ends_with(Path::new("repository").join("bin").join("jdtls"))
        );
    }

    #[test]
    fn latest_tag_reads_a_release_response() {
        let json = serde_json::json!({ "tag_name": "1.3.13" });
        assert_eq!(latest_tag_from_response(&json).as_deref(), Some("1.3.13"));
    }

    #[test]
    fn latest_tag_reads_a_tag_list_and_strips_the_v_prefix() {
        let json = serde_json::json!([{ "name": "v1.60.0" }, { "name": "v1.59.0" }]);
        assert_eq!(latest_tag_from_response(&json).as_deref(), Some("1.60.0"));
    }

    #[test]
    fn latest_tag_skips_a_leading_non_version_tag() {
        // A real hazard for jdt.ls, whose tag list is not guaranteed to
        // lead with a release tag.
        let json = serde_json::json!([{ "name": "hackathon_zrh" }, { "name": "v1.60.0" }]);
        assert_eq!(latest_tag_from_response(&json).as_deref(), Some("1.60.0"));
    }

    #[test]
    fn latest_tag_on_an_unusable_response_is_none() {
        assert!(latest_tag_from_response(&serde_json::json!({})).is_none());
        assert!(latest_tag_from_response(&serde_json::json!([])).is_none());
    }

    /// Installing an update must not build into the directory a working
    /// install is currently being run out of.
    #[test]
    fn install_dirs_are_per_server_and_per_version() {
        assert_ne!(
            install_dir_name(Server::Jdtls, "1.60.0"),
            install_dir_name(Server::Jdtls, "1.59.0")
        );
        assert_ne!(
            install_dir_name(Server::Jdtls, "1.60.0"),
            install_dir_name(Server::KotlinLanguageServer, "1.60.0")
        );
    }

    #[test]
    fn run_command_reports_the_failing_output_not_just_the_status() {
        let error = run_command(
            Command::new("sh").args(["-c", "echo boom >&2; exit 3"]),
            "the test command",
        )
        .expect_err("a non-zero exit is an error");
        assert!(error.contains("the test command failed"), "{error}");
        assert!(error.contains("boom"), "{error}");
    }

    #[test]
    fn java_major_version_reads_a_modern_jvm_banner() {
        assert_eq!(
            java_major_version("openjdk version \"21.0.2\" 2024-01-16 LTS\n"),
            Some(21)
        );
        assert_eq!(
            java_major_version("openjdk version \"17.0.4\" 2022-07-19 LTS"),
            Some(17)
        );
    }

    /// A Java 8 JVM reports `1.8.0_x`, where the major version is the
    /// second component — reading the first would call it "Java 1" and
    /// reject every JVM ever with a confusing message.
    #[test]
    fn java_major_version_reads_a_legacy_jvm_banner() {
        assert_eq!(java_major_version("java version \"1.8.0_292\""), Some(8));
    }

    #[test]
    fn java_major_version_on_unparseable_output_is_none() {
        assert!(java_major_version("no version here").is_none());
        assert!(java_major_version("version \"nonsense\"").is_none());
    }

    #[test]
    fn run_command_reports_a_missing_executable_actionably() {
        let error = run_command(&mut Command::new("definitely-not-a-real-binary-xyz"), "git clone")
            .expect_err("a missing binary is an error");
        assert!(error.contains("on PATH"), "{error}");
    }

    /// The one end-to-end check against the real release asset: downloads
    /// `server.zip` (~87 MB) and proves the whole prebuilt path — archive
    /// layout, extraction, and the launcher landing exactly where
    /// `launcher_path` claims, executable. `#[ignore]`d because it needs
    /// the network and moves real bytes; run it with
    /// `cargo test -p foxgarden --ignored` when touching this path.
    #[test]
    #[ignore = "downloads ~87 MB from GitHub"]
    fn install_prebuilt_really_installs_the_kotlin_language_server() {
        let server = Server::KotlinLanguageServer;
        let version = server.recommended_version();
        let dir = test_support::tempdir();
        let url = server
            .release_asset(version)
            .expect("this server publishes a prebuilt asset");

        let launcher = install_prebuilt(server, version, dir.path(), &url, &|_| {}).expect("installs");

        assert_eq!(launcher, dir.path().join(server.launcher_path()));
        assert!(launcher.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&launcher).unwrap().permissions().mode();
            assert!(mode & 0o111 != 0, "the launcher must be executable, got {mode:o}");
        }
    }

    #[test]
    fn state_starts_idle() {
        let state = LspManagerState::default();
        assert!(!state.busy());
        for server in ALL_SERVERS {
            assert!(!state.installing(server));
            assert!(!state.checking(server));
            assert!(state.status(server).is_none());
        }
    }

    /// A second click while an install is already running must be ignored,
    /// not start a competing job over the same directory. The already-
    /// running job is faked (a channel nobody sends on) rather than started
    /// for real — the point under test is the guard, and a real install
    /// would clone from the network.
    #[test]
    fn install_ignores_a_second_click_while_one_is_already_running() {
        let mut state = LspManagerState::default();
        let (tx, rx) = channel::<InstallEvent>();
        state.installs.insert(
            Server::Jdtls,
            InstallJob {
                rx,
                status: "Building…".to_string(),
            },
        );

        state.install(Server::Jdtls, "1.60.0".to_string());

        assert_eq!(state.installs.len(), 1);
        assert_eq!(state.status(Server::Jdtls), Some("Building…"));
        drop(tx);
    }

    #[test]
    fn poll_installs_reports_progress_in_place_and_only_removes_a_finished_job() {
        let mut state = LspManagerState::default();
        let (tx, rx) = channel();
        state.installs.insert(
            Server::Jdtls,
            InstallJob {
                rx,
                status: "Starting…".to_string(),
            },
        );

        tx.send(InstallEvent::Progress("Cloning…".to_string())).unwrap();
        assert!(state.poll_installs().is_empty());
        assert_eq!(state.status(Server::Jdtls), Some("Cloning…"));

        let installed = Installed {
            server: Server::Jdtls,
            version: "1.60.0".to_string(),
            binary: PathBuf::from("/tmp/jdtls"),
        };
        tx.send(InstallEvent::Progress("Building…".to_string())).unwrap();
        tx.send(InstallEvent::Finished(Ok(installed.clone()))).unwrap();

        assert_eq!(state.poll_installs(), vec![Ok(installed)]);
        assert!(!state.installing(Server::Jdtls));
        assert!(!state.busy());
    }
}
