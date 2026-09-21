
use super::*;

#[test]
fn download_url_matches_the_real_v4_5_asset_layout_verified_this_session() {
    // Only the host platform's asset is knowable at compile time; assert
    // whichever one this build targets against its real release URL.
    let platform = current_platform().expect("this test host is a supported platform");
    let url = platform.download_url();
    assert!(
        url.starts_with("https://github.com/async-profiler/async-profiler/releases/download/v4.5/"),
        "{url}"
    );
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
    let error =
        verify_download(&platform, b"not the real archive").expect_err("a substituted artifact must be refused");
    assert!(error.contains("checksum"), "{error}");
}

#[test]
fn sha256_hex_matches_a_known_vector() {
    assert_eq!(
        sha256_hex(b"hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
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
    assert_eq!(
        asprof,
        dir.path()
            .join("async-profiler-4.5-linux-x64")
            .join("bin")
            .join("asprof")
    );
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
        builder
            .append_data(&mut header, "unrelated/file.txt", &data[..])
            .unwrap();
        builder.finish().unwrap();
    }
    let bytes = gz.finish().unwrap();
    let _ = std::io::stdout().flush();

    // Force the tar path regardless of host so the assertion is uniform.
    let platform = Platform {
        suffix: "linux-x64",
        extension: "tar.gz",
        sha256: "",
    };
    assert!(extract_and_locate_asprof(&platform, dir.path(), &bytes).is_err());
}

#[test]
fn state_starts_idle() {
    let state = ProfilerManagerState::default();
    assert!(!state.installing());
}
