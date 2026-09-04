//! The end-to-end harness: launches a real `FoxGardenApp` against a real
//! temp project and drives it through real frames. Every topic module in
//! [`super`] goes through [`E2e`] and nothing else — no test builds its own
//! `Harness`, so how the app is launched (fonts installed, project opened,
//! root expanded) is described once, here.

use super::super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use tempfile::TempDir;

/// Contents used by any test that just needs *some* well-formed Java file.
/// A test that cares about the specifics (a syntax error, a particular
/// offset to type at) writes its own fixture instead.
pub(super) const MAIN_JAVA: &str = "class Main {\n}\n";

/// A running app plus the temp project it was launched against. The
/// `TempDir` is kept alive alongside the harness — dropping it deletes the
/// project out from under the still-running app, which is the file watcher's
/// "externally deleted" path, not what any test here is trying to exercise.
pub(super) struct E2e {
    harness: Harness<'static, FoxGardenApp>,
    dir: TempDir,
}

impl E2e {
    /// Launches the app against a fresh temp project containing `files`
    /// (`(relative path, contents)`), with that project already open — the
    /// state every test here starts from.
    pub(super) fn launch(files: &[(&str, &str)]) -> Self {
        let dir = test_support::tempdir();
        for (name, contents) in files {
            test_support::write_file(dir.path(), name, contents);
        }

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_eframe(|cc| {
                // Exactly what `main` does before constructing the app: the
                // editor renders in `FontFamily::Name("JetBrainsMono")`, and
                // painting a frame with that family unbound panics inside
                // epaint.
                crate::style::fonts::install(&cc.egui_ctx);
                FoxGardenApp::new(cc)
            });

        harness
            .state_mut()
            .state
            .open_project(dir.path().to_path_buf())
            .expect("open temp project");

        let mut app = Self { harness, dir };
        app.settle();
        // Every directory row starts collapsed (`render_node`'s
        // `load_with_default_open(..., false)`), including the project root
        // itself, so a freshly-opened project shows exactly one row. Expand
        // it here so tests start where a user does one click in: looking at
        // the project's files. `project_tree`'s own
        // `collapsing_the_project_root_hides_its_files` covers the toggle.
        app.toggle_project_root();
        // Expanding it also *selected* it (a plain click on any tree row
        // collapses the selection down to that row), and a selected root
        // silently joins every later multi-target operation — a Delete
        // aimed at two Ctrl+clicked files would offer to delete three
        // things, the project directory included. Ctrl+click toggles it
        // back out without collapsing the row again, leaving tests with an
        // expanded tree and an empty selection.
        let root = app.project_root_label();
        app.harness
            .get_by_label_contains(&root)
            .click_modifiers(egui::Modifiers::COMMAND);
        app.settle();
        app
    }

    /// Enough of the project root row's label to identify it: the temp
    /// directory's own (per-run unique) name. Matched as a fragment rather
    /// than in full because the row also carries a folder icon and, for a
    /// root whose only child is a directory, that child's name too
    /// (`collapse_chain`).
    fn project_root_label(&self) -> String {
        self.dir
            .path()
            .file_name()
            .expect("temp dir has a name")
            .to_string_lossy()
            .into_owned()
    }

    /// Clicks the project root's row in the tree, expanding or collapsing
    /// it.
    pub(super) fn toggle_project_root(&mut self) {
        let root = self.project_root_label();
        self.harness.get_by_label_contains(&root).click();
        self.settle();
    }

    /// Runs frames until nothing more is requested. `run_ok` rather than
    /// `run`: a frame that leaves an animation or a background poll running
    /// (the caret blink, an in-flight `git diff`) would make `run` panic on
    /// its step limit, and none of that is a test failure — the accessibility
    /// tree is already up to date either way.
    fn settle(&mut self) {
        self.harness.run_ok();
    }

    pub(super) fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Clicks the one widget labelled `label` — a project-tree row, a tab, a
    /// button in a modal. Panics if no such widget is on screen, which is
    /// itself the assertion most tests here want out of a click.
    pub(super) fn click(&mut self, label: &str) {
        self.harness.get_by_label(label).click();
        self.settle();
    }

