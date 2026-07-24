//! Shared test-fixture helpers: a temp directory on disk plus a file (or
//! `Document`) inside it, ready for a test to open/edit/save against.
//!
//! Only ever pulled in as a `[dev-dependencies]` entry — `core`, `syntax`,
//! and `app` each depend on it purely so their own test modules stop
//! independently reimplementing "write a file into a fresh temp dir," the
//! same handful of lines that used to be copy-pasted (sometimes byte-for-
//! byte identically, e.g. `core::editor_state`'s and `app::app`'s own
//! `java_file` helpers before this crate existed) across half a dozen
//! files in two different crates. A `TempDir` deletes its directory when
//! dropped, so every function here returns one alongside whatever path (or
//! `Document`) it produced — callers must keep it alive (`let (_dir, ...)`,
//! not `let (_, ...)`) for as long as the path is used.

use std::path::{Path, PathBuf};

use fg_core::Document;
use tempfile::TempDir;

/// A fresh, empty temporary directory. Thin wrapper over
/// `tempfile::tempdir()` purely so every call site shares one `.expect`
/// message instead of independently choosing `.unwrap()` vs `.expect(...)`.
pub fn tempdir() -> TempDir {
    tempfile::tempdir().expect("create temp dir")
}

/// Writes `contents` to `dir.join(name)`, creating any missing parent
/// directories first (so a nested name like `"controllers/Foo.java"` works
/// without a separate `create_dir_all` call), and returns the full path.
/// For a test that needs several files in the same directory — a project
/// tree, a multi-file rename/delete scenario — call this once per file
/// against one shared `tempdir()`.
pub fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directories");
    }
    std::fs::write(&path, contents).expect("write fixture file");
    path
}

/// A fresh temp directory containing one file at `name` with `contents` —
/// the common single-file-fixture case. Combines `tempdir` + `write_file`
/// for the tests (the majority) that don't need more than one file.
pub fn temp_file(name: &str, contents: &str) -> (TempDir, PathBuf) {
    let dir = tempdir();
    let path = write_file(dir.path(), name, contents);
    (dir, path)
}

/// Like `temp_file`, but opens the result as a `Document` too — for
/// widget/editor tests that need a ready-to-edit document, not just a path
/// on disk.
pub fn temp_document(name: &str, contents: &str) -> (TempDir, Document) {
    let (dir, path) = temp_file(name, contents);
    let doc = Document::open(path).expect("open just-written fixture file");
    (dir, doc)
}

/// A fresh temp directory containing `name`, written with placeholder
/// content shaped like a (not necessarily syntactically valid — a `name`
/// like `"A.java"` produces `class A.java {}`, an illegal class name) Java
/// file. For tests that only care a file exists on disk and is distinct
/// from its siblings (tab lifecycle, project-tree ordering) — anything
/// that actually parses the content should use `temp_file`/`temp_document`
/// with real content instead.
pub fn placeholder_java_file(dir: &Path, name: &str) -> PathBuf {
    write_file(dir, name, &format!("class {name} {{}}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_file_creates_missing_parent_directories() {
        let dir = tempdir();
        let path = write_file(dir.path(), "controllers/api/UserController.java", "class UserController {}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "class UserController {}");
    }

    #[test]
    fn temp_file_returns_a_path_with_the_given_contents() {
        let (_dir, path) = temp_file("Hello.java", "class Hello {}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "class Hello {}");
    }

    #[test]
    fn temp_document_opens_the_file_it_just_wrote() {
        let (_dir, doc) = temp_document("Hello.java", "class Hello {}");
        assert_eq!(doc.buffer.to_string(), "class Hello {}");
    }

    #[test]
    fn placeholder_java_file_names_the_class_after_the_file() {
        let dir = tempdir();
        let path = placeholder_java_file(dir.path(), "A.java");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "class A.java {}");
    }
}
