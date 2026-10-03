//! Shared cache-key hashing (PLAN.md Phase 0). Pulled out of `render.rs` so
//! `widget.rs`'s own per-frame caches (highlight spans, fold ranges, …) can
//! key on the same buffer/hidden-range hashes `render.rs`'s `cached_row_
//! counts` already computes, instead of each call site rolling its own.

use std::hash::{Hash, Hasher};
use std::ops::Range;

use ropey::Rope;

/// Identifies "which text is this?" for a cache key, without hashing the
/// whole document to find out.
///
/// The common case is a buffer the app owns: `fg_core::TextBuffer` already
/// tracks a revision that changes on every mutation, so an unchanged
/// document is recognized by comparing two integers. The exception is a
/// buffer built *inside* one frame from just-edited text (`shell::show`'s
/// post-edit reshape), which has no revision of its own — that one falls
/// back to hashing, which is affordable precisely because it only happens
/// on frames where the user actually typed something, not on the idle
/// blink frames the revision path exists to make free.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ContentKey {
    revision: u64,
    edited_hash: u64,
}

impl ContentKey {
    /// For text that is exactly what a `TextBuffer` currently holds.
    pub fn revision(revision: u64) -> Self {
        Self {
            revision,
            edited_hash: 0,
        }
    }

    /// For a within-frame edited copy of the buffer at `revision` — hashed
    /// so it can't collide with the un-edited text at that same revision.
    pub fn edited(revision: u64, buffer: &Rope) -> Self {
        Self {
            revision,
            edited_hash: hash_rope_content(buffer) | 1,
        }
    }
}

pub fn hash_rope_content(buffer: &Rope) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Hashes each chunk of the rope's own internal representation rather
    // than `buffer.to_string()` first, so cache-checking (the common,
    // nothing-changed case too) never pays for a whole-buffer string
    // allocation just to throw it away.
    for chunk in buffer.chunks() {
        chunk.hash(&mut hasher);
    }
    hasher.finish()
}

pub fn hash_hidden(hidden: &[Range<usize>]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for range in hidden {
        range.start.hash(&mut hasher);
        range.end.hash(&mut hasher);
    }
    hasher.finish()
}
