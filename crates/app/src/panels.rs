//! The surrounding UI chrome: the menu bar, the project side panel, the
//! status bar, and the tab bar (which also owns tab/parser lifecycle — see
//! `tabs::open_parser_for`).

pub mod build_panel;
pub mod git_diff;
pub mod git_stage;
pub mod go_to_file;
pub mod jdk_registry;
pub mod lsp_servers;
pub mod menu_bar;
pub mod new_project;
pub mod quick_switcher;
pub mod run_configs;
pub mod side_panel;
pub mod spring_config;
pub mod spring_endpoints;
pub mod static_analysis;
pub mod status_bar;
pub mod tabs;
pub mod terminal_panel;
