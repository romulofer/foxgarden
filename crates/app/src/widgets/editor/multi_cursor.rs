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
    let matches_at = |start: usize| (0..nn).all(|i| chars_eq(haystack[start + i], needle_chars[i], case_sensitive));

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
    let matches_at = |start: usize| (0..nn).all(|i| chars_eq(haystack[start + i], needle_chars[i], case_sensitive));

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
#[path = "multi_cursor_test.rs"]
mod multi_cursor_test;
