use super::*;
use crate::IncrementalParser;
use fg_extension::MemberKind;

fn parsed(language: Language, source: &str) -> Tree {
    let mut parser = IncrementalParser::new(language).expect("a bundled grammar must load");
    parser.parse(source).clone()
}

#[test]
fn the_shipped_extension_answers_for_java() {
    let source = "class Foo extends Base {\n    private int x;\n    void run(Bar bar) {\n        int here = 0;\n    }\n}\n";
    let tree = parsed(Language::Java, source);
    let byte = source.find("int here").unwrap();

    assert_eq!(enclosing_type(&tree, source, Language::Java, byte).unwrap().name, "Foo");
    assert_eq!(supertype(&tree, source, Language::Java, "Foo").as_deref(), Some("Base"));
    assert_eq!(receiver_type(&tree, source, Language::Java, byte, "bar").unwrap().type_name, "Bar");

    let members = type_members(&tree, source, Language::Java, "Foo", MemberView::Inside);
    let kinds: Vec<(&str, MemberKind)> = members.iter().map(|m| (m.name.as_str(), m.kind)).collect();
    assert_eq!(kinds, [("x", MemberKind::Field), ("run", MemberKind::Method)]);
}

#[test]
fn code_generation_goes_to_the_extension_that_offers_it() {
    assert!(generates_code(Language::Java));
    assert!(!generates_code(Language::Kotlin));
    assert!(!generates_code(Language::Yaml));

    let source = "class Foo {\n    private int x;\n}\n";
    let tree = parsed(Language::Java, source);
    let types = types_with_fields(&tree, source, Language::Java);
    assert_eq!(types.len(), 1);

    let generated = generate_code(
        Language::Java,
        CodeGeneration::Accessors {
            fields: &types[0].fields,
            getters: true,
            setters: false,
        },
        "    ",
    )
    .unwrap();
    assert!(generated.contains("getX()"));
    assert_eq!(
        generate_code(Language::Yaml, CodeGeneration::Overrides { methods: &[] }, "    "),
        None
    );
}

#[test]
fn a_language_nobody_models_gets_no_answers() {
    let source = "key: value\n";
    let tree = parsed(Language::Yaml, source);
    assert_eq!(enclosing_type(&tree, source, Language::Yaml, 0), None);
    assert_eq!(receiver_type(&tree, source, Language::Yaml, 0, "this"), None);
    assert!(type_members(&tree, source, Language::Yaml, "key", MemberView::Inside).is_empty());
}

#[test]
fn source_file_extensions_come_from_the_registered_language() {
    assert_eq!(source_file_extensions(Language::Java), ["java"]);
    assert_eq!(source_file_extensions(Language::Kotlin), ["kt"]);
    assert!(source_file_extensions(Language::new("never-registered")).is_empty());
}
