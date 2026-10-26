//! The menu bar: File, Settings, Tools, Run, View, Help — every item that
//! doesn't need an OS dialog (File > Open Folder…) or an external process
//! (Tools > Run Checkstyle/PMD) behind it.

use super::common_test::{E2e, MAIN_JAVA};
use crate::panels::bottom_dock::BottomTab;
use fg_i18n::t;

#[test]
fn file_save_writes_the_active_tab() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");
    app.type_into_active_tab(MAIN_JAVA.chars().count(), "// via the menu");

    app.menu(t().menu.file, t().common.save);

    assert_eq!(app.on_disk("Main.java"), format!("{MAIN_JAVA}// via the menu"));
    assert!(!app.shows(&E2e::dirty_row("Main.java")));
}

#[test]
fn file_close_tab_closes_the_active_one() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA), ("Other.kt", "class Other\n")]);
    app.click_tree("Main.java");
    app.click_tree("Other.kt");

    app.menu(t().menu.file, t().menu.close_tab);

    assert_eq!(app.open_tab_names(), ["Main.java"]);
}

#[test]
fn file_reopen_closed_tab_brings_the_last_one_back() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");
    app.menu(t().menu.file, t().menu.close_tab);
    assert!(app.open_tab_names().is_empty());

    app.menu(t().menu.file, t().menu.reopen_closed_tab);

    assert_eq!(app.open_tab_names(), ["Main.java"]);
}

#[test]
fn file_new_file_opens_the_side_panel_row_and_creates_the_file() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.file, t().menu.new_file);
    app.type_text("FromMenu.java");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert!(app.path("FromMenu.java").exists());
    assert_eq!(app.open_tab_names(), ["FromMenu.java"]);
}

/// Regression: an earlier version of `panels::new_project::show` called
/// `show_modal` for its side effects only and never looked at the
/// `(result, escape_pressed)` it returns, so Cancel's own `clicked()` was
/// computed and then silently discarded — clicking Cancel did nothing at
/// all, the dialog only ever closed via a successful Create.
#[test]
fn file_new_project_dialog_opens_and_cancel_closes_it() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.file, t().menu.new_project);
    assert!(
        app.shows(t().new_project.heading),
        "the wizard must open, with its heading translated"
    );

    app.click(t().common.cancel);
    assert!(!app.shows(t().new_project.heading), "Cancel must dismiss the dialog");
}

/// A build tool/language pair that can start more than one way (Gradle +
/// Kotlin: ordinary, or multiplatform) gets a template picker; one that
/// cannot (Maven + Java) does not.
#[test]
fn file_new_project_dialog_offers_the_multiplatform_template_for_gradle_kotlin_only() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.menu(t().menu.file, t().menu.new_project);
    assert!(!app.shows(t().new_project.template), "Maven + Java has a single starting point");

    app.open_combo_showing("Maven");
    app.click("Gradle");
    assert!(!app.shows(t().new_project.template), "Gradle + Java has a single starting point");

    app.open_combo_showing("Java");
    app.click("Kotlin");
    assert!(app.shows(t().new_project.template));
    assert!(app.combo_shows(t().new_project.standard_template), "the ordinary project is the default");

    app.open_combo_showing(t().new_project.standard_template);
    app.click("Kotlin Multiplatform");
    assert!(app.combo_shows("Kotlin Multiplatform"));
    assert!(!app.combo_shows(t().new_project.standard_template));
}

/// Same regression as the Cancel test above, via the keyboard rather than a
/// click — `show_modal`'s `escape_pressed` was equally unused before the
/// fix.
#[test]
fn file_new_project_dialog_closes_on_escape() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.file, t().menu.new_project);
    assert!(app.shows(t().new_project.heading));

    app.press(egui::Modifiers::NONE, egui::Key::Escape);
    assert!(!app.shows(t().new_project.heading));
}

#[test]
fn settings_theme_switches_between_light_and_dark() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(app.is_dark_mode(), "dark is the default theme");

    app.menu(t().menu.settings, t().menu.theme);
    app.click(t().menu.theme_light);
    assert!(!app.is_dark_mode(), "Settings > Theme > Light must apply immediately");

    app.menu(t().menu.settings, t().menu.theme);
    app.click(t().menu.theme_dark);
    assert!(app.is_dark_mode());
}

#[test]
fn settings_font_dialog_opens_and_closes() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.settings, t().menu.font);
    assert!(
        app.shows(t().dialogs.font_size),
        "the font dialog must show its size control"
    );

    app.click(t().common.close);
    assert!(!app.shows(t().dialogs.font_size), "Close must dismiss the dialog");
}

