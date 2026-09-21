use super::*;

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Main.java");
    std::fs::write(&file, "class Main {}\n").unwrap();
    let root = dir.path().to_path_buf();
    (dir, root)
}

#[test]
fn a_draft_survives_to_be_found_again() {
    let (_dir, root) = project();
    let file = root.join("Main.java");

    write_draft(&root, &file, "class Main { // unsaved\n}\n").unwrap();

    let pending = pending_drafts(&root);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].file_path, file);
    assert_eq!(pending[0].content, "class Main { // unsaved\n}\n");
}

#[test]
fn a_draft_matching_disk_is_not_offered_and_is_cleaned_up() {
    let (_dir, root) = project();
    let file = root.join("Main.java");

    write_draft(&root, &file, "class Main {}\n").unwrap();

    assert!(
        pending_drafts(&root).is_empty(),
        "nothing was lost, so there is nothing to restore"
    );
    assert!(pending_drafts(&root).is_empty());
    assert!(
        !root.join(".foxgarden/drafts/Main.java.draft").exists(),
        "and the stale draft is gone"
    );
}

#[test]
fn a_draft_for_a_deleted_file_is_dropped() {
    let (_dir, root) = project();
    let file = root.join("Main.java");
    write_draft(&root, &file, "unsaved").unwrap();
    std::fs::remove_file(&file).unwrap();

    assert!(pending_drafts(&root).is_empty());
}

#[test]
fn discard_removes_the_draft_and_is_fine_without_one() {
    let (_dir, root) = project();
    let file = root.join("Main.java");
    write_draft(&root, &file, "unsaved").unwrap();

    discard_draft(&root, &file).unwrap();
    assert!(pending_drafts(&root).is_empty());
    discard_draft(&root, &file).expect("discarding twice is not an error");
}

#[test]
fn drafts_mirror_the_project_s_own_directory_structure() {
    let (_dir, root) = project();
    std::fs::create_dir_all(root.join("src/main/java")).unwrap();
    let nested = root.join("src/main/java/App.java");
    std::fs::write(&nested, "class App {}\n").unwrap();

    write_draft(&root, &nested, "class App { // unsaved\n}\n").unwrap();

    assert!(root.join(".foxgarden/drafts/src/main/java/App.java.draft").is_file());
    assert_eq!(pending_drafts(&root)[0].file_path, nested);
}

#[test]
fn a_file_outside_the_project_has_no_draft() {
    let (_dir, root) = project();
    let outside = std::path::Path::new("/somewhere/else/Other.java");

    write_draft(&root, outside, "unsaved").unwrap();

    assert!(pending_drafts(&root).is_empty());
}
