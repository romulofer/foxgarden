//! Shared cache-key hashing (PLAN.md Phase 0). Pulled out of `render.rs` so
//! `widget.rs`'s own per-frame caches (highlight spans, fold ranges, …) can
//! key on the same buffer/hidden-range hashes `render.rs`'s `cached_row_
//! counts` already computes, instead of each call site rolling its own.

use std::hash::{Hash, Hasher};
use std::ops::Range;

use ropey::Rope;

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
