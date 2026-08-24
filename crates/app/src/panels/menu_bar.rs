use fg_core::EditorState;
use fg_i18n::{Lang, msg, t};
use syntax::{IncrementalParser, Scope};

use super::side_panel::SidePanelState;
use super::tabs;
use crate::auto_save::{AutoSaveMode, AutoSaveSettings};
use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::style::theme;
use crate::style::view::ViewSettings;
use crate::widgets::editor::{
    AccessorKind, CaseConversion, GLOBAL_TEMPLATES, GenerateMethodKind, JAVA_TEMPLATES, KOTLIN_TEMPLATES, UserTemplate,
    UserTemplates,
};
use crate::widgets::modal::show_modal;

/// Persistent state for menu-triggered dialogs.
#[derive(Default)]
pub struct MenuBarState {
    about_open: bool,
    /// Settings > Font… — see `show_font_settings`.
    font_settings_open: bool,
    /// Help > Live Templates… — see `show_live_templates`.
    live_templates_open: bool,
    /// A new custom Java template being typed, not yet added to
    /// `UserTemplates::java` — see `show_user_templates_editor`.
    new_java_trigger: String,
    new_java_body: String,
    /// Same as `new_java_trigger`/`new_java_body`, for Kotlin.
    new_kotlin_trigger: String,
    new_kotlin_body: String,
    /// Same as `new_java_trigger`/`new_java_body`, for the language-agnostic
    /// Global section (`UserTemplates::global`).
    new_global_trigger: String,
    new_global_body: String,
}

/// What the Tools menu wants the editor to do this frame — at most one of
/// these is ever `Some` in a given frame, since each is set by a distinct
/// button click.
#[derive(Default)]
pub struct MenuBarOutcome {
    pub generate_request: Option<AccessorKind>,
    pub generate_method_request: Option<GenerateMethodKind>,
    pub override_method_request: bool,
    pub case_conversion_request: Option<CaseConversion>,
    pub sort_lines_request: bool,
    pub unique_lines_request: bool,
    pub open_run_configs_request: bool,
    pub fold_all_request: bool,
    pub expand_all_request: bool,
    /// Settings > JDKs… — opens `panels::jdk_registry`'s dialog (`PLAN.md`
    /// Track 29 Phase 1). Deliberately separate from Language Servers…:
    /// `LspSettings::jdtls_java_home` is "which JVM runs jdt.ls itself"
    /// (always 21+), this is "which JDKs exist to *target*" (any version).
    pub open_jdk_registry_settings_request: bool,
    /// File > New Project… — opens `panels::new_project`'s wizard
    /// (`PLAN.md` Track 29 Phase 3).
    pub open_new_project_wizard_request: bool,
    /// Settings > External Tools… — opens `static_analysis::
    /// StaticAnalysisState`'s dialog, which lives on `FoxGardenApp` rather
    /// than `MenuBarState`, same "the feature's own state, not menu_bar's"
    /// shape `open_run_configs_request` already established for `RunConfigsDialogState`.
    pub open_external_tools_settings_request: bool,
    /// Settings > Language Servers… — opens `panels::lsp_servers`' dialog,
    /// which owns the enable/binary-path settings that used to live in a
    /// nested Settings submenu, plus installing the servers themselves.
    pub open_lsp_servers_settings_request: bool,
    /// Tools > Run Checkstyle.
    pub run_checkstyle_request: bool,
    /// Tools > Run PMD.
    pub run_pmd_request: bool,
    /// Tools > Run SpotBugs (`PLAN.md` Track 5 Phase 3).
    pub run_spotbugs_request: bool,
    /// Run > Build (`PLAN.md` Track 22 Phase 1).
    pub build_request: bool,
    /// Run > Run Project (`PLAN.md` Track 22 Phase 2).
    pub run_project_request: bool,
    /// Run > Run Tests (`PLAN.md` Track 22 Phase 3).
    pub run_tests_request: bool,
    /// Run > Run with Coverage (`PLAN.md` Track 13 Phase 1, Maven-only).
    pub run_with_coverage_request: bool,
    /// Run > "Docker: Build & Run" (`PLAN.md` Track 14 Phase 1).
    pub docker_build_run_request: bool,
    /// Run > "Docker Compose: Up" (`PLAN.md` Track 14 Phase 1).
    pub docker_compose_up_request: bool,
    /// Run > Debug Project, which becomes Run > Stop once `debug_running`
    /// (this same button relabels/re-targets itself, rather than the menu
    /// carrying two separate booleans for "start" and "stop") — the
    /// caller's own `debug_state::DebugState::is_running()` is what decides
    /// which of `start`/`stop` this outcome means (`PLAN.md` Track 23
    /// Phase 1).
    pub debug_request: bool,
}

