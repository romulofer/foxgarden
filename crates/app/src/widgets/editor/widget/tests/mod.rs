//! Unit tests for [`super`](../widget.rs), split by which of `widget.rs`'s
//! own interception blocks / sibling module each group exercises — this
//! module used to be one large flat file; see git history if the previous
//! shape is ever needed for reference. `common` holds every fixture/event-
//! builder helper shared across more than one of the topic modules below;
//! everything else follows this crate's usual "same module, separate file"
//! convention (`AGENTS.md`'s testing-conventions section), just one level
//! deeper than a single `<module>/tests.rs` normally goes.

mod common;

mod auto_edit;
mod click;
mod codegen;
mod context_menu;
mod line_comment;
mod multi_cursor;
mod painting;
mod read_only;
mod selection;
mod templates;
