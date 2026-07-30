//! The dockable Source Control panel (`PLAN.md` Track 9 Phase 3): a
//! project-wide `git status` as a checked/unchecked file list under
//! "Staged Changes"/"Changes" sections, plus a commit-message box and
//! Commit button. Running `git status`/`git add`/`git reset`/`git commit`
//! itself lives in `fg_core::status`; this module owns the background-
//! thread plumbing (mirroring `static_analysis`'s own `spawn_scan`/
//! `poll_scan` shape) and the panel's own UI.
//!
//! Unlike the diff gutter/inline blame (Phases 1-2), nothing here is
//! automatic — `git status` only reflects the working tree accurately
//! *after* a change, so this panel refreshes on an explicit Refresh click
//! or right after one of its own stage/unstage/commit actions completes,
//! not on a timer or every keystroke.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use fg_core::{FileDiff, StatusEntry};

type StatusResult = Result<Vec<StatusEntry>, String>;
type OpResult = Result<(), String>;
/// The currently-expanded file's unstaged (`git diff`) and staged (`git
/// diff --cached`) halves, fetched together since a partially-staged file
/// needs both to show its full hunk picture (see `expanded_diffs`'s own
/// doc comment).
type ExpandedDiffs = (FileDiff, FileDiff);
type ExpandedResult = Result<ExpandedDiffs, String>;

fn spawn<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Receiver<T> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx
}

/// Shared by `poll_status`/`poll_op` — same "drain if finished, clear the
/// slot either way" shape as `static_analysis::poll_scan`.
fn poll<T>(rx_slot: &mut Option<Receiver<T>>) -> Option<T> {
    let rx = rx_slot.as_ref()?;
    match rx.try_recv() {
        Ok(result) => {
            *rx_slot = None;
            Some(result)
        }
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => {
            *rx_slot = None;
            None
        }
    }
}

#[derive(Default)]
pub struct GitStageState {
    entries: Vec<StatusEntry>,
    status_rx: Option<Receiver<StatusResult>>,
    /// One shared slot for add/reset/commit — the panel disables every
    /// checkbox and the Commit button while any of the three is in flight
    /// (see `op_running`), so there's never more than one to track at once.
    op_rx: Option<Receiver<OpResult>>,
    /// Set by `commit`, read (and cleared) by `poll_op` — lets a completed
    /// op know whether to also clear `commit_message`, without `app.rs`
    /// needing to tell `poll_op` which kind of op just finished.
    committing: bool,
    pub commit_message: String,
    /// The local git committer identity's first name (`fg_core::
    /// git_user_first_name`), shown next to the panel heading — e.g. "Ada"
    /// from a `user.name` of "Ada Lovelace". Loaded synchronously (see
    /// `load_committer_first_name`), unlike every other piece of state
    /// here: it's a local `git config` read, not a working-tree scan, so
    /// there's nothing worth backgrounding it for.
    committer_first_name: Option<String>,
    /// The one file row currently showing its own per-hunk breakdown, if
    /// any — only one at a time, same "one detail view open" shape a lot of
    /// this app's own panels already use, rather than every row tracking
    /// its own independent expand state.
    expanded: Option<PathBuf>,
    expanded_rx: Option<Receiver<ExpandedResult>>,
    /// Both halves of `expanded`'s own diff — kept together (rather than,
    /// say, only whichever half the row is currently grouped under) because
    /// a single `StatusEntry` can carry *both* a staged and a further
    /// unstaged change at once (git's own `MM`-shaped status), and this
    /// view's whole point is showing the complete, real hunk picture for
    /// that file, not just the half its row happens to be sorted into.
    expanded_diffs: Option<ExpandedDiffs>,
}

impl GitStageState {
    /// Kicks off a fresh `git status` against `root` on a background
    /// thread, replacing any still-running one — a rapid double-click only
    /// needs the latest result.
    pub fn refresh(&mut self, root: PathBuf) {
        self.status_rx = Some(spawn(move || fg_core::git_status(&root).map_err(|e| e.to_string())));
    }

