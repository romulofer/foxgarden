use std::path::{Path, PathBuf};

use fg_core::{EditorState, Language};
use syntax::IncrementalParser;

use crate::panels::menu_bar::{self, MenuBarState};
use crate::panels::quick_switcher::{self, QuickSwitcherState};
use crate::panels::side_panel::{self, SidePanelState};
use crate::panels::tabs;
use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::style::theme;
use crate::widgets::editor::GenerateAccessorsDialog;
use crate::widgets::modal::show_modal;

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

const EDITOR_FONT_KEY: &str = "editor_font";
const FONT_SIZE_KEY: &str = "font_size";
const DARK_MODE_KEY: &str = "dark_mode";
const INDENT_USE_TABS_KEY: &str = "indent_use_tabs";
const INDENT_WIDTH_KEY: &str = "indent_width";

/// The editor's default code-font point size, before any Settings > Font
/// Size adjustment.
const DEFAULT_FONT_SIZE: f32 = 14.0;
/// Matches `egui::Visuals::default()`'s own `dark_mode: true` — the
/// out-of-the-box theme before any Settings > Theme choice or persisted
/// setting overrides it.
const DEFAULT_DARK_MODE: bool = true;

pub struct FoxGardenApp {
    state: EditorState,
    /// Kept index-aligned with `state.open_tabs`: one incremental parser per
    /// open document.
    parsers: Vec<Option<IncrementalParser>>,
    pending_close: Option<usize>,
    /// Open while the active tab's file has more than one class with
    /// eligible fields and "Generate Getters/Setters" was just requested —
    /// lets the user pick which class, then which fields, before
    /// generating (see `widgets::editor::GenerateAccessorsDialog`). A
    /// single eligible class skips this and generates immediately, so this
    /// is only ever `Some` when there was real ambiguity to resolve.
    generate_dialog: Option<GenerateAccessorsDialog>,
    /// Synthetic key events (Undo/Redo/Select All) queued by the editor's
    /// right-click menu, drained back into real input at the top of the
    /// very next frame — see `widgets::editor::show`'s doc comment on why
    /// that's the only way to drive those three from outside egui's own
    /// `TextEdit`.
    pending_editor_input: Vec<egui::Event>,
    side_panel: SidePanelState,
    menu_bar: MenuBarState,
    /// `Ctrl+E`'s recent-files popup.
    quick_switcher: QuickSwitcherState,
    editor_font: EditorFont,
    /// The editor's code-font point size, adjustable via Settings > Font
    /// Size. Independent of `editor_font` (the family) — both feed into
    /// `widgets::editor::show`'s `FontId`.
    font_size: f32,
    /// Mirrors whichever theme Settings > Theme last applied to
    /// `egui::Context`'s visuals — `eframe::App::save` has no `Context`
    /// access to read that back at persist time, so this field is the
    /// source of truth `persist_settings` reads instead.
    dark_mode: bool,
    /// Tabs-vs-spaces and indent width, adjustable via Settings >
    /// Indentation. Feeds `widgets::editor::show`'s auto-indent, Tab/
    /// Shift+Tab block indent/dedent, and plain-Tab-with-no-selection paths.
    indent_settings: IndentSettings,
    /// Hides the menu bar and side panel, leaving just the tab bar and
    /// editor. Toggled by `F11` (checked every frame, independent of
    /// whether the menu bar is currently shown — otherwise there'd be no
    /// way back out once the menu holding the toggle is itself hidden) or
    /// via View > Zen Mode while the menu is visible.
    zen_mode: bool,
    /// The most recent user-facing failure from any panel (open/save/rename/
    /// delete/create a file, open or refresh a project, restore last
    /// session, ...), shown as a dismissable modal (see `show_error_modal`).
    /// A single shared slot rather than one flag per failure kind: every
    /// site that used to just `eprintln!` — invisible outside a terminal
    /// the GUI user probably isn't watching — sets this instead, so a new
    /// failure kind never needs a new field, just another `*last_error =
    /// Some(...)` at the point it's detected. Last write wins if two
    /// failures somehow land the same frame, which in practice never
    /// happens since these all come from distinct, mutually exclusive user
    /// actions.
    last_error: Option<String>,
}

