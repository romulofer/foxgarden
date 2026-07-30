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
fn move_document_start_goes_to_char_zero_from_any_line() {
    let index = idx(GRID);
    let start = index.line_col_to_char(2, 2); // deep into the last line
    let c = move_document_start(Caret::at(start), false);
    assert_eq!(c, Caret::at(0));
}

#[test]
fn move_document_start_with_extend_keeps_the_anchor_in_place() {
    let index = idx(GRID);
    let start = index.line_col_to_char(2, 2);
    let c = move_document_start(Caret::at(start), true);
    assert_eq!(c.anchor, start, "extend must keep the selection's anchor where it was");
    assert_eq!(c.primary, 0);
}

#[test]
fn move_document_end_goes_to_the_last_char_from_any_line() {
    let index = idx(GRID);
    let c = move_document_end(&index, Caret::at(0), false);
    assert_eq!(c.primary, index.char_len());
}

#[test]
fn move_document_end_with_extend_keeps_the_anchor_in_place() {
    let index = idx(GRID);
    let c = move_document_end(&index, Caret::at(0), true);
    assert_eq!(c.anchor, 0, "extend must keep the selection's anchor where it was");
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

#[test]
fn block_selection_at_starts_zero_size() {
    let block = BlockSelection::at(3, 5);
    assert_eq!(block.lines(), 3..=3);
    assert_eq!(block.cols(), 5..5, "a fresh block has no width yet");
}

#[test]
fn block_selection_lines_and_cols_are_sorted_regardless_of_drag_direction() {
    // Dragging up-and-left of the press point: primary ends up above/before
    // the anchor, so `lines()`/`cols()` must still report a normalized,
    // ascending range rather than a "backwards" one.
    let block = BlockSelection::at(8, 10).moved_to(2, 4);
    assert_eq!(block.lines(), 2..=8);
    assert_eq!(block.cols(), 4..10);
}

#[test]
fn block_selection_moved_to_keeps_the_anchor_fixed_across_repeated_drag_frames() {
    let mut block = BlockSelection::at(1, 1);
    block = block.moved_to(4, 6);
    block = block.moved_to(4, 9);
    assert_eq!(block.lines(), 1..=4, "the anchor line must not have moved");
    assert_eq!(block.cols(), 1..9, "the anchor col must not have moved");
}

// ---- block-scoped editing (PLAN.md Track 7 Phase 2) ----

#[test]
fn replace_block_selection_inserts_the_same_text_at_the_same_column_on_every_row() {
    let text = "aaaa\nbbbb\ncccc";
    let index = idx(text);
    // A zero-width block at column 2 on rows 0-2.
    let block = BlockSelection::at(0, 2).moved_to(2, 2);
    let (out, new_block) = replace_block_selection(text, &index, block, "X");
    assert_eq!(out, "aaXaa\nbbXbb\nccXcc");
    assert_eq!(new_block.lines(), 0..=2);
    assert_eq!(new_block.cols(), 3..3, "collapses right after the inserted char on every row");
}

#[test]
fn replace_block_selection_replaces_a_real_column_range_on_every_row() {
    let text = "aaaaaa\nbbbbbb";
    let index = idx(text);
    let block = BlockSelection::at(0, 1).moved_to(1, 4); // cols 1..4 on rows 0-1
    let (out, new_block) = replace_block_selection(text, &index, block, "Z");
    assert_eq!(out, "aZaa\nbZbb");
    assert_eq!(new_block.cols(), 2..2);
}

#[test]
fn replace_block_selection_inserts_at_a_short_lines_own_end_instead_of_padding() {
    let text = "aaaaaa\nbb\ncccccc";
    let index = idx(text);
    // Column 4 doesn't exist on the short middle row ("bb", len 2) — that
    // row's own edit lands at its own end (col 2) rather than being padded
    // out to column 4.
    let block = BlockSelection::at(0, 4).moved_to(2, 4);
    let (out, new_block) = replace_block_selection(text, &index, block, "X");
    assert_eq!(out, "aaaaXaa\nbbX\nccccXcc");
    assert_eq!(
        new_block.cols(),
        5..5,
        "the block's own target column advances uniformly regardless of any one row's clamp"
    );
}

#[test]
fn block_backspace_deletes_the_char_before_the_column_on_every_row() {
    let text = "aXaa\nbXbb\ncXcc";
    let index = idx(text);
    let block = BlockSelection::at(0, 2).moved_to(2, 2);
    let (out, new_block) = block_backspace(text, &index, block).expect("there's a char before column 2 on every row");
    assert_eq!(out, "aaa\nbbb\nccc");
    assert_eq!(new_block.cols(), 1..1);
}

#[test]
fn block_backspace_deletes_a_real_column_range_on_every_row() {
    let text = "aXXXa\nbXXXb";
    let index = idx(text);
    let block = BlockSelection::at(0, 1).moved_to(1, 4);
    let (out, new_block) = block_backspace(text, &index, block).expect("a real range to delete");
    assert_eq!(out, "aa\nbb");
    assert_eq!(new_block.cols(), 1..1, "lands at the deleted range's own start");
}

#[test]
fn block_backspace_at_column_zero_does_not_merge_lines() {
    let text = "aaa\nbbb\nccc";
    let index = idx(text);
    let block = BlockSelection::at(0, 0).moved_to(2, 0);
    // Every row is already at column 0 — nothing safe to delete anywhere,
    // so this must be a clean no-op rather than eating the newline before
    // row 1/2 (which a raw absolute-offset backspace would do).
    assert_eq!(block_backspace(text, &index, block), None);
}

#[test]
fn block_backspace_skips_only_the_rows_already_at_column_zero() {
    // The middle row is an empty line — column 1 clamps down to its own
    // column 0, so *only that row* must be skipped; rows 0/2 genuinely
    // have a column 1 (not clamped at all) and must still lose their char
    // before it.
    let text = "aXa\n\ncXc";
    let index = idx(text);
    let block = BlockSelection::at(0, 1).moved_to(2, 1);
    let (out, _) = block_backspace(text, &index, block).expect("rows 0 and 2 have something to delete");
    assert_eq!(out, "Xa\n\nXc", "the empty middle row is left untouched, not merged into row 0");
}

#[test]
fn block_delete_forward_deletes_the_char_after_the_column_on_every_row() {
    let text = "aXaa\nbXbb\ncXcc";
    let index = idx(text);
    let block = BlockSelection::at(0, 1).moved_to(2, 1);
    let (out, new_block) =
        block_delete_forward(text, &index, block).expect("there's a char after column 1 on every row");
    assert_eq!(out, "aaa\nbbb\nccc");
    assert_eq!(new_block.cols(), 1..1, "delete never moves the caret");
}

#[test]
fn block_delete_forward_at_end_of_line_does_not_merge_lines() {
    let text = "aa\nbb\ncc";
    let index = idx(text);
    // Column 5 is past every row's own end — every row's clamped position
    // is already that row's own end, so Delete has nothing safe to remove
    // without eating the next line's own newline.
    let block = BlockSelection::at(0, 5).moved_to(2, 5);
    assert_eq!(block_delete_forward(text, &index, block), None);
}

// ---- block paste + copy (PLAN.md Track 7 Phase 3) ----

#[test]
fn block_selection_text_joins_each_rows_own_column_range_with_newlines() {
    let text = "aXXa\nbXXb\ncXXc";
    let index = idx(text);
    let block = BlockSelection::at(0, 1).moved_to(2, 3);
    assert_eq!(block_selection_text(text, &index, block), "XX\nXX\nXX");
}

#[test]
fn block_selection_text_reads_a_short_lines_own_clamped_range() {
    let text = "aaaaaa\nbb\ncccccc";
    let index = idx(text);
    let block = BlockSelection::at(0, 1).moved_to(2, 4); // cols 1..4
    // The short middle row ("bb") only has one char past column 1.
    assert_eq!(block_selection_text(text, &index, block), "aaa\nb\nccc");
}

#[test]
fn block_paste_replaces_each_rows_own_column_range_with_its_matching_clipboard_line() {
    let text = "aXXa\nbXXb\ncXXc";
    let index = idx(text);
    let block = BlockSelection::at(0, 1).moved_to(2, 3);
    let (out, new_block) = block_paste(text, &index, block, "11\n22\n33");
    assert_eq!(out, "a11a\nb22b\nc33c");
    assert_eq!(new_block.cols(), 3..3, "collapses right after the first row's own pasted text");
}

#[test]
fn block_paste_with_fewer_clipboard_lines_than_block_rows_leaves_the_extra_rows_untouched() {
    let text = "aXa\nbXb\ncXc";
    let index = idx(text);
    let block = BlockSelection::at(0, 1).moved_to(2, 2); // spans 3 rows
    let (out, _) = block_paste(text, &index, block, "1\n2"); // only 2 clipboard lines
    assert_eq!(out, "a1a\nb2b\ncXc", "row 2 has no matching clipboard line, so it's left exactly as-is");
}

#[test]
fn block_paste_with_more_clipboard_lines_than_block_rows_drops_the_surplus() {
    let text = "aXa\nbXb"; // spans 2 rows
    let index = idx(text);
    let block = BlockSelection::at(0, 1).moved_to(1, 2);
    let (out, _) = block_paste(text, &index, block, "1\n2\n3\n4"); // 4 clipboard lines
    assert_eq!(out, "a1a\nb2b", "clipboard lines 3/4 have no matching row and are simply dropped");
}

#[test]
fn block_paste_round_trips_through_block_selection_text() {
    let text = "aXXa\nbXXb\ncXXc";
    let index = idx(text);
    let block = BlockSelection::at(0, 1).moved_to(2, 3);
    let copied = block_selection_text(text, &index, block);

    // Pasting the copied text back over an identically-shaped (but blank)
    // block reproduces the original rectangle exactly.
    let blank = "a__a\nb__b\nc__c";
    let blank_index = idx(blank);
    let (out, _) = block_paste(blank, &blank_index, block, &copied);
    assert_eq!(out, text);
}
