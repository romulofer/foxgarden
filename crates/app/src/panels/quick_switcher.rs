use std::path::{Path, PathBuf};

use fg_core::EditorState;

/// How many recent-file entries the popup offers — plenty for a "jump back
/// to something I had open a moment ago" gesture without turning into an
/// unbounded scrollback.
const MAX_ENTRIES: usize = 20;

/// Transient state for the `Ctrl+E` recent-files popup, owned by the caller
/// across frames.
#[derive(Default)]
pub struct QuickSwitcherState {
    open: bool,
    query: String,
    selected: usize,
}

impl QuickSwitcherState {
    /// Opens the popup (resetting any leftover search text/selection from
    /// last time) if closed, or closes it if already open — `Ctrl+E`'s
    /// toggle behavior.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
    }
}

/// Every open tab, followed by recently closed ones not already open —
/// most-recently-closed first — reusing exactly the bookkeeping
/// `EditorState` already keeps for tabs/`Ctrl+Shift+T`, rather than
/// tracking a separate "most recently used" history just for this popup.
fn recent_files(state: &EditorState) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = state.open_tabs.iter().map(|doc| doc.path().to_path_buf()).collect();
    for doc in state.closed_tabs.iter().rev() {
        if result.len() >= MAX_ENTRIES {
            break;
        }
        let path = doc.path().to_path_buf();
        if !result.contains(&path) {
            result.push(path);
        }
    }
    result.truncate(MAX_ENTRIES);
    result
}

/// Whether `path`'s file name contains `query`, case-insensitively — an
/// empty query matches everything.
fn matches_query(path: &Path, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase().contains(&query.to_lowercase()))
        .unwrap_or(false)
}

/// Draws the popup if `switcher.open`, returning the path the user picked
/// (by click or Enter on the highlighted row) this frame, if any. Closes
/// the popup on a pick, on Escape, or when nothing is left to show.
pub fn show(ui: &egui::Ui, state: &EditorState, switcher: &mut QuickSwitcherState) -> Option<PathBuf> {
    if !switcher.open {
        return None;
    }

    let candidates: Vec<PathBuf> = recent_files(state)
        .into_iter()
        .filter(|path| matches_query(path, &switcher.query))
        .collect();
    if !candidates.is_empty() {
        switcher.selected = switcher.selected.min(candidates.len() - 1);
    }

    let ctx = ui.ctx().clone();
    let mut chosen = None;
    let mut escaped = false;

    egui::Modal::new(egui::Id::new("quick_switcher")).show(&ctx, |ui| {
        ui.set_min_width(420.0);
        ui.label("Go to recent file");

        let response = ui.text_edit_singleline(&mut switcher.query);
        response.request_focus();

        escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !candidates.is_empty() {
            switcher.selected = (switcher.selected + 1).min(candidates.len() - 1);
        }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            switcher.selected = switcher.selected.saturating_sub(1);
        }
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        ui.separator();
        if candidates.is_empty() {
            ui.weak("No matches");
        }
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for (index, path) in candidates.iter().enumerate() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let is_selected = index == switcher.selected;
                let response = ui.selectable_label(is_selected, name);
                if response.clicked() || (is_selected && enter_pressed) {
                    chosen = Some(path.clone());
                }
            }
        });
    });

    if chosen.is_some() || escaped {
        switcher.open = false;
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use fg_core::Document;

    fn doc_at(dir: &tempfile::TempDir, name: &str) -> Document {
        let path = dir.path().join(name);
        std::fs::write(&path, "").unwrap();
        Document::open(path).unwrap()
    }

    #[test]
    fn recent_files_lists_open_tabs_before_closed_ones() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = EditorState::new();
        state.open_tabs.push(doc_at(&dir, "Open.java"));
        state.closed_tabs.push(doc_at(&dir, "Closed.java"));

        let files = recent_files(&state);

        assert_eq!(
            files,
            vec![dir.path().join("Open.java"), dir.path().join("Closed.java")]
        );
    }

    #[test]
    fn recent_files_lists_closed_tabs_most_recently_closed_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = EditorState::new();
        state.closed_tabs.push(doc_at(&dir, "First.java"));
        state.closed_tabs.push(doc_at(&dir, "Second.java"));

        let files = recent_files(&state);

        assert_eq!(
            files,
            vec![dir.path().join("Second.java"), dir.path().join("First.java")]
        );
    }

    #[test]
    fn recent_files_does_not_duplicate_a_path_that_is_both_open_and_in_closed_tabs() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = EditorState::new();
        state.open_tabs.push(doc_at(&dir, "Both.java"));
        state.closed_tabs.push(doc_at(&dir, "Both.java"));

        let files = recent_files(&state);

        assert_eq!(files, vec![dir.path().join("Both.java")]);
    }

    #[test]
    fn matches_query_is_case_insensitive_on_the_file_name() {
        let path = Path::new("/project/src/HelloWorld.java");
        assert!(matches_query(path, "hello"));
        assert!(matches_query(path, "WORLD"));
        assert!(!matches_query(path, "goodbye"));
    }

    #[test]
    fn matches_query_empty_matches_everything() {
        assert!(matches_query(Path::new("/a/b.java"), ""));
    }

    #[test]
    fn toggle_opens_and_resets_query_and_selection() {
        let mut switcher = QuickSwitcherState {
            open: false,
            query: "leftover".to_string(),
            selected: 3,
        };

        switcher.toggle();

        assert!(switcher.open);
        assert!(switcher.query.is_empty());
        assert_eq!(switcher.selected, 0);
    }

    #[test]
    fn toggle_twice_closes_it_again() {
        let mut switcher = QuickSwitcherState::default();
        switcher.toggle();
        switcher.toggle();
        assert!(!switcher.open);
    }
}
