use super::*;

fn install(archive: ArchiveKind) -> ProfilerInstall {
    ProfilerInstall {
        version: "1.0".to_string(),
        download_url: "https://example.invalid/prof.tar.gz".to_string(),
        sha256: String::new(),
        archive,
        dir_name: "prof-1.0".to_string(),
        launcher: "bin/launch".to_string(),
    }
}

fn tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut builder = tar::Builder::new(&mut gz);
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, *data).unwrap();
        }
        builder.finish().unwrap();
    }
    gz.finish().unwrap()
}

#[test]
fn verify_download_rejects_bytes_that_do_not_match() {
    let mut pinned = install(ArchiveKind::TarGz);
    pinned.sha256 = sha256_hex(b"the real archive");
    let error = verify_download("Prof", &pinned, b"not the real archive").expect_err("a substituted artifact must be refused");
    assert!(error.contains("checksum") && error.starts_with("Prof"), "{error}");
    assert!(verify_download("Prof", &pinned, b"the real archive").is_ok());
}

#[test]
fn sha256_hex_matches_a_known_vector() {
    assert_eq!(sha256_hex(b"hello"), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
}

#[test]
fn extract_and_locate_finds_the_launcher_the_extension_names() {
    let dir = test_support::tempdir();
    let bytes = tarball(&[("prof-1.0/bin/launch", b"#!/bin/sh\necho fake\n")]);

    let launcher = extract_and_locate(&install(ArchiveKind::TarGz), dir.path(), &bytes).expect("locates the launcher");
    assert_eq!(launcher, dir.path().join("prof-1.0").join("bin").join("launch"));
    assert!(launcher.exists());
}

#[test]
fn extract_and_locate_errors_when_the_launcher_is_absent() {
    let dir = test_support::tempdir();
    let bytes = tarball(&[("unrelated/file.txt", b"nothing useful")]);
    assert!(extract_and_locate(&install(ArchiveKind::TarGz), dir.path(), &bytes).is_err());
}

#[test]
fn extract_and_locate_reads_a_zip_when_the_extension_says_zip() {
    use std::io::Write;

    let dir = test_support::tempdir();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("prof-1.0/bin/launch", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"#!/bin/sh\n").unwrap();
    let bytes = zip.finish().unwrap().into_inner();

    let launcher = extract_and_locate(&install(ArchiveKind::Zip), dir.path(), &bytes).expect("locates the launcher");
    assert!(launcher.ends_with("prof-1.0/bin/launch"), "{launcher:?}");
}

#[test]
fn state_starts_idle() {
    let state = ProfilerManagerState::default();
    assert!(!state.installing());
}
