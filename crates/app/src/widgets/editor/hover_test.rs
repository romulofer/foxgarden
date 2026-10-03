use super::*;

fn rope(text: &str) -> Rope {
    Rope::from_str(text)
}

#[test]
fn identifier_span_extends_to_both_ends_of_a_word() {
    let r = rope("foo.barBaz(qux)");
    // char 6 is the 'r' in "barBaz" (f=0,o=1,o=2,.=3,b=4,a=5,r=6).
    assert_eq!(identifier_span(&r, 6), 4..10);
}

#[test]
fn identifier_span_at_the_very_start_of_a_word_still_extends_right() {
    let r = rope("foo");
    assert_eq!(identifier_span(&r, 0), 0..3);
}

#[test]
fn identifier_span_touching_the_end_of_a_word_includes_it() {
    let r = rope("foo bar");
    // char 3 is the space right after "foo" — "touches" foo from the
    // right, same convention word_range_at-style helpers elsewhere in
    // this app already use for a boundary position.
    assert_eq!(identifier_span(&r, 3), 0..3);
}

#[test]
fn identifier_span_surrounded_by_punctuation_on_both_sides_is_empty_at_that_point() {
    let r = rope("foo . bar");
    // Char 4 sits right between the space after "foo" and the standalone
    // "." token — unlike a position right against an identifier's own
    // edge (see the "touching" test above, which correctly extends into
    // it), neither side here is an identifier char at all, so this is a
    // genuine empty span.
    let r_text = r.to_string();
    assert_eq!(&r_text[3..5], " .");
    assert_eq!(identifier_span(&r, 4), 4..4);
}

#[test]
fn identifier_span_dollar_sign_counts_as_an_identifier_char() {
    let r = rope("$foo");
    assert_eq!(identifier_span(&r, 2), 0..4);
}

/// A one-row-tall, one-pixel-wide caret rect at `x`, the exact shape
/// `TextAreaOutput::char_rect` hands `pointer_is_on_span`.
fn caret(x: f32, y: f32) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(1.0, 18.0))
}

#[test]
fn pointer_is_on_span_accepts_a_point_between_the_two_carets() {
    assert!(pointer_is_on_span(
        caret(10.0, 0.0),
        caret(40.0, 0.0),
        egui::pos2(25.0, 9.0)
    ));
}

#[test]
fn pointer_is_on_span_rejects_a_point_past_the_end_of_the_identifier() {
    // The whole point of this gate: `char_offset_for_pos` resolves the
    // blank space to the right of a line to that line's last position,
    // which `identifier_span` then extends back into the final token.
    assert!(!pointer_is_on_span(
        caret(10.0, 0.0),
        caret(40.0, 0.0),
        egui::pos2(300.0, 9.0)
    ));
}

#[test]
fn pointer_is_on_span_rejects_a_point_before_the_identifier_starts() {
    assert!(!pointer_is_on_span(
        caret(10.0, 0.0),
        caret(40.0, 0.0),
        egui::pos2(2.0, 9.0)
    ));
}

#[test]
fn pointer_is_on_span_rejects_a_point_on_another_row() {
    // The empty area below the last line resolves to the end of the
    // buffer the same way, and must be rejected the same way.
    assert!(!pointer_is_on_span(
        caret(10.0, 0.0),
        caret(40.0, 0.0),
        egui::pos2(25.0, 200.0)
    ));
}

#[test]
fn pointer_is_on_span_applies_no_right_bound_when_word_wrap_split_the_identifier() {
    // `end` on a lower row: this row holds only the identifier's first
    // half, which runs to the row's own end, so anything right of
    // `start` on this row is still on it.
    assert!(pointer_is_on_span(
        caret(10.0, 0.0),
        caret(6.0, 18.0),
        egui::pos2(500.0, 9.0)
    ));
}

#[test]
fn update_with_nothing_hovered_clears_any_tracked_hover() {
    let (_dir, mut doc) = test_support::temp_document("Foo.java", "class Foo {}");
    let mut state = HoverState::default();
    let mut lsp = LspState::default();
    state.update(&mut doc, Some(0..5), &mut lsp);
    assert!(state.tracked.is_some());
    state.update(&mut doc, None, &mut lsp);
    assert!(state.tracked.is_none());
}

#[test]
fn update_does_not_fire_a_request_before_the_hover_delay_elapses() {
    let (_dir, mut doc) = test_support::temp_document("Foo.java", "class Foo {}");
    let mut state = HoverState::default();
    let mut lsp = LspState::default();
    // Chars 6..9 are "Foo" — no language server is running (a fresh
    // `LspState::default()`), so `request_hover` itself would return
    // `None` regardless; this only asserts the *timer* gate: `fired`
    // stays false immediately after the first sighting.
    state.update(&mut doc, Some(6..9), &mut lsp);
    let tracked = state.tracked.as_ref().unwrap();
    assert!(!tracked.fired);
    assert!(tracked.pending.is_none());
}

#[test]
fn update_moving_to_a_different_identifier_resets_the_dwell_timer() {
    let (_dir, mut doc) = test_support::temp_document("Foo.java", "aaa bbb");
    let mut state = HoverState::default();
    let mut lsp = LspState::default();
    state.update(&mut doc, Some(0..3), &mut lsp);
    let first_since = state.tracked.as_ref().unwrap().since;
    state.update(&mut doc, Some(4..7), &mut lsp);
    let tracked = state.tracked.as_ref().unwrap();
    assert_eq!(tracked.span, 4..7);
    assert!(!tracked.fired);
    assert!(tracked.since >= first_since);
}

