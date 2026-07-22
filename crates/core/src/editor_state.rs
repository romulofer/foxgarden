use std::path::{Path, PathBuf};

use crate::document::{Document, OpenDocumentError};
use crate::project::Project;

#[derive(Default)]
pub struct EditorState {
    pub project: Option<Project>,
    pub open_tabs: Vec<Document>,
    pub active_tab: Option<usize>,
}

impl EditorState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_project(&mut self, root: PathBuf) -> std::io::Result<()> {
        self.project = Some(Project::open(root)?);
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

        let document = Document::open(path)?;
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

    /// Removes the tab at `index`, returning the closed document, and moves
    /// `active_tab` to a sensible neighbor if the closed tab was active.
    pub fn close_tab(&mut self, index: usize) -> Document {
        let document = self.open_tabs.remove(index);

        self.active_tab = match self.active_tab {
            None => None,
            Some(active) if self.open_tabs.is_empty() => {
                let _ = active;
                None
            }
            Some(active) if active == index => {
                Some(index.min(self.open_tabs.len() - 1))
            }
            Some(active) if active > index => Some(active - 1),
            Some(active) => Some(active),
        };

        document
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn java_file(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, format!("class {name} {{}}")).unwrap();
        path
    }

    #[test]
    fn reopening_same_file_focuses_existing_tab_without_duplicating() {
        let dir = tempfile::tempdir().unwrap();
        let path = java_file(&dir, "A.java");
        let mut state = EditorState::new();

        let first_index = state.open_tab(path.clone()).unwrap();
        let second_index = state.open_tab(path).unwrap();

        assert_eq!(first_index, second_index);
        assert_eq!(state.open_tabs.len(), 1);
        assert_eq!(state.active_tab, Some(first_index));
    }

    #[test]
    fn closing_active_tab_selects_sensible_neighbor() {
        let dir = tempfile::tempdir().unwrap();
        let a = java_file(&dir, "A.java");
        let b = java_file(&dir, "B.java");
        let c = java_file(&dir, "C.java");
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
}
