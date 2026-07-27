//! Spring MVC endpoint extraction for the endpoint map popup (`SPEC.md`
//! §1-§3) — Java's producer lives here; `kotlin_endpoints_in_file` and the
//! shared `endpoints_in_file` dispatcher join it once Kotlin extraction
//! lands (`PLAN.md` Phase 1).

use tree_sitter::{Node, Tree};

use crate::completion::child_by_kind;

/// One discovered Spring MVC endpoint — enough to render a popup row and
/// jump to its handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    /// `"GET"`/`"POST"`/`"PUT"`/`"DELETE"`/`"PATCH"`, or `"ANY"` for a bare
    /// `@RequestMapping` with no `method =` element (Spring's own default:
    /// matches every HTTP method).
    pub http_method: String,
    /// Class-level base path + method-level path, joined (see
    /// `join_paths`) — e.g. `"/api/users/{id}"`. Never empty; a mapping
    /// with no path anywhere resolves to `"/"`.
    pub path: String,
    pub controller_name: String,
    pub handler_name: String,
    /// Byte offset of the handler method's own name — where a jump should
    /// land the cursor, not the annotation or the enclosing method's start.
    pub handler_byte: usize,
}

/// Recognized mapping annotation names and the HTTP method each fixes, or
/// `None` for `@RequestMapping`, whose method comes from its own `method =`
/// element (defaulting to `"ANY"`) rather than the annotation name.
fn fixed_http_method(annotation_name: &str) -> Option<Option<&'static str>> {
    match annotation_name {
        "GetMapping" => Some(Some("GET")),
        "PostMapping" => Some(Some("POST")),
        "PutMapping" => Some(Some("PUT")),
        "DeleteMapping" => Some(Some("DELETE")),
        "PatchMapping" => Some(Some("PATCH")),
        "RequestMapping" => Some(None),
        _ => None,
    }
}

/// The simple name of an `annotation`/`marker_annotation` node — ignores the
/// rarer fully-qualified `@org.springframework....GetMapping` shape
/// (`scoped_identifier`), same "match the simple name, not the qualified
/// path" limitation `SPEC.md` §0/§2 accepts.
fn annotation_name(node: Node, source: &str) -> Option<String> {
    let name = node.child_by_field_name("name")?;
    if name.kind() != "identifier" {
        return None;
    }
    Some(source[name.byte_range()].to_string())
}

/// A `string_literal`'s actual text — its `string_fragment` child, not its
/// own span (which includes the quotes). An empty literal (`""`) has no
/// `string_fragment` child at all, hence the empty-string fallback.
fn string_literal_text(node: Node, source: &str) -> String {
    node.named_child(0)
        .map(|fragment| source[fragment.byte_range()].to_string())
        .unwrap_or_default()
}

/// The path an `annotation`/`marker_annotation` node contributes: a bare
/// positional `string_literal`, a `value =`/`path =` `element_value_pair`
/// (Spring accepts either key as a synonym), or `""` for a marker
/// annotation (no `()` at all) or an annotation with neither shape present.
fn annotation_path(node: Node, source: &str) -> String {
    let Some(args) = node.child_by_field_name("arguments") else {
        return String::new();
    };
    let mut cursor = args.walk();
    for child in args.named_children(&mut cursor) {
        match child.kind() {
            "string_literal" => return string_literal_text(child, source),
            "element_value_pair" => {
                let Some(key) = child.child_by_field_name("key") else {
                    continue;
                };
                let key_text = &source[key.byte_range()];
                if key_text != "value" && key_text != "path" {
                    continue;
                }
                if let Some(value) = child.child_by_field_name("value")
                    && value.kind() == "string_literal"
                {
                    return string_literal_text(value, source);
                }
            }
            _ => {}
        }
    }
    String::new()
}

