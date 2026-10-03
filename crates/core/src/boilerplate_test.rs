use super::*;
use std::path::PathBuf;

#[test]
fn java_with_maven_layout_gets_package_and_class() {
    let root = PathBuf::from("/repo");
    let file = PathBuf::from("/repo/src/main/java/com/example/foo/Bar.java");
    let out = generate(Language::Java, &root, &file);
    assert_eq!(out, "package com.example.foo;\n\npublic class Bar {\n\n}\n");
}

#[test]
fn kotlin_with_gradle_layout_gets_package_and_class() {
    let root = PathBuf::from("/repo");
    let file = PathBuf::from("/repo/src/main/kotlin/com/example/Bar.kt");
    let out = generate(Language::Kotlin, &root, &file);
    assert_eq!(out, "package com.example\n\nclass Bar {\n\n}\n");
}

#[test]
fn test_source_root_is_also_recognized() {
    let root = PathBuf::from("/repo");
    let file = PathBuf::from("/repo/src/test/java/com/example/BarTest.java");
    let out = generate(Language::Java, &root, &file);
    assert_eq!(out, "package com.example;\n\npublic class BarTest {\n\n}\n");
}

#[test]
fn file_directly_under_source_root_has_no_package() {
    let root = PathBuf::from("/repo");
    let file = PathBuf::from("/repo/src/main/java/Bar.java");
    let out = generate(Language::Java, &root, &file);
    assert_eq!(out, "public class Bar {\n\n}\n");
}

#[test]
fn file_outside_maven_layout_has_no_package() {
    let root = PathBuf::from("/repo");
    let file = PathBuf::from("/repo/Bar.java");
    let out = generate(Language::Java, &root, &file);
    assert_eq!(out, "public class Bar {\n\n}\n");
}
