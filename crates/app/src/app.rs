use std::path::{Path, PathBuf};

use fg_core::{EditorState, Language};
use syntax::IncrementalParser;

use crate::panels::menu_bar::{self, MenuBarState};
use crate::panels::side_panel::{self, SidePanelState};
use crate::panels::tabs;
use crate::style::fonts::EditorFont;

const LAST_PROJECT_KEY: &str = "last_project";
/// Newline-joined absolute paths of the tabs that were open at last exit, in
/// tab order. Newline-joined rather than a structured format since neither
/// `eframe::Storage` nor this crate currently pulls in `serde` — a file path
/// can't itself contain a `\n`, so this is an unambiguous, dependency-free
/// encoding.
const OPEN_TABS_KEY: &str = "open_tabs";
/// The path of whichever tab was focused at last exit, so restoring session
/// doesn't just reopen the same tabs but land on the first one arbitrarily.
const ACTIVE_TAB_KEY: &str = "active_tab";

pub struct FoxGardenApp {
    state: EditorState,
    /// Kept index-aligned with `state.open_tabs`: one incremental parser per
    /// open document.
    parsers: Vec<Option<IncrementalParser>>,
    pending_close: Option<usize>,
    side_panel: SidePanelState,
    menu_bar: MenuBarState,
    editor_font: EditorFont,
    /// Hides the menu bar and side panel, leaving just the tab bar and
    /// editor. Toggled by `F11` (checked every frame, independent of
    /// whether the menu bar is currently shown — otherwise there'd be no
    /// way back out once the menu holding the toggle is itself hidden) or
    /// via View > Zen Mode while the menu is visible.
    zen_mode: bool,
}

/// Closes the tab pointing at `path`, if any, keeping `parsers` in lockstep —
/// used when the underlying file was deleted out from under an open tab.
fn close_tab_for_path(state: &mut EditorState, parsers: &mut Vec<Option<IncrementalParser>>, path: &Path) {
    if let Some(index) = state.find_tab(path) {
        state.close_tab(index);
        parsers.remove(index);
    }
}

/// Repoints any tab open on `old` to `new`, recreating its parser if the
/// rename changed the file's language — including to or from no language at
/// all (e.g. `.java` -> `.kt`, or `.java` -> `.txt`).
fn handle_rename(state: &mut EditorState, parsers: &mut Vec<Option<IncrementalParser>>, old: &Path, new: &Path) {
    let Some(index) = state.find_tab(old) else {
        return;
    };
    state.open_tabs[index].path = new.to_path_buf();

    let new_language = new
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(Language::from_extension);
    if new_language != state.open_tabs[index].language {
        state.open_tabs[index].language = new_language;
        parsers[index] = tabs::open_parser_for(&mut state.open_tabs[index]);
    }
}

/// Reopens whatever `persist_session` saved: the last project folder, its
/// open tabs (each given a parser, same as any other tab open), and which
/// one was focused. Free function (rather than inlined into `new`) so it's
/// testable against a fake `Storage` without needing a real
/// `eframe::CreationContext`, which isn't practically constructible in a
/// unit test.
fn restore_session(storage: &dyn eframe::Storage, state: &mut EditorState, parsers: &mut Vec<Option<IncrementalParser>>) {
    if let Some(last_project) = storage.get_string(LAST_PROJECT_KEY) {
        let path = PathBuf::from(last_project);
        if path.is_dir() {
            if let Err(err) = state.open_project(path) {
                eprintln!("failed to reopen last project: {err}");
            }
        }
    }

    if let Some(open_tabs) = storage.get_string(OPEN_TABS_KEY) {
        for path_str in open_tabs.lines() {
            let path = PathBuf::from(path_str);
            if !path.is_file() {
                // Deleted, moved, or on since-unmounted storage since last
                // exit — just skip it rather than surfacing an error for a
                // tab the user can't act on.
                continue;
            }
            match state.open_tab(path) {
                Ok(index) => {
                    if index == parsers.len() {
                        let parser = tabs::open_parser_for(&mut state.open_tabs[index]);
                        parsers.push(parser);
                    }
                }
                Err(err) => eprintln!("failed to reopen tab: {err}"),
            }
        }
    }

    if let Some(active_path) = storage.get_string(ACTIVE_TAB_KEY) {
        if let Some(index) = state.find_tab(Path::new(&active_path)) {
            state.focus_tab(index);
        }
    }
}

