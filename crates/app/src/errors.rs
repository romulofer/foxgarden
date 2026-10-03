//! One place to hand a user-facing failure to, instead of assigning
//! straight into the app's single `last_error` slot.
//!
//! `last_error` is one `Option<String>` feeding one modal, but failures
//! don't arrive one at a time: a frame can finish two tool installs, sync
//! several documents with a dead language server, and auto-save a handful
//! of tabs onto a full disk. A bare `*last_error = Some(msg)` at each of
//! those sites means whichever ran last silently erases every earlier one,
//! so the user is told about one problem and never learns about the rest.
//!
//! `report` accumulates instead: messages stack up as separate lines in the
//! same modal, an identical message repeated (the common case — the same
//! failure re-reported every frame until the user acknowledges it) is
//! ignored rather than repeated, and the total is capped so a per-frame
//! failure loop can't grow an unbounded string behind a modal nobody has
//! dismissed yet.

/// How many distinct messages one modal shows before further ones are
/// dropped. Past a handful, an error dialog stops being readable — and the
/// first failures are the ones that explain the rest.
const MAX_MESSAGES: usize = 5;

/// Adds `message` to whatever is already pending, if anything.
pub fn report(last_error: &mut Option<String>, message: String) {
    match last_error {
        None => *last_error = Some(message),
        Some(existing) => {
            if existing.lines().any(|line| line == message) {
                return;
            }
            if existing.lines().count() >= MAX_MESSAGES {
                return;
            }
            existing.push('\n');
            existing.push_str(&message);
        }
    }
}

#[cfg(test)]
#[path = "errors_test.rs"]
mod errors_test;
