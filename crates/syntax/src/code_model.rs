//! `PLAN.md` Track 24 Phase 6: the type model behind dot-completion and code
//! generation, answered by whichever extension owns the language rather
//! than by a `match` here.
//!
//! What this replaces is four Java- and Kotlin-shaped modules in this crate
//! — fields, overridable methods, Kotlin members, and resolving a variable's
//! declared type — plus the Java accessor/`toString`/override templates the
//! app used to format itself. None of that is a general editor concern;
//! what is general is "list this type's members after a `.`" and "insert
//! generated members at the end of this type's body".
//!
//! Asks the extensions `providers` holds, handing over the tree the caller
//! already parsed. One language's type model belongs to one extension, so
//! every question takes the first extension that answers rather than
//! merging answers: two extensions listing the same type's members would
//! only list them twice.

use fg_core::Language;
use fg_extension::{CodeGeneration, MemberView, ReceiverType, TypeDeclaration, TypeFields, TypeMember};
use tree_sitter::Tree;

use crate::providers::providers;

/// The type whose body `byte` sits in, innermost first.
pub fn enclosing_type(tree: &Tree, source: &str, language: Language, byte: usize) -> Option<TypeDeclaration> {
    providers()
        .extensions
        .iter()
        .find_map(|extension| extension.enclosing_type(language.id(), tree, source, byte))
}

/// The one type `type_name` extends or implements first.
pub fn supertype(tree: &Tree, source: &str, language: Language, type_name: &str) -> Option<String> {
    providers()
        .extensions
        .iter()
        .find_map(|extension| extension.supertype(language.id(), tree, source, type_name))
}

/// `type_name`'s own members as `view` sees them, in source order.
pub fn type_members(tree: &Tree, source: &str, language: Language, type_name: &str, view: MemberView) -> Vec<TypeMember> {
    providers()
        .extensions
        .iter()
        .map(|extension| extension.type_members(language.id(), tree, source, type_name, view))
        .find(|members| !members.is_empty())
        .unwrap_or_default()
}

/// What `receiver`, the word right before a `.` at `byte`, resolves to.
/// `None` for anything that cannot be resolved syntactically, and for every
/// language no extension models.
pub fn receiver_type(tree: &Tree, source: &str, language: Language, byte: usize, receiver: &str) -> Option<ReceiverType> {
    providers()
        .extensions
        .iter()
        .find_map(|extension| extension.receiver_type(language.id(), tree, source, byte, receiver))
}

/// Whether any installed extension generates code for `language`.
pub fn generates_code(language: Language) -> bool {
    providers()
        .extensions
        .iter()
        .any(|extension| extension.generates_code(language.id()))
}

/// Every type in the file with fields code generation can use.
pub fn types_with_fields(tree: &Tree, source: &str, language: Language) -> Vec<TypeFields> {
    providers()
        .extensions
        .iter()
        .map(|extension| extension.types_with_fields(language.id(), tree, source))
        .find(|types| !types.is_empty())
        .unwrap_or_default()
}

/// The source text `request` asks for, from the extension that generates
/// code for `language`. `None` when none does.
pub fn generate_code(language: Language, request: CodeGeneration<'_>, indent_unit: &str) -> Option<String> {
    providers()
        .extensions
        .iter()
        .find_map(|extension| extension.generate_code(language.id(), request, indent_unit))
}

/// The file extensions `language`'s source files carry, no leading dot —
/// where to look for the file a type of that language is declared in.
pub fn source_file_extensions(language: Language) -> Vec<String> {
    providers()
        .file_extensions
        .get(language.id())
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "code_model_test.rs"]
mod code_model_test;
