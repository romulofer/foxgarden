use std::path::{Path, PathBuf};

use fg_core::{EditorState, FileKind, FileNode};

/// How many fuzzy-matched entries the popup offers — a project can easily
/// have thousands of files, so this caps the *rendered* list to the
/// best-scoring matches rather than showing every match found (unlike
/// `quick_switcher`'s `MAX_ENTRIES`, which caps the candidate *source*
/// itself, since recent files are naturally few).
const MAX_RESULTS: usize = 50;

/// Transient state for the `Ctrl+P` fuzzy-file-open popup, owned by the
/// caller across frames. Shaped identically to `quick_switcher::
/// QuickSwitcherState` — same fields, same `toggle()` — the two differ only
/// in candidate source (every file in the project tree here, vs. open/
/// recently-closed tabs there) and matching (fuzzy-scored here, plain
/// substring there).
#[derive(Default)]
pub struct GoToFileState {
    open: bool,
    query: String,
    selected: usize,
}

impl GoToFileState {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
    }
}

/// Every file (not directory) reachable from `node`, in tree order —
/// `project.tree`'s own `SKIPPED_DIR_NAMES` filtering (`.git`, `target`,
/// `node_modules`, ...) already happened when the tree was built, so
/// nothing further needs excluding here.
fn all_files(node: &FileNode, out: &mut Vec<PathBuf>) {
    match node.kind {
        FileKind::File => out.push(node.path.clone()),
        FileKind::Dir => {
            for child in &node.children {
                all_files(child, out);
            }
        }
    }
}

/// Scores how well `query` fuzzy-matches `candidate` — every query char
/// (case-insensitive) must appear in `candidate` *in order*, not
/// necessarily contiguously (so `"contuser"` matches
/// `"controllers/UserController.java"`), or this returns `None`. Higher is
/// a better match: a run of query chars matched back-to-back in the
/// candidate scores more than the same chars found scattered apart, and
/// matching starting earlier in the candidate scores more than starting
/// later — both read as "a closer, more deliberate match" rather than an
/// incidental one. An empty query matches everything with a score of `0`.
pub(crate) fn fuzzy_score(candidate: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }

    let candidate: Vec<char> = candidate.to_lowercase().chars().collect();
    let query: Vec<char> = query.to_lowercase().chars().collect();

    let mut score = 0i32;
    let mut cand_idx = 0usize;
    let mut prev_matched_idx: Option<usize> = None;

    for &qc in &query {
        let idx = (cand_idx..candidate.len()).find(|&i| candidate[i] == qc)?;

        match prev_matched_idx {
            Some(prev) if idx == prev + 1 => score += 5,
            Some(_) => {}
            None => score -= idx as i32,
        }

        prev_matched_idx = Some(idx);
        cand_idx = idx + 1;
    }

    Some(score)
}

