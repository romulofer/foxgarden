//! Keyboard editing inside the editor pane: the shortcuts `Help > About`
//! advertises, plus the edits that happen on their own (auto-close, auto-
//! indent) as a character is typed.

use super::common::{E2e, MAIN_JAVA};

/// A two-statement method body, for the line-level operations (comment,
/// join, move, duplicate) that need more than one line to be interesting.
const BODY_JAVA: &str = "class Body {\n    int a = 1;\n    int b = 2;\n}\n";

/// Char offset of the `int a = 1;` line's first character.
fn line_two_start() -> usize {
    "class Body {\n".chars().count()
}

#[test]
fn ctrl_z_undoes_the_last_edit_and_makes_the_tab_clean_again() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click("☕ Main.java");
    app.type_into_active_tab(MAIN_JAVA.chars().count(), "X");
    assert!(app.shows("*Main.java"));

    app.press(egui::Modifiers::COMMAND, egui::Key::Z);

    assert_eq!(app.active_tab_text(), MAIN_JAVA, "undo must restore the original text");
    assert!(
        !app.shows("*Main.java"),
        "a document back at its saved contents is clean again"
    );
}

#[test]
fn ctrl_slash_comments_the_current_line_and_uncomments_it_again() {
    let mut app = E2e::launch(&[("Body.java", BODY_JAVA)]);
    app.click("☕ Body.java");
    app.type_into_active_tab(line_two_start(), "");

    app.press(egui::Modifiers::COMMAND, egui::Key::Slash);
    assert_eq!(
        app.active_tab_text(),
        "class Body {\n//     int a = 1;\n    int b = 2;\n}\n"
    );

    app.press(egui::Modifiers::COMMAND, egui::Key::Slash);
    assert_eq!(app.active_tab_text(), BODY_JAVA, "the same shortcut toggles back");
}

#[test]
fn ctrl_j_joins_the_current_line_with_the_next() {
    let mut app = E2e::launch(&[("Body.java", BODY_JAVA)]);
    app.click("☕ Body.java");
    app.type_into_active_tab(line_two_start(), "");

    app.press(egui::Modifiers::COMMAND, egui::Key::J);

    assert_eq!(app.active_tab_text(), "class Body {\n    int a = 1; int b = 2;\n}\n");
}

#[test]
fn alt_down_moves_the_current_line_past_the_next_one() {
    let mut app = E2e::launch(&[("Body.java", BODY_JAVA)]);
    app.click("☕ Body.java");
    app.type_into_active_tab(line_two_start(), "");

    app.press(egui::Modifiers::ALT, egui::Key::ArrowDown);

    assert_eq!(
        app.active_tab_text(),
        "class Body {\n    int b = 2;\n    int a = 1;\n}\n"
    );
}

#[test]
fn alt_shift_down_duplicates_the_current_line() {
    let mut app = E2e::launch(&[("Body.java", BODY_JAVA)]);
    app.click("☕ Body.java");
    app.type_into_active_tab(line_two_start(), "");

    app.press(egui::Modifiers::ALT | egui::Modifiers::SHIFT, egui::Key::ArrowDown);

    assert_eq!(
        app.active_tab_text(),
        "class Body {\n    int a = 1;\n    int a = 1;\n    int b = 2;\n}\n"
    );
}

#[test]
fn ctrl_shift_u_uppercases_the_selection_without_the_menu() {
    let mut app = E2e::launch(&[("Main.java", "class Main {\n  int value;\n}\n")]);
    app.click("☕ Main.java");

    let offset = "class Main {\n  int ".chars().count();
    app.select_in_active_tab(offset, "value".chars().count());
    app.press(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::U);

    assert_eq!(app.active_tab_text(), "class Main {\n  int VALUE;\n}\n");
}

#[test]
fn typing_an_opening_bracket_closes_it_automatically() {
    let mut app = E2e::launch(&[("Main.java", "class Main {\n}\n")]);
    app.click("☕ Main.java");

    app.type_into_active_tab("class Main {\n".chars().count(), "    foo(");

    assert_eq!(
        app.active_tab_text(),
        "class Main {\n    foo()}\n",
        "the closing bracket must be inserted for free"
    );
}

#[test]
fn a_live_template_expands_on_tab() {
    let mut app = E2e::launch(&[("Main.java", "class Main {\n}\n")]);
    app.click("☕ Main.java");

    app.type_into_active_tab("class Main {\n".chars().count(), "sout");
    app.press(egui::Modifiers::NONE, egui::Key::Tab);

    assert!(
        app.active_tab_text().contains("System.out.println("),
        "`sout` + Tab must expand: {}",
        app.active_tab_text()
    );
}

#[test]
fn a_read_only_tab_ignores_the_line_operations_too() {
    let mut app = E2e::launch(&[("Body.java", BODY_JAVA)]);
    app.click("☕ Body.java");
    app.menu("Tools", "Read-Only");

    app.type_into_active_tab(line_two_start(), "");
    app.press(egui::Modifiers::COMMAND, egui::Key::Slash);
    app.press(egui::Modifiers::ALT, egui::Key::ArrowDown);

    assert_eq!(
        app.active_tab_text(),
        BODY_JAVA,
        "read-only blocks keyboard edits, not just typing"
    );
}

#[test]
fn ctrl_d_adds_a_cursor_at_the_next_occurrence_and_types_into_both() {
    let mut app = E2e::launch(&[("Dup.java", "class Dup {\n    int x;\n    int x;\n}\n")]);
    app.click("☕ Dup.java");

    // Select the first `x`, then Ctrl+D to add the second one.
    let first_x = "class Dup {\n    int ".chars().count();
    app.select_in_active_tab(first_x, 1);
    app.press(egui::Modifiers::COMMAND, egui::Key::D);
    app.type_text("y");

    assert_eq!(
        app.active_tab_text(),
        "class Dup {\n    int y;\n    int y;\n}\n",
        "both cursors must take the edit"
    );
}

#[test]
fn tab_at_the_start_of_a_line_indents_by_the_configured_width() {
    let mut app = E2e::launch(&[("Body.java", "class Body {\nint a = 1;\n}\n")]);
    app.click("☕ Body.java");

    app.type_into_active_tab("class Body {\n".chars().count(), "");
    app.press(egui::Modifiers::NONE, egui::Key::Tab);

    assert_eq!(
        app.active_tab_text(),
        "class Body {\n    int a = 1;\n}\n",
        "the default indent is four spaces, not a tab character"
    );
}

#[test]
fn shift_tab_dedents_the_line_again() {
    let mut app = E2e::launch(&[("Body.java", "class Body {\n        int a = 1;\n}\n")]);
    app.click("☕ Body.java");

    app.type_into_active_tab("class Body {\n".chars().count(), "");
    app.press(egui::Modifiers::SHIFT, egui::Key::Tab);

    assert_eq!(app.active_tab_text(), "class Body {\n    int a = 1;\n}\n");
}

#[test]
fn enter_inside_a_block_keeps_the_current_indentation() {
    let mut app = E2e::launch(&[("Body.java", "class Body {\n    int a = 1;\n}\n")]);
    app.click("☕ Body.java");

    let end_of_statement = "class Body {\n    int a = 1;".chars().count();
    app.type_into_active_tab(end_of_statement, "");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);
    app.type_text("int b = 2;");

    assert_eq!(
        app.active_tab_text(),
        "class Body {\n    int a = 1;\n    int b = 2;\n}\n",
        "the new line must start at the previous line's indentation"
    );
}
