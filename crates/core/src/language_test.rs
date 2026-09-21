use super::*;

// Recognizing a file *as* a language moved out of this type when the set
// of languages opened up (`PLAN.md` Track 24 Phase 2): extensions and
// filename patterns are contributed now, so the tests that used to live
// here — every supported extension, the three Dockerfile spellings, the
// `README`/`docker-compose.yml` near-misses — moved with them, to
// `fg-languages`'s own suite. What is left to test here is the handle
// itself.

#[test]
fn a_language_is_its_id() {
    let java = Language::new("java");
    assert_eq!(java.id(), "java");
    assert_eq!(java.to_string(), "java");
}

#[test]
fn two_handles_for_the_same_id_are_the_same_language() {
    assert_eq!(Language::new("java"), Language::Java);
    assert_ne!(Language::new("java"), Language::Kotlin);
}

/// The shipped constants are exactly the ids the shipped extensions
/// register under — if these ever drift, every `Language::Java` comparison
/// in the tree silently stops matching the registry's own `"java"`.
#[test]
fn the_shipped_constants_spell_their_registered_ids() {
    assert_eq!(Language::Java.id(), "java");
    assert_eq!(Language::Kotlin.id(), "kotlin");
    assert_eq!(Language::Properties.id(), "properties");
    assert_eq!(Language::Yaml.id(), "yaml");
    assert_eq!(Language::Xml.id(), "xml");
    assert_eq!(Language::Dockerfile.id(), "dockerfile");
}

/// A language this build ships nothing for is still a perfectly ordinary
/// `Language` — that is the whole point of the type being open.
#[test]
fn an_unknown_id_is_a_valid_language() {
    let elvish = Language::new("elvish");
    assert_eq!(elvish.id(), "elvish");
    assert_ne!(elvish, Language::Java);
}
