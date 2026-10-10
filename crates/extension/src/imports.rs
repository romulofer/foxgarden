//! `PLAN.md` Track 24 Phase 6: names that live in another namespace and the
//! `import` statement that brings them into scope.
//!
//! The editor offers a language's annotations after an `@`, and accepting
//! one also has to make the name resolve. What an import statement looks
//! like (`import a.B;` or `import a.B`), how a file's existing imports are
//! ordered and where the first one goes are a language's own business, so
//! the extension answers with the finished text edit and the editor only
//! splices it.

/// A name offered for completion that needs an import to resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCandidate {
    /// What is typed and inserted, e.g. `Component`.
    pub name: String,
    /// The fully qualified name the import brings in, e.g.
    /// `org.springframework.stereotype.Component`. Shown beside the name so
    /// the user sees which package it comes from before accepting.
    pub qualified_name: String,
}

/// Text to splice into a file so a name resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEdit {
    /// Byte offset in the file as it is now.
    pub byte: usize,
    /// The text to insert there, separators included.
    pub text: String,
}
