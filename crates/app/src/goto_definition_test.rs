
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
fn jdt_cache_path_names_the_file_from_the_uris_trailing_class_segment() {
    let path = jdt_cache_path("jdt://contents/rt.jar/java.lang/String.class?=x").unwrap();
    let name = path.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("String-"), "unexpected cache file name: {name}");
    assert!(name.ends_with(".java"));
}

#[test]
fn jdt_cache_path_is_stable_across_calls_for_the_same_uri() {
    let a = jdt_cache_path("jdt://contents/rt.jar/java.lang/String.class?=x").unwrap();
    let b = jdt_cache_path("jdt://contents/rt.jar/java.lang/String.class?=x").unwrap();
    assert_eq!(a, b);
}

#[test]
fn jdt_cache_path_differs_for_two_uris_with_the_same_trailing_class_name() {
    // Two different JARs can each contain a same-named class — the
    // human-readable name alone must never be what dedupes the cache.
    let a = jdt_cache_path("jdt://contents/rt.jar/java.lang/String.class?=x").unwrap();
    let b = jdt_cache_path("jdt://contents/other.jar/some.pkg/String.class?=y").unwrap();
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
    let target = handle_source(&uri, range, value).expect("decodes a plain string reply");
    let Target::Ready { path, byte_offset } = target else {
        panic!("expected Target::Ready")
    };
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "class String {}");
    assert_eq!(byte_offset, 6);
    let _ = std::fs::remove_file(&path);
}
