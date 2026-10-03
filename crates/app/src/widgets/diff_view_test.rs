
use super::*;

#[test]
fn diff_line_ops_reports_a_pure_modification_as_replace() {
    let ops = diff_line_ops("l1\nl2\nl3\n", "l1\nCHANGED\nl3\n");
    assert_eq!(
        ops,
        vec![
            DiffLineOp::Equal { old: 0..1, new: 0..1 },
            DiffLineOp::Replace { old: 1..2, new: 1..2 },
            DiffLineOp::Equal { old: 2..3, new: 2..3 },
        ]
    );
}

#[test]
fn diff_line_ops_reports_a_pure_deletion() {
    let ops = diff_line_ops("l1\nl2\nl3\nl4\n", "l1\nl3\nl4\n");
    assert_eq!(
        ops,
        vec![
            DiffLineOp::Equal { old: 0..1, new: 0..1 },
            DiffLineOp::Delete { old: 1..2 },
            DiffLineOp::Equal { old: 2..4, new: 1..3 },
        ]
    );
}

#[test]
fn diff_line_ops_reports_a_pure_insertion() {
    let ops = diff_line_ops("", "a\nb\n");
    assert_eq!(ops, vec![DiffLineOp::Insert { new: 0..2 }]);
}

#[test]
fn diff_stat_counts_a_pure_modification_as_one_added_and_one_removed() {
    assert_eq!(diff_stat("l1\nl2\nl3\n", "l1\nCHANGED\nl3\n"), (1, 1));
}

#[test]
fn diff_stat_counts_a_pure_deletion() {
    assert_eq!(diff_stat("l1\nl2\nl3\nl4\n", "l1\nl3\nl4\n"), (0, 1));
}

#[test]
fn diff_stat_counts_a_pure_insertion() {
    assert_eq!(diff_stat("", "a\nb\n"), (2, 0));
}

#[test]
fn diff_stat_is_zero_for_identical_text() {
    assert_eq!(diff_stat("same\n", "same\n"), (0, 0));
}

#[test]
fn diff_line_ops_indices_match_str_lines_directly() {
    // The exact contract `side_by_side_rows`/`inline_rows` depend on:
    // `old_index`/`new_index` line-index into `old.lines()`/
    // `new.lines()` with no further translation needed.
    let old = "l1\nl2\nl3\nl4\n";
    let new = "l1\nl3\nl4\n";
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let ops = diff_line_ops(old, new);
    let DiffLineOp::Delete { old: range } = &ops[1] else {
        panic!("expected a Delete op")
    };
    assert_eq!(&old_lines[range.clone()], &["l2"]);
    let DiffLineOp::Equal { new: range, .. } = &ops[2] else {
        panic!("expected an Equal op")
    };
    assert_eq!(&new_lines[range.clone()], &["l3", "l4"]);
}

#[test]
fn split_equal_run_leaves_a_short_run_untouched() {
    assert_eq!(split_equal_run(6, 3), None);
}

#[test]
fn split_equal_run_collapses_the_middle_of_a_long_run() {
    assert_eq!(split_equal_run(10, 3), Some((0..3, 4, 7..10)));
}

#[test]
fn side_by_side_rows_aligns_a_pure_modification_one_to_one() {
    let old_lines = vec!["l1", "l2", "l3"];
    let new_lines = vec!["l1", "CHANGED", "l3"];
    let ops = diff_line_ops("l1\nl2\nl3\n", "l1\nCHANGED\nl3\n");
    let rows = side_by_side_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
    assert_eq!(
        rows,
        vec![
            SideBySideRow::Line {
                old: Some("l1"),
                new: Some("l1"),
                old_changed: false,
                new_changed: false
            },
            SideBySideRow::Line {
                old: Some("l2"),
                new: Some("CHANGED"),
                old_changed: true,
                new_changed: true
            },
            SideBySideRow::Line {
                old: Some("l3"),
                new: Some("l3"),
                old_changed: false,
                new_changed: false
            },
        ]
    );
}

