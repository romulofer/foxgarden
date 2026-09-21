//! Finding a file's runnable entry points — the `public static void
//! main(String[])` of a Java class, or a Kotlin file's top-level `fun
//! main`. Feeds the editor's own run-gutter marker (the ▶ IntelliJ puts
//! beside `main`), which needs two things per entry: the line to put the
//! marker on, and the fully-qualified class name to hand `java`.
//!
//! Deliberately syntactic, not semantic: this reads the file's own
//! tree-sitter tree, exactly like `spring_endpoints`/`methods` already do,
//! rather than asking jdt.ls. A run marker has to be there the instant a
//! file opens — before any language server has finished initializing, and
//! in a file that isn't part of a project at all — and "does this
//! declaration match the `main` signature" is a question the syntax tree
//! can answer completely on its own.

use fg_core::Language;
use tree_sitter::{Node, Tree};

/// One runnable `main` found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainEntry {
    /// 0-based line the `main` declaration starts on — where the gutter
    /// marker goes.
    pub line: usize,
    /// What `java` is actually launched with: the package-qualified class
    /// name, with `$` between an outer class and a nested one (`java`'s own
    /// binary-name spelling, not the source-level `.`), or the synthetic
    /// `…Kt` class a Kotlin top-level `main` compiles into.
    pub main_class: String,
    /// The short name to show the user (the class's own simple name) — a
    /// tooltip saying `Run Main` reads better than one repeating the whole
    /// `com.example.deep.package.Main`.
    pub label: String,
}

/// Every runnable entry point in `source`, in source order. `file_stem` is
/// the file's own name without its extension, needed only for Kotlin (whose
/// top-level `main` compiles into a class named after the *file*, not after
/// anything written in it); Java ignores it.
pub fn main_entries(tree: &Tree, source: &str, language: Language, file_stem: &str) -> Vec<MainEntry> {
    match language {
        Language::Java => java_main_entries(tree, source),
        Language::Kotlin => kotlin_main_entries(tree, source, file_stem),
        // Nothing else the editor can open is a JVM entry point — a
        // `pom.xml`, an `application.yml` and a `Dockerfile` all have real
        // run *actions* of their own elsewhere (Run > Build, Docker), but
        // none of them has a `main` to put a gutter marker beside. The
        // same holds, for the same reason, for any language contributed by
        // an extension this function has never heard of: finding *its*
        // entry points is that extension's job, not this one's.
        _ => Vec::new(),
    }
}

/// The `package …;`/`package …` declared at the top of the file, if any.
/// Both grammars name the node differently (`package_declaration` in Java,
/// `package_header` in Kotlin) but agree on the shape: the last named child
/// is the dotted name.
fn package_name(tree: &Tree, source: &str, kind: &str) -> Option<String> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    let declaration = root.named_children(&mut cursor).find(|child| child.kind() == kind)?;
    let last = u32::try_from(declaration.named_child_count()).ok()?.checked_sub(1)?;
    let name = declaration.named_child(last)?;
    let text = source[name.byte_range()].trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn qualified(package: Option<&String>, class: &str) -> String {
    match package {
        Some(package) => format!("{package}.{class}"),
        None => class.to_string(),
    }
}

/// Whether `node` — a Java `method_declaration` — is a JVM entry point:
/// `static`, named `main`, returning `void`, taking exactly one `String[]`
/// (or `String...`) parameter. `public` is deliberately *not* required: a
/// non-public `main` is a real mistake the JVM rejects at launch, and
/// showing the marker anyway puts that failure in the run panel where the
/// user can read it, rather than silently offering no way to run a file
/// that visibly declares a `main`.
fn is_java_main(node: Node, source: &str) -> bool {
    if node.child_by_field_name("name").map(|n| &source[n.byte_range()]) != Some("main") {
        return false;
    }
    if node.child_by_field_name("type").map(|n| &source[n.byte_range()]) != Some("void") {
        return false;
    }
    let mut cursor = node.walk();
    let is_static = node.named_children(&mut cursor).any(|child| {
        child.kind() == "modifiers" && source[child.byte_range()].split_whitespace().any(|m| m == "static")
    });
    if !is_static {
        return false;
    }
    let Some(parameters) = node.child_by_field_name("parameters") else {
        return false;
    };
    let mut cursor = parameters.walk();
    let params: Vec<Node> = parameters.named_children(&mut cursor).collect();
    let [parameter] = params[..] else {
        return false;
    };
    // `String[] args` parses as `formal_parameter` with an `array_type`
    // type; `String... args` as a `spread_parameter`, whose own type child
    // is the bare element type. Both are valid entry points.
    match parameter.kind() {
        "formal_parameter" => parameter
            .child_by_field_name("type")
            .is_some_and(|t| t.kind() == "array_type" && source[t.byte_range()].replace(' ', "").ends_with("[]")),
        "spread_parameter" => true,
        _ => false,
    }
}

