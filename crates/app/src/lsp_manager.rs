//! Installs and updates the two language servers Settings > Language
//! Servers… configures (`PLAN.md` Track 20) into a per-user cache directory
//! — so neither has to be found, downloaded and unpacked by hand before
//! FoxGarden can use it.
//!
//! Both servers' archives are bundled straight into the FoxGarden binary
//! (`vendor/lsp-servers/`, tracked via Git LFS — see `include_bytes!` below),
//! not fetched over the network at install time:
//!
//! * **Kotlin Language Server** (`fwcd/kotlin-language-server`) publishes a
//!   ready-to-run `server.zip` on every GitHub release — vendored verbatim.
//!   Extract, point at `server/bin/kotlin-language-server`. Same shape as
//!   `tool_manager`'s own PMD/SpotBugs installs.
//! * **Eclipse JDT Language Server** (`eclipse-jdtls/eclipse.jdt.ls`) does
//!   not publish build artifacts on GitHub at all — its releases page holds
//!   exactly one unrelated 2016 hackathon `.vsix` (verified against the real
//!   API this session). The real prebuilt distribution instead lives on
//!   `download.eclipse.org/jdtls/milestones/<version>/`, as a self-contained
//!   tarball (`bin/jdtls`, a Python launcher, plus the Equinox jars and
//!   per-platform `config_*` dirs it needs) — vendored from there rather
//!   than built from source. Building it from source instead (the previous
//!   approach here) meant a `git clone` plus the project's own Maven/Tycho
//!   build, which resolves its target-platform dependencies straight off
//!   Maven Central; behind a corporate mirror that doesn't proxy every
//!   artifact that build touches, it fails outright (e.g. `com.jetbrains.
//!   intellij.java:java-decompiler-engine` unresolvable) — a failure mode
//!   that has nothing to do with FoxGarden and no fix on this end. Vendoring
//!   the official prebuilt tarball sidesteps that whole class of failure:
//!   no Maven, no git, no network at install time at all.
//!
//! jdt.ls' own README states its **runtime** minimum is Java 21 (not just a
//! build-time requirement) — `resolve_jdtls_java` resolves and verifies that
//! JVM once, and `lsp_state` pins every jdt.ls spawn to that exact
//! executable via `bin/jdtls`'s own `--java-executable` flag, rather than
//! letting its launcher fall back to whatever `java` PATH happens to resolve
//! to at that later moment.
//!
//! Every archive layout and version in this file was checked against the
//! real vendored artifacts this session rather than assumed — see each
//! function's own doc comment.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use directories::ProjectDirs;

/// The two servers' own release archives, embedded directly into the
/// FoxGarden binary at compile time — every build carries a working
/// language server with it, so installing one never depends on network
/// access, `git`, or a JDK capable of building jdt.ls' own Tycho product.
/// Tracked via Git LFS (`vendor/lsp-servers/.gitattributes`) so the plain
/// git history stays small despite the ~135 MB combined size.
const JDTLS_ARCHIVE: &[u8] = include_bytes!("../../../vendor/lsp-servers/jdtls-1.60.0.tar.gz");
const KOTLIN_LANGUAGE_SERVER_ARCHIVE: &[u8] =
    include_bytes!("../../../vendor/lsp-servers/kotlin-language-server-1.3.13.zip");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Server {
    Jdtls,
    KotlinLanguageServer,
}

/// Every server this manager knows how to install, in the order the
/// settings dialog lists them.
pub const ALL_SERVERS: [Server; 2] = [Server::Jdtls, Server::KotlinLanguageServer];