/// Opens `path` in a new tab (or focuses its existing tab, via
/// `EditorState::open_tab`'s own dedup), surfacing any failure through
/// `last_error`. Shared by the side panel's "open a file from the tree"
/// outcome and the recent-files quick switcher (`Ctrl+E`) — both just want
/// "open this path, tell the user if it didn't work," identically.
fn open_path(state: &mut EditorState, parsers: &mut Vec<Option<IncrementalParser>>, last_error: &mut Option<String>, path: PathBuf) {
    let display_path = path.display().to_string();
    match state.open_tab(path) {
        Ok(index) => {
            if index == parsers.len() {
                let parser = tabs::open_parser_for(&mut state.open_tabs[index]);
                parsers.push(parser);
            }
        }
        Err(err) => {
            // `OpenDocumentError::Binary`'s own `Display` already names the
            // path — restating it here would just duplicate it in the
            // modal, so only `Io` (whose message doesn't mention a path at
            // all) gets it prepended.
            let message = match &err {
                fg_core::OpenDocumentError::Binary(_) => format!("Couldn't open {display_path}: not a text file."),
                fg_core::OpenDocumentError::Io(_) => format!("Couldn't open {display_path}:\n{err}"),
            };
            *last_error = Some(message);
        }
    }
}

/// Closes every open tab whose path is `path` itself or starts inside it,
/// keeping `parsers` in lockstep — used when `path` was deleted out from
/// under the tree. A directory delete removes every file beneath it (via
/// `remove_dir_all`), so every tab pointing anywhere under it needs to
/// close, not just one pointing at `path` exactly; a plain file delete is
/// just the case where the only tab that can match is `path` itself.
/// Iterates back-to-front so removing an index never shifts the position
/// of one still to be checked.
fn close_tabs_under(state: &mut EditorState, parsers: &mut Vec<Option<IncrementalParser>>, path: &Path) {
    for index in (0..state.open_tabs.len()).rev() {
        if state.open_tabs[index].path().starts_with(path) {
            state.close_tab(index);
            parsers.remove(index);
        }
    }
}

/// Repoints every open tab under `old` to the equivalent path under `new`,
/// recreating each one's parser if the rename changed its language —
/// including to or from no language at all (e.g. `.java` -> `.kt`, or
/// `.java` -> `.txt`). A directory rename moves every file beneath it, so
/// every tab pointing anywhere under `old` needs repointing; `strip_prefix`
/// against a tab whose path *is* `old` (the plain file-rename case) yields
/// an empty suffix. `new.join(suffix)` on an empty suffix is *not* simply
/// `new`: it appends a trailing separator (e.g. `new/`), which `PathBuf`'s
/// `==` treats as equal to `new` but which the OS does not — a later
/// `std::fs::write` to that path fails with "Is a directory", since a
/// trailing separator tells the OS the path must resolve to one. So the
/// empty-suffix case is handled separately, without going through `join` at
/// all.
fn handle_rename(state: &mut EditorState, parsers: &mut [Option<IncrementalParser>], old: &Path, new: &Path) {
    for (index, doc) in state.open_tabs.iter_mut().enumerate() {
        let Ok(suffix) = doc.path().strip_prefix(old) else {
            continue;
        };
        let new_path = if suffix.as_os_str().is_empty() {
            new.to_path_buf()
        } else {
            new.join(suffix)
        };
        doc.path = new_path.clone();

        let new_language = new_path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(Language::from_extension);
        if new_language != doc.language {
            doc.language = new_language;
            parsers[index] = tabs::open_parser_for(doc);
        }
    }
}