/// Every project file whose path relative to `root` fuzzy-matches `query`,
/// best match first (ties broken by path, for stable ordering), capped at
/// `MAX_RESULTS`.
fn matching_files(tree: &FileNode, root: &Path, query: &str) -> Vec<PathBuf> {
    let mut all = Vec::new();
    all_files(tree, &mut all);

    let mut scored: Vec<(i32, PathBuf)> = all
        .into_iter()
        .filter_map(|path| {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
            fuzzy_score(&rel, query).map(|score| (score, path))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.truncate(MAX_RESULTS);
    scored.into_iter().map(|(_, path)| path).collect()
}

/// Draws the popup if `switcher.open`, returning the path the user picked
/// (by click or Enter on the highlighted row) this frame, if any. Closes
/// the popup on a pick, on Escape, or when there's no project open to
/// search — same shape as `quick_switcher::show`.
pub fn show(ui: &egui::Ui, state: &EditorState, switcher: &mut GoToFileState) -> Option<PathBuf> {
    if !switcher.open {
        return None;
    }
    let Some(project) = &state.project else {
        switcher.open = false;
        return None;
    };

    let candidates = matching_files(&project.tree, &project.root, &switcher.query);
    if !candidates.is_empty() {
        switcher.selected = switcher.selected.min(candidates.len() - 1);
    }

    let ctx = ui.ctx().clone();
    let mut chosen = None;
    let mut escaped = false;

    egui::Modal::new(egui::Id::new("go_to_file")).show(&ctx, |ui| {
        ui.set_min_width(480.0);
        ui.label("Go to file");

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
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (index, path) in candidates.iter().enumerate() {
                let label = path
                    .strip_prefix(&project.root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned();
                let is_selected = index == switcher.selected;
                let response = ui.selectable_label(is_selected, label);
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

    #[test]
    fn fuzzy_score_matches_a_contiguous_substring() {
        assert!(fuzzy_score("UserController.java", "User").is_some());
    }

    #[test]
    fn fuzzy_score_matches_a_scattered_subsequence_across_a_path() {
        // "contuser" -> c-o-n-t-r-o-l-l-e-r-s/-U-s-e-r-...
        assert!(fuzzy_score("controllers/UserController.java", "contuser").is_some());
    }

    #[test]
    fn fuzzy_score_is_case_insensitive() {
        assert!(fuzzy_score("UserController.java", "usercontroller").is_some());
    }

    #[test]
    fn fuzzy_score_returns_none_when_chars_are_out_of_order() {
        // "reuse" requires r,e,u,s,e in order; "user" alone has no "r" at
        // all before the rest, so this simply shouldn't match.
        assert_eq!(fuzzy_score("User.java", "ruesj"), None);
    }

    #[test]
    fn fuzzy_score_empty_query_matches_everything_with_zero_score() {
        assert_eq!(fuzzy_score("anything.java", ""), Some(0));
    }

    #[test]
    fn fuzzy_score_ranks_a_contiguous_match_above_a_scattered_one() {
        let contiguous = fuzzy_score("User.java", "User").unwrap();
        let scattered = fuzzy_score("UnrelatedStuffEndingR.java", "User").unwrap();
        assert!(
            contiguous > scattered,
            "contiguous ({contiguous}) should outscore scattered ({scattered})"
        );
    }

    #[test]
    fn fuzzy_score_ranks_an_earlier_match_above_a_later_one() {
        let earlier = fuzzy_score("UserController.java", "User").unwrap();
        let later = fuzzy_score("AbstractBaseUserController.java", "User").unwrap();
        assert!(
            earlier > later,
            "earlier match ({earlier}) should outscore later match ({later})"
        );
    }

    fn file(path: &str) -> FileNode {
        FileNode {
            path: PathBuf::from(path),
            name: PathBuf::from(path).file_name().unwrap().to_string_lossy().into_owned(),
            kind: FileKind::File,
            children: Vec::new(),
        }
    }

    fn dir(path: &str, children: Vec<FileNode>) -> FileNode {
        FileNode {
            path: PathBuf::from(path),
            name: PathBuf::from(path).file_name().unwrap().to_string_lossy().into_owned(),
            kind: FileKind::Dir,
            children,
        }
    }

    #[test]
    fn all_files_flattens_a_nested_tree_skipping_directories() {
        let tree = dir(
            "/root",
            vec![
                file("/root/Main.java"),
                dir("/root/pkg", vec![file("/root/pkg/A.java"), file("/root/pkg/B.java")]),
            ],
        );
        let mut out = Vec::new();
        all_files(&tree, &mut out);

        assert_eq!(
            out,
            vec![
                PathBuf::from("/root/Main.java"),
                PathBuf::from("/root/pkg/A.java"),
                PathBuf::from("/root/pkg/B.java"),
            ]
        );
    }

    #[test]
    fn matching_files_ranks_and_filters_by_the_query() {
        let root = PathBuf::from("/root");
        let tree = dir(
            "/root",
            vec![
                file("/root/UserController.java"),
                dir("/root/controllers", vec![file("/root/controllers/UserController.java")]),
                file("/root/Unrelated.java"),
            ],
        );

        let results = matching_files(&tree, &root, "usercontroller");

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|p| p.to_string_lossy().contains("UserController")));
    }

    #[test]
    fn matching_files_with_empty_query_returns_every_file() {
        let root = PathBuf::from("/root");
        let tree = dir("/root", vec![file("/root/A.java"), file("/root/B.java")]);

        assert_eq!(matching_files(&tree, &root, "").len(), 2);
    }

    #[test]
    fn toggle_opens_and_resets_query_and_selection() {
        let mut switcher = GoToFileState {
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
        let mut switcher = GoToFileState::default();
        switcher.toggle();
        switcher.toggle();
        assert!(!switcher.open);
    }
}
