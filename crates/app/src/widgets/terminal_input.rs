//! Keyboard-to-terminal-byte-sequence translation (`PLAN.md` terminal-panel
//! track, Phase 8; `SPEC.md` §8.5) — the piece both docs flag as needing its
//! own dedicated budget rather than a one-line `match`: every key a real
//! shell or full-screen program (`less`, `vim`, shell line-editing) actually
//! depends on, not just plain printable characters (those arrive as
//! `Event::Text` and are written byte-for-byte already, unrelated to this
//! table). Pure data in, data out — no `egui::Ui`/pty dependency — so it's
//! unit-testable headless against known-correct VT100/xterm byte sequences,
//! per both docs' own "table-test against known-correct values, not asserted
//! correct by inspection" requirement.

use egui::{Key, Modifiers};

/// Translates one non-text key press into the exact bytes a real terminal
/// sends for it. `None` means "no defined terminal meaning" (a bare
/// modifier key, a letter with no Ctrl held — that path is `Event::Text`'s
/// job, not this table's).
pub fn key_event_to_bytes(key: Key, modifiers: Modifiers) -> Option<Vec<u8>> {
    if modifiers.ctrl
        && !modifiers.alt
        && let Some(byte) = ctrl_letter_byte(key)
    {
        return Some(vec![byte]);
    }

    let bytes: &[u8] = match key {
        Key::Enter => b"\r",
        // `0x7f` (DEL) is what most modern terminals (including every
        // mainstream one on Linux/macOS) actually send for Backspace, not
        // the `0x08` (BS) control code its name suggests — `stty erase`
        // on a freshly spawned shell already expects `0x7f`.
        Key::Backspace => &[0x7f],
        Key::Tab => b"\t",
        Key::Escape => &[0x1b],
        Key::ArrowUp => b"\x1b[A",
        Key::ArrowDown => b"\x1b[B",
        Key::ArrowRight => b"\x1b[C",
        Key::ArrowLeft => b"\x1b[D",
        Key::Home => b"\x1b[H",
        Key::End => b"\x1b[F",
        Key::PageUp => b"\x1b[5~",
        Key::PageDown => b"\x1b[6~",
        Key::Insert => b"\x1b[2~",
        Key::Delete => b"\x1b[3~",
        Key::F1 => b"\x1bOP",
        Key::F2 => b"\x1bOQ",
        Key::F3 => b"\x1bOR",
        Key::F4 => b"\x1bOS",
        Key::F5 => b"\x1b[15~",
        Key::F6 => b"\x1b[17~",
        Key::F7 => b"\x1b[18~",
        Key::F8 => b"\x1b[19~",
        Key::F9 => b"\x1b[20~",
        Key::F10 => b"\x1b[21~",
        Key::F11 => b"\x1b[23~",
        Key::F12 => b"\x1b[24~",
        _ => return None,
    };
    Some(bytes.to_vec())
}

