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

use std::sync::OnceLock;
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
    install_grammars();
    let (dir, path) = temp_file(name, contents);
    let doc = Document::open(path, languages()).expect("open just-written fixture file");
    (dir, doc)
}

/// An `EditorState` that recognizes this build's languages.
///
/// The right default for essentially every test, and the reason this
/// exists rather than letting tests call `EditorState::new()`: that
/// constructor now yields an editor with *no* languages registered, so a
/// test using it would still open files and still pass, while quietly no
/// longer exercising any language-dependent behavior at all. A test that
/// genuinely wants an editor that recognizes nothing should say so by
/// calling `EditorState::new()` deliberately.
pub fn editor_state() -> fg_core::EditorState {
    install_grammars();
    fg_core::EditorState::with_languages(fg_languages::builtin_registry())
}

/// Makes this build's grammars parseable, once per test binary.
///
/// Separate from `languages()` because they answer different questions:
/// the registry says *which* languages exist, `syntax`'s grammar store says
/// what can actually be parsed (`PLAN.md` Track 24 Phase 3). A test binary
/// that skips this still opens Java files and still resolves them as Java —
/// it just gets no parse tree, so anything about highlighting, folding,
/// completion or diagnostics quietly tests nothing. Every fixture here
/// installs for that reason; a test building a parser by hand should call
/// this itself.
///
/// Idempotent: the store keeps the grammar a language already has, so
/// several fixtures (and several tests) calling this is fine.
pub fn install_grammars() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let errors = syntax::install_grammars(languages());
        assert!(errors.is_empty(), "this build's own grammars must install: {errors:?}");
    });
}

/// The languages this build ships with, for tests that open real files and
/// expect them to be recognized.
///
/// Built once per test binary rather than per call: registering leaks each
/// language's id for the process (that is what keeps `Language` `Copy` —
/// see `PLAN.md` Track 24 Checkpoint 1), so a fresh registry per fixture
/// would leak a little more on every one of several hundred tests.
pub fn languages() -> &'static fg_extension::Registry {
    static LANGUAGES: OnceLock<fg_extension::Registry> = OnceLock::new();
    LANGUAGES.get_or_init(fg_languages::builtin_registry)
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
#[path = "lib_test.rs"]
mod lib_test;
