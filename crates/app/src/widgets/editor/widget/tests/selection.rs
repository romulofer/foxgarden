//! Ctrl+W/Ctrl+Shift+W syntax-node selection expand/shrink (`syntax::selection`, wired in `widget.rs`).

use super::super::*;
use super::common::*;
use fg_core::Language;

#[test]
fn ctrl_w_expands_selection_by_syntax_node_and_ctrl_shift_w_shrinks_back() {
    let source = "class Foo {\n    void run() {\n        foo();\n    }\n}\n";
    let (_dir, mut doc) = open_fixture(source, "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
    let call_start = source.find("foo()").unwrap();

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
        );
    });
    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: call_start + 3,
            anchor: call_start,
        },
    );

    let after_first_expand =
        run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_key_event(egui::Key::W));
    assert_eq!(&source[after_first_expand.clone()], "foo()");

    let after_second_expand =
        run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_key_event(egui::Key::W));
    assert!(
        after_second_expand.start <= after_first_expand.start && after_second_expand.end >= after_first_expand.end,
        "each expand must grow the selection: {after_first_expand:?} -> {after_second_expand:?}"
    );
    assert_ne!(after_second_expand, after_first_expand);

    let after_shrink =
        run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_shift_key_event(egui::Key::W));
    assert_eq!(
        after_shrink, after_first_expand,
        "shrink must restore exactly what the second expand grew out of"
    );

    // Pure selection movement — the buffer itself must never change.
    assert_eq!(doc.buffer.to_string(), source);
}

#[test]
fn ctrl_w_on_a_file_with_no_parsed_tree_is_a_no_op_that_still_consumes_the_keystroke() {
    // No language recognized for "notes.txt" — `parser` stays `None`
    // throughout, so there's no tree for `Ctrl+W` to act on. This must
    // not fall through to egui's own built-in `Ctrl+W` ("delete
    // previous word"): the buffer has to come out unchanged either way.
    let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
    let mut parser: Option<IncrementalParser> = None;

    focused_frame_with_selection(&mut doc, &mut parser, 6..6, vec![command_key_event(egui::Key::W)]);

    assert_eq!(doc.buffer.to_string(), "hello world");
}

#[test]
fn ctrl_w_still_expands_selection_on_a_read_only_java_file() {
    // Non-mutating navigation, same as Ctrl+D — must keep working even
    // when `doc.read_only` blocks every actual edit path.
    let source = "class Foo {\n    int x;\n}\n";
    let (_dir, mut doc) = open_fixture(source, "Foo.java");
    doc.read_only = true;
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let x_pos = source.find('x').unwrap();

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            None,
            false,
            &mut None,
            &mut None,
            None,
            false,
            false,
            false,
            false,
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
        );
    });
    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: x_pos,
            anchor: x_pos,
        },
    );

    let expanded = run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_key_event(egui::Key::W));

    assert!(
        !expanded.is_empty(),
        "Ctrl+W should still grow the selection on a read-only file"
    );
    assert_eq!(
        doc.buffer.to_string(),
        source,
        "read-only must still block any actual edit"
    );
}
