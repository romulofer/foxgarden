//! The tab bar: opening files into tabs, switching between them, closing
//! them, and getting a closed one back.

use super::common::{E2e, MAIN_JAVA};
use fg_i18n::{msg, t};

#[test]
fn clicking_a_file_in_the_tree_opens_it_in_a_tab() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(app.shows(t().tabs.no_file_open), "nothing is open before the click");

    app.click("☕ Main.java");

    assert_eq!(app.open_tab_names(), ["Main.java"]);
    assert!(app.shows("Main.java"), "the opened file must get a tab");
    assert!(!app.shows(t().tabs.no_file_open));
}

#[test]
fn opening_a_second_file_adds_a_tab_and_clicking_a_tab_switches_back() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);

    app.click("☕ Main.java");
    app.click("🔷 Other.kt");

    assert_eq!(app.open_tab_names(), ["Main.java", "Other.kt"]);
    assert_eq!(app.active_tab_name().as_deref(), Some("Other.kt"));

    app.click("Main.java");

    assert_eq!(app.active_tab_name().as_deref(), Some("Main.java"));
    assert_eq!(
        app.open_tab_names(),
        ["Main.java", "Other.kt"],
        "switching must not close anything"
    );
}

#[test]
fn closing_a_clean_tab_with_the_x_button_removes_it() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);
    app.click("☕ Main.java");
    app.click("🔷 Other.kt");

    app.close_tab(0);

    assert_eq!(app.open_tab_names(), ["Other.kt"]);
    assert!(!app.shows("Main.java"), "the closed tab must be gone from the tab bar");
}

#[test]
fn a_closed_tab_comes_back_with_ctrl_shift_t() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click("☕ Main.java");
    app.close_tab(0);
    assert!(app.open_tab_names().is_empty());

    app.press(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::T);

    assert_eq!(app.open_tab_names(), ["Main.java"]);
    assert_eq!(app.active_tab_name().as_deref(), Some("Main.java"));
}

#[test]
fn opening_the_same_file_twice_focuses_the_tab_it_already_has() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);

    app.click("☕ Main.java");
    app.click("🔷 Other.kt");
    app.click("☕ Main.java");

    assert_eq!(app.open_tab_names(), ["Main.java", "Other.kt"], "no duplicate tab");
    assert_eq!(
        app.active_tab_name().as_deref(),
        Some("Main.java"),
        "the existing tab is focused"
    );
}

#[test]
fn middle_clicking_a_tab_closes_it() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);
    app.click("☕ Main.java");
    app.click("🔷 Other.kt");

    app.click_middle("Main.java");

    assert_eq!(app.open_tab_names(), ["Other.kt"]);
}

#[test]
fn a_tabs_context_menu_toggles_read_only() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click("☕ Main.java");

    app.click_secondary("Main.java");
    app.click(t().menu.read_only);
    assert!(app.shows("🔒Main.java"));

    app.click_secondary("🔒Main.java");
    app.click(t().tabs.allow_editing);
    assert!(app.shows("Main.java"), "and back again");
    assert!(!app.shows("🔒Main.java"));
}

#[test]
fn opening_a_binary_file_reports_an_error_instead_of_a_tab() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    // Invalid UTF-8, which is exactly what `Document::open` refuses.
    app.write_bytes_and_rescan("blob.bin", &[0x00, 0xff, 0xfe, 0x00]);

    app.click("📄 blob.bin");

    // The whole message, not a fragment of it: the path it names is
    // knowable here, and asserting on the fragment alone would keep
    // passing if the refusal ever started naming the wrong file.
    let refusal = msg::couldnt_open_not_text(&app.path("blob.bin").display().to_string());
    assert!(app.shows(&refusal), "a binary file must report why it won't open");
    assert!(app.open_tab_names().is_empty(), "and open no tab");
    app.click(t().common.ok);
    assert!(!app.shows(&refusal));
}
