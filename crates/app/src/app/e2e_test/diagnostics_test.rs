//! Error signals: what the editor has to squiggle, on open and while typing.

use super::common_test::{E2e, MAIN_JAVA};

#[test]
fn a_file_with_a_syntax_error_reports_a_diagnostic_when_opened() {
    let mut app = E2e::launch(&[("Broken.java", "class Broken {\n"), ("Main.java", MAIN_JAVA)]);

    app.click("☕ Main.java");
    assert_eq!(
        app.active_tab_diagnostics(),
        0,
        "a well-formed file has nothing to squiggle"
    );

    app.click("☕ Broken.java");

    assert!(
        app.active_tab_diagnostics() > 0,
        "an unclosed class body must produce the error a squiggle is drawn from"
    );
}

#[test]
fn typing_a_syntax_error_makes_a_diagnostic_appear_and_fixing_it_clears_it() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click("☕ Main.java");
    assert_eq!(app.active_tab_diagnostics(), 0);

    // Straight after `class`, so what follows is no longer a valid class
    // declaration.
    app.type_into_active_tab("class".chars().count(), " (");
    assert!(
        app.active_tab_diagnostics() > 0,
        "a broken edit must show up as a diagnostic"
    );

    app.press(egui::Modifiers::NONE, egui::Key::Backspace);
    app.press(egui::Modifiers::NONE, egui::Key::Backspace);

    assert_eq!(
        app.active_tab_diagnostics(),
        0,
        "undoing the breakage must clear the diagnostic"
    );
}

#[test]
fn a_kotlin_file_reports_its_own_syntax_errors() {
    let mut app = E2e::launch(&[("Broken.kt", "class Broken {\n"), ("Fine.kt", "class Fine\n")]);

    app.click("🔷 Fine.kt");
    assert_eq!(app.active_tab_diagnostics(), 0);

    app.click("🔷 Broken.kt");

    assert!(
        app.active_tab_diagnostics() > 0,
        "Kotlin gets the same error signals Java does"
    );
}

#[test]
fn a_file_with_no_recognized_language_never_reports_diagnostics() {
    let mut app = E2e::launch(&[("notes.txt", "this is (((( not code\n")]);

    app.click("📄 notes.txt");

    assert_eq!(
        app.active_tab_diagnostics(),
        0,
        "a plain text file has no parser, so nothing to squiggle"
    );
    assert_eq!(app.open_tab_names(), ["notes.txt"], "and still opens normally");
}

#[test]
fn fixing_a_broken_file_and_saving_clears_its_diagnostics() {
    let mut app = E2e::launch(&[("Broken.java", "class Broken {\n")]);
    app.click("☕ Broken.java");
    assert!(app.active_tab_diagnostics() > 0);

    app.type_into_active_tab("class Broken {\n".chars().count(), "}");
    app.press(egui::Modifiers::COMMAND, egui::Key::S);

    assert_eq!(
        app.active_tab_diagnostics(),
        0,
        "the reparse after a save must pick the fix up"
    );
    assert_eq!(app.on_disk("Broken.java"), "class Broken {\n}");
}