    /// Loads `committer_first_name` — called alongside `refresh` (both the
    /// panel's own "just became visible" transition in `app.rs` and, since
    /// that transition never fires for a panel that's *already* visible on
    /// a resumed session, `FoxGardenApp::new` as well). Cheap enough
    /// (see the field's own doc comment) to just call directly rather than
    /// through the `spawn`/`poll` machinery every other op here uses.
    pub fn load_committer_first_name(&mut self, root: &Path) {
        self.committer_first_name = fg_core::git_user_first_name(root);
    }

    pub fn status_running(&self) -> bool {
        self.status_rx.is_some()
    }

    /// True while a stage/unstage/commit is running.
    pub fn op_running(&self) -> bool {
        self.op_rx.is_some()
    }

    /// Drains a completed `git status` scan, if any finished since the
    /// last poll — called once a frame from `FoxGardenApp::ui`. Applies a
    /// successful result to `entries` directly (purely local book-keeping,
    /// unlike `static_analysis`'s findings, which have to route into
    /// `EditorState`'s open documents) — the caller only needs the `Result`
    /// back to decide whether to surface a failure via `last_error`.
    pub fn poll_status(&mut self) -> Option<StatusResult> {
        let result = poll(&mut self.status_rx)?;
        if let Ok(entries) = &result {
            self.entries = entries.clone();
        }
        Some(result)
    }

    /// Drains a completed stage/unstage/commit op. On a successful commit,
    /// also clears `commit_message` — the caller is still responsible for
    /// triggering a fresh `refresh` on success (this state doesn't know
    /// `root` outside of the call that started the op).
    pub fn poll_op(&mut self) -> Option<OpResult> {
        let result = poll(&mut self.op_rx)?;
        if self.committing {
            self.committing = false;
            if result.is_ok() {
                self.commit_message.clear();
            }
        }
        Some(result)
    }

    pub fn stage(&mut self, root: PathBuf, path: PathBuf) {
        self.op_rx = Some(spawn(move || fg_core::git_add(&root, &[path]).map_err(|e| e.to_string())));
    }

    pub fn unstage(&mut self, root: PathBuf, path: PathBuf) {
        self.op_rx = Some(spawn(move || fg_core::git_reset_paths(&root, &[path]).map_err(|e| e.to_string())));
    }

    pub fn commit(&mut self, root: PathBuf, message: String) {
        self.committing = true;
        self.op_rx = Some(spawn(move || fg_core::git_commit(&root, &message).map_err(|e| e.to_string())));
    }

    /// `git push` — same shared `op_rx` slot as stage/unstage/commit (a
    /// push in flight disables the panel's checkboxes/Commit button the
    /// same way any of the other three already do; nothing here needs to
    /// run concurrently with them).
    pub fn push(&mut self, root: PathBuf) {
        self.op_rx = Some(spawn(move || fg_core::git_push(&root).map_err(|e| e.to_string())));
    }

    /// Toggles `path`'s own hunk breakdown: collapses it if it's already
    /// the expanded row, otherwise expands it and kicks off a fresh fetch
    /// of both its unstaged and staged diffs on a background thread (a
    /// working-tree `git diff` with real context lines, not the gutter's
    /// own `-U0` one — see `fg_core::git_file_diff`'s own doc comment for
    /// why hunk staging needs the difference).
    pub fn toggle_expand(&mut self, root: PathBuf, path: PathBuf) {
        if self.expanded.as_ref() == Some(&path) {
            self.expanded = None;
            self.expanded_rx = None;
            self.expanded_diffs = None;
            return;
        }
        self.expanded = Some(path.clone());
        self.expanded_diffs = None;
        self.spawn_expand_fetch(root, path);
    }

    /// Re-fetches the currently-expanded row's own diffs — called after a
    /// hunk stage/unstage completes, since that changes exactly the
    /// unstaged/staged split this view shows (and shifts every later
    /// hunk's own index). A no-op with nothing expanded. Unlike
    /// `toggle_expand`, always (re-)fetches rather than collapsing on a
    /// second call — this is a refresh, not a toggle.
    pub fn refresh_expanded(&mut self, root: PathBuf) {
        if let Some(path) = self.expanded.clone() {
            self.spawn_expand_fetch(root, path);
        }
    }

