//! Spring annotation completion + auto-import: typing `@` in a Java/Kotlin
//! file offers common Spring Framework annotations (`@Component`,
//! `@Autowired`, `@RestController`, ...); accepting one inserts the
//! annotation and, if the file doesn't already import it, a matching
//! `import` statement at the correct alphabetical position among the
//! file's existing ones (`syntax::existing_imports`/`import_insertion`).
//!
//! The annotation table below is scoped to genuine `org.springframework.*`
//! packages only — verified against real jars on this machine (`spring-
//! context`/`spring-beans`/`spring-web`/`spring-tx`/`spring-boot-
//! autoconfigure`, several versions), not assumed. `@PostConstruct`/
//! `@PreDestroy` and similar JSR-250 annotations are deliberately excluded
//! even though they're routinely used in Spring code: their real package
//! is `javax.annotation.*` (pre-Spring-Boot-3) or `jakarta.annotation.*`
//! (Spring Boot 3+), and this codebase has no reliable signal yet for
//! which namespace a given project is on — inserting the wrong one would
//! be a real, silent correctness bug, not just an incomplete candidate
//! list, so these are left out rather than guessed at.

use fg_core::Language;
use syntax::{ImportInsertion, Tree, existing_imports, import_insertion};

use super::completion::{CompletionItem, CompletionKind};

struct SpringAnnotation {
    name: &'static str,
    import_path: &'static str,
}

