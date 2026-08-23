//! The text-editing engine: the highlighted/squiggled `TextEdit` wrapper
//! (`widget`), its pure edit-transform helpers (`auto_edit`), its overlay
//! rendering (`painting`), Ctrl+D multi-cursor support (`multi_cursor`),
//! Java getter/setter and constructor/toString/equals+hashCode generation
//! (`codegen`), live templates (`templates`), the completion popup
//! (`completion`), the hover-docs popup (`hover`), the peek-definition
//! popup (`peek`), the right-click context menu (`context_menu`), the
//! Spring endpoint map's whole-project scan (`spring_scan`), the git diff
//! gutter (`diff_gutter`), and shared byte↔char offset conversion
//! (`text_offset`). `show` and the
//! handful of types the Tools menu needs to request generation/
//! case-conversion are the only things used outside this module.

mod auto_edit;
mod code_action;
mod codegen;
mod completion;
mod context_menu;
mod diff_gutter;
mod folding;
mod hover;
mod multi_cursor;
mod painting;
mod peek;
mod references;
mod rename;
mod spring_annotation_completion;
mod spring_config_completion;
mod spring_scan;
mod templates;
mod text_area;
mod text_offset;
mod widget;

pub use auto_edit::CaseConversion;
pub use code_action::CodeActionGutter;
pub use codegen::{
    AccessorKind, GenerateAccessorsDialog, GenerateMethodDialog, GenerateMethodKind, OverrideMethodDialog,
};
pub use completion::CompletionState;
pub use hover::HoverState;
pub use peek::PeekState;
pub use references::FindReferencesState;
pub use rename::RenameBox;
pub use spring_scan::{EndpointCache, scan_project_endpoints_cached};
pub use templates::{
    GLOBAL_TEMPLATES, JAVA_TEMPLATES, KOTLIN_TEMPLATES, Template, UserTemplate, UserTemplates, parse_user_templates,
    serialize_user_templates,
};
pub use widget::{jump_to, show};
