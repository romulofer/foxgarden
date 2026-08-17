//! Getting somewhere without the tree: the `Ctrl+P` fuzzy file opener, the
//! `Ctrl+E` recent-files switcher, and the `Ctrl+Shift+E` Spring endpoint
//! map.

use super::common_test::{E2e, MAIN_JAVA};
use fg_i18n::t;

#[test]
fn ctrl_p_opens_the_file_it_matches() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);

    app.press(egui::Modifiers::COMMAND, egui::Key::P);
    assert!(
        app.shows(t().palettes.go_to_file),
        "Ctrl+P must open the go-to-file popup"
    );

    app.type_text("Other");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert_eq!(app.open_tab_names(), ["Other.kt"]);
    assert!(
        !app.shows(t().palettes.go_to_file),
        "picking a match must close the popup"
    );
}

#[test]
fn escape_closes_the_go_to_file_popup_without_opening_anything() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.press(egui::Modifiers::COMMAND, egui::Key::P);
    app.press(egui::Modifiers::NONE, egui::Key::Escape);

    assert!(!app.shows(t().palettes.go_to_file));
    assert!(app.open_tab_names().is_empty());
}

#[test]
fn arrow_down_moves_the_go_to_file_selection_before_enter_opens_it() {
    let mut app = E2e::launch(&[("Alpha.java", "class Alpha {}\n"), ("Beta.java", "class Beta {}\n")]);

    app.press(egui::Modifiers::COMMAND, egui::Key::P);
    app.type_text("a");
    app.press(egui::Modifiers::NONE, egui::Key::ArrowDown);
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert_eq!(
        app.open_tab_names(),
        ["Beta.java"],
        "ArrowDown must move off the first match before Enter takes it"
    );
}

#[test]
fn ctrl_e_lists_recent_files_and_reopens_one() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);
    app.click("☕ Main.java");
    app.click("🔷 Other.kt");

    app.press(egui::Modifiers::COMMAND, egui::Key::E);
    assert!(
        app.shows(t().palettes.go_to_recent_file),
        "Ctrl+E opens the recent-files switcher"
    );

    // Driven by keyboard rather than by clicking the row: the popup lists
    // `Main.java` under the same label its own tab already has, so a
    // by-label click would be ambiguous between the two. Typing narrows the
    // list to that one file, so Enter can only mean it.
    app.type_text("Main");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert_eq!(app.active_tab_name().as_deref(), Some("Main.java"));
    assert!(!app.shows(t().palettes.go_to_recent_file), "picking closes it");
}

#[test]
fn ctrl_shift_e_lists_the_projects_spring_endpoints_and_jumps_to_one() {
    const CONTROLLER: &str = "\
@RestController
@RequestMapping(\"/api\")
class UserController {
    @GetMapping(\"/users\")
    public String list() { return \"\"; }
}
";
    let mut app = E2e::launch(&[("UserController.java", CONTROLLER)]);

    app.press(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::E);
    assert!(
        app.shows(t().palettes.spring_endpoints),
        "Ctrl+Shift+E opens the endpoint map"
    );
    assert!(app.shows_containing("/api/users"), "and lists the mapped path");

    app.click_containing("/api/users");

    assert_eq!(
        app.open_tab_names(),
        ["UserController.java"],
        "picking one opens its file"
    );
    assert!(!app.shows(t().palettes.spring_endpoints));
}
