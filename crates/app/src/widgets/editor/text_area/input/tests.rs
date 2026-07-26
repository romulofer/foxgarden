//! Table tests for the pure caret + edit model ([`super`](input.rs)) — the
//! parity net for the virtualized editor's motion/editing, run without a frame.

// Fixtures like `[1..2]` are a one-element slice of hidden line *ranges*, not
// a mistaken range-of-ranges — the shape `clamp_out_of_hidden` genuinely takes.
#![allow(clippy::single_range_in_vec_init)]

use super::*;

/// Shorthand for the `LineIndex` every non-trivial function here now takes —
/// tests build one fresh per fixture, same as any real caller rebuilds one
/// per edit (see `LineIndex`'s own doc comment).
fn idx(text: &str) -> LineIndex {
    LineIndex::build(text)
}

// ---- editing ----

#[test]
fn insert_at_a_collapsed_caret_splices_and_advances() {
    let (text, caret) = replace_selection("helloworld", &idx("helloworld"), Caret::at(5), " brave ");
    assert_eq!(text, "hello brave world");
    assert_eq!(caret, Caret::at(12)); // just past the inserted 7 chars
}

#[test]
fn insert_over_a_selection_replaces_it() {
    // Select "llo" (chars 2..5), type "y".
    let (text, caret) = replace_selection("hello", &idx("hello"), Caret { primary: 5, anchor: 2 }, "y");
    assert_eq!(text, "hey");
    assert_eq!(caret, Caret::at(3));
}

#[test]
fn backspace_removes_the_prior_char_or_the_selection() {
    assert_eq!(
        backspace("abc", &idx("abc"), Caret::at(2)),
        Some(("ac".to_string(), Caret::at(1)))
    );
    // With a selection, backspace deletes the whole selection.
    assert_eq!(
        backspace("abcd", &idx("abcd"), Caret { primary: 1, anchor: 3 }),
        Some(("ad".to_string(), Caret::at(1)))
    );
    // At the start with no selection: nothing to delete.
    assert_eq!(backspace("abc", &idx("abc"), Caret::at(0)), None);
}

#[test]
fn delete_forward_removes_the_next_char_or_the_selection() {
    assert_eq!(
        delete_forward("abc", &idx("abc"), Caret::at(1)),
        Some(("ac".to_string(), Caret::at(1)))
    );
    assert_eq!(
        delete_forward("abcd", &idx("abcd"), Caret { primary: 1, anchor: 3 }),
        Some(("ad".to_string(), Caret::at(1)))
    );
    // At the end with no selection: nothing to delete.
    assert_eq!(delete_forward("abc", &idx("abc"), Caret::at(3)), None);
}

#[test]
fn editing_respects_multi_byte_chars() {
    // "héllo": 'é' is 2 bytes, 1 char. Backspacing after it removes one char,
    // not one byte (which would corrupt UTF-8).
    let (text, caret) = backspace("héllo", &idx("héllo"), Caret::at(2)).unwrap();
    assert_eq!(text, "hllo");
    assert_eq!(caret, Caret::at(1));
}

// ---- horizontal motion ----

#[test]
fn move_left_right_step_one_char_and_clamp() {
    assert_eq!(move_left(Caret::at(2), false), Caret::at(1));
    assert_eq!(move_left(Caret::at(0), false), Caret::at(0)); // clamped
    let index = idx("abc");
    assert_eq!(move_right(&index, Caret::at(2), false), Caret::at(3));
    assert_eq!(move_right(&index, Caret::at(3), false), Caret::at(3)); // clamped at end
}

#[test]
fn arrow_without_shift_collapses_a_selection_to_the_matching_edge() {
    let sel = Caret { primary: 4, anchor: 1 }; // selection 1..4
    // Left collapses to the left edge (1), not 3.
    assert_eq!(move_left(sel, false), Caret::at(1));
    // Right collapses to the right edge (4).
    assert_eq!(move_right(&idx("abcdef"), sel, false), Caret::at(4));
}

#[test]
fn shift_arrow_extends_the_selection_from_the_anchor() {
    let index = idx("abcdef");
    let c = Caret::at(2);
    let c = move_right(&index, c, true);
    assert_eq!(c, Caret { primary: 3, anchor: 2 });
    let c = move_right(&index, c, true);
    assert_eq!(c, Caret { primary: 4, anchor: 2 }); // anchor stays put
}

// ---- vertical motion ----

const GRID: &str = "abcd\nef\nghij"; // line0 len4, line1 len2, line2 len4

#[test]
fn move_down_preserves_the_preferred_column_across_a_short_line() {
    let index = idx(GRID);
    // Caret at line0 col3 (on 'd'), preferred col 3.
    let start = Caret::at(3);
    let (down1, pref) = move_down(&index, start, false, 3);
    // line1 ("ef") only has 2 chars → clamp to its end (char offset of '\n').
    assert_eq!(index.line_col(down1.primary), (1, 2));
    // Down again keeps preferred col 3 → line2 col3 ('j' area).
    let (down2, _) = move_down(&index, down1, false, pref);
    assert_eq!(index.line_col(down2.primary), (2, 3));
}

#[test]
fn move_up_from_first_line_goes_to_document_start() {
    let (c, _) = move_up(&idx("abc\ndef"), Caret::at(1), false, 1);
    assert_eq!(c, Caret::at(0));
}

#[test]
fn move_down_from_last_line_goes_to_document_end() {
    let index = idx("abc\ndef");
    let (c, _) = move_down(&index, Caret::at(5), false, 1);
    assert_eq!(c, Caret::at(index.char_len()));
}

