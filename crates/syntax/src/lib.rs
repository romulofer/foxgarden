mod brackets;
mod diagnostics;
mod document_parser;
mod fields;
mod highlight;
mod language;
mod methods;
mod selection;

pub use brackets::bracket_match;
pub use diagnostics::syntax_errors;
pub use document_parser::{byte_to_point, diff_edit, IncrementalParser};
pub use fields::{java_classes_with_fields, ClassFields, FieldInfo};
pub use highlight::{highlight_spans, Scope};
pub use methods::{enclosing_class, methods_in_type, superclass_name, MethodSignature};
pub use selection::expand_selection;
pub use tree_sitter::{InputEdit, Point, Tree};
