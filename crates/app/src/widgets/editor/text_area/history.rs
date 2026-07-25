//! The virtualized editor's **undo/redo stack** (PLAN.md 2e) — state the
//! private `egui::TextEdit` used to own, now ours (which also simplifies the
//! Ctrl+Z/menu-item plumbing: undo is a direct call, no synthesized key event).
//! Pure and frame-free, so the coalescing rule is table-tested.
//!
//! Coalescing: a *run* of same-kind edits (typing char after char, or
//! backspacing char after char) collapses to a single undo step, matching what
//! every editor does — one Ctrl+Z undoes the whole word you just typed, not one
//! letter. Any other edit kind, a caret move, or a switch between typing and
//! deleting starts a fresh step.

use super::input::Caret;

/// A restorable editor state — the buffer text plus where the caret was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub text: String,
    pub caret: Caret,
}

/// What kind of edit produced a checkpoint, for coalescing. Consecutive
/// `Typing` (or consecutive `Deleting`) checkpoints merge into one undo step;
/// `Other` (paste, multi-line transform, a caret move between edits) never
/// coalesces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Deleting,
    Other,
}

#[derive(Clone)]
pub struct History {
    past: Vec<Snapshot>,
    future: Vec<Snapshot>,
    /// Kind of the most recent checkpoint's run, for the coalescing decision;
    /// cleared by undo/redo so the next edit always starts a fresh step.
    last_kind: Option<EditKind>,
    cap: usize,
}

impl Default for History {
    fn default() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            last_kind: None,
            cap: 500,
        }
    }
}

impl History {
    /// Records the pre-edit state as a potential undo target, unless it
    /// coalesces with the previous same-kind run. Call **before** applying the
    /// edit, passing the state as it stands right now (which is what undo will
    /// restore to). Clears the redo stack — a new edit forks history.
    pub fn checkpoint(&mut self, before: Snapshot, kind: EditKind) {
        let coalesce = kind != EditKind::Other && self.last_kind == Some(kind);
        if !coalesce {
            self.past.push(before);
            if self.past.len() > self.cap {
                self.past.remove(0);
            }
        }
        self.future.clear();
        self.last_kind = Some(kind);
    }

    /// Undoes one step: returns the state to restore to, and stashes `current`
    /// for redo. `None` when there's nothing to undo.
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let restored = self.past.pop()?;
        self.future.push(current);
        self.last_kind = None; // next edit starts a fresh run
        Some(restored)
    }

    /// Redoes one step: returns the state to restore to, and stashes `current`
    /// back onto the undo stack. `None` when there's nothing to redo.
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let restored = self.future.pop()?;
        self.past.push(current);
        self.last_kind = None;
        Some(restored)
    }

    /// Forces the next `checkpoint` to start a fresh (non-coalesced) step —
    /// call on a caret move or selection change between edits, so typing after
    /// clicking elsewhere doesn't merge into the previous run.
    pub fn break_run(&mut self) {
        self.last_kind = None;
    }
}

#[cfg(test)]
mod tests;
