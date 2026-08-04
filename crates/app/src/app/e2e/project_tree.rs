//! The side panel: what the project tree lists, expanding/collapsing it,
//! creating a file from it, and hiding the panel itself.

use super::common::{E2e, MAIN_JAVA};

#[test]
fn an_open_project_lists_its_files_in_the_side_panel() {
    let app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);

    assert!(app.shows("☕ Main.java"), "the project tree must show the Java file");
    assert!(app.shows("🔷 Other.kt"), "the project tree must show the Kotlin file");
}

#[test]
fn collapsing_the_project_root_hides_its_files() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(app.shows("☕ Main.java"));

    app.toggle_project_root();

    assert!(
        !app.shows("☕ Main.java"),
        "a collapsed directory must hide its children"
    );
}

#[test]
fn ctrl_b_hides_and_shows_the_project_panel() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(app.shows("☕ Main.java"));

    app.press(egui::Modifiers::COMMAND, egui::Key::B);
    assert!(!app.shows("☕ Main.java"), "Ctrl+B must hide the project tree");

    app.press(egui::Modifiers::COMMAND, egui::Key::B);
    assert!(app.shows("☕ Main.java"), "Ctrl+B again must bring it back");
}

#[test]
fn creating_a_file_from_the_side_panel_writes_it_and_opens_it() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.click("📄");
    app.type_text("Created.java");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert_eq!(app.open_tab_names(), ["Created.java"], "a created file opens in a tab");
    assert!(app.path("Created.java").exists(), "and exists on disk");
    assert!(
        app.shows("☕ Created.java"),
        "and shows up in the refreshed project tree"
    );
}

#[test]
fn a_package_chain_collapses_into_one_row() {
    let mut app = E2e::launch(&[
        ("Main.java", MAIN_JAVA),
        (
            "src/main/java/com/example/App.java",
            "package com.example;\nclass App {}\n",
        ),
    ]);

    assert!(
        app.shows("📁 src/main"),
        "consecutive single-child directories share one row, slash-separated"
    );

    app.click("📁 src/main");
    app.click("📁 java");

    assert!(
        app.shows("📁 com.example"),
        "and below a source root the same chain is written as a package, dot-separated"
    );
}

#[test]
fn build_output_and_vcs_directories_are_not_listed() {
    let app = E2e::launch(&[
        ("Main.java", MAIN_JAVA),
        ("target/classes/Main.class", "not really a class file"),
        (".git/config", "[core]\n"),
        ("node_modules/left-pad/index.js", "module.exports = 0;\n"),
    ]);

    assert!(app.shows("☕ Main.java"));
    assert!(
        !app.shows_containing("target"),
        "build output is skipped, not just collapsed"
    );
    assert!(!app.shows_containing(".git"));
    assert!(!app.shows_containing("node_modules"));
}

#[test]
fn each_known_file_type_gets_its_own_icon() {
    let app = E2e::launch(&[
        ("Main.java", MAIN_JAVA),
        ("Other.kt", "class Other\n"),
        ("pom.xml", "<project/>\n"),
        ("application.properties", "server.port=8080\n"),
        ("application.yml", "server:\n  port: 8080\n"),
        ("notes.txt", "just text\n"),
    ]);

    assert!(app.shows("☕ Main.java"));
    assert!(app.shows("🔷 Other.kt"));
    assert!(app.shows("🏷️ pom.xml"));
    assert!(app.shows("⚙️ application.properties"));
    assert!(app.shows("📜 application.yml"));
    assert!(app.shows("📄 notes.txt"));
}

#[test]
fn settings_indentation_can_switch_to_tabs() {
    let mut app = E2e::launch(&[("Body.java", "class Body {\nint a = 1;\n}\n")]);
    app.click("☕ Body.java");

    app.menu("Settings", "Indentation");
    app.click("Tabs");

    app.type_into_active_tab("class Body {\n".chars().count(), "");
    app.press(egui::Modifiers::NONE, egui::Key::Tab);

    assert_eq!(
        app.active_tab_text(),
        "class Body {\n\tint a = 1;\n}\n",
        "with Tabs selected, Tab must insert a real tab character"
    );
}
