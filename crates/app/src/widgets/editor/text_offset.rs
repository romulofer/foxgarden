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
    text.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(text.len())
}

/// Inverse of `char_to_byte`: the char offset of whatever byte offset
/// `byte` falls on. Used to turn a byte position found by walking a syntax
/// tree (which only ever deals in bytes) into the char position
/// `codegen::insert_generated`/`TextEdit`'s cursor API expect.
pub(super) fn byte_to_char(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
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
    fn byte_to_char_at_zero_is_zero() {
        assert_eq!(byte_to_char("hello", 0), 0);
    }
}
