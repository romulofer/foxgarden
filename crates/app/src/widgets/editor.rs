//! The text-editing engine: the highlighted/squiggled `TextEdit` wrapper
//! (`widget`), its pure edit-transform helpers (`auto_edit`), its overlay
//! rendering (`painting`), Ctrl+D multi-cursor support (`multi_cursor`),
//! Java getter/setter and constructor/toString/equals+hashCode generation
//! (`codegen`), live templates (`templates`), the right-click context menu
//! (`context_menu`), and shared byte↔char offset conversion
//! (`text_offset`). `show` and the handful of types the Tools menu needs to
//! request generation/case-conversion are the only things used outside this
//! module.

mod auto_edit;
mod codegen;
mod context_menu;
mod multi_cursor;
mod painting;
mod templates;
mod text_area;
mod text_offset;
mod widget;

pub use auto_edit::CaseConversion;
pub use codegen::{AccessorKind, GenerateAccessorsDialog, GenerateMethodDialog, GenerateMethodKind, OverrideMethodDialog};
pub use widget::show;
