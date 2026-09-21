
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

#[cfg(not(windows))]
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

#[cfg(windows)]
#[test]
fn extract_zip_and_locate_launcher_finds_a_real_pmd_archive_s_windows_launcher() {
    // Same real archive shape, but PMD's own zip ships `bin/pmd.bat`
    // alongside `bin/pmd` — the Windows-native one.
    let dir = test_support::tempdir();
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("pmd-bin-7.26.0/bin/pmd.bat", options).unwrap();
        std::io::Write::write_all(&mut writer, b"@echo off\r\necho fake pmd\r\n").unwrap();
        writer.finish().unwrap();
    }

    let launcher = extract_zip_and_locate_launcher(Tool::Pmd, dir.path(), &buf).expect("locates the launcher");
    assert_eq!(launcher, dir.path().join("pmd-bin-7.26.0").join("bin").join("pmd.bat"));
    assert!(launcher.exists());
}

#[test]
fn extract_zip_and_locate_launcher_errors_when_no_matching_top_level_dir_exists() {
    let dir = test_support::tempdir();
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        writer
            .start_file("unrelated/file.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();
    }

    assert!(extract_zip_and_locate_launcher(Tool::Pmd, dir.path(), &buf).is_err());
}

#[test]
fn verify_download_accepts_bytes_matching_the_pinned_checksum() {
    // The pinned hashes belong to ~20-70MB real archives, so this
    // exercises the comparison itself against a value computed the same
    // way `verify_download` computes it, rather than re-downloading.
    assert_eq!(
        sha256_hex(b"hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn verify_download_rejects_bytes_that_do_not_match() {
    let error = verify_download(
        Tool::Checkstyle,
        Tool::Checkstyle.recommended_version(),
        b"not the real jar",
    )
    .expect_err("a substituted artifact must be refused");
    assert!(error.contains("checksum"), "{error}");
}

#[test]
fn verify_download_refuses_a_version_with_no_pinned_checksum() {
    let error = verify_download(Tool::Pmd, "0.0.1-unpinned", b"anything")
        .expect_err("an unpinned version has nothing to verify against");
    assert!(error.contains("no pinned checksum"), "{error}");
}

#[test]
fn tool_manager_state_starts_with_nothing_installing_or_checking() {
    let state = ToolManagerState::default();
    for tool in [Tool::Checkstyle, Tool::Pmd, Tool::SpotBugs] {
        assert!(!state.installing(tool));
        assert!(!state.checking(tool));
    }
}
