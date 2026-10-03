//! Receiver-type resolution for dot-completion (`SPEC.md` §3): given a bare
//! identifier `x` and where the cursor sits, what's `x`'s declared type?
//! `this.`/`super.` aren't handled here — those route directly through
//! `enclosing_class`/`superclass_name` at the call site (`SPEC.md` §3's own
//! note that this needs "no new resolution logic, just routing"). This
//! module only resolves case 2, a bare identifier: nearest declaration
//! wins, in order parameter, then local variable, then field.
//!
//! Purely syntactic, same as `methods.rs`/`fields.rs`: a JDK/stdlib-typed
//! local resolves here exactly like a project-local type would (e.g.
//! `String s` resolves to `Some("String")`) — there's no classpath
//! knowledge at this layer to tell the difference. Filtering those out is
//! `SPEC.md` §4's job, where the resolved name fails to find a matching
//! project source file and the caller falls back to word-completion.

use tree_sitter::{Node, Tree};

use crate::fields::fields_in_class_body;
use crate::methods::simple_name;

fn enclosing_function(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        if matches!(node.kind(), "method_declaration" | "constructor_declaration") {
            return Some(node);
        }
        node = node.parent()?;
    }
}

fn enclosing_class_body(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        if node.kind() == "class_declaration" {
            return node.child_by_field_name("body");
        }
        node = node.parent()?;
    }
}

fn parameter_type(function: Node, source: &str, name: &str) -> Option<String> {
    let params = function.child_by_field_name("parameters")?;
    let mut cursor = params.walk();
    for param in params.children(&mut cursor) {
        if param.kind() != "formal_parameter" {
            continue;
        }
        let param_name = param.child_by_field_name("name")?;
        if &source[param_name.byte_range()] == name {
            let type_node = param.child_by_field_name("type")?;
            return Some(source[type_node.byte_range()].to_string());
        }
    }
    None
}

/// A simpler "anywhere in the enclosing method" scan rather than a precise
/// backward-from-cursor one (`SPEC.md` §3a's own documented first-cut
/// simplification) — a variable declared *after* the cursor being wrongly
/// offered is a rare, low-consequence shadowing edge case.
fn local_variable_type(node: Node, source: &str, name: &str) -> Option<String> {
    if node.kind() == "local_variable_declaration" {
        let type_node = node.child_by_field_name("type")?;
        let mut declarator_cursor = node.walk();
        for declarator in node.children_by_field_name("declarator", &mut declarator_cursor) {
            if let Some(decl_name) = declarator.child_by_field_name("name")
                && &source[decl_name.byte_range()] == name
            {
                return Some(source[type_node.byte_range()].to_string());
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = local_variable_type(child, source, name) {
            return Some(found);
        }
    }
    None
}

/// Resolves a bare identifier `name`'s declared type at `cursor_byte`:
/// the innermost enclosing method/constructor's parameters, then its local
/// variables, then the enclosing class's own fields — nearest scope wins.
/// `None` if nothing in scope declares `name`.
pub fn type_of_identifier_java(tree: &Tree, source: &str, cursor_byte: usize, name: &str) -> Option<String> {
    let start = tree
        .root_node()
        .named_descendant_for_byte_range(cursor_byte, cursor_byte)?;

    if let Some(function) = enclosing_function(start) {
        if let Some(raw_type) = parameter_type(function, source, name) {
            return Some(simple_name(&raw_type));
        }
        if let Some(raw_type) = local_variable_type(function, source, name) {
            return Some(simple_name(&raw_type));
        }
    }

    let body = enclosing_class_body(start)?;
    // `include_static: true` — a bare identifier referring to one of the
    // enclosing class's own static fields is legitimate, unlike
    // `fields_in_type`'s external-listing call sites which skip statics.
    let field = fields_in_class_body(body, source, true)
        .into_iter()
        .find(|f| f.name == name)?;
    Some(simple_name(&field.java_type))
}

// --- Kotlin ---
//
// None of the node kinds below (`parameter`, `class_parameter`,
// `variable_declaration`, `property_declaration`, `class_parameters`,
// `function_value_parameters`) carry field names in
// `tree-sitter-kotlin-ng`'s own `node-types.json` (verified fresh against
// 1.1.0's grammar, per `TECHNICAL_DEBT.md` #3's discipline) — everything
// here is `child_by_kind`, never `child_by_field_name`.

pub(crate) fn child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}

/// `parameter`/`class_parameter` both shape as `identifier` then a type
/// node (concretely `user_type` or another `type` subtype — `type` itself
/// never appears as a literal node kind) as their two relevant named
/// children, `class_parameter` additionally allowing an optional
/// `modifiers` before and a default-value `expression` after. Taking the
/// first non-`identifier`/non-`modifiers` named child as the type relies
/// on `type` always preceding a default `expression` in source order,
/// true for both node kinds.
pub(crate) fn identifier_and_type_children<'a>(node: Node<'a>) -> (Option<Node<'a>>, Option<Node<'a>>) {
    let mut identifier = None;
    let mut type_node = None;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "identifier" if identifier.is_none() => identifier = Some(child),
            "modifiers" => {}
            _ if type_node.is_none() => type_node = Some(child),
            _ => {}
        }
    }
    (identifier, type_node)
}

fn enclosing_function_kotlin(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        if node.kind() == "function_declaration" {
            return Some(node);
        }
        node = node.parent()?;
    }
}

pub(crate) fn enclosing_class_kotlin(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        if node.kind() == "class_declaration" {
            return Some(node);
        }
        node = node.parent()?;
    }
}

