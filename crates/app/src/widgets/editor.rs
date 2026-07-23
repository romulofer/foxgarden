//! The text-editing engine: the highlighted/squiggled `TextEdit` wrapper
//! (`widget`), its pure edit-transform helpers (`auto_edit`), its overlay
//! rendering (`painting`), Ctrl+D multi-cursor support (`multi_cursor`),
//! Java getter/setter generation (`codegen`), and live templates
//! (`templates`). `show` and `AccessorKind` (the latter needed by the Tools
//! menu to request getters/setters generation) are the only things used
//! outside this module.

mod auto_edit;
mod codegen;
mod multi_cursor;
mod painting;
mod templates;
mod widget;

pub use codegen::AccessorKind;
pub use widget::show;
