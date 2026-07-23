//! The text-editing engine: the highlighted/squiggled `TextEdit` wrapper
//! (`widget`), its pure edit-transform helpers (`auto_edit`), its overlay
//! rendering (`painting`), Ctrl+D multi-cursor support (`multi_cursor`), and
//! Java getter/setter generation (`codegen`). Only `show` is used outside
//! this module.

mod auto_edit;
mod codegen;
mod multi_cursor;
mod painting;
mod widget;

pub use widget::show;