    fn spawn_expand_fetch(&mut self, root: PathBuf, path: PathBuf) {
        self.expanded_rx = Some(spawn(move || {
            let unstaged = fg_core::git_file_diff(&path, &root).map_err(|e| e.to_string())?;
            let staged = fg_core::git_file_diff_cached(&path, &root).map_err(|e| e.to_string())?;
            Ok((unstaged, staged))
        }));
    }

    pub fn expanded_running(&self) -> bool {
        self.expanded_rx.is_some()
    }

    /// Drains a completed hunk-diff fetch, applying a successful result to
    /// `expanded_diffs` directly — mirrors `poll_status`'s own shape.
    pub fn poll_expanded(&mut self) -> Option<ExpandedResult> {
        let result = poll(&mut self.expanded_rx)?;
        if let Ok(diffs) = &result {
            self.expanded_diffs = Some(diffs.clone());
        }
        Some(result)
    }

    /// Stages hunk `hunk_index` of `path`'s own *unstaged* diff (`git apply
    /// --cached`, forward) — same shared `op_rx` slot every other mutating
    /// op here uses. A no-op if `expanded_diffs`/the index is stale (e.g. a
    /// concurrent external change raced this click), rather than sending a
    /// malformed patch to `git apply`.
    pub fn stage_hunk(&mut self, root: PathBuf, hunk_index: usize) {
        let Some((unstaged, _)) = &self.expanded_diffs else { return };
        let Some(patch) = fg_core::hunk_patch(unstaged, hunk_index) else { return };
        self.op_rx = Some(spawn(move || fg_core::git_apply_cached(&root, &patch, false).map_err(|e| e.to_string())));
    }

    /// Unstages hunk `hunk_index` of `path`'s own *staged* diff (`git apply
    /// --cached --reverse`). Same staleness guard as `stage_hunk`.
    pub fn unstage_hunk(&mut self, root: PathBuf, hunk_index: usize) {
        let Some((_, staged)) = &self.expanded_diffs else { return };
        let Some(patch) = fg_core::hunk_patch(staged, hunk_index) else { return };
        self.op_rx = Some(spawn(move || fg_core::git_apply_cached(&root, &patch, true).map_err(|e| e.to_string())));
    }
}

/// Draws the panel body. Kicks off stage/unstage/commit directly from the
/// relevant click (this already owns `&mut GitStageState`, same "no need to
/// bubble a `menu_bar`-style outcome flag up through `app.rs`" reasoning
/// `static_analysis::show_install_row` uses for its own buttons) — `app.rs`
/// only needs to poll `poll_status`/`poll_op` each frame and surface a
/// failure via `last_error`.
pub fn show(ui: &mut egui::Ui, git_stage: &mut GitStageState, root: &Path) {
    if git_stage.status_running() || git_stage.op_running() || git_stage.expanded_running() {
        // Nothing else drives a repaint while a background `git status`/
        // add/reset/commit/push/hunk-fetch is in flight — this app runs in
        // egui's reactive (not continuous) repaint mode, so without this
        // the op finishes almost instantly on its own thread, but the
        // checkbox/panel wouldn't visibly reflect it until some unrelated
        // input event (a mouse move, a keystroke elsewhere) happened to
        // trigger the next frame. Same fix `spring_endpoints::show` already
        // applies to its own background scan.
        ui.ctx().request_repaint();
    }

    ui.horizontal(|ui| {
        ui.heading("Source Control");
        if let Some(name) = &git_stage.committer_first_name {
            ui.label(egui::RichText::new(name).weak());
        }
        if ui.add_enabled(!git_stage.status_running(), egui::Button::new("Refresh")).clicked() {
            git_stage.refresh(root.to_path_buf());
        }
        if ui.add_enabled(!git_stage.op_running(), egui::Button::new("Push")).clicked() {
            git_stage.push(root.to_path_buf());
        }
    });
    ui.separator();

    if git_stage.entries.is_empty() {
        ui.label(egui::RichText::new(if git_stage.status_running() { "Loading…" } else { "No changes" }).weak());
    }

    let mut action: Option<RowAction> = None;
    ui.add_enabled_ui(!git_stage.op_running(), |ui| {
        egui::ScrollArea::vertical().max_height(ui.available_height() * 0.6).show(ui, |ui| {
            let (staged, unstaged): (Vec<&StatusEntry>, Vec<&StatusEntry>) =
                git_stage.entries.iter().partition(|e| e.is_staged());
            if !staged.is_empty() {
                ui.strong(format!("Staged Changes ({})", staged.len()));
                for entry in &staged {
                    show_entry_row(ui, git_stage, entry, &mut action);
                }
                ui.add_space(6.0);
            }
            if !unstaged.is_empty() {
                ui.strong(format!("Changes ({})", unstaged.len()));
                for entry in &unstaged {
                    show_entry_row(ui, git_stage, entry, &mut action);
                }
            }
        });
    });

    match action {
        Some(RowAction::Toggle(path, true)) => git_stage.stage(root.to_path_buf(), path),
        Some(RowAction::Toggle(path, false)) => git_stage.unstage(root.to_path_buf(), path),
        Some(RowAction::ToggleExpand(path)) => git_stage.toggle_expand(root.to_path_buf(), path),
        Some(RowAction::StageHunk(index)) => git_stage.stage_hunk(root.to_path_buf(), index),
        Some(RowAction::UnstageHunk(index)) => git_stage.unstage_hunk(root.to_path_buf(), index),
        None => {}
    }

    ui.separator();
    ui.add(
        egui::TextEdit::multiline(&mut git_stage.commit_message)
            .desired_rows(3)
            .hint_text("Commit message")
            .desired_width(f32::INFINITY),
    );
    let has_staged = git_stage.entries.iter().any(StatusEntry::is_staged);
    let can_commit = has_staged && !git_stage.commit_message.trim().is_empty() && !git_stage.op_running();
    if ui.add_enabled(can_commit, egui::Button::new("Commit")).clicked() {
        git_stage.commit(root.to_path_buf(), git_stage.commit_message.clone());
    }
}