/// Inverse of `restore_session`: writes the project folder, open tab paths
/// (in tab order), and the focused tab's path, so the next launch can
/// reconstruct the same session.
fn persist_session(storage: &mut dyn eframe::Storage, state: &EditorState) {
    if let Some(project) = &state.project {
        storage.set_string(LAST_PROJECT_KEY, project.root.to_string_lossy().into_owned());
    }

    let open_tabs = state
        .open_tabs
        .iter()
        .map(|doc| doc.path().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    storage.set_string(OPEN_TABS_KEY, open_tabs);

    let active_tab = state
        .active_tab
        .and_then(|index| state.open_tabs.get(index))
        .map(|doc| doc.path().to_string_lossy().into_owned())
        .unwrap_or_default();
    storage.set_string(ACTIVE_TAB_KEY, active_tab);
}

impl FoxGardenApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();

        if let Some(storage) = cc.storage {
            restore_session(storage, &mut state, &mut parsers);
        }

        Self {
            state,
            parsers,
            pending_close: None,
            side_panel: SidePanelState::default(),
            menu_bar: MenuBarState::default(),
            editor_font: EditorFont::default(),
            zen_mode: false,
        }
    }
}

impl eframe::App for FoxGardenApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.input(|i| i.key_pressed(egui::Key::F11)) {
            self.zen_mode = !self.zen_mode;
        }

        let mut outcome = side_panel::SidePanelOutcome::default();

        if !self.zen_mode {
            egui::Panel::top("menu_bar").show(ui, |ui| {
                menu_bar::show(
                    ui,
                    &mut self.state,
                    &mut self.side_panel,
                    &mut self.parsers,
                    &mut self.pending_close,
                    &mut self.menu_bar,
                    &mut self.editor_font,
                    &mut self.zen_mode,
                );
            });

            outcome = egui::Panel::left("project_panel")
                .show(ui, |ui| side_panel::show(ui, &mut self.state, &mut self.side_panel))
                .inner;
        }

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
        persist_session(storage, &self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::Storage as _;
    use std::collections::HashMap;

    /// Minimal in-memory `eframe::Storage`, so `restore_session`/
    /// `persist_session` can be tested without a real `CreationContext`
    /// (which needs a live windowing/render backend to construct).
    #[derive(Default)]
    struct FakeStorage(HashMap<String, String>);

    impl eframe::Storage for FakeStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_string(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    fn java_file(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, format!("class {name} {{}}")).unwrap();
        path
    }

    #[test]
    fn persisted_session_round_trips_open_tabs_and_active_tab() {
        let dir = tempfile::tempdir().unwrap();
        let a = java_file(&dir, "A.java");
        let b = java_file(&dir, "B.java");

        let mut state = EditorState::new();
        state.open_tab(a.clone()).unwrap();
        state.open_tab(b.clone()).unwrap();
        state.focus_tab(0); // B was opened last (and thus focused); explicitly refocus A

        let mut storage = FakeStorage::default();
        persist_session(&mut storage, &state);

        let mut restored_state = EditorState::new();
        let mut restored_parsers: Vec<Option<IncrementalParser>> = Vec::new();
        restore_session(&storage, &mut restored_state, &mut restored_parsers);

        let restored_paths: Vec<&Path> = restored_state.open_tabs.iter().map(|doc| doc.path()).collect();
        assert_eq!(restored_paths, vec![a.as_path(), b.as_path()]);
        assert_eq!(restored_parsers.len(), 2);
        assert_eq!(restored_state.active_tab, Some(0));
        assert_eq!(restored_state.open_tabs[0].path(), a.as_path());
    }

    #[test]
    fn restore_session_skips_tabs_whose_file_no_longer_exists() {
        let dir = tempfile::tempdir().unwrap();
        let kept = java_file(&dir, "Kept.java");
        let deleted = dir.path().join("Deleted.java");

        let mut storage = FakeStorage::default();
        let open_tabs = format!("{}\n{}", kept.display(), deleted.display());
        storage.set_string(OPEN_TABS_KEY, open_tabs);

        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
        restore_session(&storage, &mut state, &mut parsers);

        assert_eq!(state.open_tabs.len(), 1);
        assert_eq!(state.open_tabs[0].path(), kept.as_path());
        assert_eq!(parsers.len(), 1);
    }

    #[test]
    fn restore_session_with_no_saved_keys_is_a_no_op() {
        let storage = FakeStorage::default();
        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();

        restore_session(&storage, &mut state, &mut parsers);

        assert!(state.open_tabs.is_empty());
        assert!(parsers.is_empty());
        assert_eq!(state.active_tab, None);
    }
}