/// Clamp range for the Settings > Font Size control — small enough to stay
/// legible, large enough to stay useful on a hi-DPI display.
const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 8.0..=32.0;
/// Clamp range for the Settings > Indentation width control.
const INDENT_WIDTH_RANGE: std::ops::RangeInclusive<usize> = 1..=8;
/// Clamp range for the Settings > Auto-save idle-seconds control — long
/// enough to survive a brief pause without saving mid-thought, short enough
/// to still count as "auto."
const AUTO_SAVE_IDLE_SECONDS_RANGE: std::ops::RangeInclusive<u32> = 5..=600;

#[expect(
    clippy::too_many_arguments,
    reason = "each parameter is an independently-owned piece of app-wide state a distinct menu section reads or mutates (editor settings, dialog state, error/input plumbing), not a bundle waiting to be a struct — same shape and reasoning as widgets::editor::show's own allowance"
)]
pub fn show(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    side_panel: &mut SidePanelState,
    parsers: &mut Vec<Option<IncrementalParser>>,
    pending_close: &mut Option<usize>,
    menu: &mut MenuBarState,
    editor_font: &mut EditorFont,
    font_size: &mut f32,
    dark_mode: &mut bool,
    indent_settings: &mut IndentSettings,
    view_settings: &mut ViewSettings,
    auto_save_settings: &mut AutoSaveSettings,
    zen_mode: &mut bool,
    side_panel_visible: &mut bool,
    terminal_panel_visible: &mut bool,
    source_control_visible: &mut bool,
    build_panel_visible: &mut bool,
    last_error: &mut Option<String>,
    custom_templates: &mut UserTemplates,
    checkstyle_running: bool,
    pmd_running: bool,
    spotbugs_running: bool,
    build_running: bool,
    run_running: bool,
    test_running: bool,
    coverage_running: bool,
    docker_build_run_running: bool,
    docker_compose_running: bool,
    debug_running: bool,
) -> MenuBarOutcome {
    let mut outcome = MenuBarOutcome::default();

    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(t().menu.file, |ui| {
            if ui
                .add(egui::Button::new(t().menu.new_file).shortcut_text("Ctrl+N"))
                .clicked()
            {
                if let Some(root) = state.project.as_ref().map(|p| p.root.clone()) {
                    side_panel.begin_new_file(root);
                }
                ui.close();
            }
            if ui.button(t().menu.open_folder).clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder()
                    && let Err(err) = state.open_project(folder)
                {
                    *last_error = Some(msg::failed_to_open_project(&err.to_string()));
                }
                ui.close();
            }
            if ui.button(t().menu.new_project).clicked() {
                outcome.open_new_project_wizard_request = true;
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(
                    state.active_tab.is_some(),
                    egui::Button::new(t().common.save).shortcut_text("Ctrl+S"),
                )
                .clicked()
            {
                tabs::save_active_tab(state, parsers, last_error);
                ui.close();
            }
            if ui
                .add_enabled(state.active_tab.is_some(), egui::Button::new(t().menu.close_tab))
                .clicked()
            {
                if let Some(active) = state.active_tab {
                    tabs::request_close_tab(state, parsers, pending_close, active);
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    !state.closed_tabs.is_empty(),
                    egui::Button::new(t().menu.reopen_closed_tab).shortcut_text("Ctrl+Shift+T"),
                )
                .clicked()
            {
                tabs::reopen_last_closed_tab(state, parsers);
                ui.close();
            }
            ui.separator();
            if ui.button(t().menu.exit).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                ui.close();
            }
        });

        ui.menu_button(t().menu.settings, |ui| {
            ui.menu_button(t().menu.theme, |ui| {
                if ui.button(t().menu.theme_light).clicked() {
                    *dark_mode = false;
                    theme::apply(ui.ctx(), false);
                    ui.close();
                }
                if ui.button(t().menu.theme_dark).clicked() {
                    *dark_mode = true;
                    theme::apply(ui.ctx(), true);
                    ui.close();
                }
            });
            // Each language is listed under its own name (`Lang::autonym`),
            // not translated into the active one — someone who opened the
            // app in a language they don't read has to be able to find
            // their own in this list.
            //
            // No outcome flag: the active language is process-global, so
            // `set_lang` is the whole change (the rest of this very frame
            // already draws translated), and `persist_settings` reads
            // `fg_i18n::lang()` straight off that global at save time —
            // there's nothing for `FoxGardenApp` to carry.
            ui.menu_button(t().menu.language, |ui| {
                let active = fg_i18n::lang();
                for language in Lang::ALL {
                    if ui.radio(active == language, language.autonym()).clicked() {
                        fg_i18n::set_lang(language);
                        ui.close();
                    }
                }
            });
            if ui.button(t().menu.font).clicked() {
                menu.font_settings_open = true;
                ui.close();
            }
            ui.menu_button(t().menu.indentation, |ui| {
                if ui.radio(!indent_settings.use_tabs, t().menu.indent_spaces).clicked() {
                    indent_settings.use_tabs = false;
                    ui.close();
                }
                if ui.radio(indent_settings.use_tabs, t().menu.indent_tabs).clicked() {
                    indent_settings.use_tabs = true;
                    ui.close();
                }
                ui.add_enabled_ui(!indent_settings.use_tabs, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(t().menu.indent_width);
                        ui.add(egui::DragValue::new(&mut indent_settings.width).range(INDENT_WIDTH_RANGE));
                    });
                });
            });
            ui.menu_button(t().menu.auto_save, |ui| {
                if ui
                    .checkbox(&mut auto_save_settings.enabled, t().menu.auto_save_enabled)
                    .clicked()
                {
                    ui.close();
                }
                ui.add_enabled_ui(auto_save_settings.enabled, |ui| {
                    if ui
                        .radio(
                            auto_save_settings.mode == AutoSaveMode::OnFocusLoss,
                            t().menu.auto_save_on_focus_loss,
                        )
                        .clicked()
                    {
                        auto_save_settings.mode = AutoSaveMode::OnFocusLoss;
                        ui.close();
                    }
                    if ui
                        .radio(
                            auto_save_settings.mode == AutoSaveMode::AfterIdle,
                            t().menu.auto_save_after_idle,
                        )
                        .clicked()
                    {
                        auto_save_settings.mode = AutoSaveMode::AfterIdle;
                        ui.close();
                    }
                    ui.add_enabled_ui(auto_save_settings.mode == AutoSaveMode::AfterIdle, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(t().menu.auto_save_idle_seconds);
                            ui.add(
                                egui::DragValue::new(&mut auto_save_settings.idle_seconds)
                                    .range(AUTO_SAVE_IDLE_SECONDS_RANGE),
                            );
                        });
                    });
                });
            });
            if ui.button(t().menu.language_servers).clicked() {
                outcome.open_lsp_servers_settings_request = true;
                ui.close();
            }
            if ui.button(t().menu.jdks).clicked() {
                outcome.open_jdk_registry_settings_request = true;
                ui.close();
            }
            if ui.button(t().menu.external_tools).clicked() {
                outcome.open_external_tools_settings_request = true;
                ui.close();
            }
        });

        ui.menu_button(t().menu.tools, |ui| {
            // Every item here is only meaningful given some precondition
            // (Java + cursor inside a class body; a non-empty selection)
            // that can't be checked from the menu — `widgets::editor::show`
            // reports a mismatch through `last_error` rather than silently
            // doing nothing, same as `Ctrl+Shift+G`/`Ctrl+Shift+U`/`L`
            // (which request the same actions, just narrower or via the
            // keyboard).
            let has_active_tab = state.active_tab.is_some();
            if let Some(active) = state.active_tab
                && ui
                    .checkbox(&mut state.open_tabs[active].read_only, t().menu.read_only)
                    .clicked()
            {
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(has_active_tab, egui::Button::new(t().menu.generate_getters))
                .clicked()
            {
                outcome.generate_request = Some(AccessorKind::Getters);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new(t().menu.generate_setters))
                .clicked()
            {
                outcome.generate_request = Some(AccessorKind::Setters);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new(t().menu.generate_constructor))
                .clicked()
            {
                outcome.generate_method_request = Some(GenerateMethodKind::Constructor);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new(t().menu.generate_to_string))
                .clicked()
            {
                outcome.generate_method_request = Some(GenerateMethodKind::ToString);
                ui.close();
            }
            if ui
                .add_enabled(
                    has_active_tab,
                    egui::Button::new(t().menu.generate_equals_and_hash_code),
                )
                .clicked()
            {
                outcome.generate_method_request = Some(GenerateMethodKind::EqualsAndHashCode);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new(t().menu.override_method))
                .clicked()
            {
                outcome.override_method_request = true;
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(
                    has_active_tab,
                    egui::Button::new(t().menu.convert_to_uppercase).shortcut_text("Ctrl+Shift+U"),
                )
                .clicked()
            {
                outcome.case_conversion_request = Some(CaseConversion::Upper);
                ui.close();
            }
            if ui
                .add_enabled(
                    has_active_tab,
                    egui::Button::new(t().menu.convert_to_lowercase).shortcut_text("Ctrl+Shift+L"),
                )
                .clicked()
            {
                outcome.case_conversion_request = Some(CaseConversion::Lower);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new(t().menu.convert_to_title_case))
                .clicked()
            {
                outcome.case_conversion_request = Some(CaseConversion::Title);
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(has_active_tab, egui::Button::new(t().menu.sort_lines))
                .clicked()
            {
                outcome.sort_lines_request = true;
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new(t().menu.unique_lines))
                .clicked()
            {
                outcome.unique_lines_request = true;
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(
                    state.project.is_some() && !checkstyle_running,
                    egui::Button::new(if checkstyle_running {
                        t().common.running_checkstyle
                    } else {
                        t().menu.run_checkstyle
                    }),
                )
                .clicked()
            {
                outcome.run_checkstyle_request = true;
                ui.close();
            }
            if ui
                .add_enabled(
                    state.project.is_some() && !pmd_running,
                    egui::Button::new(if pmd_running {
                        t().common.running_pmd
                    } else {
                        t().menu.run_pmd
                    }),
                )
                .clicked()
            {
                outcome.run_pmd_request = true;
                ui.close();
            }
            if ui
                .add_enabled(
                    state.project.is_some() && !spotbugs_running,
                    egui::Button::new(if spotbugs_running {
                        t().common.running_spotbugs
                    } else {
                        t().menu.run_spotbugs
                    }),
                )
                .clicked()
            {
                outcome.run_spotbugs_request = true;
                ui.close();
            }
        });

        ui.menu_button(t().menu.run, |ui| {
            if ui
                .add_enabled(state.project.is_some(), egui::Button::new(t().menu.edit_configurations))
                .clicked()
            {
                outcome.open_run_configs_request = true;
                ui.close();
            }
            let any_running = build_running
                || run_running
                || test_running
                || coverage_running
                || docker_build_run_running
                || docker_compose_running
                || debug_running;
            // Build/Run/Test/Coverage all disable while a debug session is
            // up too (`any_running` above already covers that), but the
            // Debug entry's own enabled-ness has to stay independent of its
            // own `debug_running` — otherwise there would be no way to ever
            // click it again to reach `outcome.debug_request` and stop a
            // session already in flight.
            let other_running =
                build_running || run_running || test_running || coverage_running || docker_build_run_running || docker_compose_running;
            if ui
                .add_enabled(
                    state.project.is_some() && !any_running,
                    egui::Button::new(if build_running { t().common.running_build } else { t().menu.build }),
                )
                .clicked()
            {
                outcome.build_request = true;
                ui.close();
            }
            if ui
                .add_enabled(
                    state.project.is_some() && !any_running,
                    egui::Button::new(if run_running { t().common.running_run } else { t().menu.run_project }),
                )
                .clicked()
            {
                outcome.run_project_request = true;
                ui.close();
            }
            if ui
                .add_enabled(
                    state.project.is_some() && !any_running,
                    egui::Button::new(if test_running { t().common.running_tests } else { t().menu.run_tests }),
                )
                .clicked()
            {
                outcome.run_tests_request = true;
                ui.close();
            }
            if ui
                .add_enabled(
                    state.project.is_some() && !any_running,
                    egui::Button::new(if coverage_running { t().common.running_coverage } else { t().menu.run_with_coverage }),
                )
                .clicked()
            {
                outcome.run_with_coverage_request = true;
                ui.close();
            }
            // Each Docker entry's own enabled-ness additionally requires
            // the file it would act on to actually exist at the project
            // root (`fg_core::has_dockerfile`/`compose_file`) — offering a
            // button that can only ever fail with "no Dockerfile found"
            // would be worse than just not showing it as clickable.
            let docker_root = state.project.as_ref().map(|p| p.root.clone());
            let has_dockerfile = docker_root.as_deref().is_some_and(fg_core::has_dockerfile);
            let has_compose_file = docker_root.as_deref().and_then(fg_core::compose_file).is_some();
            if ui
                .add_enabled(
                    has_dockerfile && !any_running,
                    egui::Button::new(if docker_build_run_running {
                        t().common.running_docker_build
                    } else {
                        t().menu.docker_build_and_run
                    }),
                )
                .clicked()
            {
                outcome.docker_build_run_request = true;
                ui.close();
            }
            if ui
                .add_enabled(
                    has_compose_file && !any_running,
                    egui::Button::new(if docker_compose_running {
                        t().common.running_docker_compose
                    } else {
                        t().menu.docker_compose_up
                    }),
                )
                .clicked()
            {
                outcome.docker_compose_up_request = true;
                ui.close();
            }
            if ui
                .add_enabled(
                    state.project.is_some() && (debug_running || !other_running),
                    egui::Button::new(if debug_running { t().common.stop } else { t().menu.debug_project }),
                )
                .clicked()
            {
                outcome.debug_request = true;
                ui.close();
            }
        });

        ui.menu_button(t().menu.view, |ui| {
            if checkbox_with_shortcut(ui, zen_mode, t().menu.zen_mode, "F11").changed() {
                ui.close();
            }
            if checkbox_with_shortcut(ui, side_panel_visible, t().menu.side_panel, "Ctrl+B").changed() {
                ui.close();
            }
            if checkbox_with_shortcut(ui, terminal_panel_visible, t().menu.terminal_panel, "Ctrl+`").changed() {
                ui.close();
            }
            if ui.checkbox(source_control_visible, t().menu.source_control).changed() {
                ui.close();
            }
            if ui.checkbox(build_panel_visible, t().menu.build_output).changed() {
                ui.close();
            }
            ui.separator();
            if ui.checkbox(&mut view_settings.word_wrap, t().menu.word_wrap).changed() {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.show_whitespace, t().menu.render_whitespace)
                .changed()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.show_indent_guides, t().menu.indentation_guides)
                .changed()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.show_sticky_scroll, t().menu.sticky_scroll)
                .on_hover_text(t().menu.sticky_scroll_hint)
                .changed()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.cursor_blink, t().menu.blinking_cursor)
                .changed()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.show_editor_outline, t().menu.editor_outline)
                .on_hover_text(t().menu.editor_outline_hint)
                .changed()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.show_inline_blame, t().menu.inline_blame)
                .on_hover_text(t().menu.inline_blame_hint)
                .changed()
            {
                ui.close();
            }
            ui.separator();
            let has_active_tab = state.active_tab.is_some();
            if ui
                .add_enabled(has_active_tab, egui::Button::new(t().menu.fold_all))
                .clicked()
            {
                outcome.fold_all_request = true;
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new(t().menu.expand_all))
                .clicked()
            {
                outcome.expand_all_request = true;
                ui.close();
            }
        });

        ui.menu_button(t().menu.help, |ui| {
            if ui.button(t().menu.live_templates).clicked() {
                menu.live_templates_open = true;
                ui.close();
            }
            if ui.button(t().menu.about).clicked() {
                menu.about_open = true;
                ui.close();
            }
        });

        // Pinned to the far right of the menu bar — unlike the matching
        // "◀" button in the side panel's own toolbar (which disappears
        // along with the rest of that panel once collapsed), the menu bar
        // stays up whenever the app isn't in Zen Mode, so this is always
        // reachable to bring the panel back, not just to hide it.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (icon, hover) = if *side_panel_visible {
                ("◀", t().menu.collapse_side_panel)
            } else {
                ("▶", t().menu.expand_side_panel)
            };
            if ui.button(icon).on_hover_text(hover).clicked() {
                *side_panel_visible = !*side_panel_visible;
            }
        });
    });

    show_about(ui, menu);
    show_font_settings(ui, menu, editor_font, font_size);
    show_live_templates(ui, menu, *dark_mode, custom_templates);

    outcome
}

