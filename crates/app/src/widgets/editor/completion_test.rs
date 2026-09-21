
use super::*;

fn rect(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
}

fn item(label: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: CompletionKind::Word,
        detail: None,
        has_params: false,
    }
}

fn method_item(label: &str, has_params: bool) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: CompletionKind::Method,
        detail: None,
        has_params,
    }
}

fn labels<'a>(items: &[&'a CompletionItem]) -> Vec<&'a str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

#[test]
fn filter_and_rank_narrows_by_prefix() {
    let candidates = [item("foo"), item("foobar"), item("bar")];
    let result = filter_and_rank(&candidates, "foo");
    assert_eq!(labels(&result), vec!["foo", "foobar"]);
}

#[test]
fn filter_and_rank_is_case_insensitive() {
    let candidates = [item("Foo"), item("bar")];
    let result = filter_and_rank(&candidates, "fo");
    assert_eq!(labels(&result), vec!["Foo"]);
}

#[test]
fn filter_and_rank_exact_case_sorts_before_case_insensitive_match() {
    let candidates = [item("Foo"), item("foo")];
    let result = filter_and_rank(&candidates, "foo");
    assert_eq!(labels(&result), vec!["foo", "Foo"]);
}

#[test]
fn filter_and_rank_shorter_labels_sort_before_longer_ones() {
    let candidates = [item("foobar"), item("foo")];
    let result = filter_and_rank(&candidates, "foo");
    assert_eq!(labels(&result), vec!["foo", "foobar"]);
}

#[test]
fn filter_and_rank_ties_break_alphabetically() {
    let candidates = [item("fooz"), item("fooa")];
    let result = filter_and_rank(&candidates, "foo");
    assert_eq!(labels(&result), vec!["fooa", "fooz"]);
}

#[test]
fn filter_and_rank_empty_prefix_returns_everything_unfiltered() {
    let candidates = [item("foo"), item("bar"), item("baz")];
    let result = filter_and_rank(&candidates, "");
    assert_eq!(result.len(), 3);
}

#[test]
fn visible_filters_against_the_anchor_to_cursor_slice() {
    let state = CompletionState::open(5, vec![item("foo"), item("foobar"), item("bar")]);
    // "text_"[5..8] == "foo", the run typed so far after the anchor.
    let result = state.visible("text_foo", 8);
    assert_eq!(labels(&result), vec!["foo", "foobar"]);
}

#[test]
fn move_selection_down_advances_and_clamps_at_the_last_row() {
    let mut state = CompletionState::open(0, vec![item("a"), item("b")]);
    state.move_selection(1, 2);
    assert_eq!(state.selected(), 1);
    state.move_selection(1, 2);
    assert_eq!(state.selected(), 1);
}

#[test]
fn move_selection_up_clamps_at_the_first_row() {
    let mut state = CompletionState::open(0, vec![item("a"), item("b")]);
    state.move_selection(-1, 2);
    assert_eq!(state.selected(), 0);
}

#[test]
fn move_selection_with_no_visible_candidates_resets_to_zero() {
    let mut state = CompletionState::open(0, vec![item("a")]);
    state.move_selection(1, 1);
    state.move_selection(1, 0);
    assert_eq!(state.selected(), 0);
}

#[test]
fn popup_position_at_start_of_a_line_sits_one_row_below_unclamped() {
    let char_rect = rect(0.0, 100.0, 1.0, 18.0);
    let pane = rect(0.0, 0.0, 800.0, 600.0);
    let pos = popup_position(char_rect, egui::vec2(200.0, 80.0), pane);
    assert_eq!(pos, egui::pos2(0.0, 118.0));
}

#[test]
fn popup_position_mid_line_sits_directly_under_the_caret_unclamped() {
    let char_rect = rect(340.0, 100.0, 1.0, 18.0);
    let pane = rect(0.0, 0.0, 800.0, 600.0);
    let pos = popup_position(char_rect, egui::vec2(200.0, 80.0), pane);
    assert_eq!(pos, egui::pos2(340.0, 118.0));
}

