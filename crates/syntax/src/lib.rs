mod brackets;
mod completion;
mod diagnostics;
mod document_parser;
mod fields;
mod folding;
mod grammars;
mod highlight;
mod http_routes;
mod imports;
mod kotlin_members;
mod run_targets;
mod methods;
mod node_kinds;
mod providers;
mod selection;
mod sticky;

pub use brackets::bracket_match;
pub use completion::{type_of_identifier, type_of_identifier_java, type_of_identifier_kotlin};
pub use diagnostics::syntax_errors;
pub use document_parser::{IncrementalParser, byte_to_point, diff_edit};
pub use fields::{ClassFields, FieldInfo, fields_in_type, java_classes_with_fields};
pub use folding::{FoldRange, foldable_ranges};
pub use fg_extension::{HttpRoute, RunTarget};
pub use grammars::{GrammarError, install as install_grammars};
pub use highlight::{Scope, highlight_spans, highlight_spans_in};
pub use http_routes::http_routes;
pub use imports::{ExistingImport, ImportInsertion, existing_imports, import_insertion};
pub use kotlin_members::{
    all_kotlin_functions_in_type, kotlin_enclosing_class, kotlin_functions_in_type, kotlin_properties_in_class_body,
    kotlin_properties_in_type, kotlin_superclass_name,
};
pub use methods::{MethodSignature, all_methods_in_type, enclosing_class, methods_in_type, superclass_name};
pub use providers::install as install_extension_providers;
pub use run_targets::{RunMarker, main_entries};
pub use selection::expand_selection;
pub use sticky::enclosing_scope_starts;
pub use tree_sitter::{InputEdit, Point, Tree};
