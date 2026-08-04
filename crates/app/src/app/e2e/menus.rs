//! The menu bar: File, Settings, Tools, Run, View, Help — every item that
//! doesn't need an OS dialog (File > Open Folder…) or an external process
//! (Tools > Run Checkstyle/PMD) behind it.

use super::common::{E2e, MAIN_JAVA};

#[test]
fn file_save_writes_the_active_tab() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click("☕ Main.java");
    app.type_into_active_tab(MAIN_JAVA.chars().count(), "// via the menu");

    app.menu("File", "Save");

    assert_eq!(app.on_disk("Main.java"), format!("{MAIN_JAVA}// via the menu"));
    assert!(!app.shows("*Main.java"));
}

#[test]
fn file_close_tab_closes_the_active_one() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);
    app.click("☕ Main.java");
    app.click("🔷 Other.kt");

    app.menu("File", "Close Tab");

    assert_eq!(app.open_tab_names(), ["Main.java"]);
}

#[test]
fn file_reopen_closed_tab_brings_the_last_one_back() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click("☕ Main.java");
    app.menu("File", "Close Tab");
    assert!(app.open_tab_names().is_empty());

    app.menu("File", "Reopen Closed Tab");

    assert_eq!(app.open_tab_names(), ["Main.java"]);
}

#[test]
fn file_new_file_opens_the_side_panel_row_and_creates_the_file() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu("File", "New File…");
    app.type_text("FromMenu.java");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert!(app.path("FromMenu.java").exists());
    assert_eq!(app.open_tab_names(), ["FromMenu.java"]);
}

#[test]
fn settings_theme_switches_between_light_and_dark() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(app.is_dark_mode(), "dark is the default theme");

    app.menu("Settings", "Theme");
    app.click("Light");
    assert!(!app.is_dark_mode(), "Settings > Theme > Light must apply immediately");

    app.menu("Settings", "Theme");
    app.click("Dark");
    assert!(app.is_dark_mode());
}

#[test]
fn settings_font_dialog_opens_and_closes() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu("Settings", "Font…");
    assert!(app.shows("Size"), "the font dialog must show its size control");

    app.click("Close");
    assert!(!app.shows("Size"), "Close must dismiss the dialog");
}

#[test]
fn settings_auto_save_can_be_enabled_from_the_menu() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(!app.app().auto_save_settings.enabled, "auto-save is off by default");

    app.menu("Settings", "Auto-save");
    app.click("Enabled");

    assert!(app.app().auto_save_settings.enabled);
}

#[test]
fn settings_external_tools_dialog_opens() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu("Settings", "External Tools…");

    assert!(
        app.shows_containing("Checkstyle"),
        "the External Tools dialog covers Checkstyle"
    );
    app.click("Close");
    assert!(!app.shows_containing("Checkstyle"));
}

#[test]
fn settings_language_servers_dialog_opens() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu("Settings", "Language Servers…");

    assert!(app.shows_containing("jdtls") || app.shows_containing("Java"));
    app.click("Close");
}

#[test]
fn view_zen_mode_hides_the_menu_bar_and_f11_brings_it_back() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu("View", "Zen Mode");
    assert!(!app.shows("File"), "zen mode hides the menu bar");
    assert!(!app.shows("☕ Main.java"), "and the side panel with it");

    app.press(egui::Modifiers::NONE, egui::Key::F11);

    assert!(
        app.shows("File"),
        "F11 must get out of zen mode even with no menu to click"
    );
    assert!(app.shows("☕ Main.java"));
}

#[test]
fn view_side_panel_checkbox_hides_the_project_tree() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu("View", "Side Panel");

    assert!(!app.shows("☕ Main.java"));
    assert!(app.shows("File"), "only the side panel goes, unlike zen mode");
}

#[test]
fn view_word_wrap_toggles_the_setting() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    let was_wrapped = app.app().view_settings.word_wrap;

    app.menu("View", "Word Wrap");

    assert_ne!(app.app().view_settings.word_wrap, was_wrapped);
}

#[test]
fn run_edit_configurations_opens_its_dialog() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu("Run", "Edit Configurations…");

    assert!(app.shows_containing("Configurations") || app.shows_containing("Configuration"));
}

#[test]
fn help_about_lists_the_shortcuts() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu("Help", "About");

    assert!(
        app.shows("Ctrl+S — save the active tab"),
        "About documents the shortcuts"
    );
    app.click("Close");
    assert!(!app.shows("Ctrl+S — save the active tab"));
}

#[test]
fn help_live_templates_dialog_opens() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu("Help", "Live Templates…");

    assert!(
        app.shows_containing("trigger"),
        "the templates dialog explains triggers"
    );
    app.click("Close");
}

#[test]
fn tools_read_only_marks_the_tab_and_blocks_typing() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click("☕ Main.java");

    app.menu("Tools", "Read-Only");
    assert!(app.shows("🔒Main.java"), "a read-only tab is marked with a lock");

    app.type_into_active_tab(MAIN_JAVA.chars().count(), "// nope");

    assert_eq!(app.active_tab_text(), MAIN_JAVA, "a read-only tab must ignore typing");
    assert!(!app.shows("*🔒Main.java"), "and so must stay clean");
}

#[test]
fn tools_convert_to_uppercase_converts_the_selection() {
    let mut app = E2e::launch(&[("Main.java", "class Main {\n  int value;\n}\n")]);
    app.click("☕ Main.java");

    // The `value` identifier on line 2.
    let offset = "class Main {\n  int ".chars().count();
    app.select_in_active_tab(offset, "value".chars().count());
    app.menu("Tools", "Convert to UPPERCASE");

    assert_eq!(app.active_tab_text(), "class Main {\n  int VALUE;\n}\n");
}

#[test]
fn tools_case_conversion_with_no_selection_reports_an_error() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click("☕ Main.java");
    app.type_into_active_tab(0, "");

    app.menu("Tools", "Convert to lowercase");

    assert!(app.shows("Select some text first, then try again."));
    app.click("OK");
    assert!(
        !app.shows("Select some text first, then try again."),
        "OK dismisses the error"
    );
}

#[test]
fn tools_sort_lines_sorts_the_selected_lines() {
    let mut app = E2e::launch(&[("Notes.java", "// c\n// a\n// b\n")]);
    app.click("☕ Notes.java");

    app.select_in_active_tab(0, "// c\n// a\n// b".chars().count());
    app.menu("Tools", "Sort Lines");

    assert_eq!(app.active_tab_text(), "// a\n// b\n// c\n");
}

#[test]
fn tools_unique_lines_drops_the_duplicates() {
    let mut app = E2e::launch(&[("Notes.java", "// a\n// a\n// b\n")]);
    app.click("☕ Notes.java");

    app.select_in_active_tab(0, "// a\n// a\n// b".chars().count());
    app.menu("Tools", "Unique Lines");

    assert_eq!(app.active_tab_text(), "// a\n// b\n");
}

#[test]
fn tools_generate_getters_inserts_one_for_the_only_class() {
    let mut app = E2e::launch(&[("Person.java", "class Person {\n    private String name;\n}\n")]);
    app.click("☕ Person.java");

    app.menu("Tools", "Generate Getters");

    let text = app.active_tab_text();
    assert!(
        text.contains("public String getName()"),
        "a single eligible class generates straight away, no dialog: {text}"
    );
}