    /// Right-clicks the one widget labelled `label`, opening its context
    /// menu — the project tree's per-row New File/Rename/Delete/Copy/Cut,
    /// the tab bar's Read-Only toggle.
    pub(super) fn click_secondary(&mut self, label: &str) {
        self.harness.get_by_label(label).click_secondary();
        self.settle();
    }

    /// Clicks `label` with `modifiers` held — the project tree's explicit
    /// Ctrl/Shift multi-select gestures, which deliberately behave
    /// differently from a plain click (they select without opening).
    pub(super) fn click_modifiers(&mut self, label: &str, modifiers: egui::Modifiers) {
        self.harness.get_by_label(label).click_modifiers(modifiers);
        self.settle();
    }

    /// Clicks the one widget whose label *contains* `fragment`. Menu items
    /// carry decoration a caller shouldn't have to reproduce — a shortcut
    /// hint (`Save Ctrl+S`), a submenu arrow (`Theme ⏵`) — so menus are
    /// driven by fragment; everything else uses the exact [`Self::click`].
    /// Still panics on an ambiguous match, so a fragment matching two
    /// widgets fails loudly rather than clicking an arbitrary one.
    pub(super) fn click_containing(&mut self, fragment: &str) {
        self.harness.get_by_label_contains(fragment).click();
        self.settle();
    }

    /// Right-clicks the one widget whose label contains `fragment` — the
    /// fragment counterpart to [`Self::click_secondary`], for tree rows
    /// whose label carries a folder icon that changes as they expand.
    pub(super) fn click_secondary_containing(&mut self, fragment: &str) {
        self.harness.get_by_label_contains(fragment).click_secondary();
        self.settle();
    }

    /// Opens `menu` in the menu bar and clicks `item` inside it. Two clicks
    /// with a frame between them, exactly as a user does it — a menu's items
    /// don't exist in the accessibility tree until the menu is open. `item`
    /// is matched by fragment (see [`Self::click_containing`]); for a
    /// submenu, call this for the submenu itself and then `click` its item.
    pub(super) fn menu(&mut self, menu: &str, item: &str) {
        self.click(menu);
        // A menu item's own label carries its shortcut too ("Salvar
        // Ctrl+S"), so this can only match by fragment — and "Salvar" is a
        // fragment of "Salvar Todos" as well. The shortest matching label
        // is the item actually named, rather than one that merely starts
        // with the same words.
        let shortest = self
            .harness
            .query_all_by_label_contains(item)
            .filter_map(|node| egui_kittest::kittest::NodeT::accesskit_node(&node).label().map(|l| l.to_owned()))
            .min_by_key(|label| label.len());
        match shortest {
            Some(label) => self.click(&label),
            None => self.click_containing(item),
        }
    }

    /// Clicks the checkbox labelled `label`, ignoring any plain text that
    /// happens to read the same. Needed where a View menu entry and the
    /// panel it toggles share a name — with the Source Control panel open,
    /// "Source Control" is both a menu checkbox and the panel's own heading,
    /// and only the checkbox is clickable.
    pub(super) fn click_checkbox(&mut self, label: &str) {
        self.harness
            .get_by_role_and_label(accesskit::Role::CheckBox, label)
            .click();
        self.settle();
    }

    /// Whether any widget currently on screen is labelled exactly `label`.
    /// The project tree/tab label for `file_name` — icon plus name, using
    /// the same `style::icons` mapping the UI itself does, so a test names
    /// a row the way a user sees it without hard-coding a glyph that would
    /// have to be updated here every time the icon set changes.
    pub(super) fn row(file_name: &str) -> String {
        format!("{} {file_name}", crate::style::icons::for_file(std::path::Path::new(file_name)))
    }

    /// The tab label for `file_name` with unsaved changes — the same row,
    /// plus the trailing dot the tab bar marks a dirty buffer with.
    /// A collapsed directory row's label: the closed-folder icon plus
    /// `name`. (An expanded row carries the open-folder icon instead, which
    /// is why tests that don't care either way match on the name alone.)
    pub(super) fn folder_row(name: &str) -> String {
        format!("{} {name}", crate::style::icons::FOLDER)
    }

    /// How many widgets carry `file_name`'s row label — 1 when it's only in
    /// the project tree, 2 once it also has a tab.
    pub(super) fn label_count(&self, file_name: &str) -> usize {
        self.harness.query_all_by_label(&Self::row(file_name)).count()
    }

