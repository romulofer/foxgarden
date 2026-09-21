//! Existing `import` statement discovery and insertion-point computation —
//! the primitive Spring-annotation auto-import (`app`-side) needs: "what
//! does this file already import, and where does a new one belong,
//! alphabetically." A sibling concern to `folding.rs`'s own `collect_
//! import_blocks` (both walk the same `node_kinds::import_kind` nodes),
//! but that one only cares about *grouping* consecutive imports for a fold
//! marker — this one cares about each import's own *path*, to compare and
//! insert against.

use std::ops::Range;

use fg_core::Language;
use tree_sitter::Tree;

use crate::node_kinds::import_kind;

/// One `import` statement already in the file, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingImport {
    /// The imported path, with any Java `static` prefix and the
    /// language's own statement terminator (`;` for Java, none for
    /// Kotlin) stripped — e.g. `"org.springframework.stereotype.
    /// Component"`, not `"import org.springframework.stereotype.
    /// Component;"`. An aliased Kotlin import (`import x.Y as Z`) is kept
    /// as-is, `"x.Y as Z"` — real but rare enough for Spring annotations
    /// specifically that this doesn't special-case it further.
    pub path: String,
    /// The whole `import` statement's own byte range (including Java's
    /// own trailing `;`, not including a trailing newline).
    pub byte_range: Range<usize>,
}

/// Every `import` statement directly in `tree`'s root, in document order.
/// Empty for a language with no import vocabulary at all
/// (`node_kinds::import_kind` returning `None`) or a file with none.
/// Verified against real Java/Kotlin grammars (`node_kinds::import_kind`'s
/// own doc comment): both `import_declaration`/`import` are always direct
/// children of the root node, never nested, so this only walks one level
/// deep rather than a full recursive tree walk.
pub fn existing_imports(tree: &Tree, source: &str, language: Language) -> Vec<ExistingImport> {
    let Some(kind) = import_kind(language) else {
        return Vec::new();
    };

    let mut cursor = tree.root_node().walk();
    tree.root_node()
        .children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .map(|child| {
            let text = &source[child.start_byte()..child.end_byte()];
            ExistingImport {
                path: import_path_text(text).to_string(),
                byte_range: child.byte_range(),
            }
        })
        .collect()
}

/// Strips `import`/`import static`'s own keyword(s) and any trailing
/// `;`/whitespace from a raw `import_declaration`/`import` node's text,
/// leaving just the dotted path.
fn import_path_text(text: &str) -> &str {
    let text = text.strip_prefix("import").unwrap_or(text).trim_start();
    let text = text.strip_prefix("static").unwrap_or(text).trim_start();
    text.trim_end_matches(';').trim()
}

/// Where a new `path` belongs among `existing` (assumed already in
/// document order, not necessarily alphabetically sorted — a caller
/// inserting one new import into an otherwise-unsorted block still lands
/// it in the *locally* correct alphabetical slot, without this function
/// re-sorting the whole file's existing imports, which isn't its job).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportInsertion {
    /// `path` is already imported exactly — nothing to insert.
    AlreadyImported,
    /// Insert immediately before this existing import's own start byte —
    /// the first one (in document order) whose own path sorts after
    /// `path`.
    Before(usize),
    /// No existing import sorts after `path` — insert immediately after
    /// this last existing import's own end byte.
    AfterLast(usize),
}

/// Computes `ImportInsertion` for adding `path` given `existing` (from
/// `existing_imports`, in document order). Pure string comparison — no
/// package-relative shortening or wildcard-import awareness (an existing
/// `import org.springframework.stereotype.*;` covering `path` isn't
/// detected as "already imported," a known, disclosed simplification).
pub fn import_insertion(existing: &[ExistingImport], path: &str) -> ImportInsertion {
    if existing.iter().any(|e| e.path == path) {
        return ImportInsertion::AlreadyImported;
    }
    match existing.iter().find(|e| e.path.as_str() > path) {
        Some(next) => ImportInsertion::Before(next.byte_range.start),
        None => match existing.last() {
            Some(last) => ImportInsertion::AfterLast(last.byte_range.end),
            None => ImportInsertion::AfterLast(0), // no existing imports at all; caller decides the real anchor
        },
    }
}

#[cfg(test)]
#[path = "imports_test.rs"]
mod imports_test;
