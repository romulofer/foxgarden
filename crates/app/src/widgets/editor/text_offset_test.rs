
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
            assert_eq!(
                char_to_byte(text, char_idx),
                expected,
                "char_to_byte({text:?}, {char_idx})"
            );
        }
        for byte in 0..=text.len() {
            if !text.is_char_boundary(byte) {
                continue;
            }
            assert_eq!(
                byte_to_char(text, byte),
                text[..byte].chars().count(),
                "byte_to_char({text:?}, {byte})"
            );
        }
    }
}

#[test]
fn byte_to_char_at_zero_is_zero() {
    assert_eq!(byte_to_char("hello", 0), 0);
}
