//! Unit tests for [`super`](side_panel.rs), extracted verbatim from that
//! file's colocated `#[cfg(test)] mod tests` so the module file stays focused
//! on the code under test. Behavior-identical to the inline module it replaced.

    use super::*;

    #[test]
    fn create_file_with_parents_creates_missing_nested_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("controllers/api/UserController.java");

        create_file_with_parents(&path, "class UserController {}").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "class UserController {}");
    }

    #[test]
    fn create_file_with_parents_works_for_a_flat_path_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Main.java");

        create_file_with_parents(&path, "class Main {}").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "class Main {}");
    }

    #[test]
    fn create_file_with_parents_leaves_already_existing_directories_alone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("existing")).unwrap();
        let path = dir.path().join("existing/File.java");

        create_file_with_parents(&path, "class File {}").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "class File {}");
    }

    #[test]
    fn delete_path_removes_a_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("File.java");
        std::fs::write(&path, "class File {}").unwrap();

        delete_path(&path).unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn delete_path_removes_a_directory_and_everything_inside_it() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("pkg");
        std::fs::create_dir_all(subdir.join("nested")).unwrap();
        std::fs::write(subdir.join("A.java"), "class A {}").unwrap();
        std::fs::write(subdir.join("nested/B.java"), "class B {}").unwrap();

        delete_path(&subdir).unwrap();

        assert!(!subdir.exists());
    }

    #[test]
    fn is_invalid_paste_target_rejects_pasting_a_directory_into_itself() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("pkg");
        std::fs::create_dir_all(&subdir).unwrap();

        assert!(is_invalid_paste_target(&subdir, &subdir));
    }

    #[test]
    fn is_invalid_paste_target_rejects_pasting_into_a_descendant() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("pkg");
        let nested = subdir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();

        assert!(is_invalid_paste_target(&subdir, &nested));
    }

    #[test]
    fn is_invalid_paste_target_allows_an_unrelated_directory() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("pkg");
        let other = dir.path().join("other");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&other).unwrap();

        assert!(!is_invalid_paste_target(&source, &other));
    }

    #[test]
    fn copy_recursive_copies_a_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("A.java");
        let dest = dir.path().join("B.java");
        std::fs::write(&source, "class A {}").unwrap();

        copy_recursive(&source, &dest).unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "class A {}");
        assert!(source.exists(), "copy must leave the original in place");
    }

    #[test]
    fn copy_recursive_copies_a_directory_and_its_contents() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("pkg");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::write(source.join("A.java"), "class A {}").unwrap();
        std::fs::write(source.join("nested/B.java"), "class B {}").unwrap();
        let dest = dir.path().join("pkg_copy");

        copy_recursive(&source, &dest).unwrap();

        assert_eq!(std::fs::read_to_string(dest.join("A.java")).unwrap(), "class A {}");
        assert_eq!(std::fs::read_to_string(dest.join("nested/B.java")).unwrap(), "class B {}");
        assert!(source.exists(), "copy must leave the original directory in place");
    }

    #[test]
    fn paste_into_with_copy_leaves_the_source_and_creates_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("A.java");
        let target_dir = dir.path().join("dest");
        std::fs::write(&source, "class A {}").unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();

        let result = paste_into(&source, &target_dir, ClipboardOp::Copy).unwrap();

        assert_eq!(result, target_dir.join("A.java"));
        assert_eq!(std::fs::read_to_string(&result).unwrap(), "class A {}");
        assert!(source.exists());
    }

    #[test]
    fn paste_into_with_cut_moves_the_source_to_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("A.java");
        let target_dir = dir.path().join("dest");
        std::fs::write(&source, "class A {}").unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();

        let result = paste_into(&source, &target_dir, ClipboardOp::Cut).unwrap();

        assert_eq!(result, target_dir.join("A.java"));
        assert_eq!(std::fs::read_to_string(&result).unwrap(), "class A {}");
        assert!(!source.exists(), "cut must remove the original");
    }

    #[test]
    fn paste_into_fails_on_a_name_collision_at_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("A.java");
        let target_dir = dir.path().join("dest");
        std::fs::write(&source, "class A {}").unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(target_dir.join("A.java"), "already here").unwrap();

        let err = paste_into(&source, &target_dir, ClipboardOp::Copy).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        // Neither side should have been touched by the failed attempt.
        assert_eq!(std::fs::read_to_string(target_dir.join("A.java")).unwrap(), "already here");
        assert!(source.exists());
    }