/// `Ctrl+<letter>` maps to that letter's 1-indexed position in the alphabet
/// as a single control byte (`Ctrl+A` -> `0x01`, ..., `Ctrl+Z` -> `0x1a`) —
/// the standard terminal convention every shell relies on for at least
/// `Ctrl+C` (interrupt, `0x03`), `Ctrl+D` (EOF, `0x04`), and `Ctrl+Z`
/// (suspend, `0x1a`), `SPEC.md` §8.5's own explicitly named minimum.
fn ctrl_letter_byte(key: Key) -> Option<u8> {
    let letter = match key {
        Key::A => 1,
        Key::B => 2,
        Key::C => 3,
        Key::D => 4,
        Key::E => 5,
        Key::F => 6,
        Key::G => 7,
        Key::H => 8,
        Key::I => 9,
        Key::J => 10,
        Key::K => 11,
        Key::L => 12,
        Key::M => 13,
        Key::N => 14,
        Key::O => 15,
        Key::P => 16,
        Key::Q => 17,
        Key::R => 18,
        Key::S => 19,
        Key::T => 20,
        Key::U => 21,
        Key::V => 22,
        Key::W => 23,
        Key::X => 24,
        Key::Y => 25,
        Key::Z => 26,
        _ => return None,
    };
    Some(letter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_modifiers() -> Modifiers {
        Modifiers::default()
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Default::default()
        }
    }

    #[test]
    fn enter_sends_carriage_return() {
        assert_eq!(key_event_to_bytes(Key::Enter, no_modifiers()), Some(b"\r".to_vec()));
    }

    #[test]
    fn tab_sends_a_literal_tab_byte() {
        assert_eq!(key_event_to_bytes(Key::Tab, no_modifiers()), Some(b"\t".to_vec()));
    }

    #[test]
    fn backspace_sends_del_not_the_bs_control_code() {
        assert_eq!(key_event_to_bytes(Key::Backspace, no_modifiers()), Some(vec![0x7f]));
    }

    #[test]
    fn escape_sends_the_bare_escape_byte() {
        assert_eq!(key_event_to_bytes(Key::Escape, no_modifiers()), Some(vec![0x1b]));
    }

    #[test]
    fn arrow_keys_send_their_standard_csi_sequences() {
        assert_eq!(key_event_to_bytes(Key::ArrowUp, no_modifiers()), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_event_to_bytes(Key::ArrowDown, no_modifiers()), Some(b"\x1b[B".to_vec()));
        assert_eq!(key_event_to_bytes(Key::ArrowRight, no_modifiers()), Some(b"\x1b[C".to_vec()));
        assert_eq!(key_event_to_bytes(Key::ArrowLeft, no_modifiers()), Some(b"\x1b[D".to_vec()));
    }

    #[test]
    fn home_and_end_send_their_standard_sequences() {
        assert_eq!(key_event_to_bytes(Key::Home, no_modifiers()), Some(b"\x1b[H".to_vec()));
        assert_eq!(key_event_to_bytes(Key::End, no_modifiers()), Some(b"\x1b[F".to_vec()));
    }

    #[test]
    fn page_up_and_down_send_their_standard_tilde_sequences() {
        assert_eq!(key_event_to_bytes(Key::PageUp, no_modifiers()), Some(b"\x1b[5~".to_vec()));
        assert_eq!(key_event_to_bytes(Key::PageDown, no_modifiers()), Some(b"\x1b[6~".to_vec()));
    }

    #[test]
    fn function_keys_send_their_standard_sequences() {
        assert_eq!(key_event_to_bytes(Key::F1, no_modifiers()), Some(b"\x1bOP".to_vec()));
        assert_eq!(key_event_to_bytes(Key::F5, no_modifiers()), Some(b"\x1b[15~".to_vec()));
        assert_eq!(key_event_to_bytes(Key::F12, no_modifiers()), Some(b"\x1b[24~".to_vec()));
    }

    #[test]
    fn ctrl_c_sends_the_interrupt_byte() {
        assert_eq!(key_event_to_bytes(Key::C, ctrl()), Some(vec![0x03]));
    }

    #[test]
    fn ctrl_d_sends_the_eof_byte() {
        assert_eq!(key_event_to_bytes(Key::D, ctrl()), Some(vec![0x04]));
    }

    #[test]
    fn ctrl_z_sends_the_suspend_byte() {
        assert_eq!(key_event_to_bytes(Key::Z, ctrl()), Some(vec![0x1a]));
    }

    #[test]
    fn ctrl_a_sends_the_first_control_byte() {
        assert_eq!(key_event_to_bytes(Key::A, ctrl()), Some(vec![0x01]));
    }

    #[test]
    fn a_plain_letter_with_no_ctrl_held_has_no_translation_here() {
        // Printable text goes through `Event::Text`, not this table.
        assert_eq!(key_event_to_bytes(Key::C, no_modifiers()), None);
    }

    #[test]
    fn a_key_with_no_terminal_meaning_translates_to_nothing() {
        assert_eq!(key_event_to_bytes(Key::F13, no_modifiers()), None);
    }

    #[test]
    fn ctrl_alt_combo_falls_through_to_the_plain_key_table_not_the_ctrl_table() {
        // Ctrl+Alt+arrow isn't a documented control-byte combo; falls
        // through to the arrow's own plain sequence rather than producing
        // a control byte meant for a bare Ctrl+letter.
        let ctrl_alt = Modifiers {
            ctrl: true,
            alt: true,
            ..Default::default()
        };
        assert_eq!(key_event_to_bytes(Key::ArrowUp, ctrl_alt), Some(b"\x1b[A".to_vec()));
    }
}
