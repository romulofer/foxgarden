use super::*;

fn snapshot_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.sort();
    files
}

#[test]
fn list_snapshots_is_empty_with_no_history_yet() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert_eq!(list_snapshots(root, &root.join("Main.java")), vec![]);
}

#[test]
fn list_snapshots_is_empty_outside_the_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    assert_eq!(list_snapshots(&root, &dir.path().join("elsewhere/Main.java")), vec![]);
}

#[test]
fn list_snapshots_returns_every_write_most_recent_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file_path = root.join("Main.java");

    write_snapshot(root, &file_path, "v1").unwrap();
    write_snapshot(root, &file_path, "v2").unwrap();
    write_snapshot(root, &file_path, "v3").unwrap();

    let snapshots = list_snapshots(root, &file_path);
    assert_eq!(snapshots.len(), 3);
    assert!(
        snapshots
            .windows(2)
            .all(|pair| pair[0].timestamp_nanos >= pair[1].timestamp_nanos)
    );
    assert_eq!(std::fs::read_to_string(&snapshots[0].path).unwrap(), "v3");
    assert_eq!(std::fs::read_to_string(&snapshots[2].path).unwrap(), "v1");
}

#[test]
fn write_snapshot_creates_a_file_under_the_relative_history_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file_path = root.join("src/main/java/Main.java");

    write_snapshot(root, &file_path, "class Main {}").unwrap();

    let history_dir = root.join(".foxgarden/history/src/main/java/Main.java");
    let files = snapshot_files(&history_dir);
    assert_eq!(files.len(), 1);
    assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), "class Main {}");
}

#[test]
fn write_snapshot_outside_the_project_root_is_a_silent_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    let outside_file = dir.path().join("elsewhere/Main.java");

    write_snapshot(&root, &outside_file, "class Main {}").unwrap();

    assert!(!root.join(".foxgarden").exists());
}

#[test]
fn repeated_writes_accumulate_multiple_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file_path = root.join("Main.java");

    write_snapshot(root, &file_path, "v1").unwrap();
    write_snapshot(root, &file_path, "v2").unwrap();
    write_snapshot(root, &file_path, "v3").unwrap();

    let history_dir = root.join(".foxgarden/history/Main.java");
    assert_eq!(snapshot_files(&history_dir).len(), 3);
}

#[test]
fn writes_beyond_the_cap_prune_the_oldest_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let history_dir = root.join(".foxgarden/history/Main.java");
    std::fs::create_dir_all(&history_dir).unwrap();

    // Seeds `SNAPSHOT_CAP` synthetic, strictly increasing timestamps
    // directly (rather than looping a real `write_snapshot`
    // `SNAPSHOT_CAP + 1` times, which `SystemTime::now()`'s coarse
    // millisecond resolution could collide on if the loop runs fast
    // enough) so this test stays fast and deterministic.
    for timestamp in 0..SNAPSHOT_CAP {
        std::fs::write(history_dir.join(format!("{timestamp}.snapshot")), "old").unwrap();
    }
    prune_snapshots(&history_dir).unwrap();
    assert_eq!(snapshot_files(&history_dir).len(), SNAPSHOT_CAP);

    // One more write pushes it over the cap; the very oldest (`0.
    // snapshot`) should be the one pruned away, not an arbitrary one.
    std::fs::write(history_dir.join(format!("{SNAPSHOT_CAP}.snapshot")), "new").unwrap();
    prune_snapshots(&history_dir).unwrap();

    let files = snapshot_files(&history_dir);
    assert_eq!(files.len(), SNAPSHOT_CAP);
    assert!(!history_dir.join("0.snapshot").exists());
    assert!(history_dir.join(format!("{SNAPSHOT_CAP}.snapshot")).exists());
}
