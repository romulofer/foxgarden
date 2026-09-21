
use super::*;

#[test]
fn start_does_nothing_when_the_caret_touches_no_identifier() {
    let (_dir, doc) = test_support::temp_document("Foo.java", "a . b");
    let mut rename = RenameBox::default();
    rename.start(&doc, 2); // the standalone "." — no identifier here
    assert!(rename.open.is_none());
}

#[test]
fn start_prefills_the_identifier_under_the_caret() {
    let (_dir, doc) = test_support::temp_document("Foo.java", "int myVariable = 1;");
    let mut rename = RenameBox::default();
    rename.start(&doc, 6); // inside "myVariable"
    let open = rename.open.as_ref().expect("a real identifier was under the caret");
    assert_eq!(open.input, "myVariable");
    assert_eq!(open.original, "myVariable");
}