#[test]
fn side_by_side_rows_pads_the_shorter_side_of_an_uneven_replace() {
    let old_lines = vec!["only"];
    let new_lines = vec!["first", "second"];
    let ops = diff_line_ops("only\n", "first\nsecond\n");
    let rows = side_by_side_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
    assert_eq!(
        rows,
        vec![
            SideBySideRow::Line {
                old: Some("only"),
                new: Some("first"),
                old_changed: true,
                new_changed: true
            },
            SideBySideRow::Line {
                old: None,
                new: Some("second"),
                old_changed: false,
                new_changed: true
            },
        ]
    );
}

#[test]
fn side_by_side_rows_leaves_the_opposite_side_blank_for_a_pure_insert_or_delete() {
    let old_lines = vec!["l1", "l2", "l3"];
    let new_lines = vec!["l1", "l3"];
    let ops = diff_line_ops("l1\nl2\nl3\n", "l1\nl3\n");
    let rows = side_by_side_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
    assert_eq!(
        rows,
        vec![
            SideBySideRow::Line {
                old: Some("l1"),
                new: Some("l1"),
                old_changed: false,
                new_changed: false
            },
            SideBySideRow::Line {
                old: Some("l2"),
                new: None,
                old_changed: true,
                new_changed: false
            },
            SideBySideRow::Line {
                old: Some("l3"),
                new: Some("l3"),
                old_changed: false,
                new_changed: false
            },
        ]
    );
}

#[test]
fn side_by_side_rows_collapses_a_long_untouched_run_with_a_context_border() {
    let old_lines: Vec<&str> = vec!["c1", "c2", "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "c3"];
    let new_lines: Vec<&str> = vec!["c1", "c2", "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "c3"];
    // Manually crafted ops: a leading change, then a 8-line Equal run
    // (long enough to collapse with context=2), then a trailing change.
    let ops = vec![
        DiffLineOp::Insert { new: 0..2 },
        DiffLineOp::Equal { old: 2..10, new: 2..10 },
        DiffLineOp::Delete { old: 10..11 },
    ];
    let rows = side_by_side_rows(&old_lines, &new_lines, &ops, 2);
    assert_eq!(
        rows,
        vec![
            SideBySideRow::Line {
                old: None,
                new: Some("c1"),
                old_changed: false,
                new_changed: true
            },
            SideBySideRow::Line {
                old: None,
                new: Some("c2"),
                old_changed: false,
                new_changed: true
            },
            SideBySideRow::Line {
                old: Some("e1"),
                new: Some("e1"),
                old_changed: false,
                new_changed: false
            },
            SideBySideRow::Line {
                old: Some("e2"),
                new: Some("e2"),
                old_changed: false,
                new_changed: false
            },
            SideBySideRow::Collapsed(4),
            SideBySideRow::Line {
                old: Some("e7"),
                new: Some("e7"),
                old_changed: false,
                new_changed: false
            },
            SideBySideRow::Line {
                old: Some("e8"),
                new: Some("e8"),
                old_changed: false,
                new_changed: false
            },
            SideBySideRow::Line {
                old: Some("c3"),
                new: None,
                old_changed: true,
                new_changed: false
            },
        ]
    );
}

#[test]
fn inline_rows_shows_a_replaces_old_lines_then_its_new_lines() {
    let old_lines = vec!["l1", "l2", "l3"];
    let new_lines = vec!["l1", "CHANGED", "l3"];
    let ops = diff_line_ops("l1\nl2\nl3\n", "l1\nCHANGED\nl3\n");
    let rows = inline_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
    assert_eq!(
        rows,
        vec![
            InlineRow::Line {
                text: "l1",
                kind: InlineRowKind::Equal
            },
            InlineRow::Line {
                text: "l2",
                kind: InlineRowKind::Deleted
            },
            InlineRow::Line {
                text: "CHANGED",
                kind: InlineRowKind::Inserted
            },
            InlineRow::Line {
                text: "l3",
                kind: InlineRowKind::Equal
            },
        ]
    );
}

#[test]
fn inline_rows_on_identical_text_is_all_equal() {
    let old_lines = vec!["a", "b"];
    let new_lines = vec!["a", "b"];
    let ops = diff_line_ops("a\nb\n", "a\nb\n");
    let rows = inline_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
    assert_eq!(
        rows,
        vec![
            InlineRow::Line {
                text: "a",
                kind: InlineRowKind::Equal
            },
            InlineRow::Line {
                text: "b",
                kind: InlineRowKind::Equal
            }
        ]
    );
}

#[test]
fn inline_rows_collapses_a_long_untouched_run_with_a_context_border() {
    let old_lines: Vec<&str> = vec!["e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8"];
    let new_lines: Vec<&str> = old_lines.clone();
    let ops = vec![DiffLineOp::Equal { old: 0..8, new: 0..8 }];
    let rows = inline_rows(&old_lines, &new_lines, &ops, 2);
    assert_eq!(
        rows,
        vec![
            InlineRow::Line {
                text: "e1",
                kind: InlineRowKind::Equal
            },
            InlineRow::Line {
                text: "e2",
                kind: InlineRowKind::Equal
            },
            InlineRow::Collapsed(4),
            InlineRow::Line {
                text: "e7",
                kind: InlineRowKind::Equal
            },
            InlineRow::Line {
                text: "e8",
                kind: InlineRowKind::Equal
            },
        ]
    );
}

/// Real font install (`crate::style::fonts::install`, exactly what
/// `FoxGardenApp::new` runs at real startup), not `FontDefinitions::
/// empty()` like most of this app's other headless widget tests —
/// `EditorFont::JetBrainsMono` maps to a *named* `FontFamily` that,
/// unlike the built-in `Monospace`/`Proportional` ones, panics on any
/// lookup if nothing ever registered it (confirmed: an empty-fonts
/// setup crashed this exact test before switching to a real install).
fn headless_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    crate::style::fonts::install(&ctx);
    ctx
}

#[test]
fn show_diff_side_by_side_does_not_panic_against_real_changed_text() {
    let ctx = headless_ctx();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        show_diff(
            ui,
            "l1\nl2\nl3\n",
            "l1\nCHANGED\nl3\nl4\n",
            DiffMode::SideBySide,
            EditorFont::JetBrainsMono,
            14.0,
            true,
        );
    });
}

