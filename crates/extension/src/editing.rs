//! `PLAN.md` Track 24 Phase 6: what a language contributes to typing in it —
//! the live templates and keywords word completion offers.
//!
//! A snippet's trigger and expansion are a language's own idiom (`sout` is
//! `System.out.println(…)` in Java and `println(…)` in Kotlin), as is the
//! list of words reserved in it, so neither belongs in the editor.

/// One live template: type `trigger`, press Tab with nothing selected, get
/// `body` in its place. `${cursor}` in `body` marks where the caret lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub trigger: String,
    pub body: String,
}

impl Snippet {
    pub fn new(trigger: &str, body: &str) -> Self {
        Self {
            trigger: trigger.to_string(),
            body: body.to_string(),
        }
    }
}
