use std::path::{Path, PathBuf};

use crate::document::{Document, OpenDocumentError};
use crate::project::Project;

/// Cap on `closed_tabs`: it exists only so `Ctrl+Shift+T` can walk
/// backwards through recently closed tabs, not as a full undo history, so
/// unbounded growth buys nothing — each entry holds a full `Document`
/// (its entire `Rope` buffer) that would otherwise sit in memory for the
/// life of the process every time a tab is closed.
const MAX_CLOSED_TABS: usize = 20;

/// One terminal session in the terminal panel (`PLAN.md`'s terminal-panel
/// track). Deliberately **not** part of `open_tabs`/`active_tab` at all:
/// `SPEC.md` §8.2 puts the terminal in its own dockable bottom panel (the
/// VSCode shape), not the file tab strip, so a session never shares a
/// namespace or an index with a file tab.
///
/// Just a title plus a blink-timing anchor — the real child process,
/// writer, and `vt100::Parser` all live on the app-side `PtySession`
/// (`crates/app/src/pty_session.rs`, index-aligned with `terminal_tabs`, the
/// same "index-aligned side vec, not on `EditorState`" shape `parsers`
/// already uses for `open_tabs`), since a live process and its background
/// reader thread can't be headless-testable the way `fg-core` needs to stay.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalTab {
    pub title: String,
    /// Mirrors `text_area::shell::ShellState::last_interaction` — the
    /// timestamp `terminal_widget::show`'s own cursor-blink cycle is
    /// anchored to, reset whenever the session receives keyboard input, so
    /// the terminal's cursor blinks on the same cadence the editor's own
    /// caret does rather than a second, independently-phased timer.
    pub last_interaction: f64,
}

#[derive(Default)]
pub struct EditorState {
    pub project: Option<Project>,
    pub open_tabs: Vec<Document>,
    pub active_tab: Option<usize>,
    /// Recently closed tabs, most-recently-closed last. `close_tab` pushes
    /// onto this (capped at `MAX_CLOSED_TABS`, dropping the oldest);
    /// `reopen_last_closed_tab` pops off it.
    pub closed_tabs: Vec<Document>,
    /// The terminal panel's own sessions — entirely independent of
    /// `open_tabs`/`active_tab` (see `TerminalTab`'s own doc comment).
    pub terminal_tabs: Vec<TerminalTab>,
    /// Which `terminal_tabs` entry the panel's own small tab strip has
    /// focused, if any.
    pub active_terminal: Option<usize>,
}