const SPRING_ANNOTATIONS: &[SpringAnnotation] = &[
    // org.springframework.stereotype
    SpringAnnotation { name: "Component", import_path: "org.springframework.stereotype.Component" },
    SpringAnnotation { name: "Controller", import_path: "org.springframework.stereotype.Controller" },
    SpringAnnotation { name: "Indexed", import_path: "org.springframework.stereotype.Indexed" },
    SpringAnnotation { name: "Repository", import_path: "org.springframework.stereotype.Repository" },
    SpringAnnotation { name: "Service", import_path: "org.springframework.stereotype.Service" },
    // org.springframework.beans.factory.annotation
    SpringAnnotation { name: "Autowired", import_path: "org.springframework.beans.factory.annotation.Autowired" },
    SpringAnnotation { name: "Configurable", import_path: "org.springframework.beans.factory.annotation.Configurable" },
    SpringAnnotation { name: "Lookup", import_path: "org.springframework.beans.factory.annotation.Lookup" },
    SpringAnnotation { name: "Qualifier", import_path: "org.springframework.beans.factory.annotation.Qualifier" },
    SpringAnnotation { name: "Required", import_path: "org.springframework.beans.factory.annotation.Required" },
    SpringAnnotation { name: "Value", import_path: "org.springframework.beans.factory.annotation.Value" },
    // org.springframework.context.annotation
    SpringAnnotation { name: "Bean", import_path: "org.springframework.context.annotation.Bean" },
    SpringAnnotation { name: "ComponentScan", import_path: "org.springframework.context.annotation.ComponentScan" },
    SpringAnnotation { name: "ComponentScans", import_path: "org.springframework.context.annotation.ComponentScans" },
    SpringAnnotation { name: "Conditional", import_path: "org.springframework.context.annotation.Conditional" },
    SpringAnnotation { name: "Configuration", import_path: "org.springframework.context.annotation.Configuration" },
    SpringAnnotation { name: "Description", import_path: "org.springframework.context.annotation.Description" },
    SpringAnnotation {
        name: "EnableAspectJAutoProxy",
        import_path: "org.springframework.context.annotation.EnableAspectJAutoProxy",
    },
    SpringAnnotation { name: "Import", import_path: "org.springframework.context.annotation.Import" },
    SpringAnnotation { name: "ImportResource", import_path: "org.springframework.context.annotation.ImportResource" },
    SpringAnnotation { name: "Lazy", import_path: "org.springframework.context.annotation.Lazy" },
    SpringAnnotation { name: "Primary", import_path: "org.springframework.context.annotation.Primary" },
    SpringAnnotation { name: "Profile", import_path: "org.springframework.context.annotation.Profile" },
    SpringAnnotation { name: "PropertySource", import_path: "org.springframework.context.annotation.PropertySource" },
    // org.springframework.transaction.annotation
    SpringAnnotation {
        name: "EnableTransactionManagement",
        import_path: "org.springframework.transaction.annotation.EnableTransactionManagement",
    },
    SpringAnnotation { name: "Transactional", import_path: "org.springframework.transaction.annotation.Transactional" },
    // org.springframework.boot.autoconfigure
    SpringAnnotation {
        name: "EnableAutoConfiguration",
        import_path: "org.springframework.boot.autoconfigure.EnableAutoConfiguration",
    },
    SpringAnnotation {
        name: "SpringBootApplication",
        import_path: "org.springframework.boot.autoconfigure.SpringBootApplication",
    },
    // org.springframework.web.bind.annotation
    SpringAnnotation { name: "ControllerAdvice", import_path: "org.springframework.web.bind.annotation.ControllerAdvice" },
    SpringAnnotation { name: "CookieValue", import_path: "org.springframework.web.bind.annotation.CookieValue" },
    SpringAnnotation { name: "CrossOrigin", import_path: "org.springframework.web.bind.annotation.CrossOrigin" },
    SpringAnnotation { name: "DeleteMapping", import_path: "org.springframework.web.bind.annotation.DeleteMapping" },
    SpringAnnotation { name: "ExceptionHandler", import_path: "org.springframework.web.bind.annotation.ExceptionHandler" },
    SpringAnnotation { name: "GetMapping", import_path: "org.springframework.web.bind.annotation.GetMapping" },
    SpringAnnotation { name: "InitBinder", import_path: "org.springframework.web.bind.annotation.InitBinder" },
    SpringAnnotation { name: "MatrixVariable", import_path: "org.springframework.web.bind.annotation.MatrixVariable" },
    SpringAnnotation { name: "ModelAttribute", import_path: "org.springframework.web.bind.annotation.ModelAttribute" },
    SpringAnnotation { name: "PatchMapping", import_path: "org.springframework.web.bind.annotation.PatchMapping" },
    SpringAnnotation { name: "PathVariable", import_path: "org.springframework.web.bind.annotation.PathVariable" },
    SpringAnnotation { name: "PostMapping", import_path: "org.springframework.web.bind.annotation.PostMapping" },
    SpringAnnotation { name: "PutMapping", import_path: "org.springframework.web.bind.annotation.PutMapping" },
    SpringAnnotation { name: "RequestAttribute", import_path: "org.springframework.web.bind.annotation.RequestAttribute" },
    SpringAnnotation { name: "RequestBody", import_path: "org.springframework.web.bind.annotation.RequestBody" },
    SpringAnnotation { name: "RequestHeader", import_path: "org.springframework.web.bind.annotation.RequestHeader" },
    SpringAnnotation { name: "RequestMapping", import_path: "org.springframework.web.bind.annotation.RequestMapping" },
    SpringAnnotation { name: "RequestParam", import_path: "org.springframework.web.bind.annotation.RequestParam" },
    SpringAnnotation { name: "RequestPart", import_path: "org.springframework.web.bind.annotation.RequestPart" },
    SpringAnnotation { name: "ResponseBody", import_path: "org.springframework.web.bind.annotation.ResponseBody" },
    SpringAnnotation { name: "ResponseStatus", import_path: "org.springframework.web.bind.annotation.ResponseStatus" },
    SpringAnnotation { name: "RestController", import_path: "org.springframework.web.bind.annotation.RestController" },
    SpringAnnotation {
        name: "RestControllerAdvice",
        import_path: "org.springframework.web.bind.annotation.RestControllerAdvice",
    },
    SpringAnnotation { name: "SessionAttribute", import_path: "org.springframework.web.bind.annotation.SessionAttribute" },
    SpringAnnotation { name: "SessionAttributes", import_path: "org.springframework.web.bind.annotation.SessionAttributes" },
];

