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
#[path = "terminal_input_test.rs"]
mod terminal_input_test;
