
use super::*;

fn server(id: &str) -> ServerInstallHandle {
    test_support::languages()
        .server_installs()
        .into_iter()
        .find(|server| server.id() == id)
        .unwrap_or_else(|| panic!("no installable server {id}"))
}

fn all_servers() -> Vec<ServerInstallHandle> {
    test_support::languages().server_installs()
}

#[test]
fn launcher_paths_match_each_artifacts_real_layout() {
    let launcher = |id: &str| server(id).install.launcher.iter().collect::<PathBuf>();
    assert_eq!(
        launcher("kotlin-language-server"),
        Path::new("server").join("bin").join("kotlin-language-server")
    );
    assert_eq!(launcher("jdtls"), Path::new("bin").join("jdtls"));
}

/// Only the vendored version can actually be installed — asking for
/// anything else must fail loudly rather than silently installing the
/// wrong bytes under the requested version's label.
#[test]
fn install_sync_rejects_a_version_that_is_not_the_bundled_one() {
    let jdtls = server("jdtls");
    let error = install_sync(&jdtls, "0.0.1", &|_| {}).expect_err("not vendored");
    assert!(error.contains(&jdtls.install.version), "{error}");
    assert!(error.contains("0.0.1"), "{error}");
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
    assert_ne!(install_dir("jdtls", "1.60.0"), install_dir("jdtls", "1.59.0"));
    assert_ne!(install_dir("jdtls", "1.60.0"), install_dir("kotlin-language-server", "1.60.0"));
}

/// Whether this build actually has the vendored archives, rather than
/// the Git LFS pointer files a clone without `git lfs` leaves in their
/// place (TECHNICAL_DEBT.md #21). `include_bytes!` embeds whatever is
/// on disk, pointer included, so a developer who cloned without LFS
/// gets a binary whose "archives" are 130-byte text files.
///
/// The tests that unpack those archives skip themselves in that case
/// instead of failing: a red suite there reports a *checkout* problem
/// as if it were a broken change, which is both misleading and
/// unactionable from the test name. The real user-facing failure is
/// still loud — `reject_lfs_pointer` refuses the install at runtime
/// with an instruction to run `git lfs pull`.
fn vendored_archives_present() -> bool {
    let missing = all_servers()
        .iter()
        .any(|server| server.archive().is_none_or(|archive| reject_lfs_pointer(archive).is_err()));
    if missing {
        eprintln!("skipping: the extension's vendored archives are Git LFS pointers, not the real archives — run `git lfs pull`");
    }
    !missing
}

/// End-to-end against the real vendored archives — no network, no
/// `#[ignore]` needed, since the bytes are already embedded in the test
/// binary: extracts each server's bundled archive and proves the
/// launcher lands exactly where the contribution claims, executable.
/// Goes through `extract_zip`/`extract_tar_gz` directly rather than
/// `install_bundled` — this proves the *archive layout*, which doesn't
/// depend on this machine happening to have a JDK 21 on it (a separate
/// concern `java_major_version`'s own tests already cover with synthetic
/// banners).
#[test]
fn bundled_archives_extract_with_the_launcher_at_its_documented_path() {
    if !vendored_archives_present() {
        return;
    }
    for server in all_servers() {
        let dir = test_support::tempdir();
        let archive = server.archive().expect("bundled");
        match server.install.archive {
            ArchiveKind::TarGz => extract_tar_gz(archive, dir.path()),
            ArchiveKind::Zip => extract_zip(archive, dir.path()),
        }
        .expect("extracts");

        let launcher = dir.path().join(server.install.launcher.iter().collect::<PathBuf>());
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


/// A clone made without `git-lfs` leaves a text pointer where each
/// vendored archive should be; the resulting install failure has to say
/// so, not just report an unreadable decoder error.
#[test]
fn an_lfs_pointer_is_reported_as_itself_rather_than_as_a_broken_archive() {
    let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 50925681\n";
    let dir = test_support::tempdir();

    for error in [
        extract_tar_gz(pointer, dir.path()).expect_err("a pointer is not a tarball"),
        extract_zip(pointer, dir.path()).expect_err("a pointer is not a zip"),
    ] {
        assert!(error.contains("Git LFS pointer"), "{error}");
        assert!(error.contains("git lfs pull"), "{error}");
    }
}

#[test]
fn state_starts_idle() {
    let state = LspManagerState::default();
    assert!(!state.busy());
    for server in all_servers() {
        assert!(!state.installing(server.id()));
        assert!(!state.checking(server.id()));
        assert!(state.status(server.id()).is_none());
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
        "jdtls".to_string(),
        InstallJob {
            rx,
            status: "Building…".to_string(),
        },
    );

    state.install(server("jdtls"), "1.60.0".to_string());

    assert_eq!(state.installs.len(), 1);
    assert_eq!(state.status("jdtls"), Some("Building…"));
    drop(tx);
}

#[test]
fn poll_installs_reports_progress_in_place_and_only_removes_a_finished_job() {
    let mut state = LspManagerState::default();
    let (tx, rx) = channel();
    state.installs.insert(
        "jdtls".to_string(),
        InstallJob {
            rx,
            status: "Starting…".to_string(),
        },
    );

    tx.send(InstallEvent::Progress("Cloning…".to_string())).unwrap();
    assert!(state.poll_installs().is_empty());
    assert_eq!(state.status("jdtls"), Some("Cloning…"));

    let installed = Installed {
        server: "jdtls".to_string(),
        version: "1.60.0".to_string(),
        binary: PathBuf::from("/tmp/jdtls"),
    };
    tx.send(InstallEvent::Progress("Building…".to_string())).unwrap();
    tx.send(InstallEvent::Finished(Ok(installed.clone()))).unwrap();

    assert_eq!(state.poll_installs(), vec![("jdtls".to_string(), Ok(installed))]);
    assert!(!state.installing("jdtls"));
    assert!(!state.busy());
}