/// Draws a checkbox with its keyboard shortcut right-aligned in weak text,
/// the same visual as `egui::Button::shortcut_text` — `egui::Checkbox` has
/// no such builder method of its own, but it does accept the same `Atoms`
/// tuple shape `shortcut_text` builds internally (label, then a growing
/// spacer, then the weak shortcut text), so building that tuple by hand gets
/// the identical look.
fn checkbox_with_shortcut(ui: &mut egui::Ui, checked: &mut bool, label: &str, shortcut: &str) -> egui::Response {
    ui.add(egui::Checkbox::new(
        checked,
        (label, egui::Atom::grow(), egui::RichText::new(shortcut).weak()),
    ))
}

/// Settings > Font… — family and size together in one dialog, rather than a
/// "Font" submenu (family radios) plus a separately-placed "Font Size" row
/// sitting right underneath it in the Settings menu, which read as two
/// unrelated settings instead of the one "what does code look like" choice
/// they actually are.
fn show_font_settings(ui: &egui::Ui, menu: &mut MenuBarState, editor_font: &mut EditorFont, font_size: &mut f32) {
    let outcome = show_modal(
        ui,
        "font_settings_dialog",
        menu.font_settings_open.then_some(()),
        |ui, ()| {
            ui.heading(t().dialogs.font_heading);
            ui.separator();
            for font in EditorFont::ALL {
                if ui.radio(*editor_font == font, font.label()).clicked() {
                    *editor_font = font;
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(t().dialogs.font_size);
                // A plain numeric input (spinner) rather than the family
                // radios' click-to-pick shape — `DragValue` doubles as both
                // a drag-to-adjust slider and, on click, an editable number
                // box, so typing an exact size still works alongside the
                // drag.
                ui.add(egui::DragValue::new(font_size).range(FONT_SIZE_RANGE).speed(0.25));
            });
            ui.separator();
            ui.button(t().common.close).clicked()
        },
    );
    if let Some((close_clicked, escape_pressed)) = outcome
        && (close_clicked || escape_pressed)
    {
        menu.font_settings_open = false;
    }
}

