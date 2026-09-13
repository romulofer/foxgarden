//! Downloads and installs async-profiler (`PLAN.md` Track 26 Phase 1 —
//! "Profiler integration") into a per-user cache directory, so profiling
//! doesn't require the user to have found and installed it manually.
//!
//! Mirrors `tool_manager`'s own download/verify/extract/background-install
//! shape (pinned version, pinned SHA-256, `directories`-provided cache,
//! `Receiver`-polled background thread) — the two differ only where
//! async-profiler itself does: it ships an OS/arch-specific native archive
//! (a `.tar.gz` on Linux, a `.zip` on macOS, containing a `bin/asprof`
//! launcher and a `lib/libasyncProfiler.so`/`.dylib`), not a platform-neutral
//! jar, so the asset name, its checksum, and the extractor all vary by
//! platform. async-profiler publishes **no** Windows build at all — its
//! release page carries only linux-x64/arm64 tarballs and a macOS zip — so
//! `current_platform` returns an error there rather than this app pretending
//! a profiler exists.
//!
//! Every version/URL/checksum/layout claim below was verified against the
//! real v4.5 release archives this session (downloaded, `sha256sum`'d, and
//! `tar tzf`/`unzip -l`'d), not assumed from the release page's naming.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use directories::ProjectDirs;

/// The pinned async-profiler release this installs — a specific, verified
/// release, not "whatever's latest," the same "pin, don't float" rationale
/// `tool_manager::Tool::recommended_version` established.
const VERSION: &str = "4.5";

/// The host platform's async-profiler release archive: the release-asset
/// suffix (`async-profiler-<VERSION>-<suffix>.<ext>`), the pinned SHA-256 of
/// that exact asset, and whether it's a gzipped tar or a zip.
struct Platform {
    /// The `<suffix>` between the version and the extension in both the asset
    /// name and the directory the archive extracts to (e.g. `linux-x64`,
    /// `macos`).
    suffix: &'static str,
    /// Archive extension — `tar.gz` on Linux, `zip` on macOS.
    extension: &'static str,
    /// SHA-256 of the real v4.5 asset for this platform.
    sha256: &'static str,
}

/// The archive for the platform this build is running on, or an error naming
/// the unsupported one (Windows — async-profiler ships no build for it).
///
/// macOS ships a single universal zip with no arch in its name, so both Mac
/// arches map to the same `macos` asset; Linux is split x64/arm64. Any other
/// target is refused rather than guessed.
fn current_platform() -> Result<Platform, String> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Ok(Platform {
            suffix: "linux-x64",
            extension: "tar.gz",
            sha256: "89546fbb9ee0fc5496c7edd4099b0709489bc78b0d8057ccbb4b801f6b032b62",
        })
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        Ok(Platform {
            suffix: "linux-arm64",
            extension: "tar.gz",
            sha256: "64c41d1465d60097439c50d7e924b4946f1f62b1cbd21ce5b034fad09c0d6979",
        })
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Platform {
            suffix: "macos",
            extension: "zip",
            sha256: "46d04ef81f532a065a0b3877e488aa706afa14aa2ea14433b323db9e6fda76dc",
        })
    }
    #[cfg(not(any(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")), target_os = "macos")))]
    {
        Err("async-profiler has no build for this platform (it supports Linux x64/arm64 and macOS only)".to_string())
    }
}

impl Platform {
    fn asset_file_name(&self) -> String {
        format!("async-profiler-{VERSION}-{}.{}", self.suffix, self.extension)
    }

    /// The directory the archive extracts to at its top level — the asset
    /// name minus its extension, e.g. `async-profiler-4.5-linux-x64`.
    fn extracted_dir_name(&self) -> String {
        format!("async-profiler-{VERSION}-{}", self.suffix)
    }

    fn download_url(&self) -> String {
        format!(
            "https://github.com/async-profiler/async-profiler/releases/download/v{VERSION}/{}",
            self.asset_file_name()
        )
    }
}

/// Where a completed install landed: `asprof` is the launcher path to pass to
/// `fg_core::profiler_command`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub asprof: PathBuf,
}

pub type InstallResult = Result<Installed, String>;

fn cache_dir() -> Result<PathBuf, String> {
    ProjectDirs::from("", "", "foxgarden")
        .map(|dirs| dirs.cache_dir().join("async-profiler"))
        .ok_or_else(|| "couldn't determine a cache directory for this platform".to_string())
}