#[test]
fn settings_auto_save_can_be_enabled_from_the_menu() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    assert!(!app.app().auto_save_settings.enabled, "auto-save is off by default");

    app.menu(t().menu.settings, t().menu.auto_save);
    app.click(t().menu.auto_save_enabled);

    assert!(app.app().auto_save_settings.enabled);
}

/// Deliberately stops at listing the languages rather than switching to
/// one: the active language is process-global, so a test that flipped it to
/// en-US would flip it under every other test running in parallel, all of
/// which assert on pt-BR labels. That the switch itself works is covered by
/// `fg_i18n`'s own `t_follows_set_lang`, in its own test binary.
#[test]
fn settings_language_lists_every_language_under_its_own_name() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.settings, t().menu.language);

    for language in fg_i18n::Lang::ALL {
        assert!(
            app.shows(language.autonym()),
            "{} must be offered, spelled the way its own speakers spell it",
            language.tag()
        );
    }
}

#[test]
fn settings_external_tools_dialog_opens() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.settings, t().menu.external_tools);

    assert!(
        app.shows_containing("Checkstyle"),
        "the External Tools dialog covers Checkstyle"
    );
    app.click(t().common.close);
    assert!(!app.shows_containing("Checkstyle"));
}

#[test]
fn settings_language_servers_dialog_opens() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.settings, t().menu.language_servers);

    assert!(app.shows_containing("jdtls") || app.shows_containing("Java"));
    app.click(t().common.close);
}

/// A server the user installs themselves still gets a row: its name, and
/// where it was found or that it was not.
#[test]
fn settings_language_servers_dialog_lists_the_python_server_found_rather_than_installed() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.settings, t().menu.language_servers);

    assert!(app.shows("Jedi Language Server"));
    assert!(
        app.shows_containing("Found:") || app.shows_containing("Not found:") || app.shows_containing("Encontrado:") || app.shows_containing("Não encontrado:"),
        "it says whether jedi-language-server was found"
    );
    app.click(t().common.close);
}

#[test]
fn view_zen_mode_hides_the_menu_bar_and_f11_brings_it_back() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.view, t().menu.zen_mode);
    assert!(!app.shows(t().menu.file), "zen mode hides the menu bar");
    assert!(!app.shows(&E2e::row("Main.java")), "and the side panel with it");

    app.press(egui::Modifiers::NONE, egui::Key::F11);

    assert!(
        app.shows(t().menu.file),
        "F11 must get out of zen mode even with no menu to click"
    );
    assert!(app.shows(&E2e::row("Main.java")));
}

#[test]
fn view_side_panel_checkbox_hides_the_project_tree() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.view, t().menu.side_panel);

    assert!(!app.shows(&E2e::row("Main.java")));
    assert!(app.shows(t().menu.file), "only the side panel goes, unlike zen mode");
}

#[test]
fn view_word_wrap_toggles_the_setting() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    let was_wrapped = app.app().view_settings.word_wrap;

    app.menu(t().menu.view, t().menu.word_wrap);

    assert_ne!(app.app().view_settings.word_wrap, was_wrapped);
}

#[test]
fn run_edit_configurations_opens_its_dialog() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.run, t().menu.edit_configurations);

    assert!(app.shows(t().run_configs.heading));
}

#[test]
fn help_about_lists_the_shortcuts() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.help, t().menu.about);

    assert!(app.shows(t().about.shortcuts[0]), "About documents the shortcuts");
    app.click(t().common.close);
    assert!(!app.shows(t().about.shortcuts[0]));
}

#[test]
fn extensions_manage_lists_the_shipped_extensions_and_flags_a_restart() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.extensions, t().menu.manage_extensions);

    assert!(app.shows(t().extensions.intro));
    assert!(app.shows_containing("Common file types"));
    assert!(!app.shows(t().extensions.restart_required));

    let file_types = fg_languages::shipped_extensions()
        .into_iter()
        .find(|e| e.manifest.id == "file-types")
        .expect("file-types ships with the build");
    app.click_checkbox(&format!("{} {}", file_types.manifest.name, file_types.manifest.version));

    assert!(app.shows(t().extensions.restart_required));
    assert_eq!(app.app().extensions_ui.disabled(), ["file-types"]);
    app.click(t().common.close);
    assert!(!app.shows(t().extensions.intro));
}

#[test]
fn settings_lists_the_runtime_kind_an_extension_contributes_and_opens_its_dialog() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.settings, "JDKs…");

    assert!(app.shows(t().runtimes.none_registered));
    assert!(app.shows(&fg_i18n::msg::add_runtime("JDK")));
    assert!(app.shows(t().runtimes.auto_detect));
    app.click(t().common.close);
    assert!(!app.shows(t().runtimes.auto_detect));
}