/// The JVM jdt.ls itself requires to *run* — its README states this
/// explicitly as a runtime minimum, not just a build-time one.
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

    /// The version bundled with this build of FoxGarden — the only one
    /// `install` can actually install (see `vendor/lsp-servers/` and the
    /// `include_bytes!` constants above). `check_latest` separately reports
    /// what upstream currently publishes, purely as an FYI: a newer version
    /// isn't installable until it's vendored into a FoxGarden release.
    pub fn recommended_version(self) -> &'static str {
        match self {
            Server::Jdtls => "1.60.0",
            Server::KotlinLanguageServer => "1.3.13",
        }
    }

    /// What this server costs to install, for the dialog to show.
    pub fn install_note(self) -> &'static str {
        match self {
            Server::Jdtls => {
                "Bundled with FoxGarden (Eclipse's own prebuilt distribution) — installs instantly, no network or \
                 build required. Needs a JDK 21+ on PATH or JAVA_HOME to run (jdt.ls' own stated minimum)."
            }
            Server::KotlinLanguageServer => {
                "Bundled with FoxGarden — installs instantly, no network required. Needs a JDK on PATH to run."
            }
        }
    }

    /// This server's own archive bytes, embedded at compile time.
    fn bundled_archive(self) -> &'static [u8] {
        match self {
            Server::Jdtls => JDTLS_ARCHIVE,
            Server::KotlinLanguageServer => KOTLIN_LANGUAGE_SERVER_ARCHIVE,
        }
    }

    /// Where the installed launcher ends up, relative to this server's own
    /// directory under the cache — the path inside the extracted archive.
    /// Both are real paths verified against the actual vendored artifact:
    /// `server/bin/kotlin-language-server` read out of the release zip's own
    /// central directory, `bin/jdtls` (a Python launcher) out of the
    /// official jdt.ls milestone tarball.
    fn launcher_path(self) -> PathBuf {
        match self {
            Server::Jdtls => ["bin", "jdtls"].iter().collect(),
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

/// Extracts a `.zip` archive's bytes (Kotlin Language Server's own
/// `server.zip`, vendored verbatim) into `dir`.
fn extract_zip(bytes: &[u8], dir: &Path) -> Result<(), String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a valid zip: {e}"))?;
    archive.extract(dir).map_err(|e| format!("failed to extract archive: {e}"))
}

/// Extracts a gzipped tarball's bytes (jdt.ls' own milestone distribution,
/// vendored verbatim) into `dir`.
fn extract_tar_gz(bytes: &[u8], dir: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    archive.unpack(dir).map_err(|e| format!("failed to extract archive: {e}"))
}

/// Verifies the JVM `crate::jdk::java_command(java_home)` resolves to is at
/// least `minimum_major` — called both at install time (fail fast with one
/// plain sentence, rather than a confusing crash the first time a session
/// tries to start) and by `resolve_jdtls_java` (to pin every jdt.ls spawn to
/// a verified JVM rather than trusting whatever `java` PATH resolves to
/// later). `crate::jdk::detect_major_version` (Track 29 Phase 1) does the
/// actual "run `java -version`, parse it" work generically — shared with
/// `jdk_registry`, which has no jdt.ls-specific minimum to enforce; this
/// function is just that generic result plus jdt.ls's own wording on top.
fn check_java(minimum_major: u32, java_home: &str) -> Result<(), String> {
    match crate::jdk::detect_major_version(java_home) {
        Ok(major) if major >= minimum_major => Ok(()),
        Ok(major) => Err(format!(
            "jdt.ls needs a JDK {minimum_major} or newer to run, but {} is Java {major} — install a newer JDK, or \
             point Settings > Language Servers… at one",
            crate::jdk::java_command(java_home).display()
        )),
        Err(e) => Err(format!("{e} — install a JDK {minimum_major}+, or set it in Settings > Language Servers…")),
    }
}

/// Resolves and verifies the exact JVM jdt.ls will run under — `lsp_state`
/// pins every jdt.ls spawn to this path via `bin/jdtls`'s own
/// `--java-executable` flag (`SPEC.md`/`PLAN.md` Track 20), rather than
/// letting its launcher fall back to whatever `java` PATH resolves to at
/// that later moment, which may not meet jdt.ls' Java 21 runtime minimum at
/// all. `java_home` is `LspSettings::jdtls_java_home` — empty means "auto-
/// detect from JAVA_HOME/PATH", same as an unset override always has.
pub fn resolve_jdtls_java(java_home: &str) -> Result<PathBuf, String> {
    check_java(JDTLS_MINIMUM_JDK, java_home)?;
    Ok(crate::jdk::java_command(java_home))
}

/// Extracts `server`'s bundled archive into `dir` and returns the launcher's
/// path — the only install path there is now (see this module's own
/// header): both servers' bytes are embedded in the FoxGarden binary itself.
fn install_bundled(server: Server, dir: &Path, report: &dyn Fn(String)) -> Result<PathBuf, String> {
    if matches!(server, Server::Jdtls) {
        // Ambient `JAVA_HOME`/`PATH` only — a soft pre-flight sanity check,
        // not the exact JVM a session actually runs under later (that's
        // `resolve_jdtls_java`, which honors `LspSettings::jdtls_java_home`
        // too). Extraction doesn't otherwise need Java at all; this exists
        // purely so a missing/too-old JDK surfaces here in one sentence
        // instead of only once a session tries and fails to start.
        report("Checking prerequisites…".to_string());
        check_java(JDTLS_MINIMUM_JDK, "")?;
    }

    report(format!("Extracting bundled {}…", server.display_name()));
    match server {
        Server::Jdtls => extract_tar_gz(server.bundled_archive(), dir)?,
        Server::KotlinLanguageServer => extract_zip(server.bundled_archive(), dir)?,
    }

    let launcher = dir.join(server.launcher_path());
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

/// `kotlin-language-server` resolves the stdlib jar an analyzed file
/// compiles against by first checking a `kotlinc` on `PATH`
/// (`BackupClassPathResolver.findKotlinCliCompilerLibrary` in its own
/// source — verified against the exact vendored tag, 1.3.13). A `kotlinc`
/// newer than this server's own bundled analysis compiler can read (roughly
/// anything above Kotlin 2.2.0) makes every stdlib symbol — `println`,
/// `Random`, even `kotlin.Unit` — come back `INCOMPATIBLE_CLASS`/
/// `UNRESOLVED_REFERENCE` on otherwise-correct code (`TECHNICAL_DEBT.md`
/// #17), independent of whether that `kotlinc` has anything to do with the
/// project being edited.
///
/// The server checks its own documented override first, though:
/// `ShellClassPathResolver.global` runs an executable script at
/// `<config root>/kotlin-language-server/classpath` (`$XDG_CONFIG_HOME`, or
/// `~/.config` if unset) and uses whatever jar paths it prints, before ever
/// falling back to `kotlinc`-on-`PATH`. Pointing that script at the stdlib
/// jars sitting next to whichever `kotlin-language-server` binary is
/// actually configured — `binary`'s own sibling `../lib/`, the upstream
/// release zip's layout, so this works for both a FoxGarden-installed build
/// and a manually pointed-at one — pins the version match regardless of the
/// user's own `PATH`.
fn kotlin_stdlib_jars(lib_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(lib_dir).map_err(|e| format!("couldn't read {}: {e}", lib_dir.display()))?;
    let mut jars: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("kotlin-stdlib") && name.ends_with(".jar") && !name.contains("-common") && !name.contains("-sources")
        })
        .collect();
    jars.sort();
    Ok(jars)
}