/// Every known Spring annotation as a completion candidate — unfiltered,
/// same "let `CompletionState::visible`'s own prefix filter narrow it"
/// shape every other candidate source in this codebase already uses.
/// `detail` shows the annotation's own import path, so the popup doubles
/// as a reminder of which package it comes from before accepting.
pub(super) fn spring_annotation_candidates() -> Vec<CompletionItem> {
    SPRING_ANNOTATIONS
        .iter()
        .map(|a| CompletionItem {
            label: a.name.to_string(),
            kind: CompletionKind::Annotation,
            detail: Some(a.import_path.to_string()),
            has_params: false,
        })
        .collect()
}

fn import_path_for(annotation_name: &str) -> Option<&'static str> {
    SPRING_ANNOTATIONS.iter().find(|a| a.name == annotation_name).map(|a| a.import_path)
}

/// If `annotation_name` is a known Spring annotation not already imported
/// in `text`, returns the exact text to splice in (a full `import
/// ...;`/`import ...` line, newline-terminated, in the language's own
/// style) and the byte offset to splice it at. `None` either when the
/// name isn't a recognized Spring annotation at all, or when it's already
/// imported (nothing to do).
pub(super) fn spring_import_insertion(tree: &Tree, text: &str, language: Language, annotation_name: &str) -> Option<(usize, String)> {
    let import_path = import_path_for(annotation_name)?;
    let existing = existing_imports(tree, text, language);

    let statement = match language {
        Language::Java => format!("import {import_path};"),
        _ => format!("import {import_path}"),
    };

    match import_insertion(&existing, import_path) {
        ImportInsertion::AlreadyImported => None,
        ImportInsertion::Before(byte) => Some((byte, format!("{statement}\n"))),
        ImportInsertion::AfterLast(byte) if !existing.is_empty() => Some((byte, format!("\n{statement}"))),
        ImportInsertion::AfterLast(_) => Some(first_import_insertion_point(tree, &statement)),
    }
}

/// Combines `insert_completion`'s own single-span replacement
/// (`completed_text`/`cursor_after_label`, already computed by the
/// caller from the *pre*-completion `old_text`/`tree`) with this
/// annotation's own import insertion, if it needs one — one final text
/// and cursor position. Splicing `import_byte` (computed against
/// `old_text`'s own coordinates) directly into `completed_text` is only
/// valid because every byte of `completed_text` before `anchor_byte` is
/// still identical to `old_text` (the completion's own edit only ever
/// touches `[anchor_byte..cursor_byte)`) — guarded explicitly rather than
/// assumed: an import that would sort *after* `anchor_byte` (not a normal
/// file shape — imports always precede where an annotation gets typed)
/// skips the import rather than risk splicing into already-edited text.
pub(super) fn apply_with_import(
    old_text: &str,
    tree: &Tree,
    language: Language,
    annotation_name: &str,
    anchor_byte: usize,
    completed_text: String,
    cursor_after_label: usize,
) -> (String, usize) {
    let Some((import_byte, import_text)) = spring_import_insertion(tree, old_text, language, annotation_name) else {
        return (completed_text, cursor_after_label);
    };
    if import_byte > anchor_byte {
        return (completed_text, cursor_after_label);
    }

    let mut final_text = completed_text;
    final_text.insert_str(import_byte, &import_text);
    let cursor_shift = import_text.chars().count();
    (final_text, cursor_after_label + cursor_shift)
}

