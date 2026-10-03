use super::*;

#[test]
fn writes_the_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Main.java");
    write_atomically(&path, b"class Main {}\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "class Main {}\n");
}

#[test]
fn overwrites_an_existing_file_and_leaves_no_temp_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Main.java");
    std::fs::write(&path, "old").unwrap();

    write_atomically(&path, b"new").unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "Main.java")
        .collect();
    assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
}

#[cfg(unix)]
#[test]
fn preserves_an_existing_file_s_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.sh");
    std::fs::write(&path, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

    write_atomically(&path, b"#!/bin/sh\necho hi\n").unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
}

#[cfg(unix)]
#[test]
fn writes_through_a_symlink_instead_of_replacing_it() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("real.java");
    let link = dir.path().join("link.java");
    std::fs::write(&target, "old").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    write_atomically(&link, b"new").unwrap();

    assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
}

#[test]
fn a_failed_write_leaves_the_previous_contents_intact() {
    let dir = tempfile::tempdir().unwrap();
    // A directory where the file should be: `File::create` on the temp
    // path still succeeds, but the rename over a directory fails — the
    // closest reliably-reproducible mid-save failure.
    let path = dir.path().join("Main.java");
    std::fs::create_dir(&path).unwrap();

    assert!(write_atomically(&path, b"new").is_err());
    assert!(path.is_dir());
}