fn parameter_type_kotlin(function: Node, source: &str, name: &str) -> Option<String> {
    let params = child_by_kind(function, "function_value_parameters")?;
    let mut cursor = params.walk();
    for param in params.named_children(&mut cursor) {
        if param.kind() != "parameter" {
            continue;
        }
        let (ident, ty) = identifier_and_type_children(param);
        if ident.is_some_and(|n| &source[n.byte_range()] == name) {
            return ty.map(|n| source[n.byte_range()].to_string());
        }
    }
    None
}

fn primary_constructor_class_parameter_type(class_node: Node, source: &str, name: &str) -> Option<String> {
    let primary_constructor = child_by_kind(class_node, "primary_constructor")?;
    let class_parameters = child_by_kind(primary_constructor, "class_parameters")?;
    let mut cursor = class_parameters.walk();
    for param in class_parameters.named_children(&mut cursor) {
        if param.kind() != "class_parameter" {
            continue;
        }
        let (ident, ty) = identifier_and_type_children(param);
        if ident.is_some_and(|n| &source[n.byte_range()] == name) {
            return ty.map(|n| source[n.byte_range()].to_string());
        }
    }
    None
}

/// A `property_declaration`'s type: its `variable_declaration` child's own
/// explicit `type` if there is one, checked *first* and returned without
/// ever looking at the initializer — otherwise the narrow "looks like a
/// constructor call" syntactic heuristic (`SPEC.md` §3b) against the
/// `property_declaration`'s own `expression` (initializer) sibling: a
/// `call_expression` whose callee is a bare, uppercase-leading identifier.
/// This is not real type inference — anything else as an initializer
/// (another function call, a literal, an existing variable) resolves to
/// `None` rather than a wrong guess.
pub(crate) fn property_declaration_type(property: Node, source: &str) -> Option<String> {
    let var_decl = child_by_kind(property, "variable_declaration")?;
    let (_, explicit_type) = identifier_and_type_children(var_decl);
    if let Some(explicit_type) = explicit_type {
        return Some(source[explicit_type.byte_range()].to_string());
    }

    let initializer = child_by_kind(property, "call_expression")?;
    let callee = initializer.named_child(0)?;
    if callee.kind() != "identifier" {
        return None;
    }
    let callee_text = &source[callee.byte_range()];
    callee_text.chars().next().filter(|c| c.is_uppercase())?;
    Some(callee_text.to_string())
}

fn local_property_type(node: Node, source: &str, name: &str) -> Option<String> {
    if node.kind() == "property_declaration"
        && let Some(var_decl) = child_by_kind(node, "variable_declaration")
    {
        let (ident, _) = identifier_and_type_children(var_decl);
        if ident.is_some_and(|n| &source[n.byte_range()] == name) {
            return property_declaration_type(node, source);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = local_property_type(child, source, name) {
            return Some(found);
        }
    }
    None
}

fn class_body_property_type(class_body: Node, source: &str, name: &str) -> Option<String> {
    let mut cursor = class_body.walk();
    for member in class_body.named_children(&mut cursor) {
        if member.kind() != "property_declaration" {
            continue;
        }
        let Some(var_decl) = child_by_kind(member, "variable_declaration") else {
            continue;
        };
        let (ident, _) = identifier_and_type_children(var_decl);
        if ident.is_some_and(|n| &source[n.byte_range()] == name) {
            return property_declaration_type(member, source);
        }
    }
    None
}

/// Resolves a bare identifier `name`'s declared type at `cursor_byte` for
/// Kotlin, same nearest-scope-wins order as `type_of_identifier_java`:
/// the innermost enclosing function's parameters, then the enclosing
/// class's primary-constructor `val`/`var` parameters, then local
/// properties in the enclosing function, then the enclosing class's own
/// properties.
pub fn type_of_identifier_kotlin(tree: &Tree, source: &str, cursor_byte: usize, name: &str) -> Option<String> {
    let start = tree
        .root_node()
        .named_descendant_for_byte_range(cursor_byte, cursor_byte)?;

    if let Some(function) = enclosing_function_kotlin(start)
        && let Some(raw_type) = parameter_type_kotlin(function, source, name)
    {
        return Some(simple_name(&raw_type));
    }

    if let Some(class_node) = enclosing_class_kotlin(start)
        && let Some(raw_type) = primary_constructor_class_parameter_type(class_node, source, name)
    {
        return Some(simple_name(&raw_type));
    }

    if let Some(function) = enclosing_function_kotlin(start)
        && let Some(raw_type) = local_property_type(function, source, name)
    {
        return Some(simple_name(&raw_type));
    }

    let class_node = enclosing_class_kotlin(start)?;
    let class_body = child_by_kind(class_node, "class_body")?;
    let raw_type = class_body_property_type(class_body, source, name)?;
    Some(simple_name(&raw_type))
}

/// Dispatches to `type_of_identifier_java`/`_kotlin` based on `language` —
/// the one function `widget.rs` actually calls; callers never need to
/// branch on language themselves. `None` for every other language: no
/// dot-completion resolution exists for them.
pub fn type_of_identifier(
    language: fg_core::Language,
    tree: &Tree,
    source: &str,
    cursor_byte: usize,
    name: &str,
) -> Option<String> {
    match language {
        fg_core::Language::Java => type_of_identifier_java(tree, source, cursor_byte, name),
        fg_core::Language::Kotlin => type_of_identifier_kotlin(tree, source, cursor_byte, name),
        _ => None,
    }
}

#[cfg(test)]
#[path = "completion_test.rs"]
mod completion_test;

#[cfg(test)]
#[path = "kotlin_test.rs"]
mod kotlin_test;
