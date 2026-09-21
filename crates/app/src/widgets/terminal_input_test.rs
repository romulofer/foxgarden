
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
    assert_eq!(
        key_event_to_bytes(Key::ArrowUp, no_modifiers()),
        Some(b"\x1b[A".to_vec())
    );
    assert_eq!(
        key_event_to_bytes(Key::ArrowDown, no_modifiers()),
        Some(b"\x1b[B".to_vec())
    );
    assert_eq!(
        key_event_to_bytes(Key::ArrowRight, no_modifiers()),
        Some(b"\x1b[C".to_vec())
    );
    assert_eq!(
        key_event_to_bytes(Key::ArrowLeft, no_modifiers()),
        Some(b"\x1b[D".to_vec())
    );
}

#[test]
fn home_and_end_send_their_standard_sequences() {
    assert_eq!(key_event_to_bytes(Key::Home, no_modifiers()), Some(b"\x1b[H".to_vec()));
    assert_eq!(key_event_to_bytes(Key::End, no_modifiers()), Some(b"\x1b[F".to_vec()));
}

#[test]
fn page_up_and_down_send_their_standard_tilde_sequences() {
    assert_eq!(
        key_event_to_bytes(Key::PageUp, no_modifiers()),
        Some(b"\x1b[5~".to_vec())
    );
    assert_eq!(
        key_event_to_bytes(Key::PageDown, no_modifiers()),
        Some(b"\x1b[6~".to_vec())
    );
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