/// Where to put the file's very first import, when it has none yet:
/// right after the `package` declaration (Java `package_declaration`,
/// Kotlin `package_header` — both, like an import itself, always a direct
/// child of the root node), or at the very start of the file if there's
/// no package declaration either (an unusual but valid default-package
/// Java file).
fn first_import_insertion_point(tree: &Tree, statement: &str) -> (usize, String) {
    let mut cursor = tree.root_node().walk();
    let package_end = tree
        .root_node()
        .children(&mut cursor)
        .find(|child| matches!(child.kind(), "package_declaration" | "package_header"))
        .map(|node| node.end_byte());

    match package_end {
        Some(byte) => (byte, format!("\n\n{statement}")),
        None => (0, format!("{statement}\n\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::IncrementalParser;

    fn parsed(language: Language, source: &str) -> Tree {
        let mut parser = IncrementalParser::new(language);
        parser.parse(source).clone()
    }

    #[test]
    fn spring_annotation_candidates_are_labeled_and_carry_their_own_import_path_as_detail() {
        let candidates = spring_annotation_candidates();
        let component = candidates.iter().find(|c| c.label == "Component").unwrap();
        assert_eq!(component.kind, CompletionKind::Annotation);
        assert_eq!(component.detail.as_deref(), Some("org.springframework.stereotype.Component"));
    }

    #[test]
    fn an_unrecognized_name_yields_no_insertion() {
        let source = "class Foo {}\n";
        let tree = parsed(Language::Java, source);
        assert!(spring_import_insertion(&tree, source, Language::Java, "NotASpringAnnotation").is_none());
    }

    #[test]
    fn already_imported_yields_no_insertion() {
        let source = "import org.springframework.stereotype.Component;\n\nclass Foo {}\n";
        let tree = parsed(Language::Java, source);
        assert!(spring_import_insertion(&tree, source, Language::Java, "Component").is_none());
    }

    #[test]
    fn java_with_no_imports_at_all_inserts_after_the_package_declaration() {
        let source = "package com.example;\n\nclass Foo {}\n";
        let tree = parsed(Language::Java, source);
        let (byte, insertion) = spring_import_insertion(&tree, source, Language::Java, "Component").unwrap();
        assert_eq!(byte, "package com.example;".len());
        assert_eq!(insertion, "\n\nimport org.springframework.stereotype.Component;");

        let mut result = source.to_string();
        result.insert_str(byte, &insertion);
        assert_eq!(result, "package com.example;\n\nimport org.springframework.stereotype.Component;\n\nclass Foo {}\n");
    }

    #[test]
    fn java_with_no_package_and_no_imports_inserts_at_the_very_start() {
        let source = "class Foo {}\n";
        let tree = parsed(Language::Java, source);
        let (byte, insertion) = spring_import_insertion(&tree, source, Language::Java, "Component").unwrap();
        assert_eq!(byte, 0);
        assert_eq!(insertion, "import org.springframework.stereotype.Component;\n\n");
    }

    #[test]
    fn java_inserts_at_the_correct_alphabetical_slot_among_existing_imports() {
        // "org.springframework.stereotype.Component" sorts after
        // "java.util.List" (`j` < `o`) but before "org.springframework.
        // web.bind.annotation.RestController" (`stereotype` < `web`) —
        // genuinely bracketing the insertion point, unlike two plain
        // `java.util.*` imports (which both sort *before* any
        // `org.springframework.*` one).
        let source = "import java.util.List;\nimport org.springframework.web.bind.annotation.RestController;\n\nclass Foo {}\n";
        let tree = parsed(Language::Java, source);
        let (byte, insertion) = spring_import_insertion(&tree, source, Language::Java, "Component").unwrap();

        let mut result = source.to_string();
        result.insert_str(byte, &insertion);
        assert_eq!(
            result,
            "import java.util.List;\nimport org.springframework.stereotype.Component;\nimport org.springframework.web.bind.annotation.RestController;\n\nclass Foo {}\n"
        );
    }

    #[test]
    fn java_appends_after_the_last_import_when_the_new_one_sorts_last() {
        let source = "import java.util.List;\nimport java.util.Map;\n\nclass Foo {}\n";
        let tree = parsed(Language::Java, source);
        let (byte, insertion) = spring_import_insertion(&tree, source, Language::Java, "Value").unwrap();

        let mut result = source.to_string();
        result.insert_str(byte, &insertion);
        assert_eq!(
            result,
            "import java.util.List;\nimport java.util.Map;\nimport org.springframework.beans.factory.annotation.Value;\n\nclass Foo {}\n"
        );
    }

    #[test]
    fn kotlin_import_has_no_semicolon() {
        let source = "package com.example\n\nclass Foo\n";
        let tree = parsed(Language::Kotlin, source);
        let (_byte, insertion) = spring_import_insertion(&tree, source, Language::Kotlin, "Component").unwrap();
        assert_eq!(insertion, "\n\nimport org.springframework.stereotype.Component");
    }
}