#[test]
fn help_live_templates_dialog_opens() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);

    app.menu(t().menu.help, t().menu.live_templates);

    assert!(
        app.shows_containing(t().dialogs.live_templates_trigger_hint),
        "the templates dialog explains triggers"
    );
    app.click(t().common.close);
}

#[test]
fn tools_read_only_marks_the_tab_and_blocks_typing() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");

    app.menu(t().menu.tools, t().menu.read_only);
    assert!(
        app.shows(&E2e::read_only_row("Main.java")),
        "a read-only tab is marked with a lock"
    );

    app.type_into_active_tab(MAIN_JAVA.chars().count(), "// nope");

    assert_eq!(app.active_tab_text(), MAIN_JAVA, "a read-only tab must ignore typing");
    assert!(
        !app.shows(&E2e::dirty_read_only_row("Main.java")),
        "and so must stay clean"
    );
}

#[test]
fn tools_convert_to_uppercase_converts_the_selection() {
    let mut app = E2e::launch(&[("Main.java", "class Main {\n  int value;\n}\n")]);
    app.click_tree("Main.java");

    // The `value` identifier on line 2.
    let offset = "class Main {\n  int ".chars().count();
    app.select_in_active_tab(offset, "value".chars().count());
    app.menu(t().menu.tools, t().menu.convert_to_uppercase);

    assert_eq!(app.active_tab_text(), "class Main {\n  int VALUE;\n}\n");
}

#[test]
fn tools_case_conversion_with_no_selection_reports_an_error() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");
    app.type_into_active_tab(0, "");

    app.menu(t().menu.tools, t().menu.convert_to_lowercase);

    assert!(app.shows(t().errors.select_text_first));
    app.dismiss_toasts();
    assert!(!app.shows(t().errors.select_text_first), "dismissing clears it");
}

#[test]
fn tools_sort_lines_sorts_the_selected_lines() {
    let mut app = E2e::launch(&[("Notes.java", "// c\n// a\n// b\n")]);
    app.click_tree("Notes.java");

    app.select_in_active_tab(0, "// c\n// a\n// b".chars().count());
    app.menu(t().menu.tools, t().menu.sort_lines);

    assert_eq!(app.active_tab_text(), "// a\n// b\n// c\n");
}

#[test]
fn tools_unique_lines_drops_the_duplicates() {
    let mut app = E2e::launch(&[("Notes.java", "// a\n// a\n// b\n")]);
    app.click_tree("Notes.java");

    app.select_in_active_tab(0, "// a\n// a\n// b".chars().count());
    app.menu(t().menu.tools, t().menu.unique_lines);

    assert_eq!(app.active_tab_text(), "// a\n// b\n");
}

#[test]
fn tools_generate_getters_inserts_one_for_the_only_class() {
    let mut app = E2e::launch(&[("Person.java", "class Person {\n    private String name;\n}\n")]);
    app.click_tree("Person.java");

    app.menu(t().menu.tools, t().menu.generate_getters);

    let text = app.active_tab_text();
    assert!(
        text.contains("public String getName()"),
        "a single eligible class generates straight away, no dialog: {text}"
    );
}

#[test]
fn tools_generate_doc_comment_documents_the_method_at_the_caret() {
    let source = "class Calc {\n    int add(int a, int b) {\n        return a + b;\n    }\n}\n";
    let mut app = E2e::launch(&[("Calc.java", source)]);
    app.click_tree("Calc.java");

    // The caret inside the method body.
    app.select_in_active_tab(source.find("return").unwrap(), 0);
    app.menu(t().menu.tools, t().menu.generate_doc_comment);

    assert_eq!(
        app.active_tab_text(),
        "class Calc {\n    /**\n     * \n     *\n     * @param a\n     * @param b\n     * @return\n     */\n    int add(int a, int b) {\n        return a + b;\n    }\n}\n"
    );
}

#[test]
fn tools_generate_doc_comment_says_so_when_the_declaration_is_already_documented() {
    let source = "class Calc {\n    /** Adds. */\n    int add(int a) {\n        return a;\n    }\n}\n";
    let mut app = E2e::launch(&[("Calc.java", source)]);
    app.click_tree("Calc.java");

    app.select_in_active_tab(source.find("return").unwrap(), 0);
    app.menu(t().menu.tools, t().menu.generate_doc_comment);

    assert_eq!(app.active_tab_text(), source, "nothing is inserted");
    assert!(app.shows_containing(t().errors.no_doc_target));
}

