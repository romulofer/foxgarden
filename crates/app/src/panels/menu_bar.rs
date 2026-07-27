use fg_core::EditorState;
use syntax::{IncrementalParser, Scope};

use super::side_panel::SidePanelState;
use super::tabs;
use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::style::theme;
use crate::style::view::ViewSettings;
use crate::widgets::editor::{
    AccessorKind, CaseConversion, GLOBAL_TEMPLATES, GenerateMethodKind, JAVA_TEMPLATES, KOTLIN_TEMPLATES,
    UserTemplate, UserTemplates,
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
}

/// Clamp range for the Settings > Font Size control — small enough to stay
/// legible, large enough to stay useful on a hi-DPI display.
const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 8.0..=32.0;
/// Clamp range for the Settings > Indentation width control.
const INDENT_WIDTH_RANGE: std::ops::RangeInclusive<usize> = 1..=8;

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
    zen_mode: &mut bool,
    side_panel_visible: &mut bool,
    last_error: &mut Option<String>,
    custom_templates: &mut UserTemplates,
) -> MenuBarOutcome {
    let mut outcome = MenuBarOutcome::default();

    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.add(egui::Button::new("New File…").shortcut_text("Ctrl+N")).clicked() {
                if let Some(root) = state.project.as_ref().map(|p| p.root.clone()) {
                    side_panel.begin_new_file(root);
                }
                ui.close();
            }
            if ui.button("Open Folder…").clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder()
                    && let Err(err) = state.open_project(folder)
                {
                    *last_error = Some(format!("failed to open project: {err}"));
                }
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(
                    state.active_tab.is_some(),
                    egui::Button::new("Save").shortcut_text("Ctrl+S"),
                )
                .clicked()
            {
                tabs::save_active_tab(state, parsers, last_error);
                ui.close();
            }
            if ui
                .add_enabled(state.active_tab.is_some(), egui::Button::new("Close Tab"))
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
                    egui::Button::new("Reopen Closed Tab").shortcut_text("Ctrl+Shift+T"),
                )
                .clicked()
            {
                tabs::reopen_last_closed_tab(state, parsers);
                ui.close();
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                ui.close();
            }
        });

        ui.menu_button("Settings", |ui| {
            ui.menu_button("Theme", |ui| {
                if ui.button("Light").clicked() {
                    *dark_mode = false;
                    theme::apply(ui.ctx(), false);
                    ui.close();
                }
                if ui.button("Dark").clicked() {
                    *dark_mode = true;
                    theme::apply(ui.ctx(), true);
                    ui.close();
                }
            });
            if ui.button("Font…").clicked() {
                menu.font_settings_open = true;
                ui.close();
            }
            ui.menu_button("Indentation", |ui| {
                if ui.radio(!indent_settings.use_tabs, "Spaces").clicked() {
                    indent_settings.use_tabs = false;
                    ui.close();
                }
                if ui.radio(indent_settings.use_tabs, "Tabs").clicked() {
                    indent_settings.use_tabs = true;
                    ui.close();
                }
                ui.add_enabled_ui(!indent_settings.use_tabs, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Width");
                        ui.add(egui::DragValue::new(&mut indent_settings.width).range(INDENT_WIDTH_RANGE));
                    });
                });
            });
        });

        ui.menu_button("Tools", |ui| {
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
                    .checkbox(&mut state.open_tabs[active].read_only, "Read-Only")
                    .clicked()
            {
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(has_active_tab, egui::Button::new("Generate Getters"))
                .clicked()
            {
                outcome.generate_request = Some(AccessorKind::Getters);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new("Generate Setters"))
                .clicked()
            {
                outcome.generate_request = Some(AccessorKind::Setters);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new("Generate Constructor"))
                .clicked()
            {
                outcome.generate_method_request = Some(GenerateMethodKind::Constructor);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new("Generate toString()"))
                .clicked()
            {
                outcome.generate_method_request = Some(GenerateMethodKind::ToString);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new("Generate equals() and hashCode()"))
                .clicked()
            {
                outcome.generate_method_request = Some(GenerateMethodKind::EqualsAndHashCode);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new("Override Method"))
                .clicked()
            {
                outcome.override_method_request = true;
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(
                    has_active_tab,
                    egui::Button::new("Convert to UPPERCASE").shortcut_text("Ctrl+Shift+U"),
                )
                .clicked()
            {
                outcome.case_conversion_request = Some(CaseConversion::Upper);
                ui.close();
            }
            if ui
                .add_enabled(
                    has_active_tab,
                    egui::Button::new("Convert to lowercase").shortcut_text("Ctrl+Shift+L"),
                )
                .clicked()
            {
                outcome.case_conversion_request = Some(CaseConversion::Lower);
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new("Convert to Title Case"))
                .clicked()
            {
                outcome.case_conversion_request = Some(CaseConversion::Title);
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(has_active_tab, egui::Button::new("Sort Lines"))
                .clicked()
            {
                outcome.sort_lines_request = true;
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new("Unique Lines"))
                .clicked()
            {
                outcome.unique_lines_request = true;
                ui.close();
            }
        });

        ui.menu_button("Run", |ui| {
            if ui
                .add_enabled(state.project.is_some(), egui::Button::new("Edit Configurations…"))
                .clicked()
            {
                outcome.open_run_configs_request = true;
                ui.close();
            }
        });

        ui.menu_button("View", |ui| {
            if checkbox_with_shortcut(ui, zen_mode, "Zen Mode", "F11").changed() {
                ui.close();
            }
            if checkbox_with_shortcut(ui, side_panel_visible, "Side Panel", "Ctrl+B").changed() {
                ui.close();
            }
            ui.separator();
            if ui.checkbox(&mut view_settings.word_wrap, "Word Wrap").changed() {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.show_whitespace, "Render Whitespace")
                .changed()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.show_indent_guides, "Indentation Guides")
                .changed()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.show_sticky_scroll, "Sticky Scroll")
                .on_hover_text("Pin the enclosing class/method header while scrolling (Java)")
                .changed()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.cursor_blink, "Blinking Cursor")
                .changed()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut view_settings.show_editor_outline, "Editor Outline")
                .on_hover_text("Border around the active editor pane, highlighted while it has focus")
                .changed()
            {
                ui.close();
            }
            ui.separator();
            let has_active_tab = state.active_tab.is_some();
            if ui.add_enabled(has_active_tab, egui::Button::new("Fold All")).clicked() {
                outcome.fold_all_request = true;
                ui.close();
            }
            if ui
                .add_enabled(has_active_tab, egui::Button::new("Expand All"))
                .clicked()
            {
                outcome.expand_all_request = true;
                ui.close();
            }
        });

        ui.menu_button("Help", |ui| {
            if ui.button("Live Templates…").clicked() {
                menu.live_templates_open = true;
                ui.close();
            }
            if ui.button("About").clicked() {
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
                ("◀", "Collapse Side Panel (Ctrl+B)")
            } else {
                ("▶", "Expand Side Panel (Ctrl+B)")
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
            ui.heading("Font");
            ui.separator();
            for font in EditorFont::ALL {
                if ui.radio(*editor_font == font, font.label()).clicked() {
                    *editor_font = font;
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Size");
                // A plain numeric input (spinner) rather than the family
                // radios' click-to-pick shape — `DragValue` doubles as both
                // a drag-to-adjust slider and, on click, an editable number
                // box, so typing an exact size still works alongside the
                // drag.
                ui.add(egui::DragValue::new(font_size).range(FONT_SIZE_RANGE).speed(0.25));
            });
            ui.separator();
            ui.button("Close").clicked()
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
            ui.heading("Live Templates");
            ui.label("Type a trigger below, then press Tab with no selection to expand it.");
            ui.label("Add your own below — a custom trigger overrides a built-in one of the same name.");
            ui.separator();
            egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
                ui.strong("Global");
                ui.label(egui::RichText::new("Expands the same way in every file, Java/Kotlin or not.").weak());
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
            ui.button("Close").clicked()
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
            ui.label(egui::RichText::new(template.trigger).monospace().strong().color(trigger_color));
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
    ui.weak("Your Templates");
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
                if ui.small_button("✗").on_hover_text("Remove").clicked() {
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
                .hint_text("trigger")
                .font(egui::TextStyle::Monospace)
                .desired_width(100.0),
        );
        ui.add(
            egui::TextEdit::singleline(new_body)
                .id_salt((id_prefix, "new_body"))
                .hint_text("expansion — ${cursor} marks where the cursor lands")
                .font(egui::TextStyle::Monospace),
        );
        if ui.button("+ Add").clicked() && !new_trigger.is_empty() {
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
        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        ui.label(env!("CARGO_PKG_DESCRIPTION"));
        ui.label("By Rômulo Fernandes Evangelista");
        ui.hyperlink_to("github.com/romulofer/foxgarden", env!("CARGO_PKG_REPOSITORY"));
        ui.separator();
        ui.label("Shortcuts:");
        ui.label("Ctrl+S — save the active tab");
        ui.label("Ctrl+Shift+T — reopen the last closed tab");
        ui.label("Middle-click a tab — close it");
        ui.label("F11 — toggle Zen Mode (hide menu bar and side panel)");
        ui.label("Ctrl+B — toggle the side panel");
        ui.label("Ctrl+J — join the current line with the next one");
        ui.label("Ctrl+E — go to a recent file");
        ui.label("Ctrl+Shift+E — search Spring endpoints");
        ui.label("Ctrl+/ — toggle line comments");
        ui.label("Ctrl+Shift+G — generate getters and setters (Java)");
        ui.label("Ctrl+Shift+U/L — convert selection to UPPER/lowercase");
        ui.label("Tools menu — generate just getters/setters, or Title Case");
        ui.label("Type a snippet trigger (e.g. \"sout\") then Tab to expand it");
        ui.label("Help > Live Templates… — full list of snippet triggers");
        // Plain "Up"/"Down" rather than `↑`/`↓` glyphs — the bundled
        // font set (Hack + Ubuntu-Light + the emoji fonts `style::
        // fonts::install` leaves untouched, see that fn's doc comment)
        // has no glyph for the plain Arrows-block `U+2191`/`U+2193`, so
        // those rendered as tofu; every other line in this list is
        // already plain shortcut text, not a symbol.
        ui.label("Alt+Up/Down — move the current line up/down");
        ui.label("Alt+Shift+Up/Down — duplicate the current line");
        ui.label("Home — jump to first non-whitespace, then column 0");
        ui.label("Ctrl+N — new file");
        ui.label("Esc — close the current dialog");
        ui.separator();
        ui.button("Close").clicked()
    });
    if let Some((close_clicked, escape_pressed)) = outcome
        && (close_clicked || escape_pressed)
    {
        menu.about_open = false;
    }
}