    pub(super) fn dirty_row(file_name: &str) -> String {
        format!("{} {}", Self::row(file_name), crate::style::icons::UNSAVED)
    }

    /// The tab label for a read-only `file_name`: a leading lock.
    pub(super) fn read_only_row(file_name: &str) -> String {
        format!("{} {}", crate::style::icons::LOCK, Self::row(file_name))
    }

    /// A read-only tab that also has unsaved changes — which should never
    /// happen, and is exactly what the read-only tests assert against.
    pub(super) fn dirty_read_only_row(file_name: &str) -> String {
        format!("{} {}", Self::read_only_row(file_name), crate::style::icons::UNSAVED)
    }

    /// `query_all_`, not `query_`: a file open in a tab is labelled
    /// identically in the tab bar and in the project tree (same icon, same
    /// name), and the singular query panics on more than one match — which
    /// for "is this on screen?" is a false failure.
    pub(super) fn shows(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }

    /// Clicks `file_name`'s row in the project tree. The tree is laid out
    /// before the tab bar, so the first node carrying the label is the tree
    /// row and the last is the tab — which is what lets a test aim at one
    /// or the other while both are on screen with the same label.
    pub(super) fn click_tree(&mut self, file_name: &str) {
        let label = Self::row(file_name);
        self.harness.get_all_by_label(&label).next().expect("a tree row for this file").click();
        self.settle();
    }

    /// Right-clicks `file_name`'s row in the project tree, opening the
    /// tree's own context menu (New File/Rename/Delete/Copy/Cut) — not the
    /// tab bar's, which carries a different set of entries under the same
    /// label. See `click_tree`.
    pub(super) fn click_tree_secondary(&mut self, file_name: &str) {
        let label = Self::row(file_name);
        self.harness
            .get_all_by_label(&label)
            .next()
            .expect("a tree row for this file")
            .click_secondary();
        self.settle();
    }

    pub(super) fn click_tab_secondary(&mut self, file_name: &str) {
        let label = Self::row(file_name);
        // `contains`, not an exact match: a tab carries decoration the
        // tree row doesn't (the unsaved dot, a read-only lock), so an exact
        // label would silently fall back to the tree row and open the
        // wrong context menu.
        self.harness
            .get_all_by_label_contains(&label)
            .last()
            .expect("a tab for this file")
            .click_secondary();
        self.settle();
    }

    pub(super) fn click_tab_middle(&mut self, file_name: &str) {
        let label = Self::row(file_name);
        self.harness
            .get_all_by_label_contains(&label)
            .last()
            .expect("a tab for this file")
            .click_button(egui::PointerButton::Middle);
        self.settle();
    }

    /// Whether any widget's label *contains* `fragment` — for rows whose
    /// full label carries extra decoration a test shouldn't have to spell
    /// out (a completion row's type detail, a tree row's collapsed
    /// directory chain).
    /// `query_all_`, not `query_`: the singular query panics when a fragment
    /// matches more than one widget, which for a plain "is this on screen?"
    /// check is a false failure — two matches is still yes.
    pub(super) fn shows_containing(&self, fragment: &str) -> bool {
        self.harness.query_all_by_label_contains(fragment).next().is_some()
    }

    /// Focuses the active tab's editor, puts the caret at `char_offset`, and
    /// types `text` — one real frame per character, the way real typing
    /// arrives.
    pub(super) fn type_into_active_tab(&mut self, char_offset: usize, text: &str) {
        let state = &self.harness.state().state;
        let active = state.active_tab.expect("a tab must be active to type into");
        let path = state.open_tabs[active].path.clone();
        jump_to(&self.harness.ctx, &path, char_offset);
        for ch in text.chars() {
            self.harness.event(egui::Event::Text(ch.to_string()));
        }
        self.settle();
    }