/// Every click a file row (or its own expanded hunk list) can produce in
/// one frame — `show` only ever acts on one per frame, same "one shared
/// slot, no concurrent ops" constraint `GitStageState::op_rx` itself
/// already enforces.
enum RowAction {
    /// Checkbox toggled: stage (`true`) or unstage (`false`) the whole file.
    Toggle(PathBuf, bool),
    ToggleExpand(PathBuf),
    /// Index into the *expanded* file's own unstaged `FileDiff::hunks`.
    StageHunk(usize),
    /// Index into the *expanded* file's own staged `FileDiff::hunks`.
    UnstageHunk(usize),
}

/// A small clickable collapse/expand triangle, vector-painted via egui's
/// own `collapsing_header::paint_default_icon` rather than a `▸`/`▾` text
/// glyph — the same triangle `CollapsingHeader` draws for itself, and
/// immune to font coverage the way a Unicode glyph isn't (a real, live-
/// verified bug: neither the app's own bundled fonts nor egui's built-ins
/// cover `▸`/`▾`, so a text-glyph button rendered as a tofu box here).
fn expand_arrow(ui: &mut egui::Ui, is_expanded: bool) -> egui::Response {
    let size = egui::Vec2::splat(ui.spacing().icon_width);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let openness = if is_expanded { 1.0 } else { 0.0 };
        egui::collapsing_header::paint_default_icon(ui, openness, &response);
    }
    response
}

fn show_entry_row(ui: &mut egui::Ui, git_stage: &GitStageState, entry: &StatusEntry, action: &mut Option<RowAction>) {
    let is_expanded = git_stage.expanded.as_deref() == Some(entry.path.as_path());
    ui.horizontal(|ui| {
        if entry.is_untracked() {
            // An untracked file has no `git diff` of its own to break into
            // hunks — no arrow, just space to keep every row's checkbox
            // aligned to the same column.
            ui.add_space(ui.spacing().icon_width);
        } else if expand_arrow(ui, is_expanded).clicked() {
            *action = Some(RowAction::ToggleExpand(entry.path.clone()));
        }
        let mut staged = entry.is_staged();
        let label = format!("{} {}", status_badge(entry), entry.path.display());
        if ui.checkbox(&mut staged, label).changed() {
            *action = Some(RowAction::Toggle(entry.path.clone(), staged));
        }
    });

    if is_expanded {
        show_hunks(ui, git_stage, action);
    }
}