/// The `$`-joined binary name of the class `node` sits in — `Outer$Inner`
/// for a `main` declared in a nested class, which is exactly what `java`
/// expects on its command line. `None` when the declaration has no
/// enclosing named type at all (a `main` in an anonymous class or a stray
/// fragment), since there's then nothing runnable to name.
fn enclosing_type_chain(node: Node, source: &str, kinds: &[&str]) -> Option<String> {
    let mut names = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        if kinds.contains(&parent.kind())
            && let Some(name) = parent.child_by_field_name("name")
        {
            names.push(&source[name.byte_range()]);
        }
        current = parent.parent();
    }
    if names.is_empty() {
        return None;
    }
    names.reverse();
    Some(names.join("$"))
}

fn java_main_entries(tree: &Tree, source: &str) -> Vec<MainEntry> {
    let package = package_name(tree, source, "package_declaration");
    let mut out = Vec::new();
    walk(tree.root_node(), &mut |node| {
        if node.kind() != "method_declaration" || !is_java_main(node, source) {
            return;
        }
        // Java allows `main` in a class, a record, an enum or an interface
        // (`static` interface methods since Java 8) — all four are real
        // `java`-launchable targets, so all four are recognized here.
        let Some(class) = enclosing_type_chain(
            node,
            source,
            &[
                "class_declaration",
                "record_declaration",
                "enum_declaration",
                "interface_declaration",
            ],
        ) else {
            return;
        };
        let label = class.rsplit('$').next().unwrap_or(&class).to_string();
        out.push(MainEntry {
            line: node.start_position().row,
            main_class: qualified(package.as_ref(), &class),
            label,
        });
    });
    out
}

/// Kotlin's `fun main(...)`, top-level only. A `main` inside a class is
/// excluded on purpose: it needs `@JvmStatic` in a companion object to be
/// a JVM entry point at all, and getting *that* wrong would produce a
/// marker that can't run, which is worse than no marker.
fn kotlin_main_entries(tree: &Tree, source: &str, file_stem: &str) -> Vec<MainEntry> {
    let package = package_name(tree, source, "package_header");
    let class = kotlin_file_class_name(file_stem);
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|node| node.kind() == "function_declaration")
        .filter(|node| node.child_by_field_name("name").map(|n| &source[n.byte_range()]) == Some("main"))
        .map(|node| MainEntry {
            line: node.start_position().row,
            main_class: qualified(package.as_ref(), &class),
            label: class.clone(),
        })
        .collect()
}

/// The JVM class name the Kotlin compiler generates for a file's top-level
/// declarations: the file name, capitalized, with `Kt` appended (`main.kt`
/// -> `MainKt`). A file whose name already ends in `Kt` still gets the
/// suffix — that's what `kotlinc` does too (`FooKt.kt` -> `FooKtKt`), a
/// quirk worth matching rather than second-guessing. `@JvmName` can
/// override all of this and isn't handled; a file that uses it gets a
/// marker that names the default class, which the run panel then reports
/// as not found, same as any other stale main class.
fn kotlin_file_class_name(file_stem: &str) -> String {
    let sanitized: String = file_stem
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    let mut chars = sanitized.chars();
    let capitalized = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    format!("{capitalized}Kt")
}

/// Pre-order walk over every named node, calling `visit` on each. Kept
/// local rather than a shared helper: `methods`/`fields` each walk with
/// their own early-exit rules, and this one has none to share.
fn walk(node: Node, visit: &mut impl FnMut(Node)) {
    visit(node);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(child, visit);
    }
}

#[cfg(test)]
#[path = "main_entry_test.rs"]
mod main_entry_test;