/// Help > Live Templates… — a lookup reference for the built-in snippet
/// triggers `templates::expand` recognizes (type the trigger word, press Tab
/// with no selection, get the expansion in its place), plus an editor for a
/// user's own custom triggers (`custom_templates`, threaded all the way
/// down to `widgets::editor::widget::show`'s Tab-expansion lookup, which
/// checks these before the built-ins — see `templates::find_expansion`).
/// Three sections: Global (`GLOBAL_TEMPLATES`/`UserTemplates::global`, which
/// expand the same way in any file regardless of language), then Java and
/// Kotlin. Lists every section unconditionally rather than only the active
/// tab's language: this is a reference dialog a user opens to remember
/// what's available, not a context-sensitive one, so showing just one
/// language when e.g. no file is open, or a non-Java/Kotlin file is active,
/// would leave it with nothing to show at all.
fn show_live_templates(ui: &egui::Ui, menu: &mut MenuBarState, dark_mode: bool, custom_templates: &mut UserTemplates) {
    let outcome = show_modal(
        ui,
        "live_templates_dialog",
        menu.live_templates_open.then_some(()),
        |ui, ()| {
            ui.set_min_width(380.0);
            ui.heading(t().dialogs.live_templates_heading);
            ui.label(t().dialogs.live_templates_intro);
            ui.label(t().dialogs.live_templates_custom_intro);
            ui.separator();
            egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
                ui.strong(t().dialogs.live_templates_global);
                ui.label(egui::RichText::new(t().dialogs.live_templates_global_hint).weak());
                ui.add_space(4.0);
                show_template_group(ui, GLOBAL_TEMPLATES, dark_mode);
                show_user_templates_editor(
                    ui,
                    "global_user_templates",
                    &mut custom_templates.global,
                    &mut menu.new_global_trigger,
                    &mut menu.new_global_body,
                    dark_mode,
                );

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                ui.strong("Java");
                ui.add_space(4.0);
                show_template_group(ui, JAVA_TEMPLATES, dark_mode);
                show_user_templates_editor(
                    ui,
                    "java_user_templates",
                    &mut custom_templates.java,
                    &mut menu.new_java_trigger,
                    &mut menu.new_java_body,
                    dark_mode,
                );

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                ui.strong("Kotlin");
                ui.add_space(4.0);
                show_template_group(ui, KOTLIN_TEMPLATES, dark_mode);
                show_user_templates_editor(
                    ui,
                    "kotlin_user_templates",
                    &mut custom_templates.kotlin,
                    &mut menu.new_kotlin_trigger,
                    &mut menu.new_kotlin_body,
                    dark_mode,
                );
            });
            ui.separator();
            ui.button(t().common.close).clicked()
        },
    );
    if let Some((close_clicked, escape_pressed)) = outcome
        && (close_clicked || escape_pressed)
    {
        menu.live_templates_open = false;
    }
}

