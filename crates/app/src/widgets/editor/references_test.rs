
use super::*;

fn doc(path: &str, text: &str) -> Document {
    let (_dir, doc) = test_support::temp_document(path, text);
    doc
}

#[test]
fn decode_references_a_null_result_is_empty() {
    let d = doc("Foo.java", "class Foo {}");
    assert!(decode_references(serde_json::Value::Null, &d).is_empty());
}

#[test]
fn decode_references_reads_the_current_docs_own_live_buffer_for_a_same_file_hit() {
    let d = doc("Foo.java", "class Foo {\n    int x;\n}\n");
    let uri = format!("file://{}", d.path.display());
    let value = serde_json::json!([{
        "uri": uri,
        "range": { "start": { "line": 1, "character": 4 }, "end": { "line": 1, "character": 5 } }
    }]);
    let hits = decode_references(value, &d);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 2);
    assert_eq!(hits[0].preview.as_deref(), Some("    int x;"));
}

#[test]
fn decode_references_drops_a_location_whose_file_cant_be_read() {
    let d = doc("Foo.java", "class Foo {}");
    let value = serde_json::json!([{
        "uri": "file:///does/not/exist/Nowhere.java",
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }
    }]);
    assert!(decode_references(value, &d).is_empty());
}

#[test]
fn decode_references_reads_multiple_hits_in_source_order() {
    let d = doc("Foo.java", "class Foo {\n    void a() {}\n    void b() {}\n}\n");
    let uri = format!("file://{}", d.path.display());
    let value = serde_json::json!([
        { "uri": uri, "range": { "start": { "line": 1, "character": 9 }, "end": { "line": 1, "character": 10 } } },
        { "uri": uri, "range": { "start": { "line": 2, "character": 9 }, "end": { "line": 2, "character": 10 } } },
    ]);
    let hits = decode_references(value, &d);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].line, 2);
    assert_eq!(hits[1].line, 3);
}

#[test]
fn msg_reference_count_singular_vs_plural() {
    assert_eq!(msg_reference_count(1), "1 reference");
    assert_eq!(msg_reference_count(2), "2 references");
    assert_eq!(msg_reference_count(0), "0 references");
}