/// The `bin/asprof` launcher path inside `dir`, if an already-extracted copy
/// of the pinned version is present — lets a caller skip the download when
/// the tool's already cached (an "Install once, profile many times" flow).
pub fn installed_asprof() -> Option<PathBuf> {
    let platform = current_platform().ok()?;
    let asprof = cache_dir().ok()?.join(platform.extracted_dir_name()).join("bin").join("asprof");
    asprof.exists().then_some(asprof)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;

    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Refuses `downloaded` unless it hashes to exactly the pinned SHA-256 for
/// this platform's asset — same integrity rationale as `tool_manager`'s own
/// `verify_download`: the archive becomes native code this app then executes
/// (`bin/asprof` and its `libasyncProfiler` injected into a live JVM), so a
/// substituted artifact is arbitrary code execution, made a loud refusable
/// failure here rather than a silent one.
fn verify_download(platform: &Platform, downloaded: &[u8]) -> Result<(), String> {
    let actual = sha256_hex(downloaded);
    if actual != platform.sha256 {
        return Err(format!(
            "async-profiler's download doesn't match its expected checksum (expected {}, got {actual}) — refusing \
             to install it",
            platform.sha256
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

/// Extracts `downloaded` under `dir` (a gzipped tar on Linux, a zip on macOS)
/// and returns the `bin/asprof` launcher inside the archive's own top-level
/// directory — read back from the actual extracted tree rather than
/// string-built, so an upstream layout change fails loudly here (a missing
/// launcher) instead of silently pointing at a path that doesn't exist.
fn extract_and_locate_asprof(platform: &Platform, dir: &Path, downloaded: &[u8]) -> Result<PathBuf, String> {
    match platform.extension {
        "zip" => {
            let mut archive =
                zip::ZipArchive::new(std::io::Cursor::new(downloaded)).map_err(|e| format!("not a valid zip: {e}"))?;
            archive.extract(dir).map_err(|e| format!("failed to extract archive: {e}"))?;
        }
        _ => {
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(downloaded));
            archive.unpack(dir).map_err(|e| format!("failed to extract archive: {e}"))?;
        }
    }

    let asprof = dir.join(platform.extracted_dir_name()).join("bin").join("asprof");
    if !asprof.exists() {
        return Err(format!("expected async-profiler's launcher at {} but it doesn't exist", asprof.display()));
    }
    Ok(asprof)
}

/// Downloads, verifies, and extracts async-profiler's pinned version into the
/// cache directory, returning where its `asprof` launcher landed. Re-running
/// for an already-installed copy re-downloads and overwrites — simple, and
/// cheap (the archive is under 500 KB) given this only runs on explicit user
/// action, never automatically.
fn install_sync() -> InstallResult {
    let platform = current_platform()?;
    let dir = cache_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let downloaded = download(&platform.download_url())?;
    verify_download(&platform, &downloaded)?;
    let asprof = extract_and_locate_asprof(&platform, &dir, &downloaded)?;

    Ok(Installed { version: VERSION.to_string(), asprof })
}

/// Background-install state — one in-flight install at a time, the same
/// `Receiver`-polled-from-`show` shape `tool_manager::ToolManagerState` and
/// every other long-running action in this app uses so a ~500 KB download
/// (plus its extraction) never blocks the UI thread.
#[derive(Default)]
pub struct ProfilerManagerState {
    install: Option<Receiver<InstallResult>>,
}

impl ProfilerManagerState {
    pub fn installing(&self) -> bool {
        self.install.is_some()
    }

    pub fn install(&mut self) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(install_sync());
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
mod tests {
    use super::*;

    #[test]
    fn download_url_matches_the_real_v4_5_asset_layout_verified_this_session() {
        // Only the host platform's asset is knowable at compile time; assert
        // whichever one this build targets against its real release URL.
        let platform = current_platform().expect("this test host is a supported platform");
        let url = platform.download_url();
        assert!(url.starts_with("https://github.com/async-profiler/async-profiler/releases/download/v4.5/"), "{url}");
        assert!(url.ends_with(&platform.asset_file_name()), "{url}");
        assert!(url.contains("async-profiler-4.5-"), "{url}");
    }

    #[test]
    fn extracted_dir_name_is_the_asset_name_without_its_extension() {
        let platform = current_platform().expect("supported platform");
        let asset = platform.asset_file_name();
        let dir = platform.extracted_dir_name();
        assert!(asset.starts_with(&dir), "asset {asset} should start with dir {dir}");
        assert_eq!(&asset[dir.len()..], format!(".{}", platform.extension));
    }

    #[test]
    fn verify_download_rejects_bytes_that_do_not_match() {
        let platform = current_platform().expect("supported platform");
        let error = verify_download(&platform, b"not the real archive").expect_err("a substituted artifact must be refused");
        assert!(error.contains("checksum"), "{error}");
    }

    #[test]
    fn sha256_hex_matches_a_known_vector() {
        assert_eq!(sha256_hex(b"hello"), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn extract_and_locate_asprof_finds_the_launcher_in_a_real_shaped_tarball() {
        // A minimal but real gzipped tar shaped exactly like async-profiler's
        // own linux-x64 archive: a versioned top-level directory holding
        // `bin/asprof`.
        use std::io::Write;

        let dir = test_support::tempdir();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        {
            let mut builder = tar::Builder::new(&mut gz);
            let script = b"#!/bin/sh\necho fake asprof\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(script.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "async-profiler-4.5-linux-x64/bin/asprof", &script[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let bytes = gz.finish().unwrap();
        let _ = std::io::stdout().flush();

        let platform = current_platform().unwrap();
        let asprof = extract_and_locate_asprof(&platform, dir.path(), &bytes).expect("locates asprof");
        assert_eq!(asprof, dir.path().join("async-profiler-4.5-linux-x64").join("bin").join("asprof"));
        assert!(asprof.exists());
    }

    #[test]
    fn extract_and_locate_asprof_errors_when_the_launcher_is_absent() {
        use std::io::Write;

        let dir = test_support::tempdir();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        {
            let mut builder = tar::Builder::new(&mut gz);
            let data = b"nothing useful";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_cksum();
            builder.append_data(&mut header, "unrelated/file.txt", &data[..]).unwrap();
            builder.finish().unwrap();
        }
        let bytes = gz.finish().unwrap();
        let _ = std::io::stdout().flush();

        // Force the tar path regardless of host so the assertion is uniform.
        let platform = Platform { suffix: "linux-x64", extension: "tar.gz", sha256: "" };
        assert!(extract_and_locate_asprof(&platform, dir.path(), &bytes).is_err());
    }

    #[test]
    fn state_starts_idle() {
        let state = ProfilerManagerState::default();
        assert!(!state.installing());
    }
}
