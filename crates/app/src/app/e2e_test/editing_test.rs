//! Editing a file: the dirty asterisk, saving, and the "you have unsaved
//! changes" prompt every way it can be answered.

use super::common_test::{E2e, MAIN_JAVA};
use fg_i18n::{msg, t};

#[test]
fn typing_marks_the_tab_dirty_and_saving_clears_it() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");

    app.type_into_active_tab(MAIN_JAVA.chars().count(), "// tail");

    assert!(app.shows(&E2e::dirty_row("Main.java")), "an edited tab must show the dirty asterisk");
    assert_eq!(
        app.on_disk("Main.java"),
        MAIN_JAVA,
        "typing alone must not write to disk"
    );

    app.press(egui::Modifiers::COMMAND, egui::Key::S);

    assert!(!app.shows(&E2e::dirty_row("Main.java")), "saving must clear the dirty asterisk");
    assert!(app.shows(&E2e::row("Main.java")));
    assert_eq!(app.on_disk("Main.java"), format!("{MAIN_JAVA}// tail"));
}

#[test]
fn closing_a_dirty_tab_asks_first_and_save_writes_the_file() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");
    app.type_into_active_tab(MAIN_JAVA.chars().count(), "// edited");

    app.close_tab(0);

    assert!(
        app.shows(&msg::save_changes_before_closing("Main.java")),
        "closing a dirty tab must ask before dropping the edit"
    );
    assert_eq!(
        app.open_tab_names(),
        ["Main.java"],
        "the tab stays open until the prompt is answered"
    );

    app.click(t().common.save);

    assert!(
        app.open_tab_names().is_empty(),
        "answering Save must also close the tab"
    );
    assert_eq!(app.on_disk("Main.java"), format!("{MAIN_JAVA}// edited"));
}

#[test]
fn discarding_a_dirty_tab_closes_it_and_leaves_the_file_alone() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");
    app.type_into_active_tab(MAIN_JAVA.chars().count(), "// edited");

    app.close_tab(0);
    app.click(t().tabs.discard);

    assert!(app.open_tab_names().is_empty());
    assert_eq!(app.on_disk("Main.java"), MAIN_JAVA, "Discard must not write the edit");
}

#[test]
fn cancelling_the_close_prompt_keeps_the_dirty_tab_open() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");
    app.type_into_active_tab(MAIN_JAVA.chars().count(), "// edited");

    app.close_tab(0);
    app.click(t().common.cancel);

    assert_eq!(app.open_tab_names(), ["Main.java"]);
    assert!(app.shows(&E2e::dirty_row("Main.java")), "the tab is still open and still dirty");
    assert!(
        !app.shows(&msg::save_changes_before_closing("Main.java")),
        "the prompt must be gone"
    );
}

#[test]
fn saving_trims_trailing_whitespace() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");

    app.type_into_active_tab("class Main {".chars().count(), "   ");
    app.press(egui::Modifiers::COMMAND, egui::Key::S);

    assert_eq!(
        app.on_disk("Main.java"),
        MAIN_JAVA,
        "trailing spaces must not survive a save"
    );
}

#[test]
fn a_kotlin_file_edits_and_saves_the_same_way_a_java_one_does() {
    let mut app = E2e::launch(&[("Other.kt", "class Other\n")]);
    app.click_tree("Other.kt");

    app.type_into_active_tab("class Other\n".chars().count(), "// kotlin tail");
    assert!(app.shows(&E2e::dirty_row("Other.kt")));

    app.press(egui::Modifiers::COMMAND, egui::Key::S);

    assert_eq!(app.on_disk("Other.kt"), "class Other\n// kotlin tail");
    assert!(!app.shows(&E2e::dirty_row("Other.kt")));
}

#[test]
fn each_tab_keeps_its_own_dirty_state() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);
    app.click_tree("Main.java");
    app.click_tree("Other.kt");

    app.type_into_active_tab("class Other\n".chars().count(), "// edited");

    assert!(app.shows(&E2e::dirty_row("Other.kt")), "the edited tab is dirty");
    assert!(app.shows(&E2e::row("Main.java")), "and the untouched one is not");
    assert!(!app.shows(&E2e::dirty_row("Main.java")));
}

#[test]
fn saving_with_no_tab_open_does_nothing_and_reports_nothing() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.press(egui::Modifiers::COMMAND, egui::Key::S);

    assert!(app.shows(t().welcome.tagline), "still nothing open");
    assert_eq!(app.on_disk("Main.java"), MAIN_JAVA);
}

#[test]
fn undoing_every_edit_before_closing_skips_the_prompt_entirely() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");
    app.type_into_active_tab(MAIN_JAVA.chars().count(), "X");
    app.press(egui::Modifiers::COMMAND, egui::Key::Z);

    app.close_tab(0);

    assert!(app.open_tab_names().is_empty(), "a clean tab closes without asking");
    assert!(!app.shows(&msg::save_changes_before_closing("Main.java")));
}
