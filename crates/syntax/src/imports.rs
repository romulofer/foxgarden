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
    let Some(kind) = import_kind(language) else { return Vec::new() };

    let mut cursor = tree.root_node().walk();
    tree.root_node()
        .children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .map(|child| {
            let text = &source[child.start_byte()..child.end_byte()];
            ExistingImport { path: import_path_text(text).to_string(), byte_range: child.byte_range() }
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
mod tests {
    use super::*;
    use crate::IncrementalParser;

    fn parsed(language: Language, source: &str) -> Tree {
        let mut parser = IncrementalParser::new(language);
        parser.parse(source).clone()
    }

    #[test]
    fn java_existing_imports_strips_the_terminator_and_reports_document_order() {
        let source = "package com.example;\n\nimport java.util.List;\nimport java.util.Map;\n\nclass Foo {}\n";
        let tree = parsed(Language::Java, source);
        let imports = existing_imports(&tree, source, Language::Java);
        assert_eq!(imports.iter().map(|i| i.path.as_str()).collect::<Vec<_>>(), vec!["java.util.List", "java.util.Map"]);
        assert_eq!(&source[imports[0].byte_range.clone()], "import java.util.List;");
    }

    #[test]
    fn java_static_import_has_the_static_keyword_stripped_from_its_own_path() {
        let source = "import static org.junit.Assert.assertEquals;\n\nclass Foo {}\n";
        let tree = parsed(Language::Java, source);
        let imports = existing_imports(&tree, source, Language::Java);
        assert_eq!(imports[0].path, "org.junit.Assert.assertEquals");
    }

    #[test]
    fn kotlin_existing_imports_has_no_terminator_to_strip() {
        let source = "package com.example\n\nimport java.util.List\n\nclass Foo\n";
        let tree = parsed(Language::Kotlin, source);
        let imports = existing_imports(&tree, source, Language::Kotlin);
        assert_eq!(imports[0].path, "java.util.List");
    }

    #[test]
    fn a_language_with_no_import_vocabulary_reports_none() {
        let source = "key: value\n";
        let tree = parsed(Language::Yaml, source);
        assert!(existing_imports(&tree, source, Language::Yaml).is_empty());
    }

    #[test]
    fn import_insertion_finds_the_alphabetically_correct_before_slot() {
        let existing = vec![
            ExistingImport { path: "java.util.List".to_string(), byte_range: 0..10 },
            ExistingImport { path: "java.util.Set".to_string(), byte_range: 20..30 },
        ];
        assert_eq!(import_insertion(&existing, "java.util.Map"), ImportInsertion::Before(20));
    }

    #[test]
    fn import_insertion_falls_back_to_after_the_last_import_when_new_path_sorts_last() {
        let existing = vec![
            ExistingImport { path: "java.util.List".to_string(), byte_range: 0..10 },
            ExistingImport { path: "java.util.Map".to_string(), byte_range: 20..30 },
        ];
        assert_eq!(import_insertion(&existing, "org.springframework.stereotype.Component"), ImportInsertion::AfterLast(30));
    }

    #[test]
    fn import_insertion_reports_already_imported_for_an_exact_match() {
        let existing = vec![ExistingImport { path: "org.springframework.stereotype.Component".to_string(), byte_range: 0..10 }];
        assert_eq!(import_insertion(&existing, "org.springframework.stereotype.Component"), ImportInsertion::AlreadyImported);
    }

    #[test]
    fn import_insertion_with_no_existing_imports_reports_after_last_zero() {
        assert_eq!(import_insertion(&[], "org.springframework.stereotype.Component"), ImportInsertion::AfterLast(0));
    }
}