/// `@RequestMapping(method = RequestMethod.DELETE)`'s HTTP method — a
/// `field_access` (`RequestMethod` `.` `DELETE`); take the identifier after
/// the dot. `None` (defaulting to `"ANY"`) if there's no `method =` element.
fn annotation_method_override(node: Node, source: &str) -> Option<String> {
    let args = node.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    for child in args.named_children(&mut cursor) {
        if child.kind() != "element_value_pair" {
            continue;
        }
        let key = child.child_by_field_name("key")?;
        if &source[key.byte_range()] != "method" {
            continue;
        }
        let value = child.child_by_field_name("value")?;
        if value.kind() != "field_access" {
            continue;
        }
        let field = value.child_by_field_name("field")?;
        return Some(source[field.byte_range()].to_string());
    }
    None
}

/// Joins a class-level base path with a method-level path: strips a
/// trailing `/` off the base, ensures a leading `/` on the method path (if
/// it's non-empty and missing one), concatenates. Both sides empty resolves
/// to `"/"` rather than `""`.
fn join_paths(base: &str, method_path: &str) -> String {
    let base = base.trim_end_matches('/');
    let method = if method_path.is_empty() {
        String::new()
    } else if let Some(stripped) = method_path.strip_prefix('/') {
        format!("/{stripped}")
    } else {
        format!("/{method_path}")
    };
    let joined = format!("{base}{method}");
    if joined.is_empty() { "/".to_string() } else { joined }
}

/// The class's own `@RequestMapping` base path, if its `modifiers` child
/// carries one — `@RestController`/`@Controller` alone contribute no path.
fn class_base_path(class_node: Node, source: &str) -> String {
    let Some(modifiers) = child_by_kind(class_node, "modifiers") else {
        return String::new();
    };
    let mut cursor = modifiers.walk();
    for annotation in modifiers.named_children(&mut cursor) {
        if !matches!(annotation.kind(), "annotation" | "marker_annotation") {
            continue;
        }
        if annotation_name(annotation, source).as_deref() == Some("RequestMapping") {
            return annotation_path(annotation, source);
        }
    }
    String::new()
}

/// `(http_method, path)` from the first recognized mapping annotation on
/// `method_node`'s `modifiers` child, if any — a method only becomes an
/// endpoint if it carries one of the five recognized annotations,
/// independent of whether the enclosing class carries
/// `@Controller`/`@RestController` at all. Other annotations on the same
/// node (`@Override`, `@Transactional`, …) are ignored, not an error.
fn method_mapping(method_node: Node, source: &str) -> Option<(String, String)> {
    let modifiers = child_by_kind(method_node, "modifiers")?;
    let mut cursor = modifiers.walk();
    for annotation in modifiers.named_children(&mut cursor) {
        if !matches!(annotation.kind(), "annotation" | "marker_annotation") {
            continue;
        }
        let Some(name) = annotation_name(annotation, source) else {
            continue;
        };
        let Some(fixed) = fixed_http_method(&name) else {
            continue;
        };
        let http_method = match fixed {
            Some(method) => method.to_string(),
            None => annotation_method_override(annotation, source).unwrap_or_else(|| "ANY".to_string()),
        };
        let path = annotation_path(annotation, source);
        return Some((http_method, path));
    }
    None
}

/// Every Spring MVC endpoint declared in `tree` — walks every
/// `class_declaration` (top-level and nested, same "don't miss nested
/// classes" precedent `fields.rs`/`methods.rs` already set), reading each
/// one's own `@RequestMapping` (if any) as a base path, then every
/// `method_declaration` in its body carrying a recognized mapping
/// annotation.
pub fn java_endpoints_in_file(tree: &Tree, source: &str) -> Vec<EndpointInfo> {
    let mut out = Vec::new();
    collect_java_endpoints(tree.root_node(), source, &mut out);
    out
}