#[test]
fn tools_generate_doc_comment_writes_a_kdoc_for_a_kotlin_function() {
    let source = "class Calc {\n    fun add(a: Int, b: Int): Int {\n        return a + b\n    }\n}\n";
    let mut app = E2e::launch(&[("Calc.kt", source)]);
    app.click_tree("Calc.kt");

    app.select_in_active_tab(source.find("return").unwrap(), 0);
    app.menu(t().menu.tools, t().menu.generate_doc_comment);

    assert_eq!(
        app.active_tab_text(),
        "class Calc {\n    /**\n     * \n     *\n     * @param a\n     * @param b\n     * @return\n     */\n    fun add(a: Int, b: Int): Int {\n        return a + b\n    }\n}\n"
    );
}

/// The palette is how an action is found by name rather than by knowing
/// which menu holds it — including one whose own shortcut this app spells
/// differently from other editors.
#[test]
fn the_command_palette_runs_an_action_by_name() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");
    app.type_into_active_tab(MAIN_JAVA.chars().count(), "// via the palette");

    app.press(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::P);
    assert!(app.shows(t().palettes.run_a_command), "Ctrl+Shift+P opens the palette");

    app.type_text(t().tabs.save_all);
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert_eq!(app.on_disk("Main.java"), format!("{MAIN_JAVA}// via the palette"));
    assert!(
        !app.shows(t().palettes.run_a_command),
        "picking a command closes the palette"
    );
}

const KMP_BUILD: &str = "plugins { kotlin(\"multiplatform\") }\nkotlin { jvm(); iosArm64() }\n";

fn open_palette_and_type(app: &mut E2e, query: &str) {
    app.press(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::P);
    app.type_text(query);
}

/// A multiplatform project's palette lists one Gradle task per target, found
/// by name like any other command.
#[test]
fn the_command_palette_lists_the_gradle_tasks_of_each_target() {
    let mut app = E2e::launch(&[("build.gradle.kts", KMP_BUILD)]);

    open_palette_and_type(&mut app, "Gradle:");

    for task in ["allTests", "jvmTest", "iosArm64Test", "linkDebugExecutableIosArm64"] {
        assert!(app.shows(&format!("Gradle: {task}")), "{task} is offered");
    }
}

#[test]
fn an_ordinary_project_lists_no_gradle_tasks() {
    let mut app = E2e::launch(&[("build.gradle.kts", "plugins { kotlin(\"jvm\") }\n")]);

    open_palette_and_type(&mut app, "Gradle:");

    assert!(!app.shows_containing("Gradle: "));
}

/// Picking a task starts it through the project's wrapper and shows the build
/// panel. The wrapper here is a script that does nothing, so the test needs a
/// shell.
#[cfg(unix)]
#[test]
fn picking_a_gradle_task_runs_it_in_the_build_panel() {
    use std::os::unix::fs::PermissionsExt;

    let mut app = E2e::launch(&[("build.gradle.kts", KMP_BUILD), ("gradlew", "#!/bin/sh\nexit 0\n")]);
    let wrapper = app.path("gradlew");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!app.app().bottom_dock.shows(BottomTab::Build), "the build panel starts closed");

    open_palette_and_type(&mut app, "Gradle: jvmTest");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    assert!(app.app().bottom_dock.shows(BottomTab::Build), "the task opens the build panel");
}

fn run_source_set_tests_from_the_palette(app: &mut E2e) {
    open_palette_and_type(app, t().palettes.run_source_set_tests);
    app.press(egui::Modifiers::NONE, egui::Key::Enter);
}

/// From a file of a multiplatform source set, one command runs that source
/// set's tests without finding the task by name.
#[cfg(unix)]
#[test]
fn the_palette_runs_the_tests_of_the_open_files_source_set() {
    use std::os::unix::fs::PermissionsExt;

    let mut app = E2e::launch(&[
        ("build.gradle.kts", KMP_BUILD),
        ("gradlew", "#!/bin/sh\nexit 0\n"),
        ("src/jvmMain/kotlin/Greeter.kt", "class Greeter\n"),
    ]);
    std::fs::set_permissions(app.path("gradlew"), std::fs::Permissions::from_mode(0o755)).unwrap();
    app.press(egui::Modifiers::COMMAND, egui::Key::P);
    app.type_text("Greeter");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);
    assert!(!app.app().bottom_dock.shows(BottomTab::Build));

    run_source_set_tests_from_the_palette(&mut app);

    assert!(app.app().bottom_dock.shows(BottomTab::Build));
}

#[test]
fn a_file_outside_any_source_set_says_there_are_no_tests_to_run() {
    let mut app = E2e::launch(&[("build.gradle.kts", KMP_BUILD), ("README.md", "hi\n")]);
    app.press(egui::Modifiers::COMMAND, egui::Key::P);
    app.type_text("README");
    app.press(egui::Modifiers::NONE, egui::Key::Enter);

    run_source_set_tests_from_the_palette(&mut app);

    assert!(app.shows(t().palettes.no_source_set_tests));
    assert!(!app.app().bottom_dock.shows(BottomTab::Build));
}
