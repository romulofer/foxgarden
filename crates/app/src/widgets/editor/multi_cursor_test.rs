
use super::*;

#[test]
fn word_range_at_mid_word() {
    assert_eq!(word_range_at("foobar", 3), 0..6);
}

#[test]
fn word_range_at_word_start_boundary() {
    assert_eq!(word_range_at("  foo", 2), 2..5);
}

#[test]
fn word_range_at_word_end_boundary() {
    assert_eq!(word_range_at("foo  ", 3), 0..3);
}

#[test]
fn word_range_at_in_whitespace_is_empty() {
    assert_eq!(word_range_at("foo   bar", 5), 5..5);
}

#[test]
fn word_range_at_leading_underscore() {
    assert_eq!(word_range_at("_foo", 0), 0..4);
    assert_eq!(word_range_at("_foo", 4), 0..4);
}

#[test]
fn word_range_at_empty_string() {
    assert_eq!(word_range_at("", 0), 0..0);
}

#[test]
fn find_next_occurrence_forward_match() {
    assert_eq!(find_next_occurrence("foo bar foo", "foo", 1, true), Some(8..11));
}

#[test]
fn find_next_occurrence_wraps_around() {
    assert_eq!(find_next_occurrence("foo bar", "foo", 4, true), Some(0..3));
}

#[test]
fn find_next_occurrence_no_match() {
    assert_eq!(find_next_occurrence("foo bar", "baz", 0, true), None);
}

#[test]
fn find_next_occurrence_empty_needle() {
    assert_eq!(find_next_occurrence("foo bar", "", 0, true), None);
}

#[test]
fn find_next_occurrence_case_insensitive_finds_differently_cased() {
    assert_eq!(find_next_occurrence("Foo foo FOO", "foo", 1, false), Some(4..7));
}

#[test]
fn find_next_occurrence_case_sensitive_skips_differently_cased() {
    // "FOO" (uppercase) should be skipped in favor of wrapping back to
    // the only exact-case "foo" match.
    assert_eq!(find_next_occurrence("foo FOO", "foo", 1, true), Some(0..3));
}

#[test]
fn find_next_unclaimed_occurrence_skips_claimed() {
    #[allow(clippy::single_range_in_vec_init)] // a genuine one-entry Vec<Range<usize>>, not a range of a Vec
    let claimed = vec![0..3];
    assert_eq!(
        find_next_unclaimed_occurrence("foo bar foo baz foo", "foo", 0, &claimed, true),
        Some(8..11)
    );
}

#[test]
fn find_next_unclaimed_occurrence_all_claimed_returns_none_without_looping_forever() {
    #[allow(clippy::single_range_in_vec_init)] // a genuine one-entry Vec<Range<usize>>, not a range of a Vec
    let claimed = vec![0..3];
    assert_eq!(
        find_next_unclaimed_occurrence("foo bar", "foo", 3, &claimed, true),
        None
    );
}

#[test]
fn find_next_unclaimed_occurrence_finds_first_unclaimed_after_cursor() {
    assert_eq!(
        find_next_unclaimed_occurrence("foo bar foo", "foo", 1, &[], true),
        Some(8..11)
    );
}

#[test]
fn find_all_occurrences_finds_every_match_in_order() {
    assert_eq!(
        find_all_occurrences("foo bar foo baz foo", "foo", true),
        vec![0..3, 8..11, 16..19]
    );
}

#[test]
fn find_all_occurrences_is_case_sensitive_by_default() {
    assert_eq!(find_all_occurrences("foo FOO Foo", "foo", true), vec![0..3]);
}

#[test]
fn find_all_occurrences_case_insensitive_finds_every_casing() {
    assert_eq!(
        find_all_occurrences("foo FOO Foo", "foo", false),
        vec![0..3, 4..7, 8..11]
    );
}

#[test]
fn find_all_occurrences_empty_needle_returns_nothing() {
    assert_eq!(find_all_occurrences("foo bar", "", true), Vec::<Range<usize>>::new());
}

#[test]
fn find_all_occurrences_no_match_returns_nothing() {
    assert_eq!(find_all_occurrences("foo bar", "baz", true), Vec::<Range<usize>>::new());
}

#[test]
fn find_all_occurrences_are_non_overlapping() {
    // "aa" in "aaaa" — non-overlapping gives 2 matches, not 3.
    assert_eq!(find_all_occurrences("aaaa", "aa", true), vec![0..2, 2..4]);
}

#[test]
fn apply_multi_edit_insert_at_three_carets_uses_ascending_delta_correction() {
    // Regression test: a naive descending-order implementation gets the
    // cursor positions wrong for every caret but the last one processed.
    let selections = vec![2..2, 4..4, 6..6];
    let (text, positions) = apply_multi_edit("aXbXcXd", &selections, &MultiEditOp::Insert("Y".to_string()));
    assert_eq!(text, "aXYbXYcXYd");
    assert_eq!(positions, vec![3, 6, 9]);
}

#[test]
fn apply_multi_edit_backspace_at_two_empty_carets() {
    let selections = vec![1..1, 3..3];
    let (text, positions) = apply_multi_edit("abcd", &selections, &MultiEditOp::Backspace);
    assert_eq!(text, "bd");
    assert_eq!(positions, vec![0, 1]);
}

#[test]
fn apply_multi_edit_delete_at_end_of_buffer_is_a_no_op_for_that_caret_only() {
    let selections = vec![0..0, 3..3];
    let (text, positions) = apply_multi_edit("abc", &selections, &MultiEditOp::Delete);
    assert_eq!(text, "bc");
    assert_eq!(positions, vec![0, 2]);
}

#[test]
fn apply_multi_edit_insert_replaces_two_non_empty_selections() {
    let selections = vec![0..3, 8..11];
    let (text, positions) = apply_multi_edit("aaa bbb ccc", &selections, &MultiEditOp::Insert("X".to_string()));
    assert_eq!(text, "X bbb X");
    assert_eq!(positions, vec![1, 7]);
}

#[test]
fn apply_multi_edit_output_order_matches_input_order_not_processing_order() {
    let selections = vec![4..4, 0..0];
    let (_, positions) = apply_multi_edit("abcde", &selections, &MultiEditOp::Insert("_".to_string()));
    assert_eq!(positions, vec![6, 1]);
}