#[test]
fn show_diff_inline_does_not_panic_against_real_changed_text() {
    let ctx = headless_ctx();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        show_diff(
            ui,
            "l1\nl2\nl3\n",
            "l1\nCHANGED\nl3\nl4\n",
            DiffMode::Inline,
            EditorFont::Default,
            14.0,
            false,
        );
    });
}

#[test]
fn show_diff_on_identical_text_does_not_panic() {
    let ctx = headless_ctx();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        show_diff(
            ui,
            "same\n",
            "same\n",
            DiffMode::SideBySide,
            EditorFont::JetBrainsMono,
            14.0,
            true,
        );
    });
}

/// A larger real file (well beyond `2 * DEFAULT_CONTEXT_LINES`) with
/// one small change in the middle — the actual scenario the user
/// reported wanting abridged, exercised as a real render, not just the
/// pure row-builder tests above.
#[test]
fn show_diff_abridges_a_large_mostly_unchanged_file_without_panicking() {
    let lines: Vec<String> = (1..=200).map(|n| format!("line {n}")).collect();
    let old = lines.join("\n") + "\n";
    let mut changed = lines.clone();
    changed[100] = "CHANGED".to_string();
    let new = changed.join("\n") + "\n";

    let ctx = headless_ctx();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        show_diff(ui, &old, &new, DiffMode::SideBySide, EditorFont::Default, 14.0, true);
    });

    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let ops = diff_line_ops(&old, &new);
    let rows = inline_rows(&old_lines, &new_lines, &ops, DEFAULT_CONTEXT_LINES);
    // Two collapsed runs (before and after the one changed line) plus
    // a small number of real lines — not all 200 shown in full.
    assert_eq!(rows.iter().filter(|r| matches!(r, InlineRow::Collapsed(_))).count(), 2);
    assert!(
        rows.len() < 20,
        "expected the bulk of the file to be abridged, got {} rows",
        rows.len()
    );
}
