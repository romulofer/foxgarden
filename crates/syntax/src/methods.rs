use tree_sitter::{Node, Tree};

/// One overridable method found by `methods_in_type` — enough to render a
/// picker entry and generate an `@Override` stub from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSignature {
    pub name: String,
    pub return_type: String,
    /// `(type, name)` per parameter, in declaration order.
    pub params: Vec<(String, String)>,
}

/// The class declaration whose body contains `cursor_byte`, if any — the
/// starting point for "Override Method": which class a stub gets added to.
/// Returns the class's name and the byte offset generated code should be
/// inserted at (just before its closing `}`), the same convention
/// `ClassFields::insertion_byte` uses. Walking up from the cursor's own
/// node (rather than scanning the whole tree for every class and checking
/// which one contains `cursor_byte`) naturally finds the *innermost*
/// enclosing class first for nested classes, with no extra bookkeeping.
pub fn enclosing_class(tree: &Tree, source: &str, cursor_byte: usize) -> Option<(String, usize)> {
    let mut node = tree
        .root_node()
        .named_descendant_for_byte_range(cursor_byte, cursor_byte)?;
    loop {
        if node.kind() == "class_declaration" {
            let name = node.child_by_field_name("name")?;
            let body = node.child_by_field_name("body")?;
            return Some((source[name.byte_range()].to_string(), body.end_byte().saturating_sub(1)));
        }
        node = node.parent()?;
    }
}

/// Strips generic type arguments (`Foo<Bar>` -> `Foo`) and any package
/// qualification (`java.util.Foo` -> `Foo`) down to the bare simple name —
/// what a same-name-as-the-class `.java` file is actually called on disk,
/// which is what the caller needs to look the superclass's source up by.
pub(crate) fn simple_name(raw: &str) -> String {
    let no_generics = raw.split('<').next().unwrap_or(raw);
    no_generics.rsplit('.').next().unwrap_or(no_generics).trim().to_string()
}

fn find_class_node<'a>(node: Node<'a>, source: &str, class_name: &str) -> Option<Node<'a>> {
    if node.kind() == "class_declaration"
        && node
            .child_by_field_name("name")
            .is_some_and(|n| &source[n.byte_range()] == class_name)
    {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_class_node(child, source, class_name) {
            return Some(found);
        }
    }
    None
}

/// The simple name of the type `class_name` extends, or — if it has no
/// `extends` clause — the first type it `implements`, if any. Only ever
/// one name: `Override Method`'s scope is deliberately limited to a single
/// in-project supertype (see `crates/app`'s `codegen`), so a class
/// implementing several interfaces only ever offers methods from the
/// first one — full multi-interface merging is out of scope for this
/// first cut.
pub fn superclass_name(tree: &Tree, source: &str, class_name: &str) -> Option<String> {
    let class_node = find_class_node(tree.root_node(), source, class_name)?;

    if let Some(superclass) = class_node.child_by_field_name("superclass") {
        let type_node = superclass.named_child(0)?;
        return Some(simple_name(&source[type_node.byte_range()]));
    }

    let interfaces = class_node.child_by_field_name("interfaces")?;
    let type_list = interfaces.named_child(0)?;
    let first_type = type_list.named_child(0)?;
    Some(simple_name(&source[first_type.byte_range()]))
}

/// One method's signature, if `node` is a `method_declaration` — filtered
/// down to only what can actually be overridden (`static`/`private`/
/// `final` methods excluded, detected the same way `fields.rs`'s
/// `fields_in_class_body` detects `static`/`final` fields: modifiers are
/// anonymous tokens in tree-sitter-java's grammar, not their own named
/// child, so the reliable way to find them is a text search over the span
/// between the declaration's start and its return type — there's nothing
/// else that could appear there) unless `unfiltered` is set, in which case
/// every method is included regardless of modifiers — completion's
/// `this.`/`super.` case, where code inside the same class can call any of
/// its own members (`SPEC.md` §4).
fn method_signature(node: Node, source: &str, unfiltered: bool) -> Option<MethodSignature> {
    if node.kind() != "method_declaration" {
        return None;
    }
    let type_node = node.child_by_field_name("type")?;
    let name_node = node.child_by_field_name("name")?;
    if !unfiltered {
        let modifiers_text = &source[node.start_byte()..type_node.start_byte()];
        if modifiers_text.contains("static") || modifiers_text.contains("private") || modifiers_text.contains("final") {
            return None;
        }
    }

    let params_node = node.child_by_field_name("parameters")?;
    let mut params = Vec::new();
    let mut param_cursor = params_node.walk();
    for param in params_node.children(&mut param_cursor) {
        if param.kind() != "formal_parameter" {
            continue;
        }
        let Some(ptype) = param.child_by_field_name("type") else {
            continue;
        };
        let Some(pname) = param.child_by_field_name("name") else {
            continue;
        };
        params.push((
            source[ptype.byte_range()].to_string(),
            source[pname.byte_range()].to_string(),
        ));
    }

    Some(MethodSignature {
        name: source[name_node.byte_range()].to_string(),
        return_type: source[type_node.byte_range()].to_string(),
        params,
    })
}

/// Every overridable method declared directly in `type_name`'s class or
/// interface body — the candidate list `Override Method` offers, before
/// the caller excludes whatever the current class already overrides.
/// Constructors aren't included: they're a distinct `constructor_
/// declaration` node kind in tree-sitter-java's grammar, never a
/// `method_declaration`, so `method_signature`'s own node-kind check
/// already excludes them without needing a name-based check.
pub fn methods_in_type(tree: &Tree, source: &str, type_name: &str) -> Vec<MethodSignature> {
    let mut out = Vec::new();
    collect_methods(tree.root_node(), source, type_name, false, &mut out);
    out
}

