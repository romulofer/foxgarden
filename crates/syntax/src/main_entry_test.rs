use super::*;
use crate::IncrementalParser;

fn java(source: &str) -> Vec<MainEntry> {
    let mut parser = IncrementalParser::new(Language::Java);
    let tree = parser.parse(source).clone();
    main_entries(&tree, source, Language::Java, "Main")
}

fn kotlin(source: &str, file_stem: &str) -> Vec<MainEntry> {
    let mut parser = IncrementalParser::new(Language::Kotlin);
    let tree = parser.parse(source).clone();
    main_entries(&tree, source, Language::Kotlin, file_stem)
}

#[test]
fn a_plain_java_main_is_found_with_its_package() {
    let entries = java(
        "package com.example;\n\
             \n\
             public class Main {\n\
             \x20   public static void main(String[] args) {\n\
             \x20   }\n\
             }\n",
    );
    assert_eq!(
        entries,
        vec![MainEntry {
            line: 3,
            main_class: "com.example.Main".to_string(),
            label: "Main".to_string(),
        }]
    );
}

#[test]
fn a_file_with_no_package_uses_the_bare_class_name() {
    let entries = java("public class Main {\n    public static void main(String[] args) {}\n}\n");
    assert_eq!(entries[0].main_class, "Main");
}

#[test]
fn a_varargs_main_counts_too() {
    let entries = java("class Main {\n    public static void main(String... args) {}\n}\n");
    assert_eq!(entries.len(), 1);
}

#[test]
fn a_nested_class_main_uses_the_binary_name() {
    let entries = java(
        "package com.example;\n\
             class Outer {\n\
             \x20   static class Inner {\n\
             \x20       public static void main(String[] args) {}\n\
             \x20   }\n\
             }\n",
    );
    assert_eq!(entries[0].main_class, "com.example.Outer$Inner");
    assert_eq!(
        entries[0].label, "Inner",
        "the tooltip names the class that actually runs"
    );
}

#[test]
fn a_non_static_or_wrongly_typed_main_is_not_an_entry_point() {
    assert!(java("class Main {\n    public void main(String[] args) {}\n}\n").is_empty());
    assert!(java("class Main {\n    public static int main(String[] args) { return 0; }\n}\n").is_empty());
    assert!(java("class Main {\n    public static void main(int count) {}\n}\n").is_empty());
    assert!(java("class Main {\n    public static void main() {}\n}\n").is_empty());
    assert!(java("class Main {\n    public static void run(String[] args) {}\n}\n").is_empty());
}

#[test]
fn a_kotlin_top_level_main_compiles_into_the_files_own_class() {
    let entries = kotlin("package com.example\n\nfun main() {\n    println(\"hi\")\n}\n", "App");
    assert_eq!(
        entries,
        vec![MainEntry {
            line: 2,
            main_class: "com.example.AppKt".to_string(),
            label: "AppKt".to_string(),
        }]
    );
}

#[test]
fn a_kotlin_main_inside_a_class_is_not_offered() {
    let entries = kotlin("class Runner {\n    fun main() {}\n}\n", "Runner");
    assert!(entries.is_empty());
}

#[test]
fn a_lowercase_kotlin_file_name_is_capitalized() {
    assert_eq!(kotlin_file_class_name("main"), "MainKt");
    assert_eq!(kotlin_file_class_name("my-app"), "My_appKt");
}