#[test]
fn popup_position_near_right_edge_clamps_x_to_stay_inside_the_pane() {
    let char_rect = rect(750.0, 100.0, 1.0, 18.0);
    let pane = rect(0.0, 0.0, 800.0, 600.0);
    let pos = popup_position(char_rect, egui::vec2(200.0, 80.0), pane);
    assert_eq!(pos.x, 600.0); // pane_rect.right() - popup width
    assert_eq!(pos.y, 118.0); // y unaffected by the x clamp
}

#[test]
fn popup_position_near_bottom_edge_clamps_y_to_stay_inside_the_pane() {
    let char_rect = rect(0.0, 550.0, 1.0, 18.0);
    let pane = rect(0.0, 0.0, 800.0, 600.0);
    let pos = popup_position(char_rect, egui::vec2(200.0, 80.0), pane);
    assert_eq!(pos.x, 0.0); // x unaffected by the y clamp
    assert_eq!(pos.y, 520.0); // pane_rect.bottom() - popup height
}

#[test]
fn popup_position_oversized_popup_stays_flush_with_the_near_edge_instead_of_going_negative() {
    // Popup (200x80) is bigger than the pane (100x50) in both
    // dimensions — "one row below the caret" (y=18) can't be honored
    // at all here, so the y clamp wins and pins it flush to the pane's
    // top edge instead of pushing it off-screen.
    let char_rect = rect(0.0, 0.0, 1.0, 18.0);
    let pane = rect(0.0, 0.0, 100.0, 50.0);
    let pos = popup_position(char_rect, egui::vec2(200.0, 80.0), pane);
    assert_eq!(pos, egui::pos2(0.0, 0.0));
}

#[test]
fn insert_completion_replaces_the_typed_prefix_with_the_label() {
    let (text, cursor) = insert_completion("foo.ba", 4, 6, &item("bar"));
    assert_eq!(text, "foo.bar");
    assert_eq!(cursor, 7);
}

#[test]
fn insert_completion_with_no_prefix_typed_yet_just_inserts_the_label() {
    let (text, cursor) = insert_completion("foo.", 4, 4, &item("bar"));
    assert_eq!(text, "foo.bar");
    assert_eq!(cursor, 7);
}

#[test]
fn insert_completion_a_zero_arg_method_places_the_cursor_after_the_closing_paren() {
    let (text, cursor) = insert_completion("foo.ru", 4, 6, &method_item("run", false));
    assert_eq!(text, "foo.run()");
    assert_eq!(cursor, text.chars().count());
}

#[test]
fn insert_completion_a_parameterized_method_places_the_cursor_between_the_parens() {
    let (text, cursor) = insert_completion("foo.co", 4, 6, &method_item("compute", true));
    assert_eq!(text, "foo.compute()");
    assert_eq!(&text[..cursor], "foo.compute(");
}

#[test]
fn bare_label_and_has_params_strips_a_signature_decorated_label() {
    let (name, has_params) = bare_label_and_has_params("add(E e) : boolean");
    assert_eq!(name, "add");
    assert!(has_params);
}

#[test]
fn bare_label_and_has_params_a_zero_arg_method_has_no_params() {
    let (name, has_params) = bare_label_and_has_params("run() : void");
    assert_eq!(name, "run");
    assert!(!has_params);
}

#[test]
fn bare_label_and_has_params_a_plain_field_label_is_unchanged() {
    let (name, has_params) = bare_label_and_has_params("length");
    assert_eq!(name, "length");
    assert!(!has_params);
}