/// Every method declared directly in `type_name`'s class or interface
/// body, regardless of visibility/`static`/`final` — completion's
/// `this.`/`super.` listing (`SPEC.md` §4), as opposed to
/// `methods_in_type`'s "what's overridable" filtering.
pub fn all_methods_in_type(tree: &Tree, source: &str, type_name: &str) -> Vec<MethodSignature> {
    let mut out = Vec::new();
    collect_methods(tree.root_node(), source, type_name, true, &mut out);
    out
}

fn collect_methods(node: Node, source: &str, type_name: &str, unfiltered: bool, out: &mut Vec<MethodSignature>) {
    let is_target = matches!(node.kind(), "class_declaration" | "interface_declaration")
        && node
            .child_by_field_name("name")
            .is_some_and(|n| &source[n.byte_range()] == type_name);

    if is_target {
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if let Some(sig) = method_signature(child, source, unfiltered) {
                    out.push(sig);
                }
            }
        }
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_methods(child, source, type_name, unfiltered, out);
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

    #[test]
    fn enclosing_class_finds_the_class_the_cursor_sits_in() {
        let source = "class Foo {\n    void run() {\n    }\n}\n";
        let tree = parsed(source);
        let cursor = source.find("run").unwrap();

        let (name, insertion_byte) = enclosing_class(&tree, source, cursor).unwrap();
        assert_eq!(name, "Foo");
        assert_eq!(&source[insertion_byte..insertion_byte + 1], "}");
    }

    #[test]
    fn enclosing_class_finds_the_innermost_class_for_nested_classes() {
        let source = "class Outer {\n    class Inner {\n        void run() {\n        }\n    }\n}\n";
        let tree = parsed(source);
        let cursor = source.find("run").unwrap();

        let (name, _) = enclosing_class(&tree, source, cursor).unwrap();
        assert_eq!(name, "Inner");
    }

    #[test]
    fn enclosing_class_returns_none_outside_any_class() {
        let source = "// just a comment\n";
        let tree = parsed(source);
        assert_eq!(enclosing_class(&tree, source, 0), None);
    }

    #[test]
    fn superclass_name_finds_the_extends_clause() {
        let source = "class Foo extends Bar {\n}\n";
        let tree = parsed(source);
        assert_eq!(superclass_name(&tree, source, "Foo"), Some("Bar".to_string()));
    }

    #[test]
    fn superclass_name_strips_generic_arguments() {
        let source = "class Foo extends Bar<String> {\n}\n";
        let tree = parsed(source);
        assert_eq!(superclass_name(&tree, source, "Foo"), Some("Bar".to_string()));
    }

    #[test]
    fn superclass_name_strips_package_qualification() {
        let source = "class Foo extends java.util.AbstractList {\n}\n";
        let tree = parsed(source);
        assert_eq!(superclass_name(&tree, source, "Foo"), Some("AbstractList".to_string()));
    }

    #[test]
    fn superclass_name_falls_back_to_the_first_implemented_interface() {
        let source = "class Foo implements Runnable, java.io.Closeable {\n}\n";
        let tree = parsed(source);
        assert_eq!(superclass_name(&tree, source, "Foo"), Some("Runnable".to_string()));
    }

    #[test]
    fn superclass_name_prefers_extends_over_implements() {
        let source = "class Foo extends Bar implements Runnable {\n}\n";
        let tree = parsed(source);
        assert_eq!(superclass_name(&tree, source, "Foo"), Some("Bar".to_string()));
    }

    #[test]
    fn superclass_name_is_none_with_neither_clause() {
        let source = "class Foo {\n}\n";
        let tree = parsed(source);
        assert_eq!(superclass_name(&tree, source, "Foo"), None);
    }

    #[test]
    fn methods_in_type_finds_overridable_methods() {
        let source = "class Bar {\n    public void run() {\n    }\n    public int compute(int x) {\n        return x;\n    }\n}\n";
        let tree = parsed(source);

        let methods = methods_in_type(&tree, source, "Bar");

        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].name, "run");
        assert_eq!(methods[0].return_type, "void");
        assert!(methods[0].params.is_empty());
        assert_eq!(methods[1].name, "compute");
        assert_eq!(methods[1].return_type, "int");
        assert_eq!(methods[1].params, vec![("int".to_string(), "x".to_string())]);
    }

    #[test]
    fn methods_in_type_excludes_static_private_and_final_methods() {
        let source = "class Bar {\n    public static void a() {}\n    private void b() {}\n    public final void c() {}\n    public void d() {}\n}\n";
        let tree = parsed(source);

        let methods = methods_in_type(&tree, source, "Bar");

        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name, "d");
    }

    #[test]
    fn methods_in_type_finds_interface_methods() {
        let source = "interface Bar {\n    void run();\n}\n";
        let tree = parsed(source);

        let methods = methods_in_type(&tree, source, "Bar");

        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name, "run");
    }

    #[test]
    fn methods_in_type_returns_empty_for_an_unknown_type() {
        let source = "class Bar {\n    void run() {}\n}\n";
        let tree = parsed(source);
        assert_eq!(methods_in_type(&tree, source, "NoSuchType"), vec![]);
    }

    #[test]
    fn all_methods_in_type_includes_static_private_and_final_methods() {
        let source = "class Bar {\n    public static void a() {}\n    private void b() {}\n    public final void c() {}\n    public void d() {}\n}\n";
        let tree = parsed(source);

        let methods = all_methods_in_type(&tree, source, "Bar");

        assert_eq!(methods.len(), 4);
        let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c", "d"]);
    }
}