/// Where `kotlin-language-server` itself looks for the override script —
/// same resolution order as its own `ShellClassPathResolver.global`
/// (`$XDG_CONFIG_HOME`, else `<home>/.config`), taken as an explicit
/// `config_root` parameter here rather than read from the environment
/// directly so this stays testable against a tempdir instead of a
/// developer's real config directory.
fn kotlin_classpath_override_path(config_root: &Path) -> PathBuf {
    let name = if cfg!(windows) { "classpath.bat" } else { "classpath" };
    config_root.join("kotlin-language-server").join(name)
}

/// The override script's own contents — a one-liner that just echoes the
/// joined jar paths back out, in the platform path-list separator
/// `kotlin-language-server`'s `ShellClassPathResolver` splits on (`:` on
/// Unix, `;` on Windows, both matching `java.io.File.pathSeparator`).
fn kotlin_classpath_override_script(jars: &[PathBuf]) -> String {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let joined = jars.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(&separator.to_string());
    if cfg!(windows) {
        format!("@echo off\r\necho {joined}\r\n")
    } else {
        format!("#!/bin/sh\necho \"{joined}\"\n")
    }
}

/// Writes (or refreshes) the classpath override for whichever
/// `kotlin-language-server` `binary` is configured, under `config_root`.
/// Idempotent — skips the write (and, on Unix, the `chmod` syscall) once the
/// script's content already matches, since this runs on every session
/// start, not just once at install time (a manually configured `binary`
/// this module never installed still needs the override written the first
/// time it's used). Every failure here — no `lib/` next to `binary`, no
/// stdlib jars in it, an unwritable config directory — is returned rather
/// than panicking; the caller treats it as best-effort.
fn ensure_kotlin_stdlib_override(binary: &Path, config_root: &Path) -> Result<(), String> {
    let lib_dir = binary
        .parent() // server/bin
        .and_then(Path::parent) // server
        .map(|server_dir| server_dir.join("lib"))
        .ok_or_else(|| format!("couldn't find a lib/ directory next to {}", binary.display()))?;

    let jars = kotlin_stdlib_jars(&lib_dir)?;
    if jars.is_empty() {
        return Err(format!("no kotlin-stdlib*.jar found in {}", lib_dir.display()));
    }

    let script_path = kotlin_classpath_override_path(config_root);
    let script = kotlin_classpath_override_script(&jars);
    if std::fs::read_to_string(&script_path).ok().as_deref() == Some(script.as_str()) {
        return Ok(());
    }

    if let Some(parent) = script_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("couldn't create {}: {e}", parent.display()))?;
    }
    std::fs::write(&script_path, &script).map_err(|e| format!("couldn't write {}: {e}", script_path.display()))?;
    ensure_executable(&script_path)?;
    Ok(())
}