#[test]
fn from_lsp_response_maps_kind_strips_signatures_and_reads_detail() {
    let value = serde_json::json!([
        { "label": "add(E e) : boolean", "kind": 2, "detail": "boolean" },
        { "label": "length", "kind": 5, "detail": "int" },
        { "label": "class", "kind": 14 },
        { "label": "SomeInterface", "kind": 8 }
    ]);
    let items = from_lsp_response(value);
    assert_eq!(items[0].label, "add");
    assert_eq!(items[0].kind, CompletionKind::Method);
    assert!(items[0].has_params);
    assert_eq!(items[0].detail.as_deref(), Some("boolean"));
    assert_eq!(items[1].label, "length");
    assert_eq!(items[1].kind, CompletionKind::Field);
    assert_eq!(items[2].kind, CompletionKind::Keyword);
    // CLASS (7)/INTERFACE (8)/... aren't in the explicit mapping table —
    // fall back to the neutral `Word` kind rather than inventing a new
    // `CompletionKind` variant for every LSP kind this popup has no
    // special insertion/rendering behavior for.
    assert_eq!(items[3].kind, CompletionKind::Word);
}

#[test]
fn from_lsp_response_never_trusts_a_snippet_formatted_insert_text() {
    // A real server may send `insertText: "add($0)"`/`insertTextFormat:
    // Snippet` for a method; this decode step must never use that
    // field at all (only `label`, which the protocol never allows to
    // be a snippet itself) — otherwise raw `$0` tab-stop syntax would
    // land in the user's own buffer.
    let value = serde_json::json!([
        { "label": "add(E e) : boolean", "kind": 2, "insertText": "add($0)", "insertTextFormat": 2 }
    ]);
    let items = from_lsp_response(value);
    assert_eq!(items[0].label, "add");
    assert!(!items[0].label.contains('$'));
}

#[test]
fn from_lsp_response_handles_the_completion_list_wrapper_shape() {
    let value = serde_json::json!({
        "isIncomplete": false,
        "items": [{ "label": "size", "kind": 2 }]
    });
    let items = from_lsp_response(value);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "size");
}

#[test]
fn from_lsp_response_a_null_response_is_an_empty_list() {
    assert!(from_lsp_response(serde_json::Value::Null).is_empty());
}

#[test]
fn merge_candidates_dedups_by_exact_label() {
    let mut state = CompletionState::open(0, vec![item("add"), item("size")]);
    state.merge_candidates(vec![item("add"), item("get")]);
    let labels: Vec<&str> = state.candidates.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, vec!["add", "size", "get"]);
}

#[test]
fn poll_lsp_merges_a_successful_response_and_clears_the_pending_request() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = CompletionState::open(0, vec![item("add")]);
    state.set_pending_lsp(rx);
    tx.send(Ok(serde_json::json!([{ "label": "size", "kind": 2 }])))
        .unwrap();
    state.poll_lsp();
    assert!(state.pending_lsp.is_none());
    let labels: Vec<&str> = state.candidates.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, vec!["add", "size"]);
}

#[test]
fn poll_lsp_a_disconnected_channel_clears_pending_without_panicking() {
    let mut state = CompletionState::open(0, vec![item("add")]);
    let (tx, rx) = std::sync::mpsc::channel();
    drop(tx);
    state.set_pending_lsp(rx);
    state.poll_lsp();
    assert!(state.pending_lsp.is_none());
    assert_eq!(state.candidates.len(), 1);
}

#[test]
fn has_pending_lsp_is_true_until_a_response_is_polled() {
    // Regression coverage for a real bug this popup's own live-verify
    // hit: a dot-completion popup opened with zero *local* candidates
    // (a JDK/stdlib-typed receiver) stays empty for however many
    // frames a real server takes to reply — `widget.rs`'s own two
    // "close the popup if its filtered list is empty" checks both key
    // off this method specifically so neither one closes a popup an
    // async reply could still populate.
    let mut state = CompletionState::open(0, Vec::new());
    assert!(!state.has_pending_lsp());
    let (tx, rx) = std::sync::mpsc::channel();
    state.set_pending_lsp(rx);
    assert!(state.has_pending_lsp());
    // Still nothing sent — still pending after a poll.
    state.poll_lsp();
    assert!(state.has_pending_lsp());
    tx.send(Ok(serde_json::json!([]))).unwrap();
    state.poll_lsp();
    assert!(!state.has_pending_lsp());
}
