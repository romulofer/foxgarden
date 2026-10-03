/// A language the editor knows how to open, identified by the stable id
/// whichever extension contributed it declared (`"java"`, `"yaml"`).
///
/// **This used to be a closed six-variant enum** (`PLAN.md` Track 24
/// Phase 2). It is now open: any id an extension registers is a valid
/// `Language`, and the core no longer knows the set. What it deliberately
/// keeps is the *ergonomics* of the enum — `Copy`, cheap `==`, and usable
/// in patterns — so that opening the set did not require touching the
/// couple of hundred call sites that merely name a language or compare
/// two, only the handful that genuinely enumerated all of them. Those the
/// compiler found on its own: a `match` over the constants below is no
/// longer exhaustive, so every such site had to be given a real answer for
/// "a language this build has never heard of", which is the whole point of
/// the phase.
///
/// The id is `&'static str` because a registered language lasts for the
/// process: the registry leaks the ids it registers, the same bargain it
/// already makes for grammars (a loaded grammar's library must outlive
/// every tree parsed with it, so unloading is not supported — see Track 24
/// Checkpoint 0). That is what keeps this `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Language(&'static str);

impl Language {
    /// Wraps an already-'static id. Normally the registry calls this, with
    /// an id it has leaked for the process lifetime; the constants below
    /// are the exception, being `&'static` literals already.
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    /// The stable id — what contributions match on, and what a config file
    /// or a manifest would name.
    pub const fn id(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// The languages this build ships with, under the names the old enum used
/// so that existing call sites read unchanged.
///
/// **Transitional.** These belong to whichever extension contributes them
/// — `java`/`kotlin` to `spring`, the rest to a general file-types
/// extension — and Track 24's Phase 6 moves them there. They are still
/// here, and still spelled in the enum's `CamelCase`, purely so that Phase
/// 2 could open the type without also renaming every use of it; doing both
/// at once would have buried the one change that matters (matches becoming
/// non-exhaustive) under a few hundred lines of mechanical diff.
#[expect(
    non_upper_case_globals,
    reason = "deliberately keeps the old enum variants' spelling so opening the type did not churn every call site; Track 24 Phase 6 moves these to their owning extensions and renames them then"
)]
impl Language {
    pub const Java: Language = Language::new("java");
    pub const Kotlin: Language = Language::new("kotlin");
    pub const Properties: Language = Language::new("properties");
    pub const Yaml: Language = Language::new("yaml");
    pub const Xml: Language = Language::new("xml");
    pub const Dockerfile: Language = Language::new("dockerfile");
}

#[cfg(test)]
#[path = "language_test.rs"]
mod language_test;
