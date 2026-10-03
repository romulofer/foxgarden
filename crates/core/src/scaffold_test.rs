use super::*;

/// A minimal file set, standing in for whatever an extension generates —
/// `write_scaffold` never looks at what it is writing, only at where.
fn files() -> Vec<(PathBuf, String)> {
    vec![
        (PathBuf::from("pom.xml"), "<project/>\n".to_string()),
        (
            PathBuf::from("src/main/java/com/example/Main.java"),
            "class Main {}\n".to_string(),
        ),
    ]
}

#[test]
fn write_scaffold_creates_every_file_under_the_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("my-app");
    write_scaffold(&root, &files()).unwrap();

    assert!(root.join("pom.xml").exists());
    assert!(root.join("src/main/java/com/example/Main.java").exists());
}

#[test]
fn write_scaffold_refuses_a_non_empty_existing_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("already-here.txt"), "hi").unwrap();

    let error = write_scaffold(dir.path(), &files()).unwrap_err();
    assert!(error.contains("not empty"), "{error}");
    assert!(!dir.path().join("pom.xml").exists());
}

#[test]
fn write_scaffold_accepts_an_existing_but_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    write_scaffold(dir.path(), &files()).unwrap();
    assert!(dir.path().join("pom.xml").exists());
}
