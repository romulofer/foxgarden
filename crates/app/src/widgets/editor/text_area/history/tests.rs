//! Table tests for the undo/redo stack ([`super`](history.rs)) — coalescing
//! and redo-fork behaviour, no frame.

use super::*;

fn snap(text: &str, pos: usize) -> Snapshot {
    Snapshot {
        text: text.to_string(),
        caret: Caret::at(pos),
    }
}

#[test]
fn a_run_of_typing_coalesces_into_one_undo_step() {
    let mut h = History::default();
    // Simulate typing "abc" one char at a time: checkpoint the pre-edit state
    // before each keystroke.
    h.checkpoint(snap("", 0), EditKind::Typing); // before 'a'
    h.checkpoint(snap("a", 1), EditKind::Typing); // before 'b' — coalesces
    h.checkpoint(snap("ab", 2), EditKind::Typing); // before 'c' — coalesces

    // One undo restores all the way to before the run started.
    let restored = h.undo(snap("abc", 3)).unwrap();
    assert_eq!(restored, snap("", 0));
    // Nothing more to undo — the run was a single step.
    assert_eq!(h.undo(snap("", 0)), None);
}

#[test]
fn switching_edit_kind_breaks_coalescing() {
    let mut h = History::default();
    h.checkpoint(snap("", 0), EditKind::Typing); // before typing "ab"
    h.checkpoint(snap("a", 1), EditKind::Typing);
    h.checkpoint(snap("ab", 2), EditKind::Deleting); // backspace starts a new step

    // First undo reverts the delete run back to "ab".
    assert_eq!(h.undo(snap("a", 1)).unwrap(), snap("ab", 2));
    // Second undo reverts the typing run back to "".
    assert_eq!(h.undo(snap("ab", 2)).unwrap(), snap("", 0));
}

#[test]
fn other_kind_never_coalesces() {
    let mut h = History::default();
    h.checkpoint(snap("", 0), EditKind::Other); // paste 1
    h.checkpoint(snap("x", 1), EditKind::Other); // paste 2 — distinct step
    assert_eq!(h.undo(snap("xy", 2)).unwrap(), snap("x", 1));
    assert_eq!(h.undo(snap("x", 1)).unwrap(), snap("", 0));
}

#[test]
fn redo_restores_an_undone_step_and_a_new_edit_forks_it_away() {
    let mut h = History::default();
    h.checkpoint(snap("", 0), EditKind::Other);
    let undone = h.undo(snap("a", 1)).unwrap();
    assert_eq!(undone, snap("", 0));

    // Redo brings back the state we undid from.
    assert_eq!(h.redo(snap("", 0)).unwrap(), snap("a", 1));

    // Undo again, then make a *new* edit: the old redo target is forked away.
    h.undo(snap("a", 1)).unwrap();
    h.checkpoint(snap("", 0), EditKind::Typing); // new edit clears the future
    assert_eq!(h.redo(snap("z", 1)), None);
}

#[test]
fn break_run_forces_a_fresh_step_between_same_kind_edits() {
    let mut h = History::default();
    h.checkpoint(snap("", 0), EditKind::Typing); // type "a"
    h.break_run(); // e.g. the caret was moved / clicked elsewhere
    h.checkpoint(snap("a", 1), EditKind::Typing); // type "b" — must not coalesce

    assert_eq!(h.undo(snap("ab", 2)).unwrap(), snap("a", 1));
    assert_eq!(h.undo(snap("a", 1)).unwrap(), snap("", 0));
}

#[test]
fn undo_on_empty_history_is_none() {
    let mut h = History::default();
    assert_eq!(h.undo(snap("x", 1)), None);
    assert_eq!(h.redo(snap("x", 1)), None);
}
