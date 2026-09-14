//! The docked panels the View menu toggles: the terminal at the bottom and
//! Source Control on the right. Both are covered only as far as "opening it
//! shows the panel, closing it takes it away" — what they *do* afterwards
//! runs a real shell or a real `git` subprocess on a background thread, so
//! asserting on their contents from here would be timing-dependent; that
//! logic has its own unit tests in `pty_session`/`panels::git_stage`.

use super::common_test::{E2e, MAIN_JAVA};
use crate::panels::bottom_dock::BottomTab;
use fg_i18n::t;

#[test]
fn the_terminal_panel_opens_and_closes_with_its_shortcut() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(!app.app().bottom_dock.is_open(), "the terminal starts closed");

    app.press(egui::Modifiers::COMMAND, egui::Key::Backtick);
    assert!(
        app.app().bottom_dock.shows(BottomTab::Terminal),
        "Ctrl+` opens the terminal panel"
    );
    assert!(
        !app.app().state.terminal_tabs.is_empty(),
        "opening the panel starts a session rather than showing an empty panel"
    );

    app.press(egui::Modifiers::COMMAND, egui::Key::Backtick);
    assert!(!app.app().bottom_dock.is_open(), "and Ctrl+` again closes it");
}

#[test]
fn the_terminal_panel_can_also_be_toggled_from_the_view_menu() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.view, t().menu.terminal_panel);
    assert!(app.app().bottom_dock.shows(BottomTab::Terminal));

    app.menu(t().menu.view, t().menu.terminal_panel);
    assert!(!app.app().bottom_dock.is_open());
}

/// The dock's whole point (`FEATURES.md`'s "Tab-controlled bottom panels"):
/// asking for a second bottom panel *replaces* what's showing rather than
/// stacking a second one under it, and the strip stays open on the newly
/// picked tab.
#[test]
fn a_second_bottom_panel_switches_tabs_instead_of_stacking() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.view, t().menu.terminal_panel);
    assert!(app.app().bottom_dock.shows(BottomTab::Terminal));

    app.menu(t().menu.view, t().menu.build_output);
    assert!(app.app().bottom_dock.shows(BottomTab::Build));
    assert!(
        !app.app().bottom_dock.shows(BottomTab::Terminal),
        "the terminal must give up the dock rather than stay open underneath"
    );
    assert!(app.shows(t().dock.terminal), "its tab stays reachable in the strip");
}

#[test]
fn the_source_control_panel_opens_from_the_view_menu() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(!app.shows(t().menu.source_control));

    app.menu(t().menu.view, t().menu.source_control);
    assert!(
        app.shows(t().menu.source_control),
        "the panel shows its heading once open"
    );

    app.click(t().menu.view);
    app.click_checkbox(t().menu.source_control);
    assert!(!app.shows(t().menu.source_control));
}

/// The status bar is chrome, not a toggleable panel: it's simply always
/// there, saying either what the app is doing on its own or that it isn't
/// doing anything. What it says while a job *is* running isn't driven from
/// here — every one of those jobs finishes whenever its own thread finishes
/// (this module's own header) — so `panels::status_bar`'s unit tests cover
/// the wording and this covers that the bar reaches the screen at all.
#[test]
fn the_status_bar_reports_an_idle_app_and_goes_away_in_zen_mode() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(
        app.shows(t().status_bar.ready),
        "an app with nothing running says so along the bottom"
    );

    app.menu(t().menu.view, t().menu.zen_mode);
    assert!(
        !app.shows(t().status_bar.ready),
        "zen mode hides the status bar along with the rest of the chrome"
    );
}

/// The status bar reports the file being edited, not just background work:
/// caret position, language, indentation. All of it was previously
/// invisible anywhere in the UI.
#[test]
fn the_status_bar_reports_the_active_file_s_position_and_language() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(!app.shows_containing("Ln 1"), "with no file open there is nothing to report");

    app.click_tree("Main.java");

    assert!(app.shows_containing("Ln 1, Col 1"), "a freshly opened file starts at the top");
    assert!(app.shows_containing("Java"), "and says what language it is");
    assert!(app.shows_containing("Espaços: 4"), "and what a Tab inserts");
}
