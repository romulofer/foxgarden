//! Getting somewhere without the tree: the `Ctrl+P` fuzzy file opener, the
//! `Ctrl+E` recent-files switcher, and the `Ctrl+Shift+E` HTTP route
//! map, plus Go to expect/actual.

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
    app.click_tree("Main.java");
    app.click_tree("Other.kt");

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
fn ctrl_shift_e_lists_the_projects_http_routes_and_jumps_to_one() {
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
        app.shows(t().palettes.http_routes),
        "Ctrl+Shift+E opens the route map"
    );
    assert!(app.shows_containing("/api/users"), "and lists the mapped path");

    app.click_containing("/api/users");

    assert_eq!(
        app.open_tab_names(),
        ["UserController.java"],
        "picking one opens its file"
    );
    assert!(!app.shows(t().palettes.http_routes));
}

const BUILD_KMP: &str = "plugins { kotlin(\"multiplatform\") }\n";

fn open_by_search(app: &mut E2e, query: &str) {
    app.press(egui::Modifiers::COMMAND, egui::Key::P);
    app.type_text(query);
    app.press(egui::Modifiers::NONE, egui::Key::Enter);
}

fn go_to_counterpart_from_the_palette(app: &mut E2e) {
    app.press(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::P);
    app.type_text(t().palettes.go_to_counterpart);
    app.press(egui::Modifiers::NONE, egui::Key::Enter);
}

/// An `actual` leads straight back to its only `expect`, and the caret lands
/// on the declaration's name.
#[test]
fn go_to_expect_actual_jumps_from_an_actual_to_its_expect() {
    let mut app = E2e::launch(&[
        ("build.gradle.kts", BUILD_KMP),
        ("src/commonMain/kotlin/Greeter.kt", "expect fun greet(): String\n"),
        ("src/jvmMain/kotlin/GreeterJvm.kt", "actual fun greet(): String = \"jvm\"\n"),
    ]);
    open_by_search(&mut app, "GreeterJvm");
    app.type_into_active_tab(0, "");

    go_to_counterpart_from_the_palette(&mut app);

    assert_eq!(app.active_tab_name().as_deref(), Some("Greeter.kt"));
}

/// An `expect` with an `actual` per platform asks which one.
#[test]
fn go_to_expect_actual_asks_when_an_expect_has_several_actuals() {
    let mut app = E2e::launch(&[
        ("build.gradle.kts", BUILD_KMP),
        ("src/commonMain/kotlin/Greeter.kt", "expect fun greet(): String\n"),
        ("src/jvmMain/kotlin/GreeterJvm.kt", "actual fun greet(): String = \"jvm\"\n"),
        ("src/iosMain/kotlin/GreeterIos.kt", "actual fun greet(): String = \"ios\"\n"),
    ]);
    open_by_search(&mut app, "Greeter.kt");
    app.type_into_active_tab(0, "");

    go_to_counterpart_from_the_palette(&mut app);
    assert!(app.shows_containing("iosMain"), "every source set with an actual is offered");
    assert!(app.shows_containing("jvmMain"));

    app.click_containing("jvmMain");

    assert_eq!(app.active_tab_name().as_deref(), Some("GreeterJvm.kt"));
    assert!(!app.shows_containing("iosMain"), "choosing closes the picker");
}

/// A line that declares neither says so instead of doing nothing.
#[test]
fn go_to_expect_actual_says_when_the_line_has_no_counterpart() {
    let mut app = E2e::launch(&[
        ("build.gradle.kts", BUILD_KMP),
        ("src/commonMain/kotlin/Greeter.kt", "val x = 1\n"),
    ]);
    open_by_search(&mut app, "Greeter.kt");
    app.type_into_active_tab(0, "");

    go_to_counterpart_from_the_palette(&mut app);

    assert!(app.shows(t().palettes.no_counterpart));
    assert_eq!(app.active_tab_name().as_deref(), Some("Greeter.kt"));
}