/// One language's built-in trigger/expansion cards for `show_live_templates`
/// — each template gets its own bordered `ui.group` card (rather than a
/// striped grid row) so a multi-line expansion like `psvm`'s stays visually
/// separated from its neighbors instead of blending into the next row's
/// stripe. The trigger is colored with the same "Function" highlight
/// `color_for_scope` uses in the editor itself, so it reads as a callable
/// name at a glance rather than plain body text. The `${cursor}` marker
/// `templates::expand` strips out at expansion time is shown here as `|`,
/// standing in for where the cursor lands, since a user reading this table
/// never sees the literal marker text.
fn show_template_group(ui: &mut egui::Ui, templates: &[crate::widgets::editor::Template], dark_mode: bool) {
    let trigger_color = theme::color_for_scope(Scope::Function, dark_mode);
    for template in templates {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(template.trigger)
                    .monospace()
                    .strong()
                    .color(trigger_color),
            );
            ui.label(egui::RichText::new(template.body.replace("${cursor}", "|")).monospace());
        });
        ui.add_space(6.0);
    }
}

/// The editable "Your Templates" section beneath one language's built-in
/// cards: an editable trigger/body pair per existing custom template (with
/// a "✗" to remove it) plus a blank trigger/body row at the bottom that
/// appends a new one to `templates` on "+ Add" — same "type into scratch
/// fields, `mem::take` them into a new entry" shape as `run_configs`' own
/// environment-variable adder. `id_prefix` keeps the two languages'
/// multiline text edits (which need a stable `egui::Id` to track cursor/
/// selection state across frames) from colliding, since both would
/// otherwise share the same auto-generated id derived from position alone.
fn show_user_templates_editor(
    ui: &mut egui::Ui,
    id_prefix: &str,
    templates: &mut Vec<UserTemplate>,
    new_trigger: &mut String,
    new_body: &mut String,
    dark_mode: bool,
) {
    let trigger_color = theme::color_for_scope(Scope::Function, dark_mode);
    ui.add_space(4.0);
    ui.weak(t().dialogs.live_templates_yours);
    ui.add_space(4.0);

    let mut remove_index = None;
    for (index, template) in templates.iter_mut().enumerate() {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut template.trigger)
                        .id_salt((id_prefix, index))
                        .font(egui::TextStyle::Monospace)
                        .text_color(trigger_color)
                        .desired_width(100.0),
                );
                if ui.small_button("✗").on_hover_text(t().common.remove).clicked() {
                    remove_index = Some(index);
                }
            });
            ui.add(
                egui::TextEdit::multiline(&mut template.body)
                    .id_salt((id_prefix, "body", index))
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(1)
                    .desired_width(ui.available_width()),
            );
        });
        ui.add_space(6.0);
    }
    if let Some(index) = remove_index {
        templates.remove(index);
    }

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(new_trigger)
                .id_salt((id_prefix, "new_trigger"))
                .hint_text(t().dialogs.live_templates_trigger_hint)
                .font(egui::TextStyle::Monospace)
                .desired_width(100.0),
        );
        ui.add(
            egui::TextEdit::singleline(new_body)
                .id_salt((id_prefix, "new_body"))
                .hint_text(t().dialogs.live_templates_expansion_hint)
                .font(egui::TextStyle::Monospace),
        );
        if ui.button(t().common.add).clicked() && !new_trigger.is_empty() {
            templates.push(UserTemplate {
                trigger: std::mem::take(new_trigger),
                body: std::mem::take(new_body),
            });
        }
    });
}

