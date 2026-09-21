
use super::*;
use fg_core::Language;

#[test]
fn save_tab_reparses_so_trimmed_content_is_not_highlighted_against_a_stale_tree() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Hello.java");
    // Trailing whitespace on the first line: `Document::save` trims
    // this, shortening the buffer by however many spaces there were —
    // exactly the class of side effect that leaves an un-reparsed tree
    // silently stale. Trailing whitespace on the *first* line matters
    // here — trimming it shifts the byte offset of every token after
    // it (the `private String name;` line), so a stale tree's node
    // ranges land on the wrong bytes instead of coincidentally still
    // being correct (which is exactly what happened when this test
    // first put the trimmed whitespace *after* all the highlighted
    // tokens: nothing downstream of the trim to shift meant a stale
    // tree and a fresh one produced identical spans by accident).
    std::fs::write(&path, "public class Hello {   \n    private String name;\n}\n").unwrap();

    let mut state = EditorState::new();
    state.open_tab(path).unwrap();
    let parser = open_parser_for(&mut state.open_tabs[0]);
    let mut parsers = vec![parser];
    let mut last_error = None;

    save_tab(&mut state, &mut parsers, 0, &mut last_error, true);

    let doc = &state.open_tabs[0];
    assert_eq!(
        doc.buffer.to_string(),
        "public class Hello {\n    private String name;\n}\n"
    );
    assert!(!doc.is_dirty(), "buffer and saved_buffer must agree right after save");

    let tree = parsers[0].as_ref().unwrap().tree().unwrap();
    let text = doc.buffer.to_string();
    let spans = syntax::highlight_spans(tree, &text, Language::Java);

    // Cross-check against a from-scratch parse of the same (trimmed)
    // text: if `save_tab`'s reparse kept the tree in sync, the two
    // must match exactly. Before the fix, the tree still reflected the
    // pre-trim (longer) text, so node byte ranges no longer lined up
    // with `text` at all past the trimmed line.
    let mut fresh_parser = IncrementalParser::new(Language::Java);
    fresh_parser.parse(&text);
    let fresh_spans = syntax::highlight_spans(fresh_parser.tree().unwrap(), &text, Language::Java);
    assert_eq!(spans, fresh_spans);
}
