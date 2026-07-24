use std::ops::Range;

/// The editing operations that can be applied uniformly across every active
/// cursor while multi-cursor mode is active. Enter is just `Insert("\n")` —
/// auto-indent is deliberately skipped while multiple cursors are active, so
/// there's nothing enter-specific left to do.
pub(super) enum MultiEditOp {
    Insert(String),
    Backspace,
    Delete,
}

fn chars_eq(a: char, b: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        a == b
    } else {
        a.to_lowercase().eq(b.to_lowercase())
    }
}

/// Finds the identifier (`alnum` or `_`) touching `char_idx`, matching
/// VSCode's "word under or touching the cursor" semantics: a cursor sitting
/// between two word characters, or immediately before/after a word, all
/// count as touching it. Returns an empty range at `char_idx` if no word
/// touches that position.
pub(super) fn word_range_at(text: &str, char_idx: usize) -> Range<usize> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let idx = char_idx.min(n);
    let is_word = |c: char| c.is_alphanumeric() || c == '_';

    if idx < n && is_word(chars[idx]) {
        let mut start = idx;
        let mut end = idx;
        while start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        }
        while end < n && is_word(chars[end]) {
            end += 1;
        }
        start..end
    } else if idx > 0 && is_word(chars[idx - 1]) {
        let mut start = idx;
        while start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        }
        start..idx
    } else {
        idx..idx
    }
}

/// Finds the next occurrence of `needle` at or after `after_char`, wrapping
/// around the buffer if nothing is found before the end. Returns `None` for
/// an empty needle or if `needle` doesn't occur anywhere in `text`.
pub(super) fn find_next_occurrence(
    text: &str,
    needle: &str,
    after_char: usize,
    case_sensitive: bool,
) -> Option<Range<usize>> {
    let haystack: Vec<char> = text.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();
    let hn = haystack.len();
    let nn = needle_chars.len();
    if nn == 0 || nn > hn {
        return None;
    }

    let max_start = hn - nn;
    let after_char = after_char.min(max_start + 1);
    let matches_at =
        |start: usize| (0..nn).all(|i| chars_eq(haystack[start + i], needle_chars[i], case_sensitive));

    (after_char..=max_start)
        .chain(0..after_char)
        .find(|&start| matches_at(start))
        .map(|start| start..start + nn)
}

/// Like `find_next_occurrence`, but skips any match already present in
/// `claimed` (exact range match — occurrences of the same text are
/// exact-match-dedupable). Bounded by the buffer's total occurrence count,
/// so it always terminates even when every occurrence is already claimed.
pub(super) fn find_next_unclaimed_occurrence(
    text: &str,
    needle: &str,
    after_char: usize,
    claimed: &[Range<usize>],
    case_sensitive: bool,
) -> Option<Range<usize>> {
    if needle.is_empty() {
        return None;
    }

    let mut cursor = after_char;
    let mut first_seen: Option<usize> = None;
    loop {
        let found = find_next_occurrence(text, needle, cursor, case_sensitive)?;
        if first_seen == Some(found.start) {
            return None;
        }
        first_seen.get_or_insert(found.start);

        if !claimed.iter().any(|r| r.start == found.start && r.end == found.end) {
            return Some(found);
        }
        cursor = found.end;
    }
}

/// Every non-overlapping occurrence of `needle` in `text`, in order — used
/// for "highlight every occurrence of the word under the cursor" (see
/// `widgets::editor::show`'s occurrence-highlight painting), which wants
/// the complete set at once rather than `find_next_occurrence`'s single
/// forward/wrapping search from a cursor.
pub(super) fn find_all_occurrences(text: &str, needle: &str, case_sensitive: bool) -> Vec<Range<usize>> {
    let haystack: Vec<char> = text.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();
    let hn = haystack.len();
    let nn = needle_chars.len();
    if nn == 0 || nn > hn {
        return Vec::new();
    }
    let matches_at =
        |start: usize| (0..nn).all(|i| chars_eq(haystack[start + i], needle_chars[i], case_sensitive));

    let mut result = Vec::new();
    let mut start = 0;
    while start + nn <= hn {
        if matches_at(start) {
            result.push(start..start + nn);
            start += nn; // non-overlapping: skip past this match
        } else {
            start += 1;
        }
    }
    result
}

