
use super::*;

/// Ground truth for every case below: the same per-offset conversion
/// `paint_diagnostics` used before this file's byte→char batching (see
/// `TECHNICAL_DEBT.md`'s now-resolved entry on this function) — kept
/// here purely as an oracle to check `char_offsets_for` against, not as
/// production code.
fn naive_char_offset(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset].chars().count()
}

#[test]
fn char_offsets_for_matches_naive_conversion_on_ascii() {
    let text = "abcde";
    let queries = vec![0, 2, 5];
    let offsets = char_offsets_for(text, &queries);

    for &q in &queries {
        assert_eq!(offsets[&q], naive_char_offset(text, q));
    }
}

#[test]
fn char_offsets_for_matches_naive_conversion_across_multi_byte_chars() {
    // "h" (1 byte) + "é" (2 bytes, U+00E9) + "llo" (3 bytes) = 6 bytes,
    // 5 chars — byte offsets land mid-string on both sides of the
    // 2-byte character.
    let text = "héllo";
    let queries = vec![0, 1, 3, 4, 5, 6];
    let offsets = char_offsets_for(text, &queries);

    for &q in &queries {
        assert_eq!(
            offsets[&q],
            naive_char_offset(text, q),
            "byte offset {q} in {text:?} converted incorrectly"
        );
    }
    // Spot-check the interesting one directly: byte 3 sits right after
    // the 2-byte "é", so exactly 2 chars ("h", "é") precede it.
    assert_eq!(offsets[&3], 2);
}

#[test]
fn char_offsets_for_handles_empty_text() {
    let offsets = char_offsets_for("", &[0]);
    assert_eq!(offsets[&0], 0);
}

#[test]
fn relative_time_buckets_span_seconds_to_years() {
    assert_eq!(relative_time(1000, 990), "just now");
    assert_eq!(relative_time(1000, 400), "10m ago");
    assert_eq!(relative_time(10_000, 3_600), "1h ago");
    assert_eq!(relative_time(200_000, 100_000), "1d ago");
    assert_eq!(relative_time(10_000_000, 5_000_000), "1mo ago");
    assert_eq!(relative_time(100_000_000, 10_000_000), "2y ago");
}

#[test]
fn relative_time_clamps_a_timestamp_in_the_future_to_just_now() {
    assert_eq!(relative_time(1000, 2000), "just now");
}

fn line(sha: &str, author: &str, author_time: i64, summary: &str) -> BlameLine {
    BlameLine {
        sha: sha.to_string(),
        author: author.to_string(),
        author_time,
        summary: summary.to_string(),
    }
}

#[test]
fn blame_annotation_text_joins_author_relative_time_and_summary() {
    let l = line("abc123abc123abc123abc123abc123abc123abcd", "Ada", 400, "Fix the thing");
    assert_eq!(blame_annotation_text(&l, 1000), "Ada • 10m ago • Fix the thing");
}

#[test]
fn blame_annotation_text_shows_a_short_label_for_an_uncommitted_line() {
    let l = line(UNCOMMITTED_SHA, "Not Committed Yet", 900, "Version of f.txt from f.txt");
    assert_eq!(blame_annotation_text(&l, 1000), "Uncommitted change");
}
