//! Kotlin member extraction for dot-completion's cross-project lookup —
//! the Kotlin counterpart to `fields.rs`/`methods.rs`. Reuses `FieldInfo`/
//! `MethodSignature` as-is rather than forking them: `is_final` maps to
//! `val`/`var`, `java_type` to Kotlin's declared type.

use tree_sitter::{Node, Tree};

use crate::completion::{
    child_by_kind, enclosing_class_kotlin, identifier_and_type_children, property_declaration_type,
};
use crate::fields::FieldInfo;
use crate::methods::{MethodSignature, simple_name};

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

/// Name of the class the cursor sits in — callers re-look-up members by
/// name afterward, mirroring `java_dot_completion_candidates`.
pub fn kotlin_enclosing_class(tree: &Tree, source: &str, cursor_byte: usize) -> Option<String> {
    let start = tree
        .root_node()
        .named_descendant_for_byte_range(cursor_byte, cursor_byte)?;
    let class_node = enclosing_class_kotlin(start)?;
    let name = class_node.child_by_field_name("name")?;
    Some(source[name.byte_range()].to_string())
}

/// Simple name of the first type `class_name` delegates to (superclass or
/// first interface — `delegation_specifiers`' first entry; Kotlin's
/// grammar doesn't distinguish `extends`/`implements`). Only the first
/// entry, same scope limit Java's `superclass_name` already accepts.
pub fn kotlin_superclass_name(tree: &Tree, source: &str, class_name: &str) -> Option<String> {
    let class_node = find_class_node(tree.root_node(), source, class_name)?;
    let delegation_specifiers = child_by_kind(class_node, "delegation_specifiers")?;
    let specifier = delegation_specifiers.named_child(0)?;
    let inner = specifier.named_child(0)?;
    let type_node = match inner.kind() {
        "constructor_invocation" => inner.named_child(0)?,
        _ => inner,
    };
    Some(simple_name(&source[type_node.byte_range()]))
}

/// `FieldInfo` for every `property_declaration` directly in `body`, in
/// source order. Skips a property whose type can't be resolved (no
/// explicit type, and an initializer that isn't a constructor call)
/// rather than guessing.
pub fn kotlin_properties_in_class_body(body: Node, source: &str) -> Vec<FieldInfo> {
    let mut out = Vec::new();
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        if member.kind() != "property_declaration" {
            continue;
        }
        let Some(var_decl) = child_by_kind(member, "variable_declaration") else {
            continue;
        };
        let (Some(ident), _) = identifier_and_type_children(var_decl) else {
            continue;
        };
        let Some(raw_type) = property_declaration_type(member, source) else {
            continue;
        };
        out.push(FieldInfo {
            name: source[ident.byte_range()].to_string(),
            java_type: simple_name(&raw_type),
            is_final: child_by_kind(member, "val").is_some(),
        });
    }
    out
}

/// Constructor-promoted `val`/`var` parameters of `class_node`'s primary
/// constructor — `class Foo(val x: Int)`'s `x` is a member. A parameter
/// with neither modifier is constructor-only, not a member, and excluded.
fn kotlin_constructor_properties(class_node: Node, source: &str) -> Vec<FieldInfo> {
    let mut out = Vec::new();
    let Some(primary_constructor) = child_by_kind(class_node, "primary_constructor") else {
        return out;
    };
    let Some(class_parameters) = child_by_kind(primary_constructor, "class_parameters") else {
        return out;
    };
    let mut cursor = class_parameters.walk();
    for param in class_parameters.named_children(&mut cursor) {
        if param.kind() != "class_parameter" {
            continue;
        }
        let is_final = if child_by_kind(param, "val").is_some() {
            true
        } else if child_by_kind(param, "var").is_some() {
            false
        } else {
            continue;
        };
        let (ident, ty) = identifier_and_type_children(param);
        let (Some(ident), Some(ty)) = (ident, ty) else {
            continue;
        };
        out.push(FieldInfo {
            name: source[ident.byte_range()].to_string(),
            java_type: simple_name(&source[ty.byte_range()]),
            is_final,
        });
    }
    out
}