/// Reopens whatever `persist_session` saved: the last project folder, its
/// open tabs (each given a parser, same as any other tab open), and which
/// one was focused. Free function (rather than inlined into `new`) so it's
/// testable against a fake `Storage` without needing a real
/// `eframe::CreationContext`, which isn't practically constructible in a
/// unit test.
fn restore_session(
    storage: &dyn eframe::Storage,
    state: &mut EditorState,
    parsers: &mut Vec<Option<IncrementalParser>>,
    last_error: &mut Option<String>,
) {
    if let Some(last_project) = storage.get_string(LAST_PROJECT_KEY) {
        let path = PathBuf::from(last_project);
        if path.is_dir() {
            if let Err(err) = state.open_project(path) {
                *last_error = Some(format!("failed to reopen last project: {err}"));
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
                Err(err) => *last_error = Some(format!("failed to reopen tab: {err}")),
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

/// Restores the font family/size, theme choice, and indentation style saved
/// by `persist_settings`, leaving each parameter at its current (default)
/// value if its key is missing or unparseable — same "best-effort, one
/// missing key doesn't block the rest" shape as `restore_session`. Separate
/// from that function because this is "how the editor looks/behaves," not
/// "what was open"; the two happen to both live in `eframe::Storage` but
/// are independent concerns.
fn restore_settings(
    storage: &dyn eframe::Storage,
    editor_font: &mut EditorFont,
    font_size: &mut f32,
    dark_mode: &mut bool,
    indent_settings: &mut IndentSettings,
) {
    if let Some(key) = storage.get_string(EDITOR_FONT_KEY) {
        if let Some(font) = EditorFont::from_storage_key(&key) {
            *editor_font = font;
        }
    }
    if let Some(size) = storage.get_string(FONT_SIZE_KEY).and_then(|s| s.parse::<f32>().ok()) {
        *font_size = size;
    }
    if let Some(dark) = storage.get_string(DARK_MODE_KEY) {
        *dark_mode = dark == "true";
    }
    if let Some(use_tabs) = storage.get_string(INDENT_USE_TABS_KEY) {
        indent_settings.use_tabs = use_tabs == "true";
    }
    if let Some(width) = storage.get_string(INDENT_WIDTH_KEY).and_then(|s| s.parse::<usize>().ok()) {
        indent_settings.width = width;
    }
}

/// Inverse of `restore_settings`.
fn persist_settings(
    storage: &mut dyn eframe::Storage,
    editor_font: EditorFont,
    font_size: f32,
    dark_mode: bool,
    indent_settings: IndentSettings,
) {
    storage.set_string(EDITOR_FONT_KEY, editor_font.storage_key().to_string());
    storage.set_string(FONT_SIZE_KEY, font_size.to_string());
    storage.set_string(DARK_MODE_KEY, dark_mode.to_string());
    storage.set_string(INDENT_USE_TABS_KEY, indent_settings.use_tabs.to_string());
    storage.set_string(INDENT_WIDTH_KEY, indent_settings.width.to_string());
}

impl FoxGardenApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
        let mut last_error = None;
        let mut editor_font = EditorFont::default();
        let mut font_size = DEFAULT_FONT_SIZE;
        let mut dark_mode = DEFAULT_DARK_MODE;
        let mut indent_settings = IndentSettings::default();

        if let Some(storage) = cc.storage {
            restore_session(storage, &mut state, &mut parsers, &mut last_error);
            restore_settings(storage, &mut editor_font, &mut font_size, &mut dark_mode, &mut indent_settings);
        }
        theme::apply(&cc.egui_ctx, dark_mode);

        Self {
            state,
            parsers,
            pending_close: None,
            generate_dialog: None,
            pending_editor_input: Vec::new(),
            side_panel: SidePanelState::default(),
            menu_bar: MenuBarState::default(),
            quick_switcher: QuickSwitcherState::default(),
            editor_font,
            font_size,
            dark_mode,
            indent_settings,
            zen_mode: false,
            last_error,
        }
    }
}

/// Shows `last_error` (if any) as a dismissable modal, and clears it once
/// acknowledged. Free function rather than a method so its borrow of
/// `last_error` doesn't overlap `&mut self` for the rest of `ui()`.
///
/// Borrows the message for the label instead of cloning it — `show_modal`
/// returning the closure's result (whether "OK" was clicked) is what lets
/// the actual `*last_error = None` write happen after `show_modal` returns,
/// once the borrow of `last_error` used for `message` has ended, without
/// needing a separate `dismissed` flag mutated from inside the closure.
fn show_error_modal(ui: &egui::Ui, last_error: &mut Option<String>) {
    let message = last_error.as_deref();
    let dismissed = show_modal(ui, "error_modal", message, |ui, message| {
        ui.label(*message);
        ui.button("OK").clicked()
    });
    if dismissed == Some(true) {
        *last_error = None;
    }
}

impl eframe::App for FoxGardenApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.input(|i| i.key_pressed(egui::Key::F11)) {
            self.zen_mode = !self.zen_mode;
        }
        if ui.input(|i| i.key_pressed(egui::Key::E) && i.modifiers.command) {
            self.quick_switcher.toggle();
        }

        let mut outcome = side_panel::SidePanelOutcome::default();
        let mut menu_outcome = menu_bar::MenuBarOutcome::default();

        if !self.zen_mode {
            menu_outcome = egui::Panel::top("menu_bar")
                .show(ui, |ui| {
                    menu_bar::show(
                        ui,
                        &mut self.state,
                        &mut self.side_panel,
                        &mut self.parsers,
                        &mut self.pending_close,
                        &mut self.menu_bar,
                        &mut self.editor_font,
                        &mut self.font_size,
                        &mut self.dark_mode,
                        &mut self.indent_settings,
                        &mut self.zen_mode,
                        &mut self.last_error,
                    )
                })
                .inner;

            outcome = egui::Panel::left("project_panel")
                .show(ui, |ui| side_panel::show(ui, &mut self.state, &mut self.side_panel))
                .inner;
        }

        if let Some(path) = outcome.open {
            open_path(&mut self.state, &mut self.parsers, &mut self.last_error, path);
        }
        if let Some((old, new)) = outcome.renamed {
            handle_rename(&mut self.state, &mut self.parsers, &old, &new);
        }
        if let Some(path) = outcome.deleted {
            close_tabs_under(&mut self.state, &mut self.parsers, &path);
        }
        if let Some(err) = outcome.error {
            self.last_error = Some(err);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            tabs::show(
                ui,
                &mut self.state,
                &mut self.pending_close,
                &mut self.parsers,
                self.editor_font,
                self.font_size,
                self.indent_settings,
                menu_outcome.generate_request,
                &mut self.generate_dialog,
                menu_outcome.case_conversion_request,
                &mut self.last_error,
                &mut self.pending_editor_input,
            );
        });

        if let Some(path) = quick_switcher::show(ui, &self.state, &mut self.quick_switcher) {
            open_path(&mut self.state, &mut self.parsers, &mut self.last_error, path);
        }

        show_error_modal(ui, &mut self.last_error);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        persist_session(storage, &self.state);
        persist_settings(storage, self.editor_font, self.font_size, self.dark_mode, self.indent_settings);
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
        restore_session(&storage, &mut restored_state, &mut restored_parsers, &mut None);

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
        restore_session(&storage, &mut state, &mut parsers, &mut None);

        assert_eq!(state.open_tabs.len(), 1);
        assert_eq!(state.open_tabs[0].path(), kept.as_path());
        assert_eq!(parsers.len(), 1);
    }

    #[test]
    fn restore_session_with_no_saved_keys_is_a_no_op() {
        let storage = FakeStorage::default();
        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();

        restore_session(&storage, &mut state, &mut parsers, &mut None);

        assert!(state.open_tabs.is_empty());
        assert!(parsers.is_empty());
        assert_eq!(state.active_tab, None);
    }

    #[test]
    fn persisted_settings_round_trip() {
        let mut storage = FakeStorage::default();
        let saved_indent = IndentSettings { use_tabs: true, width: 2 };
        persist_settings(&mut storage, EditorFont::Default, 22.5, false, saved_indent);

        let mut editor_font = EditorFont::JetBrainsMono;
        let mut font_size = DEFAULT_FONT_SIZE;
        let mut dark_mode = DEFAULT_DARK_MODE;
        let mut indent_settings = IndentSettings::default();
        restore_settings(&storage, &mut editor_font, &mut font_size, &mut dark_mode, &mut indent_settings);

        assert_eq!(editor_font, EditorFont::Default);
        assert_eq!(font_size, 22.5);
        assert!(!dark_mode);
        assert_eq!(indent_settings, saved_indent);
    }

    #[test]
    fn restore_settings_with_no_saved_keys_leaves_defaults_untouched() {
        let storage = FakeStorage::default();
        let mut editor_font = EditorFont::default();
        let mut font_size = DEFAULT_FONT_SIZE;
        let mut dark_mode = DEFAULT_DARK_MODE;
        let mut indent_settings = IndentSettings::default();

        restore_settings(&storage, &mut editor_font, &mut font_size, &mut dark_mode, &mut indent_settings);

        assert_eq!(editor_font, EditorFont::default());
        assert_eq!(font_size, DEFAULT_FONT_SIZE);
        assert_eq!(dark_mode, DEFAULT_DARK_MODE);
        assert_eq!(indent_settings, IndentSettings::default());
    }

    #[test]
    fn restore_settings_ignores_an_unparseable_font_size() {
        let mut storage = FakeStorage::default();
        storage.set_string(FONT_SIZE_KEY, "not-a-number".to_string());
        let mut editor_font = EditorFont::default();
        let mut font_size = DEFAULT_FONT_SIZE;
        let mut dark_mode = DEFAULT_DARK_MODE;
        let mut indent_settings = IndentSettings::default();

        restore_settings(&storage, &mut editor_font, &mut font_size, &mut dark_mode, &mut indent_settings);

        assert_eq!(font_size, DEFAULT_FONT_SIZE);
    }

    #[test]
    fn restore_settings_ignores_an_unparseable_indent_width() {
        let mut storage = FakeStorage::default();
        storage.set_string(INDENT_WIDTH_KEY, "not-a-number".to_string());
        let mut editor_font = EditorFont::default();
        let mut font_size = DEFAULT_FONT_SIZE;
        let mut dark_mode = DEFAULT_DARK_MODE;
        let mut indent_settings = IndentSettings::default();

        restore_settings(&storage, &mut editor_font, &mut font_size, &mut dark_mode, &mut indent_settings);

        assert_eq!(indent_settings.width, IndentSettings::default().width);
    }

    #[test]
    fn close_tabs_under_closes_every_tab_inside_a_deleted_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("pkg/sub")).unwrap();
        let a = java_file(&dir, "pkg/A.java");
        let b = java_file(&dir, "pkg/sub/B.java");
        let root = java_file(&dir, "Root.java");

        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
        for path in [&a, &b, &root] {
            state.open_tab(path.clone()).unwrap();
            parsers.push(None);
        }

        close_tabs_under(&mut state, &mut parsers, &dir.path().join("pkg"));

        let remaining: Vec<&Path> = state.open_tabs.iter().map(|doc| doc.path()).collect();
        assert_eq!(remaining, vec![root.as_path()]);
        assert_eq!(parsers.len(), 1);
    }

    #[test]
    fn close_tabs_under_closes_a_single_file_by_exact_path() {
        let dir = tempfile::tempdir().unwrap();
        let a = java_file(&dir, "A.java");

        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
        state.open_tab(a.clone()).unwrap();
        parsers.push(None);

        close_tabs_under(&mut state, &mut parsers, &a);

        assert!(state.open_tabs.is_empty());
        assert!(parsers.is_empty());
    }

    #[test]
    fn handle_rename_repoints_every_tab_inside_a_renamed_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("old_pkg/sub")).unwrap();
        let a = java_file(&dir, "old_pkg/A.java");
        let b = java_file(&dir, "old_pkg/sub/B.java");

        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
        for path in [&a, &b] {
            let index = state.open_tab(path.clone()).unwrap();
            parsers.push(tabs::open_parser_for(&mut state.open_tabs[index]));
        }

        let old_pkg = dir.path().join("old_pkg");
        let new_pkg = dir.path().join("new_pkg");
        handle_rename(&mut state, &mut parsers, &old_pkg, &new_pkg);

        let repointed: Vec<PathBuf> = state.open_tabs.iter().map(|doc| doc.path().to_path_buf()).collect();
        assert_eq!(repointed, vec![new_pkg.join("A.java"), new_pkg.join("sub/B.java")]);
    }

    #[test]
    fn renaming_the_open_file_itself_leaves_it_saveable() {
        let dir = tempfile::tempdir().unwrap();
        let a = java_file(&dir, "Old.java");

        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
        let index = state.open_tab(a.clone()).unwrap();
        parsers.push(tabs::open_parser_for(&mut state.open_tabs[index]));
        state.open_tabs[index].buffer.insert(0, "// edited\n");

        let new_path = dir.path().join("New.java");
        std::fs::rename(&a, &new_path).unwrap();
        handle_rename(&mut state, &mut parsers, &a, &new_path);

        let mut last_error = None;
        tabs::save_active_tab(&mut state, &mut parsers, &mut last_error);

        assert_eq!(last_error, None, "save produced an error: {last_error:?}");
        assert_eq!(std::fs::read_to_string(&new_path).unwrap(), "// edited\nclass Old.java {}");
    }

    #[test]
    fn handle_rename_repoints_a_single_tab_by_exact_path() {
        let dir = tempfile::tempdir().unwrap();
        let a = java_file(&dir, "Old.java");

        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
        state.open_tab(a.clone()).unwrap();
        parsers.push(tabs::open_parser_for(&mut state.open_tabs[0]));

        let new_path = dir.path().join("New.java");
        handle_rename(&mut state, &mut parsers, &a, &new_path);

        assert_eq!(state.open_tabs[0].path(), new_path.as_path());
        // `Path`'s `==` normalizes away a trailing separator, so it alone
        // wouldn't have caught `new.join("")` producing "New.java/" — the
        // OS-facing representation has to be checked too.
        assert_eq!(state.open_tabs[0].path().as_os_str(), new_path.as_os_str());
    }
}
