//! The docked panels the View menu toggles: the terminal at the bottom and
//! Source Control on the right. Both are covered only as far as "opening it
//! shows the panel, closing it takes it away" — what they *do* afterwards
//! runs a real shell or a real `git` subprocess on a background thread, so
//! asserting on their contents from here would be timing-dependent; that
//! logic has its own unit tests in `pty_session`/`panels::git_stage`.

use super::common::{E2e, MAIN_JAVA};

#[test]
fn the_terminal_panel_opens_and_closes_with_its_shortcut() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(!app.app().terminal_panel_visible, "the terminal starts closed");

    app.press(egui::Modifiers::COMMAND, egui::Key::Backtick);
    assert!(app.app().terminal_panel_visible, "Ctrl+` opens the terminal panel");
    assert!(
        !app.app().state.terminal_tabs.is_empty(),
        "opening the panel starts a session rather than showing an empty panel"
    );

    app.press(egui::Modifiers::COMMAND, egui::Key::Backtick);
    assert!(!app.app().terminal_panel_visible, "and Ctrl+` again closes it");
}

#[test]
fn the_terminal_panel_can_also_be_toggled_from_the_view_menu() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu("View", "Terminal Panel");
    assert!(app.app().terminal_panel_visible);

    app.menu("View", "Terminal Panel");
    assert!(!app.app().terminal_panel_visible);
}

#[test]
fn the_source_control_panel_opens_from_the_view_menu() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(!app.shows("Source Control"));

    app.menu("View", "Source Control");
    assert!(app.shows("Source Control"), "the panel shows its heading once open");

    app.click("View");
    app.click_checkbox("Source Control");
    assert!(!app.shows("Source Control"));
}
