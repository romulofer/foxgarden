
use std::str::FromStr;

use super::*;

#[test]
fn first_location_reads_a_scalar_location() {
    let value = serde_json::json!({
        "uri": "file:///a/Foo.java",
        "range": { "start": { "line": 1, "character": 2 }, "end": { "line": 1, "character": 5 } }
    });
    let (uri, range) = first_location(value).unwrap();
    assert_eq!(uri.as_str(), "file:///a/Foo.java");
    assert_eq!(range.start.line, 1);
}

#[test]
fn first_location_reads_the_first_entry_of_an_array() {
    let value = serde_json::json!([
        { "uri": "file:///a/Foo.java", "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } } },
        { "uri": "file:///a/Bar.java", "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } } }
    ]);
    let (uri, _) = first_location(value).unwrap();
    assert_eq!(uri.as_str(), "file:///a/Foo.java");
}

#[test]
fn first_location_reads_a_location_links_own_target_selection_range_not_its_target_range() {
    let value = serde_json::json!([{
        "targetUri": "jdt://contents/rt.jar/java.lang/String.class?=x",
        "targetRange": { "start": { "line": 0, "character": 0 }, "end": { "line": 100, "character": 0 } },
        "targetSelectionRange": { "start": { "line": 10, "character": 4 }, "end": { "line": 10, "character": 10 } }
    }]);
    let (uri, range) = first_location(value).unwrap();
    assert!(uri.as_str().starts_with("jdt:"));
    assert_eq!(range.start.line, 10);
}

#[test]
fn first_location_a_null_result_is_none() {
    assert!(first_location(serde_json::Value::Null).is_none());
}

#[test]
fn first_location_an_empty_array_is_none() {
    assert!(first_location(serde_json::json!([])).is_none());
}

#[test]
fn virtual_cache_path_names_the_file_from_the_uris_trailing_class_segment() {
    let path = virtual_cache_path("jdt://contents/rt.jar/java.lang/String.class?=x", "java").unwrap();
    let name = path.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("String-"), "unexpected cache file name: {name}");
    assert!(name.ends_with(".java"));
}

#[test]
fn virtual_cache_path_is_stable_across_calls_for_the_same_uri() {
    let a = virtual_cache_path("jdt://contents/rt.jar/java.lang/String.class?=x", "java").unwrap();
    let b = virtual_cache_path("jdt://contents/rt.jar/java.lang/String.class?=x", "java").unwrap();
    assert_eq!(a, b);
}

#[test]
fn virtual_cache_path_differs_for_two_uris_with_the_same_trailing_class_name() {
    // Two different JARs can each contain a same-named class — the
    // human-readable name alone must never be what dedupes the cache.
    let a = virtual_cache_path("jdt://contents/rt.jar/java.lang/String.class?=x", "java").unwrap();
    let b = virtual_cache_path("jdt://contents/other.jar/some.pkg/String.class?=y", "java").unwrap();
    assert_ne!(a, b);
}

#[test]
fn handle_source_writes_the_reply_text_and_resolves_the_byte_offset() {
    let uri = Uri::from_str("jdt://contents/rt.jar/java.lang/String.class?=handle_source_test").unwrap();
    let range = lsp_types::Range {
        start: lsp_types::Position { line: 0, character: 6 },
        end: lsp_types::Position { line: 0, character: 6 },
    };
    let value = serde_json::json!("class String {}");
    let target = handle_source(&uri, range, "java", value).expect("decodes a plain string reply");
    let Target::Ready { path, byte_offset } = target else {
        panic!("expected Target::Ready")
    };
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "class String {}");
    assert_eq!(byte_offset, 6);
    let _ = std::fs::remove_file(&path);
}

/// With no language server to ask, a call to a Kotlin Multiplatform `expect`
/// still resolves, through the extension that knows the layout.
#[test]
fn a_definition_the_server_cannot_give_falls_back_to_the_extensions() {
    test_support::install_grammars();
    let dir = test_support::tempdir();
    test_support::write_file(dir.path(), "build.gradle.kts", "plugins { kotlin(\"multiplatform\") }");
    let common = test_support::write_file(dir.path(), "src/commonMain/kotlin/P.kt", "expect fun platformName(): String\n");
    let user = test_support::write_file(dir.path(), "src/jvmMain/kotlin/U.kt", "fun use() = platformName()\n");
    let mut doc = fg_core::Document::open(user, test_support::languages()).unwrap();
    let byte = doc.buffer.to_string().find("platformName").unwrap() + 2;

    let mut state = GotoDefinitionState::default();
    let mut lsp = LspState::default();
    state.request(&mut doc, byte, &mut lsp);
    assert!(state.wants_repaint(), "the fallback is pending until polled");

    let Some(Target::Source { path, byte_offset }) = state.poll(&mut lsp, Some(dir.path())) else {
        panic!("the expect should have been found");
    };
    assert_eq!(path, common);
    assert_eq!(byte_offset, "expect fun ".len());
    assert!(!state.wants_repaint());
    assert!(state.poll(&mut lsp, Some(dir.path())).is_none(), "the fallback is used once");
}

#[test]
fn without_a_project_the_fallback_finds_nothing() {
    let (_dir, mut doc) = test_support::temp_document("U.kt", "fun use() = platformName()\n");
    let mut state = GotoDefinitionState::default();
    let mut lsp = LspState::default();
    state.request(&mut doc, 14, &mut lsp);
    assert!(state.poll(&mut lsp, None).is_none());
    assert!(!state.wants_repaint());
}
