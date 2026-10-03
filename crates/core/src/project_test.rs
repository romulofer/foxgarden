use super::*;

#[test]
fn remove_path_drops_a_file_and_a_whole_subtree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/main")).unwrap();
    std::fs::write(root.join("src/main/Main.java"), "class Main {}").unwrap();
    std::fs::write(root.join("README.md"), "").unwrap();
    let mut project = Project::open(root.to_path_buf()).unwrap();

    assert!(project.remove_path(&root.join("src/main/Main.java")));
    assert!(!project.directories().is_empty());
    assert!(project.remove_path(&root.join("src")));
    assert!(!project.directories().contains(&root.join("src/main")));
    assert!(!project.remove_path(&root.join("does/not/exist")));
}

#[test]
fn insert_path_lands_where_a_rebuilt_tree_would_have_put_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("README.md"), "").unwrap();
    let mut project = Project::open(root.to_path_buf()).unwrap();

    std::fs::write(root.join("src/New.java"), "class New {}").unwrap();
    assert!(project.insert_path(&root.join("src/New.java"), FileKind::File));

    let rebuilt = Project::open(root.to_path_buf()).unwrap();
    assert_eq!(project, rebuilt, "an inserted node must match a full rebuild");
}

#[test]
fn insert_path_creates_missing_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut project = Project::open(root.to_path_buf()).unwrap();

    std::fs::create_dir_all(root.join("src/main/java")).unwrap();
    std::fs::write(root.join("src/main/java/A.java"), "class A {}").unwrap();
    assert!(project.insert_path(&root.join("src/main/java/A.java"), FileKind::File));

    assert_eq!(project, Project::open(root.to_path_buf()).unwrap());
}

#[test]
fn insert_path_ignores_anything_outside_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = Project::open(dir.path().to_path_buf()).unwrap();
    assert!(!project.insert_path(Path::new("/somewhere/else/A.java"), FileKind::File));
}

#[test]
fn directories_lists_every_directory_and_skips_the_ignored_ones() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/main/java")).unwrap();
    std::fs::create_dir_all(root.join("target/classes")).unwrap();
    std::fs::write(root.join("src/main/java/Main.java"), "class Main {}").unwrap();

    let project = Project::open(root.to_path_buf()).unwrap();
    let dirs = project.directories();

    assert!(dirs.contains(&root.to_path_buf()));
    assert!(dirs.contains(&root.join("src/main/java")));
    assert!(!dirs.iter().any(|d| d.starts_with(root.join("target"))));
}

#[test]
fn builds_nested_tree_with_empty_dirs_and_mixed_file_types() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/main/java")).unwrap();
    std::fs::create_dir_all(root.join("empty_dir")).unwrap();
    std::fs::write(root.join("src/main/java/Main.java"), "class Main {}").unwrap();
    std::fs::write(root.join("pom.xml"), "<project/>").unwrap();

    let project = Project::open(root.to_path_buf()).unwrap();
    assert_eq!(project.root, root);
    assert_eq!(project.tree.kind, FileKind::Dir);

    // dirs before files, each group sorted: empty_dir, src, pom.xml
    let names: Vec<&str> = project.tree.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, vec!["empty_dir", "src", "pom.xml"]);

    let empty_dir = &project.tree.children[0];
    assert_eq!(empty_dir.kind, FileKind::Dir);
    assert!(empty_dir.children.is_empty());

    let src = &project.tree.children[1];
    assert_eq!(src.kind, FileKind::Dir);
    let main = &src.children[0];
    assert_eq!(main.name, "main");
    let java = &main.children[0];
    assert_eq!(java.name, "java");
    let main_java = &java.children[0];
    assert_eq!(main_java.name, "Main.java");
    assert_eq!(main_java.kind, FileKind::File);

    let pom = &project.tree.children[2];
    assert_eq!(pom.kind, FileKind::File);
    assert!(pom.children.is_empty());
}

#[test]
fn skips_git_target_and_node_modules_without_descending_into_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join(".git/objects")).unwrap();
    std::fs::write(root.join(".git/objects/deadbeef"), "").unwrap();
    std::fs::create_dir_all(root.join("target/classes")).unwrap();
    std::fs::write(root.join("target/classes/Main.class"), "").unwrap();
    std::fs::create_dir_all(root.join("node_modules/some-pkg")).unwrap();
    // This app's own state directory: history snapshots and drafts are
    // bookkeeping about the project, not files to browse.
    std::fs::create_dir_all(root.join(".foxgarden/drafts")).unwrap();
    std::fs::write(root.join(".foxgarden/drafts/Main.java.draft"), "").unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/Main.java"), "class Main {}").unwrap();

    let project = Project::open(root.to_path_buf()).unwrap();

    let names: Vec<&str> = project.tree.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, vec!["src"], "only src should remain in the tree");
}
