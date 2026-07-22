mod diagnostics;
mod document_parser;
mod highlight;
mod language;

pub use diagnostics::syntax_errors;
pub use document_parser::{byte_to_point, diff_edit, IncrementalParser};
pub use highlight::{highlight_spans, Scope};
pub use tree_sitter::{InputEdit, Point, Tree};
