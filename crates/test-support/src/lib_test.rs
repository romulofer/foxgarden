use super::*;

#[test]
fn write_file_creates_missing_parent_directories() {
    let dir = tempdir();
    let path = write_file(
        dir.path(),
        "controllers/api/UserController.java",
        "class UserController {}",
    );
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