    /// Types into whatever text field currently has focus — the side panel's
    /// New File row, the `Ctrl+P` popup's query box. Unlike
    /// `type_into_active_tab` this needs no id: a focused `egui::TextEdit`
    /// consumes `Event::Text` on its own.
    pub(super) fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.harness.event(egui::Event::Text(ch.to_string()));
        }
        self.settle();
    }

    /// Selects everything in the focused text field and types `text` over
    /// it — for the tree's Rename field, which opens pre-filled with the
    /// current name, so plain typing would append to it rather than replace
    /// it.
    pub(super) fn replace_focused_text(&mut self, text: &str) {
        self.press(egui::Modifiers::COMMAND, egui::Key::A);
        self.type_text(text);
    }

    pub(super) fn press(&mut self, modifiers: egui::Modifiers, key: egui::Key) {
        self.harness.key_press_modifiers(modifiers, key);
        self.settle();
    }

    /// Clicks the `index`th tab's close button. Every tab renders its own
    /// button labelled `x`, so they can only be told apart by position —
    /// accessibility-tree order is tab-bar order.
    pub(super) fn close_tab(&mut self, index: usize) {
        let close = crate::style::icons::CLOSE.to_string();
        let buttons = self.harness.get_all_by_label(&close).collect::<Vec<_>>();
        assert!(index < buttons.len(), "no close button for tab {index}");
        buttons[index].click();
        self.settle();
    }

    pub(super) fn open_tab_names(&self) -> Vec<String> {
        self.harness
            .state()
            .state
            .open_tabs
            .iter()
            .map(|doc| {
                doc.path()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    pub(super) fn active_tab_name(&self) -> Option<String> {
        let state = &self.harness.state().state;
        let active = state.active_tab?;
        Some(
            state.open_tabs[active]
                .path()
                .file_name()?
                .to_string_lossy()
                .into_owned(),
        )
    }

    /// The active tab's current buffer contents — what a test asserts on
    /// after an edit that hasn't been saved yet (and can't be read back off
    /// disk), and the only way to see an editor transform's result: the
    /// editor paints its own text rather than exposing it as an
    /// accessibility value.
    pub(super) fn active_tab_text(&self) -> String {
        let state = &self.harness.state().state;
        let active = state.active_tab.expect("a tab must be active");
        state.open_tabs[active].buffer.to_string()
    }

    /// Selects `length` characters starting at `char_offset` in the active
    /// tab, by placing the caret and holding Shift+Right — the same events a
    /// real keyboard selection produces, since there's no editor node to
    /// drag across.
    pub(super) fn select_in_active_tab(&mut self, char_offset: usize, length: usize) {
        self.type_into_active_tab(char_offset, "");
        for _ in 0..length {
            self.press(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
        }
    }

    /// How many diagnostics the active tab currently has — the squiggles
    /// `widgets::editor::painting` draws. Read off the document rather than
    /// off the screen because a squiggle is painted, not a labelled widget,
    /// so it has no accessibility node to query; that painting itself is
    /// covered by `widget::tests::painting`.
    pub(super) fn active_tab_diagnostics(&self) -> usize {
        let state = &self.harness.state().state;
        let active = state.active_tab.expect("a tab must be active");
        state.open_tabs[active].diagnostics.len()
    }

    /// Writes raw bytes into the project — for a fixture that can't be a
    /// `&str` (a binary file) — and rescans the tree, exactly as
    /// `side_panel::show` does after any create/rename/delete, so the new
    /// file has a row to click.
    pub(super) fn write_bytes_and_rescan(&mut self, name: &str, bytes: &[u8]) {
        std::fs::write(self.path(name), bytes).expect("write binary fixture");
        let root = self.dir.path().to_path_buf();
        self.harness
            .state_mut()
            .state
            .open_project(root)
            .expect("rescan project");
        self.settle();
    }

    pub(super) fn on_disk(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).expect("read fixture file back")
    }

    /// The live app, for the handful of settings that have no on-screen
    /// consequence a test can query (word wrap, indentation width) — a
    /// Settings menu click has to be verifiable *somehow*, and these only
    /// show up as a different painted result. Prefer asserting on labels or
    /// on disk wherever the change is actually visible.
    pub(super) fn app(&self) -> &FoxGardenApp {
        self.harness.state()
    }

    /// Whether the *context's* visuals are currently dark — the thing
    /// Settings > Theme actually applies, as opposed to `app().dark_mode`,
    /// which is only the copy kept for persistence.
    pub(super) fn is_dark_mode(&self) -> bool {
        let ctx = &self.harness.ctx;
        ctx.style_of(ctx.theme()).visuals.dark_mode
    }
}
