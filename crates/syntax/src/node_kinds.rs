//! Per-language tree-sitter node-kind vocabularies shared by the
//! sticky-scroll (`sticky.rs`) and code-folding (`folding.rs`) analyses, kept
//! in one place so the two can't drift on what counts as a "scope" or a
//! "foldable region" for a given grammar. Kotlin's kinds are deliberately
//! absent for now: `tree-sitter-kotlin-ng`'s node names must be re-derived
//! from its own `node-types.json` rather than assumed from the Java grammar
//! (see `TECHNICAL_DEBT.md` #3), so Kotlin support is a separate follow-up in
//! both features rather than a guessed-at entry here.

use fg_core::Language;

/// Declaration nodes whose header line sticky scroll pins to the top of the
/// viewport while their body scrolls underneath. Declarations only — not
/// control-flow blocks (`if`/`for`/`while`) — matching VSCode's default
/// sticky scope; control flow can become an opt-in later without changing
/// this contract.
pub(crate) fn scope_kinds(language: Language) -> &'static [&'static str] {
    match language {
        Language::Java => &[
            "class_declaration",
            "interface_declaration",
            "enum_declaration",
            "record_declaration",
            "annotation_type_declaration",
            "method_declaration",
            "constructor_declaration",
        ],
        // Kotlin/Properties/Yaml/Xml/Dockerfile have no scope vocabulary yet
        // — the feature is a no-op for them, same as every other
        // Java-first analysis in this crate.
        _ => &[],
    }
}

/// Brace/comment-delimited nodes whose body collapses when folded. These are
/// the *bodies* (and block comments), not the declarations `scope_kinds`
/// returns: folding hides the content between a `{` and its `}` (or a `/* */`
/// comment's interior), leaving the opening line — where the fold marker
/// sits — visible. A class body and the method bodies inside it are both
/// foldable, which is exactly the nested-fold behavior wanted.
pub(crate) fn foldable_kinds(language: Language) -> &'static [&'static str] {
    match language {
        Language::Java => &[
            "class_body",
            "interface_body",
            "enum_body",
            "annotation_type_body",
            "constructor_body",
            "block", // method and control-flow bodies
            "block_comment",
        ],
        _ => &[],
    }
}

/// The node kind a single `import` statement is — unlike `scope_kinds`/
/// `foldable_kinds` above, verified fresh for *both* languages (not just
/// Java) against each grammar's real parse output, since import-block
/// folding (`folding.rs`'s `collect_import_blocks`) is a fresh addition
/// rather than an extension of Java-only work already in place:
/// `import_declaration` for Java, `import` for Kotlin — both always direct
/// children of the file's root node (`program`/`source_file`), never
/// nested. `None` for a language with no import-block folding.
pub(crate) fn import_kind(language: Language) -> Option<&'static str> {
    match language {
        Language::Java => Some("import_declaration"),
        Language::Kotlin => Some("import"),
        _ => None,
    }
}
