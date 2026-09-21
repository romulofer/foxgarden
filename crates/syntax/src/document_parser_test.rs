use super::*;

#[test]
fn byte_to_point_tracks_rows_and_columns() {
    let text = "abc\ndef\nghi";
    assert_eq!(byte_to_point(text, 0), Point { row: 0, column: 0 });
    assert_eq!(byte_to_point(text, 3), Point { row: 0, column: 3 });
    assert_eq!(byte_to_point(text, 4), Point { row: 1, column: 0 });
    assert_eq!(byte_to_point(text, 9), Point { row: 2, column: 1 });
}

#[test]
fn diff_edit_detects_pure_insertion() {
    let old = "class Hello {}";
    let new = "class Hello { int x; }";
    let edit = diff_edit(old, new);
    assert_eq!(edit.start_byte, 13);
    assert_eq!(edit.old_end_byte, 13);
    assert_eq!(edit.new_end_byte, 21);
    assert_eq!(&new[edit.start_byte..edit.new_end_byte], " int x; ");
}

#[test]
fn diff_edit_detects_pure_deletion() {
    let old = "class Hello { int x; }";
    let new = "class Hello {}";
    let edit = diff_edit(old, new);
    assert_eq!(edit.start_byte, 13);
    assert_eq!(edit.old_end_byte, 21);
    assert_eq!(edit.new_end_byte, 13);
    assert_eq!(&old[edit.start_byte..edit.old_end_byte], " int x; ");
}

#[test]
fn diff_edit_detects_replacement() {
    let old = "let x = 1;";
    let new = "let x = 999;";
    let edit = diff_edit(old, new);
    assert_eq!(&old[edit.start_byte..edit.old_end_byte], "1");
    assert_eq!(&new[edit.start_byte..edit.new_end_byte], "999");
}

#[test]
fn diff_edit_handles_multibyte_boundary() {
    let old = "// caf\u{e9} shop\nlet x = 1;";
    let new = "// caf\u{e9} shop\nlet x = 42;";
    let edit = diff_edit(old, new);
    assert!(old.is_char_boundary(edit.start_byte));
    assert!(old.is_char_boundary(edit.old_end_byte));
    assert!(new.is_char_boundary(edit.new_end_byte));
    assert_eq!(&old[edit.start_byte..edit.old_end_byte], "1");
    assert_eq!(&new[edit.start_byte..edit.new_end_byte], "42");
}