#[test]
fn update_editing_the_document_re_arms_the_same_hovered_span() {
    // A stationary pointer over a buffer being typed into: same path,
    // same span, but the resolved content (and the position it was
    // resolved at) belong to a version of the text that's gone.
    let (_dir, mut doc) = test_support::temp_document("Foo.java", "class Foo {}");
    let mut state = HoverState::default();
    let mut lsp = LspState::default();
    state.update(&mut doc, Some(6..9), &mut lsp);
    state.tracked.as_mut().unwrap().content = Some("stale docs".to_string());
    doc.lsp_version += 1;
    state.update(&mut doc, Some(6..9), &mut lsp);
    let tracked = state.tracked.as_ref().unwrap();
    assert_eq!(tracked.version, doc.lsp_version);
    assert!(tracked.content.is_none());
    assert!(!state.has_content());
}

#[test]
fn update_an_unchanged_hover_keeps_its_resolved_content() {
    let (_dir, mut doc) = test_support::temp_document("Foo.java", "class Foo {}");
    let mut state = HoverState::default();
    let mut lsp = LspState::default();
    state.update(&mut doc, Some(6..9), &mut lsp);
    state.tracked.as_mut().unwrap().content = Some("class Foo".to_string());
    state.update(&mut doc, Some(6..9), &mut lsp);
    assert!(state.has_content());
}

#[test]
fn hover_text_from_response_reads_a_scalar_marked_string() {
    let value = serde_json::json!({ "contents": "plain text doc" });
    assert_eq!(hover_text_from_response(value).as_deref(), Some("plain text doc"));
}

#[test]
fn hover_text_from_response_reads_markup_content() {
    let value = serde_json::json!({ "contents": { "kind": "markdown", "value": "plain doc" } });
    assert_eq!(hover_text_from_response(value).as_deref(), Some("plain doc"));
}

#[test]
fn hover_text_from_response_strips_markdown_noise_from_markup_content() {
    // Real captured jdtls output shape (TECHNICAL_DEBT.md #22): bold,
    // inline code, and a link with a long jdt:// target all appear in
    // the same reply.
    let value = serde_json::json!({
        "contents": { "kind": "markdown", "value": "**Since:** 1.0 — see `String` and [Character](jdt://contents/x)" }
    });
    assert_eq!(
        hover_text_from_response(value).as_deref(),
        Some("Since: 1.0 — see String and Character")
    );
}

#[test]
fn strip_markdown_removes_fenced_code_block_markers_but_keeps_their_content() {
    let text = "```java\njava.lang.String\n```\n\nsome docs";
    assert_eq!(strip_markdown(text).trim(), "java.lang.String\n\nsome docs");
}

#[test]
fn strip_markdown_drops_single_asterisk_italic_markers() {
    // Real captured jdtls output (found live-verifying #20 against
    // `java.util.ArrayList`'s own Javadoc): `*capacity*`-style single-
    // asterisk emphasis, distinct from the `**bold**` case above.
    let text = "The *capacity* of an ArrayList grows automatically.";
    assert_eq!(
        strip_markdown(text).trim(),
        "The capacity of an ArrayList grows automatically."
    );
}

#[test]
fn strip_markdown_drops_blockquote_markers() {
    let text = "> indented note\n>> nested note";
    assert_eq!(strip_markdown(text).trim(), "indented note\nnested note");
}

#[test]
fn strip_markdown_collapses_a_table_row_to_plain_cells() {
    // Real captured jdtls shape (java.util.Deque method summary,
    // bugs/tooltip_formatting_and_lack_of_scrollbar.png): leading/inner
    // padding cells and a `|---|` separator row that must vanish.
    let text = "| |First Element (Head) | | Last Element (Tail) | |\n|---|---|\n|Insert |addFirst(e) |addLast(e) |";
    assert_eq!(
        strip_markdown(text).trim(),
        "First Element (Head)   Last Element (Tail)\nInsert   addFirst(e)   addLast(e)"
    );
}

#[test]
fn strip_markdown_strips_inline_markup_inside_table_cells() {
    let text = "| **Method** | `addFirst(e)` |";
    assert_eq!(strip_markdown(text).trim(), "Method   addFirst(e)");
}

#[test]
fn strip_markdown_leaves_plain_text_untouched() {
    let text = "just a plain sentence with no markup";
    assert_eq!(strip_markdown(text).trim(), text);
}

#[test]
fn strip_links_keeps_only_the_link_text() {
    assert_eq!(
        strip_links("see [Character](jdt://contents/java.base/java.lang/Character.class?=x)"),
        "see Character"
    );
}

#[test]
fn strip_links_leaves_an_unmatched_bracket_alone() {
    assert_eq!(strip_links("array[i] stays as-is"), "array[i] stays as-is");
}

#[test]
fn hover_text_from_response_joins_an_array_of_marked_strings() {
    let value = serde_json::json!({ "contents": ["first", { "language": "java", "value": "int x" }] });
    assert_eq!(hover_text_from_response(value).as_deref(), Some("first\n\nint x"));
}

#[test]
fn hover_text_from_response_a_null_result_is_none() {
    assert!(hover_text_from_response(serde_json::Value::Null).is_none());
}

#[test]
fn hover_text_from_response_blank_content_is_none() {
    let value = serde_json::json!({ "contents": "   " });
    assert!(hover_text_from_response(value).is_none());
}