/// The real config root `kotlin-language-server` itself resolves at
/// runtime — `$XDG_CONFIG_HOME`, else `~/.config` — matching
/// `ShellClassPathResolver.global`'s own Kotlin-side resolution exactly, so
/// the script lands exactly where that server will look for it.
fn xdg_config_root() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir));
    }
    directories::UserDirs::new()
        .map(|dirs| dirs.home_dir().join(".config"))
        .ok_or_else(|| "couldn't determine a home directory".to_string())
}

/// Best-effort entry point `lsp_state` calls right before spawning a
/// `kotlin-language-server` session — see this module's own
/// `kotlin_stdlib_jars` doc comment for why. Never fails the caller: a
/// server started without the override just falls back to
/// `kotlin-language-server`'s pre-existing (possibly version-mismatched)
/// `PATH`-based resolution, exactly as before this existed.
pub fn ensure_kotlin_stdlib_override_for(binary: &Path) {
    let root = match xdg_config_root() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("kotlin-language-server stdlib override: {e}");
            return;
        }
    };
    if let Err(e) = ensure_kotlin_stdlib_override(binary, &root) {
        eprintln!("kotlin-language-server stdlib override: {e}");
    }
}

/// Installs one specific `version` of `server`, reporting each step through
/// `report`. Runs entirely on a background thread (see
/// `LspManagerState::install`) so extraction never blocks the UI thread.
/// Only the bundled version can actually be installed — see
/// `Server::recommended_version`'s own doc comment.
fn install_sync(server: Server, version: &str, report: &dyn Fn(String)) -> InstallResult {
    if version != server.recommended_version() {
        return Err(format!(
            "only the version bundled with this FoxGarden build ({}) can be installed — {version} isn't vendored",
            server.recommended_version()
        ));
    }

    let dir = install_dir(server, version)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;

    let binary = install_bundled(server, &dir, report)?;

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
    fn launcher_paths_match_each_artifacts_real_layout() {
        assert_eq!(
            Server::KotlinLanguageServer.launcher_path(),
            Path::new("server").join("bin").join("kotlin-language-server")
        );
        assert_eq!(Server::Jdtls.launcher_path(), Path::new("bin").join("jdtls"));
    }

    /// Only the vendored version can actually be installed — asking for
    /// anything else must fail loudly rather than silently installing the
    /// wrong bytes under the requested version's label.
    #[test]
    fn install_sync_rejects_a_version_that_is_not_the_bundled_one() {
        let error = install_sync(Server::Jdtls, "1.59.0", &|_| {}).expect_err("not vendored");
        assert!(error.contains("1.60.0"), "{error}");
        assert!(error.contains("1.59.0"), "{error}");
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

    // `java_major_version`'s own tests now live in `crate::jdk` (Track 29
    // Phase 1) — `lsp_manager` no longer defines that function itself.

    /// End-to-end against the real vendored archives — no network, no
    /// `#[ignore]` needed, since the bytes are already embedded in the test
    /// binary: extracts each server's bundled archive and proves the
    /// launcher lands exactly where `launcher_path` claims, executable.
    /// Goes through `extract_zip`/`extract_tar_gz` directly rather than
    /// `install_bundled` — this proves the *archive layout*, which doesn't
    /// depend on this machine happening to have a JDK 21 on it (a separate
    /// concern `java_major_version`'s own tests already cover with synthetic
    /// banners).
    #[test]
    fn bundled_archives_extract_with_the_launcher_at_its_documented_path() {
        for server in ALL_SERVERS {
            let dir = test_support::tempdir();
            match server {
                Server::Jdtls => extract_tar_gz(server.bundled_archive(), dir.path()),
                Server::KotlinLanguageServer => extract_zip(server.bundled_archive(), dir.path()),
            }
            .expect("extracts");

            let launcher = dir.path().join(server.launcher_path());
            assert!(launcher.is_file(), "{}", launcher.display());
            #[cfg(unix)]
            {
                ensure_executable(&launcher).expect("chmod");
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&launcher).unwrap().permissions().mode();
                assert!(mode & 0o111 != 0, "the launcher must be executable, got {mode:o}");
            }
        }
    }

    /// Regression for TECHNICAL_DEBT.md #17: the override must only pick up
    /// real stdlib jars, matching `kotlin-language-server`'s own
    /// `WithStdlibResolver.isStdlib` filter (excludes `-common`, and this
    /// codebase's own scan also excludes `-sources`) — anything else in
    /// `lib/` (the compiler jar, unrelated dependency jars) must not leak
    /// into the override.
    #[test]
    fn kotlin_stdlib_jars_finds_only_real_stdlib_jars_not_the_compiler_or_common() {
        let dir = test_support::tempdir();
        for name in [
            "kotlin-stdlib-2.1.0.jar",
            "kotlin-stdlib-jdk7-2.1.0.jar",
            "kotlin-stdlib-jdk8-2.1.0.jar",
            "kotlin-stdlib-common-2.1.0.jar",
            "kotlin-stdlib-2.1.0-sources.jar",
            "kotlin-compiler-2.1.0.jar",
            "kotlin-reflect-2.1.0.jar",
        ] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }

        let jars = kotlin_stdlib_jars(dir.path()).expect("reads dir");
        let names: Vec<&str> = jars.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(
            names,
            vec!["kotlin-stdlib-2.1.0.jar", "kotlin-stdlib-jdk7-2.1.0.jar", "kotlin-stdlib-jdk8-2.1.0.jar"]
        );
    }

    #[test]
    fn kotlin_stdlib_jars_on_a_missing_dir_is_an_error_not_a_panic() {
        let dir = test_support::tempdir();
        assert!(kotlin_stdlib_jars(&dir.path().join("does-not-exist")).is_err());
    }

    #[test]
    fn kotlin_classpath_override_path_matches_the_servers_own_resolution() {
        let root = Path::new("/home/dev/.config");
        let expected =
            if cfg!(windows) { "classpath.bat" } else { "classpath" };
        assert_eq!(
            kotlin_classpath_override_path(root),
            root.join("kotlin-language-server").join(expected)
        );
    }

    /// The script's own separator must match `java.io.File.pathSeparator` on
    /// the platform `kotlin-language-server`'s `ShellClassPathResolver`
    /// actually splits on — `:` on Unix, `;` on Windows — or a correctly
    /// found jar still wouldn't parse back out on the server's side.
    #[test]
    fn kotlin_classpath_override_script_joins_with_the_platform_path_separator() {
        let jars = vec![PathBuf::from("/a/kotlin-stdlib.jar"), PathBuf::from("/a/kotlin-stdlib-jdk8.jar")];
        let script = kotlin_classpath_override_script(&jars);
        if cfg!(windows) {
            assert!(script.contains("/a/kotlin-stdlib.jar;/a/kotlin-stdlib-jdk8.jar"), "{script}");
        } else {
            assert!(script.starts_with("#!/bin/sh\n"), "{script}");
            assert!(script.contains("/a/kotlin-stdlib.jar:/a/kotlin-stdlib-jdk8.jar"), "{script}");
        }
    }

    /// End-to-end against the real vendored `kotlin-language-server` archive
    /// (same as `bundled_archives_extract_with_the_launcher_at_its_documented_
    /// path`, but proving the stdlib-override side rather than the launcher
    /// path): extracts it into a temp "install dir", points a temp "config
    /// root" at it, and confirms the written script is both executable and
    /// lists the real jars that shipped in this build's own vendored
    /// archive — the actual regression scenario from #17, not a synthetic
    /// stand-in.
    #[test]
    fn ensure_kotlin_stdlib_override_writes_a_script_naming_the_real_vendored_stdlib_jars() {
        let install_dir = test_support::tempdir();
        extract_zip(Server::KotlinLanguageServer.bundled_archive(), install_dir.path()).expect("extracts");
        let binary = install_dir.path().join(Server::KotlinLanguageServer.launcher_path());

        let config_root = test_support::tempdir();
        ensure_kotlin_stdlib_override(&binary, config_root.path()).expect("writes the override");

        let script_path = kotlin_classpath_override_path(config_root.path());
        let script = std::fs::read_to_string(&script_path).expect("script was written");
        assert!(script.contains("kotlin-stdlib-2.1.0.jar"), "{script}");
        assert!(!script.contains("kotlin-compiler"), "{script}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&script_path).unwrap().permissions().mode();
            assert!(mode & 0o111 != 0, "the override script must be executable, got {mode:o}");
        }
    }

    /// Regression: the whole point of the idempotency check is that a
    /// session-start call that finds nothing changed must not re-`chmod`/
    /// rewrite the file (relevant if a user's own tooling ever needs to
    /// tweak it) — verified here by writing once, mutating the file's
    /// content to something else, then calling again and confirming the
    /// *second* call still rewrites back to the expected content (proving
    /// the skip path is content-based, not "only ever runs once").
    #[test]
    fn ensure_kotlin_stdlib_override_is_idempotent_and_self_heals_if_the_script_changes() {
        let install_dir = test_support::tempdir();
        extract_zip(Server::KotlinLanguageServer.bundled_archive(), install_dir.path()).expect("extracts");
        let binary = install_dir.path().join(Server::KotlinLanguageServer.launcher_path());
        let config_root = test_support::tempdir();

        ensure_kotlin_stdlib_override(&binary, config_root.path()).unwrap();
        let script_path = kotlin_classpath_override_path(config_root.path());
        let first = std::fs::read_to_string(&script_path).unwrap();

        std::fs::write(&script_path, "echo tampered").unwrap();
        ensure_kotlin_stdlib_override(&binary, config_root.path()).unwrap();
        let second = std::fs::read_to_string(&script_path).unwrap();

        assert_eq!(first, second);
        assert_ne!(second, "echo tampered");
    }

    /// A `binary` that doesn't have the upstream `server/bin/…` shape (e.g.
    /// a typo'd manual override in Settings > Language Servers…) must fail
    /// the override cleanly rather than writing garbage or panicking on the
    /// `Path::parent` chain.
    #[test]
    fn ensure_kotlin_stdlib_override_on_a_binary_with_no_lib_dir_sibling_is_an_error() {
        let config_root = test_support::tempdir();
        let error = ensure_kotlin_stdlib_override(Path::new("/kotlin-language-server"), config_root.path())
            .expect_err("no lib/ next to a root-level binary");
        assert!(error.contains("lib"), "{error}");
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
