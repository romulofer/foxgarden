
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
fn write_debug_plugin_jar_writes_the_real_vendored_bytes() {
    if !vendored_archives_present() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = write_debug_plugin_jar(dir.path()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), JAVA_DEBUG_PLUGIN_JAR);
}

#[test]
fn write_debug_plugin_jar_is_idempotent_and_skips_a_redundant_write() {
    if !vendored_archives_present() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = write_debug_plugin_jar(dir.path()).unwrap();
    let written_at = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    write_debug_plugin_jar(dir.path()).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), written_at);
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
    let missing = ALL_SERVERS
        .iter()
        .any(|server| reject_lfs_pointer(server.bundled_archive()).is_err())
        || reject_lfs_pointer(JAVA_DEBUG_PLUGIN_JAR).is_err();
    if missing {
        eprintln!("skipping: vendor/lsp-servers/ holds Git LFS pointers, not the real archives — run `git lfs pull`");
    }
    !missing
}

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
    if !vendored_archives_present() {
        return;
    }
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
        vec![
            "kotlin-stdlib-2.1.0.jar",
            "kotlin-stdlib-jdk7-2.1.0.jar",
            "kotlin-stdlib-jdk8-2.1.0.jar"
        ]
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
    let expected = if cfg!(windows) { "classpath.bat" } else { "classpath" };
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
    let jars = vec![
        PathBuf::from("/a/kotlin-stdlib.jar"),
        PathBuf::from("/a/kotlin-stdlib-jdk8.jar"),
    ];
    let script = kotlin_classpath_override_script(&jars);
    if cfg!(windows) {
        assert!(
            script.contains("/a/kotlin-stdlib.jar;/a/kotlin-stdlib-jdk8.jar"),
            "{script}"
        );
    } else {
        assert!(script.starts_with("#!/bin/sh\n"), "{script}");
        assert!(
            script.contains("/a/kotlin-stdlib.jar:/a/kotlin-stdlib-jdk8.jar"),
            "{script}"
        );
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
    if !vendored_archives_present() {
        return;
    }
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
        assert!(
            mode & 0o111 != 0,
            "the override script must be executable, got {mode:o}"
        );
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
    if !vendored_archives_present() {
        return;
    }
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

/// Ordering is the whole point of the candidate list: a machine with
/// several JDKs installed must be offered its newest one, and plain
/// lexicographic order would rank `java-8` above `java-21`.
#[test]
fn java_home_candidates_rank_a_search_roots_installs_newest_first() {
    let root = test_support::tempdir();
    for name in ["java-8-openjdk", "java-21-openjdk", "java-17-openjdk"] {
        let bin = root.path().join(name).join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("java"), b"#!/bin/sh\n").unwrap();
    }

    let candidates = java_home_candidates(None, None, &[root.path().to_path_buf()]);

    let names: Vec<_> = candidates
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, ["java-21-openjdk", "java-17-openjdk", "java-8-openjdk"]);
}

/// `JAVA_HOME` and `PATH` are what jdt.ls' own launcher would pick, so
/// detection agrees with the machine's configured default whenever that
/// default is new enough — and never offers the same home twice.
#[test]
fn java_home_candidates_lead_with_java_home_then_path_and_never_repeat_one() {
    let root = test_support::tempdir();
    let configured = root.path().join("configured");
    let on_path = root.path().join("on-path");
    for home in [&configured, &on_path] {
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::write(home.join("bin").join("java"), b"#!/bin/sh\n").unwrap();
    }

    let candidates = java_home_candidates(
        Some(configured.clone()),
        Some(on_path.join("bin").join("java")),
        // The same two homes again, as a search root would find them.
        &[root.path().to_path_buf()],
    );

    assert_eq!(candidates, [configured, on_path]);
}

/// A directory that isn't a JDK at all can't be a candidate, however
/// promising its name — `bin/java` has to actually be there.
#[test]
fn java_home_candidates_skip_a_directory_with_no_java_in_it() {
    let root = test_support::tempdir();
    std::fs::create_dir_all(root.path().join("java-21-not-really")).unwrap();
    assert!(java_home_candidates(None, None, &[root.path().to_path_buf()]).is_empty());
}

/// macOS ships JDKs as bundles, where the home is `Contents/Home` under
/// the install directory rather than the directory itself.
#[test]
fn java_home_candidates_understand_the_macos_bundle_layout() {
    let root = test_support::tempdir();
    let home = root.path().join("temurin-21.jdk").join("Contents").join("Home");
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::write(home.join("bin").join("java"), b"#!/bin/sh\n").unwrap();

    assert_eq!(java_home_candidates(None, None, &[root.path().to_path_buf()]), [home]);
}

#[test]
fn version_hint_reads_the_major_version_out_of_a_jdk_directory_name() {
    assert_eq!(version_hint("java-21-openjdk-amd64"), 21);
    assert_eq!(version_hint("21.0.11-zulu"), 21);
    assert_eq!(version_hint("temurin-17.jdk"), 17);
    assert_eq!(version_hint("openjdk"), 0);
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

    assert_eq!(state.poll_installs(), vec![(Server::Jdtls, Ok(installed))]);
    assert!(!state.installing(Server::Jdtls));
    assert!(!state.busy());
}