#[test]
fn move_end_goes_to_the_line_end_not_past_the_newline() {
    let index = idx(GRID);
    // Caret at start of line1 ("ef"). End → offset of the char after "ef".
    let start = index.line_col_to_char(1, 0);
    let c = move_end(&index, Caret::at(start), false);
    assert_eq!(index.line_col(c.primary), (1, 2));
    // End on the last line goes to end-of-text.
    let last = index.line_col_to_char(2, 0);
    let c = move_end(&index, Caret::at(last), false);
    assert_eq!(c.primary, index.char_len());
}

#[test]
fn column_of_reports_within_line_column() {
    let index = idx(GRID);
    assert_eq!(column_of(&index, 0), 0);
    assert_eq!(column_of(&index, 3), 3);
    assert_eq!(column_of(&index, index.line_col_to_char(2, 2)), 2);
}

#[test]
fn clamp_out_of_hidden_snaps_to_the_marker_line_end() {
    let index = idx(GRID);
    // Line 1 ("ef") is folded away (hidden lines 1..2); a caret that ended
    // up there snaps to the end of line 0 (the marker line), not line 1.
    let hidden = [1..2];
    let inside = index.line_col_to_char(1, 1);
    let clamped = clamp_out_of_hidden(&index, inside, &hidden);
    assert_eq!(
        index.line_col(clamped),
        (0, 4),
        "snaps to the end of line 0, the marker line"
    );
}

#[test]
fn clamp_out_of_hidden_is_a_no_op_on_a_visible_line() {
    let index = idx(GRID);
    let hidden = [1..2];
    let visible = index.line_col_to_char(2, 1);
    assert_eq!(clamp_out_of_hidden(&index, visible, &hidden), visible);
}

#[test]
fn clamp_out_of_hidden_handles_an_empty_hidden_set() {
    assert_eq!(clamp_out_of_hidden(&idx(GRID), 5, &[]), 5);
}

// ---- LineIndex parity (SPEC.md §7 / PLAN.md 5b): the indexed lookups must
// agree with what a plain linear scan over the same text would find, across
// every offset in a multi-line fixture — the same "prove the new path
// matches the old one" discipline word-wrap's Phase 4 used before switching
// call sites over. Reference implementations here are deliberately the
// pre-index linear-scan algorithms (kept only in this test module, not in
// production code anymore), not a second copy of `LineIndex` itself.

fn linear_line_col(text: &str, char_off: usize) -> (usize, usize) {
    let (mut line, mut col) = (0, 0);
    for (i, c) in text.chars().enumerate() {
        if i == char_off {
            return (line, col);
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn linear_line_col_to_char(text: &str, target_line: usize, target_col: usize) -> usize {
    let (mut line, mut col) = (0, 0);
    for (i, c) in text.chars().enumerate() {
        if line == target_line && col == target_col {
            return i;
        }
        if c == '\n' {
            if line == target_line {
                return i;
            }
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    text.chars().count()
}

fn linear_char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(text.len())
}

const PARITY_FIXTURE: &str =
    "class Foo {\n    int x = 1;\n\n    void bar() {\n        // café\n        return;\n    }\n}\n";

#[test]
fn line_col_matches_the_linear_scan_at_every_offset() {
    let index = idx(PARITY_FIXTURE);
    // 0..=len so the one-past-the-end offset (a valid caret position) is
    // covered too, not just in-bounds chars.
    for char_off in 0..=PARITY_FIXTURE.chars().count() {
        assert_eq!(
            index.line_col(char_off),
            linear_line_col(PARITY_FIXTURE, char_off),
            "line_col mismatch at char_off={char_off}"
        );
    }
}

#[test]
fn line_col_to_char_matches_the_linear_scan_across_every_line_and_a_range_of_columns() {
    let index = idx(PARITY_FIXTURE);
    let last_line = PARITY_FIXTURE.matches('\n').count();
    for target_line in 0..=last_line + 1 {
        // Columns 0..=8 covers every real column in this fixture's longest
        // line plus a few that overrun (exercising the end-of-line clamp).
        for target_col in 0..=8 {
            assert_eq!(
                index.line_col_to_char(target_line, target_col),
                linear_line_col_to_char(PARITY_FIXTURE, target_line, target_col),
                "line_col_to_char mismatch at line={target_line} col={target_col}"
            );
        }
    }
}

#[test]
fn char_to_byte_matches_the_linear_scan_at_every_offset() {
    let index = idx(PARITY_FIXTURE);
    for char_off in 0..=PARITY_FIXTURE.chars().count() {
        assert_eq!(
            index.char_to_byte(PARITY_FIXTURE, char_off),
            linear_char_to_byte(PARITY_FIXTURE, char_off),
            "char_to_byte mismatch at char_off={char_off}"
        );
    }
}

#[test]
fn line_index_handles_an_empty_buffer() {
    let index = idx("");
    assert_eq!(index.char_len(), 0);
    assert_eq!(index.last_line(), 0);
    assert_eq!(index.line_col(0), (0, 0));
    assert_eq!(index.char_to_byte("", 0), 0);
}

#[test]
fn line_index_handles_a_trailing_newline_as_its_own_empty_last_line() {
    let index = idx("a\nb\n");
    assert_eq!(index.last_line(), 2, "the trailing \\n starts an empty line 2");
    assert_eq!(index.line_col(4), (2, 0));
}
