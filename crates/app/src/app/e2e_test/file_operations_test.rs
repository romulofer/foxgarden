//! The project tree's per-row context menu: rename, delete, copy/cut/paste,
//! New File in a specific directory, and the multi-select gestures those
//! operations respect.

use super::common_test::{E2e, MAIN_JAVA};
use fg_i18n::{msg, t};

#[test]
fn renaming_a_file_from_the_tree_renames_it_on_disk_and_repoints_its_tab() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");

    app.click_tree_secondary("Main.java");
    app.click(t().common.rename);
    app.replace_focused_text("Renamed.java");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert!(app.path("Renamed.java").exists(), "the file must be renamed on disk");
    assert!(!app.path("Main.java").exists());
    assert_eq!(
        app.open_tab_names(),
        ["Renamed.java"],
        "and the open tab must follow it"
    );
    assert!(app.shows(&E2e::row("Renamed.java")), "and so must the tree row");
}

#[test]
fn a_renamed_open_file_is_still_saveable_under_its_new_name() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");

    app.click_tree_secondary("Main.java");
    app.click(t().common.rename);
    app.replace_focused_text("Renamed.java");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    app.type_into_active_tab(MAIN_JAVA.chars().count(), "// after rename");
    app.press(egui::Modifiers::COMMAND, egui::Key::S);

    assert_eq!(app.on_disk("Renamed.java"), format!("{MAIN_JAVA}// after rename"));
}

#[test]
fn deleting_a_file_asks_first_then_removes_it_and_closes_its_tab() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);
    app.click_tree("Main.java");

    app.click_tree_secondary("Main.java");
    app.click(t().common.delete);
    assert!(
        app.shows(&msg::confirm_delete_file("Main.java")),
        "a delete must be confirmed before it happens"
    );

    app.click(t().common.delete);

    assert!(!app.path("Main.java").exists(), "confirming deletes the file");
    assert!(
        app.open_tab_names().is_empty(),
        "and closes the tab that was showing it"
    );
    assert!(!app.shows(&E2e::row("Main.java")));
    assert!(app.shows(&E2e::row("Other.kt")), "its siblings stay put");
}

#[test]
fn cancelling_a_delete_leaves_the_file_alone() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.click_tree_secondary("Main.java");
    app.click(t().common.delete);
    app.click(t().common.cancel);

    assert!(app.path("Main.java").exists());
    assert!(app.shows(&E2e::row("Main.java")));
}

#[test]
fn copying_a_file_and_pasting_into_a_directory_duplicates_it() {
    let mut app = E2e::launch(&[
        ("Main.java", MAIN_JAVA),
        ("sub/Placeholder.java", "class Placeholder {}\n"),
    ]);

    app.click_tree_secondary("Main.java");
    app.click(t().common.copy);
    app.click_secondary_containing("sub");
    app.click(t().common.paste);

    assert!(
        app.path("Main.java").exists(),
        "a copy leaves the original where it was"
    );
    assert_eq!(
        std::fs::read_to_string(app.path("sub/Main.java")).expect("pasted copy"),
        MAIN_JAVA
    );
}

#[test]
fn cutting_a_file_and_pasting_into_a_directory_moves_it() {
    let mut app = E2e::launch(&[
        ("Main.java", MAIN_JAVA),
        ("sub/Placeholder.java", "class Placeholder {}\n"),
    ]);

    app.click_tree_secondary("Main.java");
    app.click(t().common.cut);
    app.click_secondary_containing("sub");
    app.click(t().common.paste);

    assert!(!app.path("Main.java").exists(), "a cut removes the original");
    assert_eq!(
        std::fs::read_to_string(app.path("sub/Main.java")).expect("moved file"),
        MAIN_JAVA
    );
}

#[test]
fn new_file_from_a_directory_row_creates_it_inside_that_directory() {
    // `Main.java` at the root keeps the root row from collapsing into a
    // single `root/sub` row (`collapse_chain`), so `sub` has a row of its
    // own to right-click.
    let mut app = E2e::launch(&[
        ("Main.java", MAIN_JAVA),
        ("sub/Placeholder.java", "class Placeholder {}\n"),
    ]);

    app.click_containing("sub");
    app.click_secondary_containing("sub");
    app.click(t().common.new_file);
    app.type_text("Inner.java");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert!(
        app.path("sub/Inner.java").exists(),
        "the new file lands in the clicked directory"
    );
    assert_eq!(app.open_tab_names(), ["Inner.java"]);
}

#[test]
fn a_created_java_file_starts_from_boilerplate() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.click(&crate::style::icons::NEW_FILE.to_string());
    app.type_text("Fresh.java");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert!(
        app.on_disk("Fresh.java").contains("class Fresh"),
        "a new Java file is generated with its class declaration: {}",
        app.on_disk("Fresh.java")
    );
}

#[test]
fn ctrl_n_opens_the_new_file_row() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.press(egui::Modifiers::COMMAND, egui::Key::N);
    app.type_text("Shortcut.java");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert!(app.path("Shortcut.java").exists());
}

#[test]
fn ctrl_clicking_two_rows_deletes_both_in_one_confirmation() {
    let mut app = E2e::launch(&[("A.java", "class A {}\n"), ("B.java", "class B {}\n")]);

    app.click_modifiers(&E2e::row("A.java"), egui::Modifiers::COMMAND);
    app.click_modifiers(&E2e::row("B.java"), egui::Modifiers::COMMAND);
    assert!(app.open_tab_names().is_empty(), "a Ctrl+click selects without opening");

    app.click_tree_secondary("B.java");
    app.click(t().common.delete);
    assert!(app.shows(&msg::confirm_delete_many(2)), "one prompt for the whole set");

    app.click(t().common.delete);

    assert!(!app.path("A.java").exists());
    assert!(!app.path("B.java").exists());
}

#[test]
fn escape_cancels_a_rename_without_touching_the_file() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.click_tree_secondary("Main.java");
    app.click(t().common.rename);
    app.replace_focused_text("Nope.java");
    app.press(egui::Modifiers::NONE, egui::Key::Escape);

    assert!(app.path("Main.java").exists());
    assert!(!app.path("Nope.java").exists());
    assert!(app.shows(&E2e::row("Main.java")));
}