/// Every property `type_name` has: `class_body` properties plus
/// constructor-promoted parameters. Completion's actual entry point
/// (analogous to `fields::fields_in_type`).
pub fn kotlin_properties_in_type(tree: &Tree, source: &str, type_name: &str) -> Vec<FieldInfo> {
    let Some(class_node) = find_class_node(tree.root_node(), source, type_name) else {
        return Vec::new();
    };
    let mut out = kotlin_constructor_properties(class_node, source);
    if let Some(body) = child_by_kind(class_node, "class_body") {
        out.extend(kotlin_properties_in_class_body(body, source));
    }
    out
}

/// A function's return type: its one child that isn't a modifier, name,
/// parameter list, or body. `None` maps to `"Unit"` (Kotlin's default)
/// when there's no explicit return type.
fn return_type_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|c| {
        !matches!(
            c.kind(),
            "modifiers"
                | "identifier"
                | "function_value_parameters"
                | "type_parameters"
                | "type_constraints"
                | "function_body"
        )
    })
}

/// One function's signature, if `node` is a `function_declaration` —
/// excludes `private` functions unless `unfiltered` (`this.`/`super.`
/// sees everything; an external receiver doesn't, mirroring Java's
/// `method_signature`). Modifiers are anonymous tokens in a `modifiers`
/// node, so a text search over its span is how `private` is detected.
fn kotlin_function_signature(node: Node, source: &str, unfiltered: bool) -> Option<MethodSignature> {
    if node.kind() != "function_declaration" {
        return None;
    }
    if !unfiltered
        && let Some(modifiers) = child_by_kind(node, "modifiers")
        && source[modifiers.byte_range()].contains("private")
    {
        return None;
    }

    let name_node = node.child_by_field_name("name")?;
    let return_type = return_type_node(node)
        .map(|n| source[n.byte_range()].to_string())
        .unwrap_or_else(|| "Unit".to_string());

    let params_node = child_by_kind(node, "function_value_parameters")?;
    let mut params = Vec::new();
    let mut cursor = params_node.walk();
    for param in params_node.named_children(&mut cursor) {
        if param.kind() != "parameter" {
            continue;
        }
        let (ident, ty) = identifier_and_type_children(param);
        if let (Some(ident), Some(ty)) = (ident, ty) {
            params.push((
                source[ty.byte_range()].to_string(),
                source[ident.byte_range()].to_string(),
            ));
        }
    }

    Some(MethodSignature {
        name: source[name_node.byte_range()].to_string(),
        return_type,
        params,
    })
}

fn collect_kotlin_functions(tree: &Tree, source: &str, type_name: &str, unfiltered: bool) -> Vec<MethodSignature> {
    let Some(class_node) = find_class_node(tree.root_node(), source, type_name) else {
        return Vec::new();
    };
    let Some(body) = child_by_kind(class_node, "class_body") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        if let Some(sig) = kotlin_function_signature(member, source, unfiltered) {
            out.push(sig);
        }
    }
    out
}

/// Every function in `type_name`'s body, excluding `private` — an
/// external receiver's view, mirroring `methods_in_type`.
pub fn kotlin_functions_in_type(tree: &Tree, source: &str, type_name: &str) -> Vec<MethodSignature> {
    collect_kotlin_functions(tree, source, type_name, false)
}

/// Every function in `type_name`'s body, regardless of visibility —
/// `this.`/`super.`'s listing, mirroring `all_methods_in_type`.
pub fn all_kotlin_functions_in_type(tree: &Tree, source: &str, type_name: &str) -> Vec<MethodSignature> {
    collect_kotlin_functions(tree, source, type_name, true)
}

#[cfg(test)]
#[path = "kotlin_members_test.rs"]
mod kotlin_members_test;
