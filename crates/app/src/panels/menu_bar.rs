use fg_core::EditorState;
use syntax::IncrementalParser;

use super::side_panel::SidePanelState;
use super::tabs;
use crate::style::fonts::EditorFont;
use crate::style::theme;

/// Persistent state for menu-triggered dialogs.
#[derive(Default)]
pub struct MenuBarState {
    about_open: bool,
}

pub fn show(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    side_panel: &mut SidePanelState,
    parsers: &mut Vec<Option<IncrementalParser>>,
    pending_close: &mut Option<usize>,
    menu: &mut MenuBarState,
    editor_font: &mut EditorFont,
) {
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
                        eprintln!("failed to open project: {err}");
                    }
                }
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(state.active_tab.is_some(), egui::Button::new("Save").shortcut_text("Ctrl+S"))
                .clicked()
            {
                tabs::save_active_tab(state);
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
                    let mut visuals = egui::Visuals::light();
                    visuals.panel_fill = theme::RAYWHITE;
                    visuals.window_fill = theme::RAYWHITE;
                    visuals.extreme_bg_color = theme::RAYWHITE;
                    ui.ctx().set_visuals(visuals);
                    ui.close();
                }
                if ui.button("Dark").clicked() {
                    ui.ctx().set_visuals(egui::Visuals::dark());
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
        });

        ui.menu_button("Help", |ui| {
            if ui.button("About").clicked() {
                menu.about_open = true;
                ui.close();
            }
        });
    });

    show_about(ui, menu);
}

fn show_about(ui: &mut egui::Ui, menu: &mut MenuBarState) {
    if !menu.about_open {
        return;
    }
    let ctx = ui.ctx().clone();
    egui::Modal::new(egui::Id::new("about_dialog")).show(&ctx, |ui| {
        ui.heading("FoxGarden");
        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        ui.label("A light code editor for Java and Kotlin.");
        ui.separator();
        ui.label("Shortcuts:");
        ui.label("Ctrl+S — save the active tab");
        ui.label("Ctrl+Shift+T — reopen the last closed tab");
        ui.label("Middle-click a tab — close it");
        ui.separator();
        if ui.button("Close").clicked() {
            menu.about_open = false;
        }
    });
}
