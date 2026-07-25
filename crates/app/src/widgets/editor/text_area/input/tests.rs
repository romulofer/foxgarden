//! Table tests for the pure caret + edit model ([`super`](input.rs)) — the
//! parity net for the virtualized editor's motion/editing, run without a frame.

// Fixtures like `[1..2]` are a one-element slice of hidden line *ranges*, not
// a mistaken range-of-ranges — the shape `clamp_out_of_hidden` genuinely takes.
#![allow(clippy::single_range_in_vec_init)]

use super::*;

// ---- editing ----

#[test]
fn insert_at_a_collapsed_caret_splices_and_advances() {
    let (text, caret) = replace_selection("helloworld", Caret::at(5), " brave ");
    assert_eq!(text, "hello brave world");
    assert_eq!(caret, Caret::at(12)); // just past the inserted 7 chars
}

#[test]
fn insert_over_a_selection_replaces_it() {
    // Select "llo" (chars 2..5), type "y".
    let (text, caret) = replace_selection(
        "hello",
        Caret {
            primary: 5,
            anchor: 2,
        },
        "y",
    );
    assert_eq!(text, "hey");
    assert_eq!(caret, Caret::at(3));
}

#[test]
fn backspace_removes_the_prior_char_or_the_selection() {
    assert_eq!(
        backspace("abc", Caret::at(2)),
        Some(("ac".to_string(), Caret::at(1)))
    );
    // With a selection, backspace deletes the whole selection.
    assert_eq!(
        backspace(
            "abcd",
            Caret {
                primary: 1,
                anchor: 3
            }
        ),
        Some(("ad".to_string(), Caret::at(1)))
    );
    // At the start with no selection: nothing to delete.
    assert_eq!(backspace("abc", Caret::at(0)), None);
}

#[test]
fn delete_forward_removes_the_next_char_or_the_selection() {
    assert_eq!(
        delete_forward("abc", Caret::at(1)),
        Some(("ac".to_string(), Caret::at(1)))
    );
    assert_eq!(
        delete_forward(
            "abcd",
            Caret {
                primary: 1,
                anchor: 3
            }
        ),
        Some(("ad".to_string(), Caret::at(1)))
    );
    // At the end with no selection: nothing to delete.
    assert_eq!(delete_forward("abc", Caret::at(3)), None);
}

#[test]
fn editing_respects_multi_byte_chars() {
    // "héllo": 'é' is 2 bytes, 1 char. Backspacing after it removes one char,
    // not one byte (which would corrupt UTF-8).
    let (text, caret) = backspace("héllo", Caret::at(2)).unwrap();
    assert_eq!(text, "hllo");
    assert_eq!(caret, Caret::at(1));
}

// ---- horizontal motion ----

#[test]
fn move_left_right_step_one_char_and_clamp() {
    assert_eq!(move_left("abc", Caret::at(2), false), Caret::at(1));
    assert_eq!(move_left("abc", Caret::at(0), false), Caret::at(0)); // clamped
    assert_eq!(move_right("abc", Caret::at(2), false), Caret::at(3));
    assert_eq!(move_right("abc", Caret::at(3), false), Caret::at(3)); // clamped at end
}

#[test]
fn arrow_without_shift_collapses_a_selection_to_the_matching_edge() {
    let sel = Caret {
        primary: 4,
        anchor: 1,
    }; // selection 1..4
    // Left collapses to the left edge (1), not 3.
    assert_eq!(move_left("abcdef", sel, false), Caret::at(1));
    // Right collapses to the right edge (4).
    assert_eq!(move_right("abcdef", sel, false), Caret::at(4));
}

#[test]
fn shift_arrow_extends_the_selection_from_the_anchor() {
    let c = Caret::at(2);
    let c = move_right("abcdef", c, true);
    assert_eq!(
        c,
        Caret {
            primary: 3,
            anchor: 2
        }
    );
    let c = move_right("abcdef", c, true);
    assert_eq!(
        c,
        Caret {
            primary: 4,
            anchor: 2
        }
    ); // anchor stays put
}

// ---- vertical motion ----

const GRID: &str = "abcd\nef\nghij"; // line0 len4, line1 len2, line2 len4

#[test]
fn move_down_preserves_the_preferred_column_across_a_short_line() {
    // Caret at line0 col3 (on 'd'), preferred col 3.
    let start = Caret::at(3);
    let (down1, pref) = move_down(GRID, start, false, 3);
    // line1 ("ef") only has 2 chars → clamp to its end (char offset of '\n').
    assert_eq!(line_col(GRID, down1.primary), (1, 2));
    // Down again keeps preferred col 3 → line2 col3 ('j' area).
    let (down2, _) = move_down(GRID, down1, false, pref);
    assert_eq!(line_col(GRID, down2.primary), (2, 3));
}

#[test]
fn move_up_from_first_line_goes_to_document_start() {
    let (c, _) = move_up("abc\ndef", Caret::at(1), false, 1);
    assert_eq!(c, Caret::at(0));
}

#[test]
fn move_down_from_last_line_goes_to_document_end() {
    let (c, _) = move_down("abc\ndef", Caret::at(5), false, 1);
    assert_eq!(c, Caret::at(char_len("abc\ndef")));
}

#[test]
fn move_end_goes_to_the_line_end_not_past_the_newline() {
    // Caret at start of line1 ("ef"). End → offset of the char after "ef".
    let start = line_col_to_char(GRID, 1, 0);
    let c = move_end(GRID, Caret::at(start), false);
    assert_eq!(line_col(GRID, c.primary), (1, 2));
    // End on the last line goes to end-of-text.
    let last = line_col_to_char(GRID, 2, 0);
    let c = move_end(GRID, Caret::at(last), false);
    assert_eq!(c.primary, char_len(GRID));
}

#[test]
fn column_of_reports_within_line_column() {
    assert_eq!(column_of(GRID, 0), 0);
    assert_eq!(column_of(GRID, 3), 3);
    assert_eq!(column_of(GRID, line_col_to_char(GRID, 2, 2)), 2);
}

#[test]
fn clamp_out_of_hidden_snaps_to_the_marker_line_end() {
    // Line 1 ("ef") is folded away (hidden lines 1..2); a caret that ended
    // up there snaps to the end of line 0 (the marker line), not line 1.
    let hidden = [1..2];
    let inside = line_col_to_char(GRID, 1, 1);
    let clamped = clamp_out_of_hidden(GRID, inside, &hidden);
    assert_eq!(
        line_col(GRID, clamped),
        (0, 4),
        "snaps to the end of line 0, the marker line"
    );
}

#[test]
fn clamp_out_of_hidden_is_a_no_op_on_a_visible_line() {
    let hidden = [1..2];
    let visible = line_col_to_char(GRID, 2, 1);
    assert_eq!(clamp_out_of_hidden(GRID, visible, &hidden), visible);
}

#[test]
fn clamp_out_of_hidden_handles_an_empty_hidden_set() {
    assert_eq!(clamp_out_of_hidden(GRID, 5, &[]), 5);
}
