mod brackets;
mod code_model;
mod counterparts;
mod server_diagnostics;
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
    enclosing_type, file_icon, generate_code, generates_code, is_debuggable, is_source_root_dir, is_workspace_edit_command, language_keywords, language_snippets, receiver_type,
    source_file_extensions, supertype, type_members, types_with_fields,
};
pub use counterparts::{counterpart_lines, counterparts, file_diagnostics};
pub use server_diagnostics::is_spurious_server_diagnostic;
pub use diagnostics::syntax_errors;
pub use document_parser::{IncrementalParser, byte_to_point, diff_edit};
pub use folding::{FoldRange, foldable_ranges};
pub use fg_extension::{
    CodeGeneration, Counterpart, DocStub, FileDiagnostic, ProblemSeverity, HttpRoute, ImportCandidate, ImportEdit, MemberKind, MemberView, ReceiverType, RunTarget, Snippet, TypeDeclaration, TypeFields, TypeMember,
};
pub use grammars::{GrammarError, install as install_grammars};
pub use highlight::{Scope, highlight_spans, highlight_spans_in};
pub use http_routes::http_routes;
pub use imports::{annotation_candidates, doc_stub, import_edit};
pub use providers::install as install_extension_providers;
pub use run_targets::{RunMarker, main_entries};
pub use selection::expand_selection;
pub use sticky::enclosing_scope_starts;
pub use tree_sitter::{InputEdit, Point, Tree};
