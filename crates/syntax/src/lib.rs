mod brackets;
mod code_model;
mod diagnostics;
mod document_parser;
mod folding;
mod grammars;
mod highlight;
mod http_routes;
mod imports;
mod run_targets;
mod node_kinds;
mod providers;
mod selection;
mod sticky;

pub use brackets::bracket_match;
pub use code_model::{
    enclosing_type, generate_code, generates_code, receiver_type, source_file_extensions, supertype, type_members,
    types_with_fields,
};
pub use diagnostics::syntax_errors;
pub use document_parser::{IncrementalParser, byte_to_point, diff_edit};
pub use folding::{FoldRange, foldable_ranges};
pub use fg_extension::{
    CodeGeneration, HttpRoute, MemberKind, MemberView, ReceiverType, RunTarget, TypeDeclaration, TypeFields, TypeMember,
};
pub use grammars::{GrammarError, install as install_grammars};
pub use highlight::{Scope, highlight_spans, highlight_spans_in};
pub use http_routes::http_routes;
pub use imports::{ExistingImport, ImportInsertion, existing_imports, import_insertion};
pub use providers::install as install_extension_providers;
pub use run_targets::{RunMarker, main_entries};
pub use selection::expand_selection;
pub use sticky::enclosing_scope_starts;
pub use tree_sitter::{InputEdit, Point, Tree};