fn show_about(ui: &mut egui::Ui, menu: &mut MenuBarState) {
    let outcome = show_modal(ui, "about_dialog", menu.about_open.then_some(()), |ui, ()| {
        ui.heading("FoxGarden");
        ui.label(msg::version(env!("CARGO_PKG_VERSION")));
        ui.label(env!("CARGO_PKG_DESCRIPTION"));
        ui.label(t().about.author);
        ui.hyperlink_to("github.com/romulofer/foxgarden", env!("CARGO_PKG_REPOSITORY"));
        ui.separator();
        ui.label(t().about.shortcuts_heading);
        // The list lives in the catalogue (`About::shortcuts`) rather than
        // as literals here, since every line is translated. Note that both
        // languages spell the arrow keys as plain "Up"/"Down" rather than
        // `↑`/`↓`: the bundled font set (Hack + Ubuntu-Light + the emoji
        // fonts `style::fonts::install` leaves untouched, see that fn's doc
        // comment) has no glyph for the plain Arrows-block `U+2191`/
        // `U+2193`, so those rendered as tofu; every other line in the list
        // is already plain shortcut text, not a symbol.
        for shortcut in t().about.shortcuts {
            ui.label(shortcut);
        }
        ui.separator();
        ui.button(t().common.close).clicked()
    });
    if let Some((close_clicked, escape_pressed)) = outcome
        && (close_clicked || escape_pressed)
    {
        menu.about_open = false;
    }
}