/// Applies `op` at every position in `selections` simultaneously, as if each
/// were an independent cursor. Selections are assumed non-overlapping.
/// Processes them in ascending order of `start`, tracking a running
/// character-count delta so each selection's effective offset accounts for
/// every edit already applied to its left — this is what makes the returned
/// cursor positions correct for every selection, not just the last one
/// processed. Returns the resulting text plus one cursor char-position per
/// entry of `selections`, in the SAME order as the input slice.
pub(super) fn apply_multi_edit(text: &str, selections: &[Range<usize>], op: &MultiEditOp) -> (String, Vec<usize>) {
    let mut order: Vec<usize> = (0..selections.len()).collect();
    order.sort_by_key(|&i| selections[i].start);

    let mut chars: Vec<char> = text.chars().collect();
    let mut cumulative_delta: isize = 0;
    let mut new_positions = vec![0usize; selections.len()];

    for i in order {
        let sel = &selections[i];
        let eff_start = (sel.start as isize + cumulative_delta) as usize;
        let eff_end = (sel.end as isize + cumulative_delta) as usize;

        let (cursor, delta) = match op {
            MultiEditOp::Insert(s) => {
                let inserted: Vec<char> = s.chars().collect();
                let inserted_len = inserted.len();
                let removed_len = eff_end - eff_start;
                chars.splice(eff_start..eff_end, inserted);
                (eff_start + inserted_len, inserted_len as isize - removed_len as isize)
            }
            MultiEditOp::Backspace => {
                if eff_start != eff_end {
                    chars.drain(eff_start..eff_end);
                    (eff_start, -((eff_end - eff_start) as isize))
                } else if eff_start == 0 {
                    (eff_start, 0)
                } else {
                    chars.drain(eff_start - 1..eff_start);
                    (eff_start - 1, -1)
                }
            }
            MultiEditOp::Delete => {
                if eff_start != eff_end {
                    chars.drain(eff_start..eff_end);
                    (eff_start, -((eff_end - eff_start) as isize))
                } else if eff_start >= chars.len() {
                    (eff_start, 0)
                } else {
                    chars.drain(eff_start..eff_start + 1);
                    (eff_start, -1)
                }
            }
        };

        new_positions[i] = cursor;
        cumulative_delta += delta;
    }

    (chars.into_iter().collect(), new_positions)
}

#[cfg(test)]
mod tests {
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
        assert_eq!(find_next_unclaimed_occurrence("foo bar", "foo", 3, &claimed, true), None);
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
        assert_eq!(find_all_occurrences("foo bar foo baz foo", "foo", true), vec![0..3, 8..11, 16..19]);
    }

    #[test]
    fn find_all_occurrences_is_case_sensitive_by_default() {
        assert_eq!(find_all_occurrences("foo FOO Foo", "foo", true), vec![0..3]);
    }

    #[test]
    fn find_all_occurrences_case_insensitive_finds_every_casing() {
        assert_eq!(find_all_occurrences("foo FOO Foo", "foo", false), vec![0..3, 4..7, 8..11]);
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
        let (text, positions) =
            apply_multi_edit("aaa bbb ccc", &selections, &MultiEditOp::Insert("X".to_string()));
        assert_eq!(text, "X bbb X");
        assert_eq!(positions, vec![1, 7]);
    }

    #[test]
    fn apply_multi_edit_output_order_matches_input_order_not_processing_order() {
        let selections = vec![4..4, 0..0];
        let (_, positions) = apply_multi_edit("abcde", &selections, &MultiEditOp::Insert("_".to_string()));
        assert_eq!(positions, vec![6, 1]);
    }
}
