//! Byte↔char offset conversion for editor text. Generic across everything
//! in `widgets::editor` that needs to translate between the byte offsets a
//! syntax tree deals in and the char offsets egui's `TextEdit` cursor API
//! expects — promoted out of `auto_edit` (which is about specific edit
//! transforms, not this) once a second and third file started importing
//! from it for something that had nothing to do with auto-pair/auto-indent.

/// The byte offset of `char_idx`, i.e. the char at that index's first byte.
/// `char_idx == text.chars().count()` (one past the last char) is a valid
/// query and returns `text.len()`, matching how a cursor position can
/// legitimately sit at the end of the buffer.
pub(super) fn char_to_byte(text: &str, char_idx: usize) -> usize {
    // Counts UTF-8 *lead* bytes rather than decoding characters
    // (`char_indices().nth()`): a lead byte is any byte that isn't a
    // `0b10xxxxxx` continuation, so finding the n-th character is a byte
    // scan with no decoding, no `char` construction, and no branch per
    // scalar value. Same answer, several times faster on the long spans
    // these conversions routinely cover — this is called from edit and
    // completion paths that hand it whole-document text.
    let bytes = text.as_bytes();
    let mut seen = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if is_char_boundary_byte(byte) {
            if seen == char_idx {
                return index;
            }
            seen += 1;
        }
    }
    text.len()
}

/// Whether `byte` starts a UTF-8 character (i.e. isn't a `10xxxxxx`
/// continuation byte).
fn is_char_boundary_byte(byte: u8) -> bool {
    (byte as i8) >= -0x40
}

/// Inverse of `char_to_byte`: the char offset of whatever byte offset
/// `byte` falls on. Used to turn a byte position found by walking a syntax
/// tree (which only ever deals in bytes) into the char position
/// `codegen::insert_generated`/`TextEdit`'s cursor API expect.
pub(super) fn byte_to_char(text: &str, byte: usize) -> usize {
    // Same lead-byte counting as `char_to_byte`, for the same reason.
    text.as_bytes()[..byte].iter().filter(|&&b| is_char_boundary_byte(b)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_to_char_round_trips_with_char_to_byte() {
        let text = "class Foo { // café\n}\n";
        for char_idx in 0..text.chars().count() {
            let byte = char_to_byte(text, char_idx);
            assert_eq!(byte_to_char(text, byte), char_idx);
        }
    }

    #[test]
    fn char_to_byte_one_past_the_end_is_the_text_length() {
        let text = "café";
        assert_eq!(char_to_byte(text, text.chars().count()), text.len());
    }

    #[test]
    fn conversions_agree_with_the_straightforward_definitions() {
        for text in ["", "plain ascii", "class Café { // ação\n}\n", "🦊 emoji + ünïcödé"] {
            let char_count = text.chars().count();
            for char_idx in 0..=char_count {
                let expected = text.char_indices().nth(char_idx).map_or(text.len(), |(b, _)| b);
                assert_eq!(char_to_byte(text, char_idx), expected, "char_to_byte({text:?}, {char_idx})");
            }
            for byte in 0..=text.len() {
                if !text.is_char_boundary(byte) {
                    continue;
                }
                assert_eq!(byte_to_char(text, byte), text[..byte].chars().count(), "byte_to_char({text:?}, {byte})");
            }
        }
    }

    #[test]
    fn byte_to_char_at_zero_is_zero() {
        assert_eq!(byte_to_char("hello", 0), 0);
    }
}
