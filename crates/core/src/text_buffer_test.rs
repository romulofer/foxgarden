use super::*;

#[test]
fn a_mutable_borrow_advances_the_revision() {
    let mut buffer = TextBuffer::new(Rope::from_str("class A {}"));
    let before = buffer.revision();

    buffer.insert(0, "// hi\n");

    assert_ne!(buffer.revision(), before);
}

#[test]
fn reads_never_advance_the_revision() {
    let buffer = TextBuffer::new(Rope::from_str("class A {}"));
    let before = buffer.revision();

    let _ = buffer.to_string();
    let _ = buffer.len_chars();
    let _ = buffer.line(0);

    assert_eq!(buffer.revision(), before);
}

#[test]
fn replace_advances_the_revision() {
    let mut buffer = TextBuffer::new(Rope::from_str("class A {}"));
    let before = buffer.revision();

    buffer.replace(Rope::from_str("class B {}"));

    assert_ne!(buffer.revision(), before);
    assert_eq!(buffer.to_string(), "class B {}");
}

#[test]
fn equality_is_by_text_not_by_edit_history() {
    let mut edited = TextBuffer::new(Rope::from_str("class A {}"));
    edited.insert(0, "x");
    edited.remove(0..1);

    let fresh = TextBuffer::new(Rope::from_str("class A {}"));
    assert_eq!(edited, fresh);
    assert_ne!(edited.revision(), fresh.revision());
}

/// A reopened file gets a new buffer; it must not share a revision with
/// the one it replaces, or caches keyed on (path, revision) would match.
#[test]
fn two_new_buffers_never_share_a_revision() {
    let first = TextBuffer::new(Rope::from_str("class A {}"));
    let second = TextBuffer::new(Rope::from_str("class A {}"));
    assert_ne!(first.revision(), second.revision());
}
