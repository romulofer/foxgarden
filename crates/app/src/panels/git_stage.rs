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

use fg_core::StatusEntry;

type StatusResult = Result<Vec<StatusEntry>, String>;
type OpResult = Result<(), String>;

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
}

/// Draws the panel body. Kicks off stage/unstage/commit directly from the
/// relevant click (this already owns `&mut GitStageState`, same "no need to
/// bubble a `menu_bar`-style outcome flag up through `app.rs`" reasoning
/// `static_analysis::show_install_row` uses for its own buttons) — `app.rs`
/// only needs to poll `poll_status`/`poll_op` each frame and surface a
/// failure via `last_error`.
pub fn show(ui: &mut egui::Ui, git_stage: &mut GitStageState, root: &Path) {
    if git_stage.status_running() || git_stage.op_running() {
        // Nothing else drives a repaint while a background `git status`/
        // add/reset/commit is in flight — this app runs in egui's reactive
        // (not continuous) repaint mode, so without this the op finishes
        // almost instantly on its own thread, but the checkbox/panel
        // wouldn't visibly reflect it until some unrelated input event
        // (a mouse move, a keystroke elsewhere) happened to trigger the
        // next frame. Same fix `spring_endpoints::show` already applies to
        // its own background scan.
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
    });
    ui.separator();

    if git_stage.entries.is_empty() {
        ui.label(egui::RichText::new(if git_stage.status_running() { "Loading…" } else { "No changes" }).weak());
    }

    let mut toggled: Option<(PathBuf, bool)> = None;
    ui.add_enabled_ui(!git_stage.op_running(), |ui| {
        egui::ScrollArea::vertical().max_height(ui.available_height() * 0.6).show(ui, |ui| {
            let (staged, unstaged): (Vec<&StatusEntry>, Vec<&StatusEntry>) =
                git_stage.entries.iter().partition(|e| e.is_staged());
            if !staged.is_empty() {
                ui.strong(format!("Staged Changes ({})", staged.len()));
                for entry in &staged {
                    show_entry_row(ui, entry, &mut toggled);
                }
                ui.add_space(6.0);
            }
            if !unstaged.is_empty() {
                ui.strong(format!("Changes ({})", unstaged.len()));
                for entry in &unstaged {
                    show_entry_row(ui, entry, &mut toggled);
                }
            }
        });
    });

    if let Some((path, staged)) = toggled {
        if staged {
            git_stage.stage(root.to_path_buf(), path);
        } else {
            git_stage.unstage(root.to_path_buf(), path);
        }
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

fn show_entry_row(ui: &mut egui::Ui, entry: &StatusEntry, toggled: &mut Option<(PathBuf, bool)>) {
    let mut staged = entry.is_staged();
    let label = format!("{} {}", status_badge(entry), entry.path.display());
    if ui.checkbox(&mut staged, label).changed() {
        *toggled = Some((entry.path.clone(), staged));
    }
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
}
