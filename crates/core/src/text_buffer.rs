//! A `Rope` that knows when it was last changed.
//!
//! The editor's per-frame caches (syntax-highlight spans, fold ranges,
//! shaped row counts, occurrence matches) all need to answer one question
//! before reusing what they stored last frame: *has the text changed since
//! then?* Answering it by hashing the whole rope makes the cheap path — the
//! overwhelmingly common "nothing changed, reuse everything" one — cost a
//! full pass over the document, on every frame, several times over. On a
//! large file that's more work than the rendering it was meant to avoid.
//!
//! A monotonic counter answers the same question in a comparison. It's kept
//! honest by construction rather than by discipline: the only way to reach
//! the underlying `Rope` mutably is `DerefMut`, which bumps the counter.
//! That's deliberately conservative — taking a `&mut` and then not editing
//! still counts as a change, which can only cost one redundant recompute,
//! never a stale cache showing text that isn't there.

use std::ops::{Deref, DerefMut};

use ropey::Rope;

#[derive(Debug, Clone)]
pub struct TextBuffer {
    rope: Rope,
    revision: u64,
}

impl TextBuffer {
    pub fn new(rope: Rope) -> Self {
        Self { rope, revision: 0 }
    }

    /// A value that changes whenever this buffer might have. Cache keys
    /// compare it; nothing should read meaning into its magnitude beyond
    /// "different means possibly-changed".
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The underlying rope, for the places that need to *own* a copy (a
    /// saved-state snapshot, a comparison against one) rather than read
    /// through this wrapper.
    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Replaces the whole text at once — reloading from disk, restoring a
    /// history snapshot, applying a workspace edit. The counterpart to a
    /// plain `buffer = Rope::from_str(...)` assignment, which would
    /// otherwise be the one way to change the text without the revision
    /// noticing.
    pub fn replace(&mut self, rope: Rope) {
        self.rope = rope;
        self.revision += 1;
    }
}

impl From<Rope> for TextBuffer {
    fn from(rope: Rope) -> Self {
        Self::new(rope)
    }
}

impl Deref for TextBuffer {
    type Target = Rope;

    fn deref(&self) -> &Rope {
        &self.rope
    }
}

impl DerefMut for TextBuffer {
    fn deref_mut(&mut self) -> &mut Rope {
        self.revision += 1;
        &mut self.rope
    }
}

/// Compares text only — two buffers holding the same characters are equal
/// regardless of how many edits each took to get there, which is what
/// `Document::is_dirty`'s "buffer versus saved buffer" check means.
impl PartialEq for TextBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.rope == other.rope
    }
}

impl PartialEq<Rope> for TextBuffer {
    fn eq(&self, other: &Rope) -> bool {
        &self.rope == other
    }
}

impl Eq for TextBuffer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mutable_borrow_advances_the_revision() {
        let mut buffer = TextBuffer::new(Rope::from_str("class A {}"));
        let before = buffer.revision();

        buffer.insert(0, "// hi\n");

        assert_ne!(buffer.revision(), before);
    }

    #[test]
    fn reads_never_advance_the_revision() {
        let buffer = TextBuffer::new(Rope::from_str("class A {}"));
        let before = buffer.revision();

        let _ = buffer.to_string();
        let _ = buffer.len_chars();
        let _ = buffer.line(0);

        assert_eq!(buffer.revision(), before);
    }

    #[test]
    fn replace_advances_the_revision() {
        let mut buffer = TextBuffer::new(Rope::from_str("class A {}"));
        let before = buffer.revision();

        buffer.replace(Rope::from_str("class B {}"));

        assert_ne!(buffer.revision(), before);
        assert_eq!(buffer.to_string(), "class B {}");
    }

    #[test]
    fn equality_is_by_text_not_by_edit_history() {
        let mut edited = TextBuffer::new(Rope::from_str("class A {}"));
        edited.insert(0, "x");
        edited.remove(0..1);

        assert_eq!(edited, TextBuffer::new(Rope::from_str("class A {}")));
        assert_ne!(edited.revision(), 0);
    }
}
