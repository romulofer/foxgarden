//! Per-language tree-sitter node-kind vocabularies shared by the
//! sticky-scroll (`sticky.rs`) and code-folding (`folding.rs`) analyses.
//!
//! The vocabularies themselves are not here: a node kind is one grammar's
//! own name for one of its node types, so it travels with the grammar as
//! part of its contribution (`fg_extension::NodeKinds`) and is read back out
//! of the installed-grammar store. What stays here is the one place both
//! analyses ask through, so they cannot drift on what counts as a "scope" or
//! a "foldable region" for a given language.
//!
//! An empty answer is normal, not a gap to fill with a guess: a language
//! whose grammar declares no scopes simply has no sticky scroll, exactly as
//! it did when these lists were hardcoded and most languages had no entry.

use fg_core::Language;

use crate::grammars;

/// Declaration nodes whose header line sticky scroll pins to the top of the
/// viewport while their body scrolls underneath.
pub(crate) fn scope_kinds(language: Language) -> &'static [&'static str] {
    grammars::node_kinds(language).map_or(&[], |kinds| &kinds.scopes)
}

/// Brace/comment-delimited nodes whose body collapses when folded.
pub(crate) fn foldable_kinds(language: Language) -> &'static [&'static str] {
    grammars::node_kinds(language).map_or(&[], |kinds| &kinds.foldable)
}

/// The node kind a single `import` statement is, for folding a whole import
/// block as a unit. `None` disables import-block folding for the language.
pub(crate) fn import_kind(language: Language) -> Option<&'static str> {
    grammars::node_kinds(language).and_then(|kinds| kinds.import)
}
