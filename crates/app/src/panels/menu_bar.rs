use fg_core::EditorState;
use syntax::IncrementalParser;

use super::side_panel::SidePanelState;
use super::tabs;
use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::style::theme;
use crate::widgets::editor::AccessorKind;
use crate::widgets::modal::show_modal;

/// Persistent state for menu-triggered dialogs.
#[derive(Default)]
pub struct MenuBarState {
    about_open: bool,
}

/// Clamp range for the Settings > Font Size control — small enough to stay
/// legible, large enough to stay useful on a hi-DPI display.
const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 8.0..=32.0;
/// Clamp range for the Settings > Indentation width control.
const INDENT_WIDTH_RANGE: std::ops::RangeInclusive<usize> = 1..=8;

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
    zen_mode: &mut bool,
    last_error: &mut Option<String>,
) -> Option<AccessorKind> {
    let mut generate_request = None;

    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("New File…").clicked() {
                if let Some(root) = state.project.as_ref().map(|p| p.root.clone()) {
                    side_panel.begin_new_file(root);
                }
                ui.close();
            }
            if ui.button("Open Folder…").clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    if let Err(err) = state.open_project(folder) {
                        *last_error = Some(format!("failed to open project: {err}"));
                    }
                }
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(state.active_tab.is_some(), egui::Button::new("Save").shortcut_text("Ctrl+S"))
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
            ui.menu_button("Font", |ui| {
                for font in EditorFont::ALL {
                    if ui.radio(*editor_font == font, font.label()).clicked() {
                        *editor_font = font;
                        ui.close();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Font Size");
                ui.add(egui::DragValue::new(font_size).range(FONT_SIZE_RANGE).speed(0.25));
            });
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
            // Java-only, and only meaningful with the cursor inside a
            // class body — `widgets::editor::show` reports that as an
            // error (via `last_error`) rather than silently doing nothing
            // if it doesn't apply, same as `Ctrl+Shift+G` (which requests
            // both at once; these two are separate, narrower requests).
            let has_active_tab = state.active_tab.is_some();
            if ui.add_enabled(has_active_tab, egui::Button::new("Generate Getters")).clicked() {
                generate_request = Some(AccessorKind::Getters);
                ui.close();
            }
            if ui.add_enabled(has_active_tab, egui::Button::new("Generate Setters")).clicked() {
                generate_request = Some(AccessorKind::Setters);
                ui.close();
            }
        });

        ui.menu_button("View", |ui| {
            if ui.checkbox(zen_mode, "Zen Mode").on_hover_text("F11").changed() {
                ui.close();
            }
        });

        ui.menu_button("Help", |ui| {
            if ui.button("About").clicked() {
                menu.about_open = true;
                ui.close();
            }
        });
    });

    show_about(ui, menu);

    generate_request
}

fn show_about(ui: &mut egui::Ui, menu: &mut MenuBarState) {
    let closed = show_modal(ui, "about_dialog", menu.about_open.then_some(()), |ui, ()| {
        ui.heading("FoxGarden");
        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        ui.label("A light code editor for Java and Kotlin.");
        ui.separator();
        ui.label("Shortcuts:");
        ui.label("Ctrl+S — save the active tab");
        ui.label("Ctrl+Shift+T — reopen the last closed tab");
        ui.label("Middle-click a tab — close it");
        ui.label("F11 — toggle Zen Mode (hide menu bar and side panel)");
        ui.label("Ctrl+J — join the current line with the next one");
        ui.label("Ctrl+E — go to a recent file");
        ui.label("Ctrl+/ — toggle line comments");
        ui.label("Ctrl+Shift+G — generate getters and setters (Java)");
        ui.label("Tools menu — generate just getters, or just setters");
        ui.label("Type a snippet trigger (e.g. \"sout\") then Tab to expand it");
        ui.label("Alt+↑/↓ — move the current line up/down");
        ui.label("Alt+Shift+↑/↓ — duplicate the current line");
        ui.separator();
        ui.button("Close").clicked()
    });
    if closed == Some(true) {
        menu.about_open = false;
    }
}
