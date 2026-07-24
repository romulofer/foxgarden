//! The text-editing engine: the highlighted/squiggled `TextEdit` wrapper
//! (`widget`), its pure edit-transform helpers (`auto_edit`), its overlay
//! rendering (`painting`), Ctrl+D multi-cursor support (`multi_cursor`),
//! Java getter/setter generation (`codegen`), and live templates
//! (`templates`). `show`, `AccessorKind`, and `CaseConversion` (the latter
//! two needed by the Tools menu to request getter/setter generation and
//! case conversion) are the only things used outside this module.

mod auto_edit;
mod codegen;
mod multi_cursor;
mod painting;
mod templates;
mod widget;

pub use auto_edit::CaseConversion;
pub use codegen::{AccessorKind, GenerateAccessorsDialog};
pub use widget::show;