/// The expanded row's own per-hunk breakdown: every staged hunk (`git diff
/// --cached`'s own, each offering "Unstage Hunk") above every unstaged one
/// (`git diff`'s own, each offering "Stage Hunk") — staged-first so a
/// partially-staged file reads top-to-bottom as "what's already staged,
/// then what still isn't," matching the panel's own Staged/Changes section
/// ordering above.
fn show_hunks(ui: &mut egui::Ui, git_stage: &GitStageState, action: &mut Option<RowAction>) {
    ui.indent("git_stage_hunks", |ui| {
        let Some((unstaged, staged)) = &git_stage.expanded_diffs else {
            ui.label(egui::RichText::new("Loading hunks…").weak());
            return;
        };
        for (index, hunk) in staged.hunks.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&hunk.header).monospace().weak());
                if ui.small_button("Unstage Hunk").clicked() {
                    *action = Some(RowAction::UnstageHunk(index));
                }
            });
        }
        for (index, hunk) in unstaged.hunks.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&hunk.header).monospace().weak());
                if ui.small_button("Stage Hunk").clicked() {
                    *action = Some(RowAction::StageHunk(index));
                }
            });
        }
    });
}

/// A short `[X]` badge summarizing an entry's status — reads whichever half
/// (index if staged, worktree otherwise) is actually the "current" state
/// the checkbox reflects, so a staged-then-further-edited file's badge
/// still describes its checked (staged) half, not the dirtier worktree one.
fn status_badge(entry: &StatusEntry) -> &'static str {
    if entry.is_untracked() {
        return "[U]";
    }
    let code = if entry.is_staged() { entry.index_status } else { entry.worktree_status };
    match code {
        'A' => "[A]",
        'D' => "[D]",
        'R' => "[R]",
        'C' => "[C]",
        _ => "[M]",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, index: char, worktree: char) -> StatusEntry {
        StatusEntry { path: PathBuf::from(path), index_status: index, worktree_status: worktree }
    }

    #[test]
    fn poll_status_applies_a_successful_result_to_entries() {
        let mut state = GitStageState {
            status_rx: Some(spawn(|| Ok(vec![entry("f.txt", '?', '?')]))),
            ..GitStageState::default()
        };
        let result = loop {
            if let Some(result) = state.poll_status() {
                break result;
            }
        };
        assert!(result.is_ok());
        assert_eq!(&state.entries, &[entry("f.txt", '?', '?')]);
    }

    #[test]
    fn poll_status_returns_the_error_without_touching_entries() {
        let mut state = GitStageState {
            entries: vec![entry("stale.txt", '?', '?')],
            status_rx: Some(spawn(|| Err("boom".to_string()))),
            ..GitStageState::default()
        };
        let result = loop {
            if let Some(result) = state.poll_status() {
                break result;
            }
        };
        assert_eq!(result, Err("boom".to_string()));
        assert_eq!(&state.entries, &[entry("stale.txt", '?', '?')]);
    }

    #[test]
    fn commit_marks_running_and_clears_the_message_only_on_success() {
        let mut state = GitStageState {
            commit_message: "wip".to_string(),
            op_rx: Some(spawn(|| Ok(()))),
            committing: true,
            ..GitStageState::default()
        };
        assert!(state.op_running());

        let result = loop {
            if let Some(result) = state.poll_op() {
                break result;
            }
        };
        assert!(result.is_ok());
        assert!(!state.op_running());
        assert_eq!(state.commit_message, "");
    }

    #[test]
    fn a_failed_commit_leaves_the_message_untouched() {
        let mut state = GitStageState {
            commit_message: "wip".to_string(),
            op_rx: Some(spawn(|| Err("nothing to commit".to_string()))),
            committing: true,
            ..GitStageState::default()
        };

        let result = loop {
            if let Some(result) = state.poll_op() {
                break result;
            }
        };
        assert!(result.is_err());
        assert_eq!(state.commit_message, "wip");
    }

    #[test]
    fn stage_and_unstage_are_not_marked_as_committing() {
        let mut state = GitStageState {
            commit_message: "keep me".to_string(),
            op_rx: Some(spawn(|| Ok(()))),
            ..GitStageState::default()
        };
        // Not going through `stage`/`unstage` themselves (no real repo
        // needed for this assertion) — `committing` simply starts `false`
        // and only `commit` ever sets it, so a stage/unstage completing
        // must never clear the message.
        assert!(!state.committing);

        let result = loop {
            if let Some(result) = state.poll_op() {
                break result;
            }
        };
        assert!(result.is_ok());
        assert_eq!(state.commit_message, "keep me");
    }

    fn file_diff(headers: &[&str]) -> FileDiff {
        FileDiff {
            preamble: "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n".to_string(),
            hunks: headers
                .iter()
                .map(|h| fg_core::RawHunk { header: h.to_string(), lines: vec![" ctx".to_string()] })
                .collect(),
        }
    }

    #[test]
    fn poll_expanded_applies_a_successful_result() {
        let mut state = GitStageState {
            expanded_rx: Some(spawn(|| Ok((file_diff(&["@@ -1 +1 @@"]), file_diff(&[]))))),
            ..GitStageState::default()
        };
        let result = loop {
            if let Some(result) = state.poll_expanded() {
                break result;
            }
        };
        assert!(result.is_ok());
        assert_eq!(state.expanded_diffs.unwrap().0.hunks.len(), 1);
    }

    #[test]
    fn toggle_expand_on_the_same_path_twice_collapses_it() {
        let mut state = GitStageState::default();
        let path = PathBuf::from("f.txt");
        state.toggle_expand(PathBuf::from("/nonexistent"), path.clone());
        assert_eq!(state.expanded, Some(path.clone()));

        state.toggle_expand(PathBuf::from("/nonexistent"), path);
        assert_eq!(state.expanded, None);
        assert!(state.expanded_rx.is_none());
        assert!(state.expanded_diffs.is_none());
    }

    #[test]
    fn stage_hunk_and_unstage_hunk_are_no_ops_with_nothing_expanded() {
        let mut state = GitStageState::default();
        state.stage_hunk(PathBuf::from("/nonexistent"), 0);
        assert!(!state.op_running(), "no expanded_diffs means no patch to apply");

        state.unstage_hunk(PathBuf::from("/nonexistent"), 0);
        assert!(!state.op_running());
    }

    #[test]
    fn refresh_expanded_is_a_no_op_with_nothing_expanded() {
        let mut state = GitStageState::default();
        state.refresh_expanded(PathBuf::from("/nonexistent"));
        assert!(state.expanded_rx.is_none());
    }

    /// End-to-end: expanding a real two-hunk file, staging just its second
    /// hunk, and confirming the *real* `git diff --cached` afterward shows
    /// exactly that one hunk — not just that `git_apply_cached` itself
    /// works (already covered in `fg_core::status`'s own tests), but that
    /// this state's own `expanded_diffs`/`hunk_patch` wiring picks the
    /// right hunk out of the right (unstaged) half.
    #[test]
    fn expand_then_stage_hunk_round_trips_through_a_real_repo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let file = root.join("f.txt");
        let lines: Vec<String> = (1..=20).map(|n| format!("l{n}")).collect();
        std::fs::write(&file, lines.join("\n") + "\n").unwrap();
        let run = |args: &[&str]| std::process::Command::new("git").current_dir(&root).args(args).output().unwrap();
        run(&["init", "-q"]);
        run(&["config", "user.email", "a@b.com"]);
        run(&["config", "user.name", "test"]);
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);

        let mut edited = lines.clone();
        edited[1] = "CHANGED2".to_string();
        edited[17] = "CHANGED18".to_string();
        std::fs::write(&file, edited.join("\n") + "\n").unwrap();

        let mut state = GitStageState::default();
        let path = PathBuf::from("f.txt");
        state.toggle_expand(root.clone(), path);
        loop {
            if let Some(result) = state.poll_expanded() {
                result.expect("diff fetch succeeds");
                break;
            }
        }
        assert_eq!(state.expanded_diffs.as_ref().unwrap().0.hunks.len(), 2);

        state.stage_hunk(root.clone(), 1); // the CHANGED18 hunk
        loop {
            if let Some(result) = state.poll_op() {
                result.expect("apply --cached succeeds");
                break;
            }
        }

        let cached = fg_core::git_file_diff_cached(&file, &root).unwrap();
        assert_eq!(cached.hunks.len(), 1);
        assert!(fg_core::hunk_patch(&cached, 0).unwrap().contains("+CHANGED18"));
    }
}
