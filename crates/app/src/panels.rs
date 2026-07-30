//! The surrounding UI chrome: the menu bar, the project side panel, and the
//! tab bar (which also owns tab/parser lifecycle — see `tabs::open_parser_for`).

pub mod git_diff;
pub mod git_stage;
pub mod go_to_file;
pub mod menu_bar;
pub mod quick_switcher;
pub mod run_configs;
pub mod side_panel;
pub mod spring_config;
pub mod spring_endpoints;
pub mod static_analysis;
pub mod tabs;
pub mod terminal_panel;