fn collect_java_endpoints(node: Node, source: &str, out: &mut Vec<EndpointInfo>) {
    if node.kind() == "class_declaration"
        && let Some(name_node) = node.child_by_field_name("name")
        && let Some(body) = node.child_by_field_name("body")
    {
        let controller_name = source[name_node.byte_range()].to_string();
        let base_path = class_base_path(node, source);

        let mut cursor = body.walk();
        for member in body.children(&mut cursor) {
            if member.kind() != "method_declaration" {
                continue;
            }
            let Some((http_method, method_path)) = method_mapping(member, source) else {
                continue;
            };
            let Some(handler_name_node) = member.child_by_field_name("name") else {
                continue;
            };
            out.push(EndpointInfo {
                http_method,
                path: join_paths(&base_path, &method_path),
                controller_name: controller_name.clone(),
                handler_name: source[handler_name_node.byte_range()].to_string(),
                handler_byte: handler_name_node.start_byte(),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_java_endpoints(child, source, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IncrementalParser;
    use fg_core::Language;

    fn parsed(source: &str) -> Tree {
        let mut parser = IncrementalParser::new(Language::Java);
        parser.parse(source).clone()
    }

    fn endpoints(source: &str) -> Vec<EndpointInfo> {
        let tree = parsed(source);
        java_endpoints_in_file(&tree, source)
    }

    #[test]
    fn positional_string_path() {
        let source = "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n";
        let eps = endpoints(source);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].http_method, "GET");
        assert_eq!(eps[0].path, "/x");
        assert_eq!(eps[0].controller_name, "Foo");
        assert_eq!(eps[0].handler_name, "run");
    }

    #[test]
    fn value_equals_path() {
        let source = "class Foo {\n    @GetMapping(value = \"/x\")\n    public void run() {}\n}\n";
        let eps = endpoints(source);
        assert_eq!(eps[0].path, "/x");
    }

    #[test]
    fn path_equals_path() {
        let source = "class Foo {\n    @GetMapping(path = \"/x\")\n    public void run() {}\n}\n";
        let eps = endpoints(source);
        assert_eq!(eps[0].path, "/x");
    }

    #[test]
    fn method_equals_combined_with_class_level_base_path() {
        let source = "@RequestMapping(\"/api\")\nclass Foo {\n    @RequestMapping(method = RequestMethod.DELETE)\n    public void run() {}\n}\n";
        let eps = endpoints(source);
        assert_eq!(eps[0].http_method, "DELETE");
        assert_eq!(eps[0].path, "/api");
    }

    #[test]
    fn bare_marker_annotation_with_no_args() {
        let source = "class Foo {\n    @PostMapping\n    public void run() {}\n}\n";
        let eps = endpoints(source);
        assert_eq!(eps[0].http_method, "POST");
        assert_eq!(eps[0].path, "/");
    }

    #[test]
    fn no_class_level_base_path() {
        let source = "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n";
        let eps = endpoints(source);
        assert_eq!(eps[0].path, "/x");
    }

    #[test]
    fn nested_class() {
        let source = "class Outer {\n    class Inner {\n        @GetMapping(\"/inner\")\n        public void run() {}\n    }\n}\n";
        let eps = endpoints(source);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].controller_name, "Inner");
        assert_eq!(eps[0].path, "/inner");
    }

    #[test]
    fn multiple_unrelated_annotations_only_recognized_one_counts() {
        let source = "class Foo {\n    @Override\n    @GetMapping(\"/x\")\n    @Transactional\n    public void run() {}\n}\n";
        let eps = endpoints(source);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].http_method, "GET");
        assert_eq!(eps[0].path, "/x");
    }

    #[test]
    fn no_recognized_annotation_at_all() {
        let source = "class Foo {\n    @Override\n    public void run() {}\n}\n";
        assert_eq!(endpoints(source), vec![]);
    }

    #[test]
    fn handler_byte_points_at_the_method_name() {
        let source = "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n";
        let eps = endpoints(source);
        let byte = eps[0].handler_byte;
        assert_eq!(&source[byte..byte + 3], "run");
    }

    #[test]
    fn class_with_request_mapping_but_no_recognized_method_annotations_contributes_nothing() {
        let source = "@RequestMapping(\"/api\")\nclass Foo {\n    public void run() {}\n}\n";
        assert_eq!(endpoints(source), vec![]);
    }

    #[test]
    fn multiple_endpoints_in_source_order() {
        let source = "@RequestMapping(\"/api\")\nclass Foo {\n    @GetMapping(\"/a\")\n    public void a() {}\n\n    @PostMapping(\"/b\")\n    public void b() {}\n}\n";
        let eps = endpoints(source);
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].handler_name, "a");
        assert_eq!(eps[0].path, "/api/a");
        assert_eq!(eps[1].handler_name, "b");
        assert_eq!(eps[1].path, "/api/b");
    }
}
