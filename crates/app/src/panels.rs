//! The surrounding UI chrome: the menu bar, the project side panel, and the
//! tab bar (which also owns tab/parser lifecycle — see `tabs::open_parser_for`).

pub mod go_to_file;
pub mod menu_bar;
pub mod quick_switcher;
pub mod run_configs;
pub mod side_panel;
pub mod tabs;
