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

use std::collections::VecDeque;
use std::sync::Arc;

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
    // `Arc`-wrapped (PLAN.md Phase 1 / SPEC.md §1 — `Arc` rather than `Rc`
    // since `egui`'s `ctx.data` temp storage requires `Send + Sync`, even
    // though a single egui frame never actually crosses a thread) so the
    // per-frame `ShellState::clone()` every focused/idle frame pays for —
    // most of which touch no history at all — copies a pointer instead of
    // deep-cloning every buffered `Snapshot::text`. Only an actual mutation
    // (`checkpoint`/`undo`/`redo`) pays a real clone, via `Arc::make_mut`'s
    // copy-on-write, and only when the frame's own clone is still shared
    // with what's stored in `ctx.data` (the common case, since `load`
    // always clones before mutating).
    ///
    /// A `VecDeque` rather than a `Vec` purely so dropping the oldest entry
    /// once `cap` is reached is O(1): a `Vec::remove(0)` memmoves every
    /// remaining `Snapshot` (each holding a full buffer `String`) down one
    /// slot on *every* checkpoint past the limit, which a long editing
    /// session on a large file hits constantly.
    past: Arc<VecDeque<Snapshot>>,
    future: Arc<VecDeque<Snapshot>>,
    /// Kind of the most recent checkpoint's run, for the coalescing decision;
    /// cleared by undo/redo so the next edit always starts a fresh step.
    last_kind: Option<EditKind>,
    cap: usize,
}

impl Default for History {
    fn default() -> Self {
        Self {
            past: Arc::new(VecDeque::new()),
            future: Arc::new(VecDeque::new()),
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
            let past = Arc::make_mut(&mut self.past);
            past.push_back(before);
            if past.len() > self.cap {
                past.pop_front();
            }
        }
        Arc::make_mut(&mut self.future).clear();
        self.last_kind = Some(kind);
    }

    /// Undoes one step: returns the state to restore to, and stashes `current`
    /// for redo. `None` when there's nothing to undo.
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let restored = Arc::make_mut(&mut self.past).pop_back()?;
        Arc::make_mut(&mut self.future).push_back(current);
        self.last_kind = None; // next edit starts a fresh run
        Some(restored)
    }

    /// Redoes one step: returns the state to restore to, and stashes `current`
    /// back onto the undo stack. `None` when there's nothing to redo.
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let restored = Arc::make_mut(&mut self.future).pop_back()?;
        Arc::make_mut(&mut self.past).push_back(current);
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
mod history_test;