impl EditorState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_project(&mut self, root: PathBuf) -> std::io::Result<()> {
        self.project = Some(Project::open(root)?);
        // Reopening a tab closed in a project you've since navigated away
        // from would be a confusing "resurrection", not a useful undo.
        self.closed_tabs.clear();
        Ok(())
    }

    pub fn find_tab(&self, path: &Path) -> Option<usize> {
        self.open_tabs.iter().position(|doc| doc.path() == path)
    }

    /// Opens `path` in a new tab, or focuses its existing tab if already open.
    pub fn open_tab(&mut self, path: PathBuf) -> Result<usize, OpenDocumentError> {
        if let Some(index) = self.find_tab(&path) {
            self.active_tab = Some(index);
            return Ok(index);
        }

        let mut document = Document::open(path)?;
        document.project_root = self.project.as_ref().map(|project| project.root.clone());
        self.open_tabs.push(document);
        let index = self.open_tabs.len() - 1;
        self.active_tab = Some(index);
        Ok(index)
    }

    pub fn focus_tab(&mut self, index: usize) {
        if index < self.open_tabs.len() {
            self.active_tab = Some(index);
        }
    }

    /// Removes the tab at `index`, pushing it onto `closed_tabs` (so
    /// `reopen_last_closed_tab` can restore it later), and moves
    /// `active_tab` to a sensible neighbor if the closed tab was active.
    pub fn close_tab(&mut self, index: usize) {
        let document = self.open_tabs.remove(index);

        self.active_tab = match self.active_tab {
            None => None,
            Some(active) if self.open_tabs.is_empty() => {
                let _ = active;
                None
            }
            Some(active) if active == index => Some(index.min(self.open_tabs.len() - 1)),
            Some(active) if active > index => Some(active - 1),
            Some(active) => Some(active),
        };

        self.closed_tabs.push(document);
        if self.closed_tabs.len() > MAX_CLOSED_TABS {
            self.closed_tabs.remove(0); // drop the oldest
        }
    }

    /// Pops the most recently closed tab back onto `open_tabs` and focuses
    /// it. If a tab for that path is already open (e.g. the user reopened it
    /// manually since closing it), just focuses that tab instead of creating
    /// a duplicate. Returns the newly active tab's index, or `None` if
    /// there's nothing left to reopen.
    pub fn reopen_last_closed_tab(&mut self) -> Option<usize> {
        let document = self.closed_tabs.pop()?;

        if let Some(index) = self.find_tab(document.path()) {
            self.active_tab = Some(index);
            return Some(index);
        }

        self.open_tabs.push(document);
        let index = self.open_tabs.len() - 1;
        self.active_tab = Some(index);
        Some(index)
    }

    /// Pushes a new terminal session and focuses it in the panel's own tab
    /// strip. Returns its `terminal_tabs` index. The caller (`app.rs`'s own
    /// `new_terminal_session`) is responsible for spawning the matching
    /// `PtySession` and keeping the two index-aligned.
    pub fn new_terminal_tab(&mut self) -> usize {
        let index = self.terminal_tabs.len();
        self.terminal_tabs.push(TerminalTab {
            title: format!("Terminal {}", index + 1),
            last_interaction: 0.0,
        });
        self.active_terminal = Some(index);
        index
    }

    /// Removes the terminal session at `index` and moves `active_terminal`
    /// to a sensible neighbor if it was active — same "next, or the
    /// previous one if it was last" shape `close_tab` already uses for file
    /// tabs. Still a no-op beyond that for now: no process to kill until
    /// Phase 6 wires one up. Never touches `closed_tabs`: a closed terminal
    /// session has nothing meaningful to reopen (`SPEC.md` §8's own
    /// non-goal).
    pub fn close_terminal_tab(&mut self, index: usize) {
        self.terminal_tabs.remove(index);

        self.active_terminal = match self.active_terminal {
            None => None,
            Some(_) if self.terminal_tabs.is_empty() => None,
            Some(active) if active == index => Some(index.min(self.terminal_tabs.len() - 1)),
            Some(active) if active > index => Some(active - 1),
            Some(active) => Some(active),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopening_same_file_focuses_existing_tab_without_duplicating() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_support::placeholder_java_file(dir.path(), "A.java");
        let mut state = EditorState::new();

        let first_index = state.open_tab(path.clone()).unwrap();
        let second_index = state.open_tab(path).unwrap();

        assert_eq!(first_index, second_index);
        assert_eq!(state.open_tabs.len(), 1);
        assert_eq!(state.active_tab, Some(first_index));
    }

    #[test]
    fn opening_a_tab_within_an_open_project_records_its_root() {
        let dir = tempfile::tempdir().unwrap();
        let a = test_support::placeholder_java_file(dir.path(), "A.java");
        let mut state = EditorState::new();

        state.open_project(dir.path().to_path_buf()).unwrap();
        let index = state.open_tab(a).unwrap();

        assert_eq!(state.open_tabs[index].project_root, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn opening_a_tab_with_no_project_open_records_no_root() {
        let dir = tempfile::tempdir().unwrap();
        let a = test_support::placeholder_java_file(dir.path(), "A.java");
        let mut state = EditorState::new();

        let index = state.open_tab(a).unwrap();

        assert_eq!(state.open_tabs[index].project_root, None);
    }

    #[test]
    fn closing_active_tab_selects_sensible_neighbor() {
        let dir = tempfile::tempdir().unwrap();
        let a = test_support::placeholder_java_file(dir.path(), "A.java");
        let b = test_support::placeholder_java_file(dir.path(), "B.java");
        let c = test_support::placeholder_java_file(dir.path(), "C.java");
        let mut state = EditorState::new();

        state.open_tab(a).unwrap();
        state.open_tab(b).unwrap();
        state.open_tab(c).unwrap();
        // active_tab is now Some(2) (C.java)

        state.focus_tab(1); // focus B.java
        state.close_tab(1); // close B.java: next tab (C.java, now at index 1) becomes active
        assert_eq!(state.active_tab, Some(1));
        assert_eq!(state.open_tabs[1].path().file_name().unwrap(), "C.java");

        state.close_tab(1); // close C.java, the only remaining tab is A.java at index 0
        assert_eq!(state.active_tab, Some(0));

        state.close_tab(0); // close last tab
        assert_eq!(state.active_tab, None);
        assert!(state.open_tabs.is_empty());
    }

    #[test]
    fn reopen_last_closed_tab_restores_it_and_focuses_it() {
        let dir = tempfile::tempdir().unwrap();
        let a = test_support::placeholder_java_file(dir.path(), "A.java");
        let b = test_support::placeholder_java_file(dir.path(), "B.java");
        let mut state = EditorState::new();

        state.open_tab(a).unwrap();
        state.open_tab(b).unwrap();
        state.close_tab(1); // close B.java
        assert_eq!(state.open_tabs.len(), 1);

        let index = state.reopen_last_closed_tab().unwrap();
        assert_eq!(state.open_tabs.len(), 2);
        assert_eq!(state.active_tab, Some(index));
        assert_eq!(state.open_tabs[index].path().file_name().unwrap(), "B.java");
    }

    #[test]
    fn reopen_last_closed_tab_pops_in_lifo_order() {
        let dir = tempfile::tempdir().unwrap();
        let a = test_support::placeholder_java_file(dir.path(), "A.java");
        let b = test_support::placeholder_java_file(dir.path(), "B.java");
        let mut state = EditorState::new();

        state.open_tab(a).unwrap();
        state.open_tab(b).unwrap();
        state.close_tab(0); // close A.java
        state.close_tab(0); // close B.java (now the only remaining tab)

        let first = state.reopen_last_closed_tab().unwrap();
        assert_eq!(state.open_tabs[first].path().file_name().unwrap(), "B.java");

        let second = state.reopen_last_closed_tab().unwrap();
        assert_eq!(state.open_tabs[second].path().file_name().unwrap(), "A.java");

        assert!(state.reopen_last_closed_tab().is_none());
    }

    #[test]
    fn reopen_last_closed_tab_with_nothing_closed_is_a_no_op() {
        let mut state = EditorState::new();
        assert!(state.reopen_last_closed_tab().is_none());
    }

    #[test]
    fn reopening_a_tab_already_open_focuses_it_instead_of_duplicating() {
        let dir = tempfile::tempdir().unwrap();
        let a = test_support::placeholder_java_file(dir.path(), "A.java");
        let mut state = EditorState::new();

        state.open_tab(a.clone()).unwrap();
        state.close_tab(0);
        // Reopened manually (e.g. via the side panel) before Ctrl+Shift+T.
        let manual_index = state.open_tab(a).unwrap();

        let index = state.reopen_last_closed_tab().unwrap();
        assert_eq!(index, manual_index);
        assert_eq!(state.open_tabs.len(), 1);
    }

    #[test]
    fn closed_tabs_is_capped_and_drops_the_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = EditorState::new();

        let paths: Vec<PathBuf> = (0..=MAX_CLOSED_TABS)
            .map(|i| test_support::placeholder_java_file(dir.path(), &format!("F{i}.java")))
            .collect();
        for path in &paths {
            state.open_tab(path.clone()).unwrap();
        }
        // Close them oldest-first (F0.java, F1.java, ...) — one more close
        // than the cap allows.
        for _ in 0..paths.len() {
            state.close_tab(0);
        }

        assert_eq!(state.closed_tabs.len(), MAX_CLOSED_TABS);
        assert!(
            state
                .closed_tabs
                .iter()
                .all(|doc| doc.path().file_name().unwrap() != "F0.java"),
            "the oldest closed tab should have been evicted to stay under the cap"
        );
        let most_recent = state
            .closed_tabs
            .last()
            .unwrap()
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(most_recent, format!("F{MAX_CLOSED_TABS}.java"));
    }

    #[test]
    fn open_project_clears_closed_tabs_from_the_previous_project() {
        let dir = tempfile::tempdir().unwrap();
        let a = test_support::placeholder_java_file(dir.path(), "A.java");
        let mut state = EditorState::new();

        state.open_tab(a).unwrap();
        state.close_tab(0);
        assert_eq!(state.closed_tabs.len(), 1);

        let other_project_dir = tempfile::tempdir().unwrap();
        state.open_project(other_project_dir.path().to_path_buf()).unwrap();

        assert!(state.closed_tabs.is_empty());
    }

    #[test]
    fn new_terminal_tab_appends_and_focuses_it_independently_of_file_tabs() {
        let dir = tempfile::tempdir().unwrap();
        let a = test_support::placeholder_java_file(dir.path(), "A.java");
        let mut state = EditorState::new();

        state.open_tab(a).unwrap();
        let index = state.new_terminal_tab();

        assert_eq!(state.terminal_tabs.len(), 1);
        assert_eq!(state.terminal_tabs[index].title, "Terminal 1");
        assert_eq!(state.active_terminal, Some(0));
        // A terminal session never touches the file-tab fields at all.
        assert_eq!(state.active_tab, Some(0));
        assert_eq!(state.open_tabs.len(), 1);
    }

    #[test]
    fn close_terminal_tab_selects_sensible_neighbor() {
        let mut state = EditorState::new();
        state.new_terminal_tab(); // 0
        state.new_terminal_tab(); // 1
        state.new_terminal_tab(); // 2, active

        state.close_terminal_tab(1); // close the middle one, not the active one
        assert_eq!(state.active_terminal, Some(1), "index 2 shifted down to 1");

        state.close_terminal_tab(1); // now closes the (shifted) active one
        assert_eq!(state.active_terminal, Some(0));

        state.close_terminal_tab(0);
        assert_eq!(state.active_terminal, None);
        assert!(state.terminal_tabs.is_empty());
    }

    #[test]
    fn closing_a_terminal_tab_never_pushes_onto_closed_tabs() {
        let mut state = EditorState::new();
        state.new_terminal_tab();
        state.close_terminal_tab(0);

        assert!(
            state.closed_tabs.is_empty(),
            "a closed terminal session has nothing to reopen, unlike a file tab"
        );
    }
}
