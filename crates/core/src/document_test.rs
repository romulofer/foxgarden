use super::*;

#[test]
fn edit_marks_document_dirty() {
    let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
    let mut doc = Document::open(path, test_support::languages()).unwrap();
    assert!(!doc.is_dirty());

    doc.buffer.insert(0, "// comment\n");
    assert!(doc.is_dirty());
}

#[test]
fn save_clears_dirty_state() {
    let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
    let mut doc = Document::open(path.clone(), test_support::languages()).unwrap();

    doc.buffer.insert(0, "// comment\n");
    assert!(doc.is_dirty());

    doc.save(true).unwrap();
    assert!(!doc.is_dirty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), doc.buffer.to_string());
}

#[test]
fn edit_undone_to_original_content_is_not_dirty() {
    let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
    let mut doc = Document::open(path, test_support::languages()).unwrap();

    doc.buffer.insert(0, "// comment\n");
    assert!(doc.is_dirty());

    doc.buffer.remove(0.."// comment\n".len());
    assert!(!doc.is_dirty());
}

#[test]
fn save_snapshots_into_history_when_project_root_is_set() {
    let (dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
    let mut doc = Document::open(path, test_support::languages()).unwrap();
    doc.project_root = Some(dir.path().to_path_buf());

    doc.buffer.insert(0, "// comment\n");
    doc.save(true).unwrap();

    let history_dir = dir.path().join(".foxgarden/history/Hello.java");
    let snapshots: Vec<_> = std::fs::read_dir(&history_dir).unwrap().collect();
    assert_eq!(snapshots.len(), 1);
}

#[test]
fn save_writes_no_history_without_a_project_root() {
    let (dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
    let mut doc = Document::open(path, test_support::languages()).unwrap();
    assert_eq!(doc.project_root, None);

    doc.buffer.insert(0, "// comment\n");
    doc.save(true).unwrap();

    assert!(!dir.path().join(".foxgarden").exists());
}

#[test]
fn unrecognized_extension_opens_as_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("readme.txt");
    std::fs::write(&path, "hello").unwrap();

    let doc = Document::open(path, test_support::languages()).unwrap();
    assert_eq!(doc.language, None);
    assert_eq!(doc.buffer.to_string(), "hello");
}

#[test]
fn extensionless_file_opens_as_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("README");
    std::fs::write(&path, "hello").unwrap();

    let doc = Document::open(path, test_support::languages()).unwrap();
    assert_eq!(doc.language, None);
}

#[test]
fn save_trims_trailing_whitespace_from_every_line() {
    let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
    let mut doc = Document::open(path.clone(), test_support::languages()).unwrap();

    doc.buffer
        .replace(Rope::from_str("class Hello {   \n\tint x;\t\t\n}   \n"));
    doc.save(true).unwrap();

    let expected = "class Hello {\n\tint x;\n}\n";
    assert_eq!(doc.buffer.to_string(), expected);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    assert!(!doc.is_dirty(), "buffer and saved_buffer must agree right after save");
}

#[test]
fn save_trims_a_final_line_with_no_trailing_newline() {
    let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
    let mut doc = Document::open(path, test_support::languages()).unwrap();

    doc.buffer.replace(Rope::from_str("class Hello {}  "));
    doc.save(true).unwrap();

    // No newline should be added where there wasn't one.
    assert_eq!(doc.buffer.to_string(), "class Hello {}");
}

#[test]
fn save_leaves_trailing_whitespace_alone_when_trimming_is_off() {
    let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
    let mut doc = Document::open(path.clone(), test_support::languages()).unwrap();

    let untouched = "class Hello {   \n\tint x;\t\t\n}   \n";
    doc.buffer.replace(Rope::from_str(untouched));
    doc.save(false).unwrap();

    assert_eq!(doc.buffer.to_string(), untouched);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), untouched);
    assert!(!doc.is_dirty());
}

#[test]
fn save_preserves_crlf_line_endings_while_trimming() {
    let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
    let mut doc = Document::open(path, test_support::languages()).unwrap();

    doc.buffer.replace(Rope::from_str("class Hello {}  \r\n  int x;\r\n"));
    doc.save(true).unwrap();

    assert_eq!(doc.buffer.to_string(), "class Hello {}\r\n  int x;\r\n");
}

#[test]
fn binary_file_is_rejected_without_reading_it_fully() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("icon.png");
    // A NUL a few bytes into an otherwise huge file: if `open` fell
    // through to `read_to_string`, this would still succeed in reading
    // (then fail UTF-8 validation) — the point of `looks_binary` is to
    // catch it from the first 8KB alone, before that full read happens.
    let mut contents = vec![0x89, b'P', b'N', b'G', 0x00];
    contents.extend(std::iter::repeat_n(b'a', 50 * 1024 * 1024));
    std::fs::write(&path, &contents).unwrap();

    let result = Document::open(path.clone(), test_support::languages());
    assert!(matches!(result, Err(OpenDocumentError::Binary(p)) if p == path));
}
