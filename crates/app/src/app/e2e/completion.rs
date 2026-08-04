//! The completion popup: what opens it, what it offers, and what accepting
//! (or dismissing) one does to the buffer.

use super::common::E2e;

/// A class with one long, distinctive identifier already in it — the word
/// completion candidate every test here types a prefix of.
const WORDS_JAVA: &str = "class Words {\n    int inventoryCount = 0;\n}\n";

/// Char offset of the empty line the tests type on, just after the field.
fn body_offset() -> usize {
    "class Words {\n    int inventoryCount = 0;\n".chars().count()
}

#[test]
fn typing_a_prefix_offers_a_word_already_in_the_file() {
    let mut app = E2e::launch(&[("Words.java", WORDS_JAVA)]);
    app.click("☕ Words.java");

    app.type_into_active_tab(body_offset(), "    invent");

    assert!(
        app.shows_containing("inventoryCount"),
        "an identifier already in the buffer must be offered as a completion"
    );
}

#[test]
fn accepting_a_completion_inserts_the_whole_word() {
    let mut app = E2e::launch(&[("Words.java", WORDS_JAVA)]);
    app.click("☕ Words.java");

    app.type_into_active_tab(body_offset(), "    invent");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert!(
        app.active_tab_text().contains("    inventoryCount"),
        "Enter must complete the prefix, not just insert a newline: {}",
        app.active_tab_text()
    );
    assert!(!app.shows_containing("inventoryCount\n"), "and close the popup");
}

#[test]
fn escape_dismisses_the_popup_and_leaves_what_was_typed() {
    let mut app = E2e::launch(&[("Words.java", WORDS_JAVA)]);
    app.click("☕ Words.java");

    app.type_into_active_tab(body_offset(), "    invent");
    app.press(egui::Modifiers::NONE, egui::Key::Escape);

    assert_eq!(
        app.active_tab_text(),
        "class Words {\n    int inventoryCount = 0;\n    invent}\n",
        "dismissing must leave the prefix exactly as typed, uncompleted"
    );
    assert!(
        !app.shows_containing("inventoryCount = 0"),
        "and the popup must be gone"
    );
}

#[test]
fn ctrl_space_opens_the_popup_with_no_prefix_typed() {
    let mut app = E2e::launch(&[("Words.java", WORDS_JAVA)]);
    app.click("☕ Words.java");

    app.type_into_active_tab(body_offset(), "    ");
    app.press(egui::Modifiers::COMMAND, egui::Key::Space);

    assert!(
        app.shows_containing("inventoryCount"),
        "Ctrl+Space must offer the file's identifiers with nothing typed yet"
    );
}

#[test]
fn a_java_keyword_is_offered_too() {
    let mut app = E2e::launch(&[("Words.java", WORDS_JAVA)]);
    app.click("☕ Words.java");

    app.type_into_active_tab(body_offset(), "    priv");

    assert!(
        app.shows_containing("private"),
        "keywords are candidates alongside identifiers"
    );
}
