//! FoxGarden's UI strings, in Brazilian Portuguese (the primary language)
//! and US English.
//!
//! The catalogue is a plain `struct` of `&'static str` fields, with one
//! `const` per language ([`PT_BR`], [`EN_US`]). That shape is deliberate:
//!
//! * **A missing translation is a compile error.** Adding a field to
//!   [`Strings`] fails both `const` initialisers until every language has a
//!   value for it, so a locale can't silently fall back to English at
//!   runtime the way a key-lookup catalogue would.
//! * **Reading a string costs a relaxed atomic load and a field access.**
//!   The UI is immediate-mode — every label is re-read on every frame, a few
//!   hundred times per frame — so a per-string hash lookup would be pure
//!   overhead against the performance bar this editor is held to.
//!
//! Strings that interpolate runtime values (paths, error text) can't be
//! `&'static str`, so they live in [`msg`] as functions returning `String`.
//!
//! ```
//! # use fg_i18n::{Lang, set_lang, t};
//! set_lang(Lang::PtBr);
//! assert_eq!(t().menu.file, "Arquivo");
//! set_lang(Lang::EnUs);
//! assert_eq!(t().menu.file, "File");
//! ```

use std::sync::atomic::{AtomicU8, Ordering};

pub mod catalog;
pub mod msg;

pub use catalog::{EN_US, PT_BR, Strings};

/// A language the UI can be displayed in.
///
/// pt-BR is listed first and is the default because it's the project's
/// primary language; en-US is the localisation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    /// Brazilian Portuguese — the primary language.
    #[default]
    PtBr,
    /// US English.
    EnUs,
}

impl Lang {
    /// Every language, in the order the Settings menu lists them.
    pub const ALL: [Lang; 2] = [Lang::PtBr, Lang::EnUs];

    /// The language's own name for itself — "Português (Brasil)", not
    /// "Portuguese (Brazil)". Autonyms are what a language picker wants:
    /// someone who can't read the currently-active language still has to be
    /// able to find their own in the list.
    pub const fn autonym(self) -> &'static str {
        match self {
            Lang::PtBr => "Português (Brasil)",
            Lang::EnUs => "English (US)",
        }
    }

    /// The BCP 47 tag, used as the persisted settings value.
    pub const fn tag(self) -> &'static str {
        match self {
            Lang::PtBr => "pt-BR",
            Lang::EnUs => "en-US",
        }
    }

    /// Parses a persisted [`Self::tag`] back. Returns `None` for anything
    /// unrecognised, so a settings file written by a newer build (or hand
    /// edited) falls back to detection rather than to an arbitrary language.
    pub fn from_tag(tag: &str) -> Option<Self> {
        Lang::ALL.into_iter().find(|lang| lang.tag() == tag)
    }

    /// The language a locale identifier like `pt_BR.UTF-8`, `pt`, or
    /// `en_US.UTF-8` selects.
    ///
    /// Only the primary subtag matters: every Portuguese variant maps to
    /// pt-BR (a pt-PT speaker is far better served by Brazilian Portuguese
    /// than by English), and everything else — including an unset or `C`
    /// locale — maps to en-US.
    fn from_locale(locale: &str) -> Self {
        let primary = locale
            .split(['_', '-', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if primary == "pt" { Lang::PtBr } else { Lang::EnUs }
    }
}

/// The active language, as a [`Lang`] discriminant.
///
/// `AtomicU8` rather than a `OnceLock`/`RwLock`: this is written rarely (at
/// startup, and whenever Settings > Language is used) and read on every
/// frame from the UI thread, so the read wants to be as close to free as a
/// mutable global gets. `Relaxed` is enough — nothing else is published
/// alongside it, and a language switch landing one frame later than the
/// click that caused it is invisible.
static ACTIVE: AtomicU8 = AtomicU8::new(Lang::PtBr as u8);

/// The active language.
pub fn lang() -> Lang {
    match ACTIVE.load(Ordering::Relaxed) {
        x if x == Lang::EnUs as u8 => Lang::EnUs,
        _ => Lang::PtBr,
    }
}

/// Switches the UI to `lang`. Takes effect on the next frame, since the
/// catalogue is read fresh every frame.
pub fn set_lang(lang: Lang) {
    ACTIVE.store(lang as u8, Ordering::Relaxed);
}

/// The active language's strings.
///
/// The entry point for essentially every label in the app: `t().menu.file`,
/// `t().common.cancel`.
pub fn t() -> &'static Strings {
    match lang() {
        Lang::PtBr => &PT_BR,
        Lang::EnUs => &EN_US,
    }
}

/// The language the host system asks for, from the standard POSIX
/// environment variables in the order the C library itself consults them.
///
/// Used only when the user hasn't explicitly picked a language in Settings —
/// an explicit choice is persisted and always wins.
pub fn detect_from_env() -> Lang {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|var| std::env::var(var).ok())
        .filter(|value| !value.is_empty())
        .map_or(Lang::EnUs, |value| Lang::from_locale(&value))
}

/// Runs `f` with `lang` active, restoring whatever was active before.
///
/// The active language is process-global, and `cargo test` runs a crate's
/// tests on parallel threads, so two tests each calling [`set_lang`] would
/// otherwise clobber each other's language between the store and the read.
/// Serializing through one mutex is enough — nothing here is slow.
#[cfg(test)]
pub(crate) fn with_lang<T>(target: Lang, f: impl FnOnce() -> T) -> T {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A test that panicked mid-closure poisoned the mutex without leaving
    // any state a later test could misread (the language is overwritten on
    // entry either way), so the poison is recovered from rather than
    // cascading into a failure in every subsequent test.
    let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let previous = lang();
    set_lang(target);
    let result = f();
    set_lang(previous);
    result
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_test;
