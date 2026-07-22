use std::path::{Path, PathBuf};

use fg_core::{EditorState, Language};
use syntax::IncrementalParser;

use crate::fonts::EditorFont;
use crate::menu_bar::{self, MenuBarState};
use crate::side_panel::{self, SidePanelState};
use crate::tabs;

const LAST_PROJECT_KEY: &str = "last_project";

pub struct FoxGardenApp {
    state: EditorState,
    /// Kept index-aligned with `state.open_tabs`: one incremental parser per
    /// open document.
    parsers: Vec<IncrementalParser>,
    pending_close: Option<usize>,
    side_panel: SidePanelState,
    menu_bar: MenuBarState,
    editor_font: EditorFont,
}

/// Closes the tab pointing at `path`, if any, keeping `parsers` in lockstep —
/// used when the underlying file was deleted out from under an open tab.
fn close_tab_for_path(state: &mut EditorState, parsers: &mut Vec<IncrementalParser>, path: &Path) {
    if let Some(index) = state.find_tab(path) {
        state.close_tab(index);
        parsers.remove(index);
    }
}

/// Repoints any tab open on `old` to `new`, recreating its parser if the
/// rename changed the file's language (e.g. `.java` -> `.kt`).
fn handle_rename(state: &mut EditorState, parsers: &mut Vec<IncrementalParser>, old: &Path, new: &Path) {
    let Some(index) = state.find_tab(old) else {
        return;
    };
    state.open_tabs[index].path = new.to_path_buf();

    let new_language = new
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(Language::from_extension);
    if let Some(new_language) = new_language {
        if new_language != state.open_tabs[index].language {
            state.open_tabs[index].language = new_language;
            parsers[index] = tabs::open_parser_for(&mut state.open_tabs[index]);
        }
    }
}

impl FoxGardenApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state = EditorState::new();

        if let Some(storage) = cc.storage {
            if let Some(last_project) = storage.get_string(LAST_PROJECT_KEY) {
                let path = PathBuf::from(last_project);
                if path.is_dir() {
                    if let Err(err) = state.open_project(path) {
                        eprintln!("failed to reopen last project: {err}");
                    }
                }
            }
        }

        Self {
            state,
            parsers: Vec::new(),
            pending_close: None,
            side_panel: SidePanelState::default(),
            menu_bar: MenuBarState::default(),
            editor_font: EditorFont::default(),
        }
    }
}

impl eframe::App for FoxGardenApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("menu_bar").show(ui, |ui| {
            menu_bar::show(
                ui,
                &mut self.state,
                &mut self.side_panel,
                &mut self.parsers,
                &mut self.pending_close,
                &mut self.menu_bar,
                &mut self.editor_font,
            );
        });

        let outcome = egui::Panel::left("project_panel")
            .show(ui, |ui| side_panel::show(ui, &mut self.state, &mut self.side_panel))
            .inner;

        if let Some(path) = outcome.open {
            match self.state.open_tab(path) {
                Ok(index) => {
                    if index == self.parsers.len() {
                        let parser = tabs::open_parser_for(&mut self.state.open_tabs[index]);
                        self.parsers.push(parser);
                    }
                }
                Err(err) => eprintln!("failed to open file: {err}"),
            }
        }
        if let Some((old, new)) = outcome.renamed {
            handle_rename(&mut self.state, &mut self.parsers, &old, &new);
        }
        if let Some(path) = outcome.deleted {
            close_tab_for_path(&mut self.state, &mut self.parsers, &path);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            tabs::show(
                ui,
                &mut self.state,
                &mut self.pending_close,
                &mut self.parsers,
                self.editor_font,
            );
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Some(project) = &self.state.project {
            storage.set_string(LAST_PROJECT_KEY, project.root.to_string_lossy().into_owned());
        }
    }
}
