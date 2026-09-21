
use std::str::FromStr;

use super::*;

fn edit(start_line: u32, start_char: u32, end_line: u32, end_char: u32, new_text: &str) -> lsp_types::TextEdit {
    lsp_types::TextEdit {
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: start_line,
                character: start_char,
            },
            end: lsp_types::Position {
                line: end_line,
                character: end_char,
            },
        },
        new_text: new_text.to_string(),
    }
}

#[test]
fn apply_text_edits_replaces_a_single_occurrence() {
    let text = "int oldName = 1;";
    let edits = vec![edit(0, 4, 0, 11, "newName")];
    assert_eq!(apply_text_edits(text, &edits).unwrap(), "int newName = 1;");
}

#[test]
fn apply_text_edits_applies_multiple_edits_without_shifting_earlier_offsets() {
    let text = "oldName + oldName";
    let edits = vec![edit(0, 0, 0, 7, "newName"), edit(0, 10, 0, 17, "newName")];
    assert_eq!(apply_text_edits(text, &edits).unwrap(), "newName + newName");
}

#[test]
fn apply_text_edits_handles_a_rename_that_spans_multiple_lines() {
    let text = "oldName\n.field";
    let edits = vec![edit(0, 0, 0, 7, "newName")];
    assert_eq!(apply_text_edits(text, &edits).unwrap(), "newName\n.field");
}

#[test]
fn apply_text_edits_errors_on_a_range_outside_the_text() {
    let text = "short";
    let edits = vec![edit(5, 0, 5, 3, "x")];
    assert!(apply_text_edits(text, &edits).is_err());
}

#[test]
fn edits_by_file_prefers_document_changes_over_changes() {
    let uri = lsp_types::Uri::from_str("file:///a/Foo.java").unwrap();
    let document_edit = lsp_types::TextDocumentEdit {
        text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version: None,
        },
        edits: vec![lsp_types::OneOf::Left(edit(0, 0, 0, 3, "new"))],
    };
    let workspace_edit = lsp_types::WorkspaceEdit {
        changes: None,
        document_changes: Some(lsp_types::DocumentChanges::Edits(vec![document_edit])),
        change_annotations: None,
    };
    let result = edits_by_file(&workspace_edit);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].1.len(), 1);
}

#[test]
fn edits_by_file_skips_a_document_change_operation() {
    let workspace_edit = lsp_types::WorkspaceEdit {
        changes: None,
        document_changes: Some(lsp_types::DocumentChanges::Operations(Vec::new())),
        change_annotations: None,
    };
    assert!(edits_by_file(&workspace_edit).is_empty());
}
