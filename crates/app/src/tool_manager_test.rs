use super::*;

fn install(sha256: &str) -> AnalyzerInstall {
    AnalyzerInstall {
        version: "1.0.0".to_string(),
        download_url: "https://example.invalid/tool.zip".to_string(),
        sha256: sha256.to_string(),
        packaging: AnalyzerPackaging::File {
            name: "tool.jar".to_string(),
        },
        default_config: None,
        latest_release: None,
    }
}

fn zip_with(entry: &str, contents: &[u8], executable: bool) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let mut options = zip::write::SimpleFileOptions::default();
        if executable {
            options = options.unix_permissions(0o755);
        }
        writer.start_file(entry, options).unwrap();
        std::io::Write::write_all(&mut writer, contents).unwrap();
        writer.finish().unwrap();
    }
    buf
}

#[test]
fn extract_zip_and_locate_launcher_finds_the_launcher_inside_the_versioned_directory() {
    // A minimal but real zip, built in-memory, shaped exactly like PMD's own
    // release archive: a versioned top-level directory holding `bin/pmd`.
    let dir = test_support::tempdir();
    let buf = zip_with("pmd-bin-7.26.0/bin/pmd", b"#!/bin/sh\necho fake pmd\n", true);

    let launcher =
        extract_zip_and_locate_launcher("PMD", "pmd", "bin/pmd", dir.path(), &buf).expect("locates the launcher");
    assert_eq!(launcher, dir.path().join("pmd-bin-7.26.0").join("bin").join("pmd"));
    assert!(launcher.exists());
}

#[test]
fn the_launcher_path_is_whatever_the_extension_names() {
    // PMD's Windows archive ships `bin/pmd.bat` alongside `bin/pmd`; the
    // extension spells the one for its platform.
    let dir = test_support::tempdir();
    let buf = zip_with("pmd-bin-7.26.0/bin/pmd.bat", b"@echo off\r\n", false);

    let launcher =
        extract_zip_and_locate_launcher("PMD", "pmd", "bin/pmd.bat", dir.path(), &buf).expect("locates the launcher");
    assert_eq!(launcher, dir.path().join("pmd-bin-7.26.0").join("bin").join("pmd.bat"));
}

#[test]
fn extract_zip_and_locate_launcher_errors_when_no_matching_top_level_dir_exists() {
    let dir = test_support::tempdir();
    let buf = zip_with("unrelated/file.txt", b"", false);

    assert!(extract_zip_and_locate_launcher("PMD", "pmd", "bin/pmd", dir.path(), &buf).is_err());
}

#[test]
fn extract_zip_and_locate_launcher_errors_when_the_launcher_is_missing() {
    let dir = test_support::tempdir();
    let buf = zip_with("pmd-bin-7.26.0/lib/other.jar", b"", false);

    let error = extract_zip_and_locate_launcher("PMD", "pmd", "bin/pmd", dir.path(), &buf).unwrap_err();
    assert!(error.contains("doesn't exist"), "{error}");
}

#[test]
fn verify_download_accepts_bytes_matching_the_pinned_checksum() {
    assert_eq!(
        sha256_hex(b"hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    let pinned = install("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    assert!(verify_download("Tool", &pinned, b"hello").is_ok());
}

#[test]
fn verify_download_rejects_bytes_that_do_not_match() {
    let pinned = install("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    let error =
        verify_download("Tool", &pinned, b"not the real jar").expect_err("a substituted artifact must be refused");
    assert!(error.contains("checksum"), "{error}");
    assert!(error.contains("Tool"), "{error}");
}

#[test]
fn an_install_with_no_pinned_checksum_refuses_everything() {
    let error = verify_download("Tool", &install(""), b"anything")
        .expect_err("an empty pin has nothing to verify against");
    assert!(error.contains("checksum"), "{error}");
}

#[test]
fn tool_manager_state_starts_with_nothing_installing_or_checking() {
    let state = ToolManagerState::default();
    for id in ["checkstyle", "pmd", "spotbugs"] {
        assert!(!state.installing(id));
        assert!(!state.checking(id));
    }
}
