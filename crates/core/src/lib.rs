mod boilerplate;
mod diagnostic;
mod document;
mod editor_state;
mod language;
mod project;

pub use boilerplate::generate as generate_boilerplate;
pub use diagnostic::{Diagnostic, Severity};
pub use document::{Document, OpenDocumentError};
pub use editor_state::EditorState;
pub use language::Language;
pub use project::{FileKind, FileNode, Project};
