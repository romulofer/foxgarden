//! Settings > Accessibility…'s whole-interface zoom: that the keyboard
//! shortcuts move it, that it clamps, and that the dialog is reachable from
//! the menu.

use super::common_test::{E2e, MAIN_JAVA};
use fg_i18n::t;

#[test]
fn ctrl_plus_and_minus_move_the_interface_scale() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert_eq!(app.app().ui_scale, 1.0, "a fresh install starts at 100%");

    app.press(egui::Modifiers::COMMAND, egui::Key::Equals);
    assert!(app.app().ui_scale > 1.0, "Ctrl+= enlarges the whole interface");

    app.press(egui::Modifiers::COMMAND, egui::Key::Minus);
    assert!(
        (app.app().ui_scale - 1.0).abs() < 0.001,
        "and Ctrl+- takes the same step back"
    );
}

#[test]
fn ctrl_zero_returns_to_the_default_scale() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    for _ in 0..3 {
        app.press(egui::Modifiers::COMMAND, egui::Key::Equals);
    }
    assert!(app.app().ui_scale > 1.2);

    app.press(egui::Modifiers::COMMAND, egui::Key::Num0);
    assert_eq!(app.app().ui_scale, 1.0);
}

/// The clamp is what keeps the setting escapable: a zoom past the top of
/// the range would leave the menu itself unreadable, and the menu is where
/// the way back lives.
#[test]
fn holding_the_shortcut_cannot_zoom_past_the_offered_range() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    for _ in 0..40 {
        app.press(egui::Modifiers::COMMAND, egui::Key::Equals);
    }
    assert!(app.app().ui_scale <= 3.0, "scale ran away to {}", app.app().ui_scale);

    for _ in 0..60 {
        app.press(egui::Modifiers::COMMAND, egui::Key::Minus);
    }
    assert!(app.app().ui_scale >= 0.8, "scale shrank to {}", app.app().ui_scale);
}

#[test]
fn the_accessibility_dialog_opens_from_the_settings_menu() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.settings, t().menu.accessibility);

    assert!(app.shows(t().dialogs.ui_scale), "the dialog offers the scale control");
    assert!(app.shows(t().dialogs.ui_scale_reset));
}
