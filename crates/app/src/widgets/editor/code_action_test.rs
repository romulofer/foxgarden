
use super::*;

#[test]
fn diagnostic_on_line_finds_a_diagnostic_starting_on_that_line() {
    let (_dir, mut doc) = test_support::temp_document("Foo.java", "class Foo {\n  int x;\n}");
    doc.lsp_diagnostics = vec![fg_core::Diagnostic {
        range: 14..19, // "int x" on line 1
        severity: fg_core::Severity::Warning,
        message: "unused variable".to_string(),
    }];
    let found = diagnostic_on_line(&doc, 1).expect("a diagnostic starts on line 1");
    assert_eq!(found.message, "unused variable");
}

#[test]
fn diagnostic_on_line_is_none_when_no_diagnostic_starts_there() {
    let (_dir, mut doc) = test_support::temp_document("Foo.java", "class Foo {\n  int x;\n}");
    doc.lsp_diagnostics = vec![fg_core::Diagnostic {
        range: 14..19,
        severity: fg_core::Severity::Warning,
        message: "unused variable".to_string(),
    }];
    assert!(diagnostic_on_line(&doc, 0).is_none());
}

#[test]
fn offers_from_response_keeps_only_actions_with_a_real_edit() {
    let value = serde_json::json!([
        { "title": "Organize imports", "kind": "source.organizeImports" },
        {
            "title": "Remove unused import",
            "kind": "quickfix",
            "edit": {
                "changes": {
                    "file:///a/Foo.java": [
                        { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 1, "character": 0 } }, "newText": "" }
                    ]
                }
            }
        }
    ]);
    let offers = offers_from_response(value);
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].title, "Remove unused import");
}

#[test]
fn offers_from_response_a_null_result_is_empty() {
    assert!(offers_from_response(serde_json::Value::Null).is_empty());
}

fn tracked_with_offers(offers: Vec<Offer>) -> CodeActionGutter {
    CodeActionGutter {
        tracked: Some(Tracked {
            doc_path: PathBuf::from("/a/Main.java"),
            line: 0,
            version: 0,
            pending: None,
            offers,
            open: false,
        }),
        confirmed: None,
    }
}

fn dummy_offer() -> Offer {
    Offer {
        title: "Organize imports".to_string(),
        edit: lsp_types::WorkspaceEdit::default(),
    }
}

#[test]
fn open_picker_opens_the_popup_when_the_caret_line_has_an_offer() {
    let mut gutter = tracked_with_offers(vec![dummy_offer()]);
    gutter.open_picker();
    assert!(gutter.tracked.as_ref().unwrap().open);
}

#[test]
fn open_picker_is_a_no_op_with_no_offer_to_show() {
    let mut gutter = tracked_with_offers(Vec::new());
    gutter.open_picker();
    assert!(!gutter.tracked.as_ref().unwrap().open);

    let mut empty = CodeActionGutter::default();
    empty.open_picker();
    assert!(empty.tracked.is_none());
}

/// Real captured jdtls 1.60.0 reply (trimmed), live-verifying this
/// checkpoint's own example (an unused import): every offer it sent —
/// "Organize imports" among them — came back as a bare `Command`
/// named `java.apply.workspaceEdit` whose single argument *is* the
/// edit, not a `CodeAction` literal with its own `edit` field. Without
/// `offer_from_item`'s special case for this exact shape, this whole
/// reply parsed to zero offers — a real gap this test pins down.
#[test]
fn offers_from_response_reads_jdtls_own_java_apply_workspace_edit_command() {
    let value = serde_json::json!([
        {
            "title": "Organize imports",
            "command": "java.apply.workspaceEdit",
            "arguments": [{
                "changes": {
                    "file:///a/Main.java": [
                        { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 1, "character": 0 } }, "newText": "" }
                    ]
                }
            }]
        },
        { "title": "Some other server-side command", "command": "java.some.other.command" }
    ]);
    let offers = offers_from_response(value);
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].title, "Organize imports");
}
