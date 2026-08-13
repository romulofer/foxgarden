//! Git diff gutter + inline blame (`PLAN.md` Track 9 Phases 1-2) — app-side
//! wiring: kicks off a background `git diff` **and** `git blame` per
//! document (one thread, two shell-outs — they share every trigger point,
//! so there's no reason to run them on separate threads or track them as
//! separate in-flight scans) and applies each completed result to the
//! matching open tab's `Document::diff_hunks`/`Document::blame`. Running
//! either command itself lives in `fg_core` (`git_diff_hunks`/`git_blame`);
//! this module only owns the background-thread plumbing and result
//! routing, mirroring `static_analysis`'s own `spawn_scan`/`poll_scan` shape
//! (see that module's doc comment) — except keyed per-path (`HashMap`, not
//! a single `Option` slot), since a scan is triggered per-document from
//! several independent points (open/save/reload) rather than one
//! project-wide action at a time, and more than one can legitimately be in
//! flight at once (e.g. two files saved in quick succession).
//!
//! Unlike Checkstyle/PMD, nothing here is menu-triggered — `PLAN.md`
//! Phase 1 wants this to run automatically, so `check_for_saves` (see its
//! own doc comment) detects a save generically, at the frame level, rather
//! than requiring every one of this app's several save call sites (Ctrl+S,
//! File > Save, the close-confirmation modal, the editor's own right-click
//! Save, auto-save) to know this feature exists and call in explicitly.
//! Open and reload are few enough call sites that `app.rs` triggers `run`
//! directly at each of them instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use fg_core::{BlameLine, DiffHunk, EditorState};

/// The diff and blame halves are kept as independent `Result`s, not one
/// combined `Result<(Vec<DiffHunk>, Vec<BlameLine>), String>` — each of
/// `git_diff_hunks`/`git_blame` can only fail if `git` itself couldn't be
/// launched at all (vanishingly rare, and the same failure mode for both),
/// but keeping them separate means that hypothetical failure on one side
/// still lets the other side's still-good result land, instead of one
/// `Err` silently discarding both.
type ScanResult = (Result<Vec<DiffHunk>, String>, Result<Vec<BlameLine>, String>);

#[derive(Default)]
pub struct DiffState {
    scans: HashMap<PathBuf, Receiver<ScanResult>>,
    /// Each currently-open tab's dirty state as of the last `check_for_
    /// saves` call — compared against this frame's fresh `Document::
    /// is_dirty()` to detect a `true` -> `false` transition (a save, by
    /// whichever call site produced it). Reset to exactly this frame's open
    /// tabs on every call, so a closed-then-reopened file starts fresh
    /// rather than remembering stale state from a previous time it was open
    /// this session.
    last_dirty: HashMap<PathBuf, bool>,
}

impl DiffState {
    /// Whether any `git diff`/`git blame` scan is still running — the
    /// status bar's own "is git busy" signal, alongside `GitStageState`'s.
    pub fn running(&self) -> bool {
        !self.scans.is_empty()
    }

    /// Kicks off a `git diff` **and** `git blame` for `path` on one
    /// background thread, replacing any still-running scan already in
    /// flight for the same path (a rapid save-then-save only needs the
    /// latest result, not every intermediate one — dropping the old
    /// `Receiver` here drops its still-running thread's *send* target, not
    /// the thread itself, but nothing is left waiting on it either way).
    pub fn run(&mut self, path: PathBuf, root: PathBuf) {
        let (tx, rx) = channel();
        let path_for_thread = path.clone();
        std::thread::spawn(move || {
            let hunks = fg_core::git_diff_hunks(&path_for_thread, &root).map_err(|e| e.to_string());
            let blame = fg_core::git_blame(&path_for_thread, &root).map_err(|e| e.to_string());
            let _ = tx.send((hunks, blame));
        });
        self.scans.insert(path, rx);
    }

    /// Drains every scan that's finished since the last poll, applying each
    /// half straight to its matching open tab's `diff_hunks`/`blame` —
    /// called once a frame from `FoxGardenApp::ui`. A result for a path
    /// that's since closed is dropped entirely; a failed half (including
    /// "not a git repository," which `fg_core::git_diff_hunks`/`git_blame`
    /// deliberately don't distinguish from "no changes"/"no blame info" —
    /// see their own doc comments) is dropped on its own without touching
    /// the other, still-good half, rather than surfaced through
    /// `last_error`: this is an automatic background refresh, not a
    /// user-requested action, so silently showing no marks is the right
    /// degrade, not an error toast on every non-git file.
    pub fn poll(&mut self, state: &mut EditorState) {
        let mut done = Vec::new();
        for (path, rx) in &self.scans {
            match rx.try_recv() {
                Ok(result) => done.push((path.clone(), Some(result))),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => done.push((path.clone(), None)),
            }
        }
        for (path, result) in done {
            self.scans.remove(&path);
            let Some((hunks, blame)) = result else { continue };
            if let Some(doc) = state.open_tabs.iter_mut().find(|d| d.path == path) {
                if let Ok(hunks) = hunks {
                    doc.diff_hunks = hunks;
                }
                if let Ok(blame) = blame {
                    doc.blame = blame;
                }
            }
        }
    }

    /// Detects every open tab whose dirty state just went from `true` to
    /// `false` since the last call — a save, no matter which of this app's
    /// several save paths produced it — and kicks off a fresh `git diff`
    /// for each. Also covers the external-change banner's manual "Reload"
    /// button (discarding local edits to match disk is itself a dirty ->
    /// clean transition), so only the *transparent* auto-reload path (never
    /// dirty before or after, since it only fires when there were no local
    /// edits to begin with) needs its own explicit trigger — see `app.rs`'s
    /// `reload_tab_from_disk`. A no-op when `root` is `None` (no project
    /// open, so nothing to diff against).
    pub fn check_for_saves(&mut self, state: &EditorState, root: Option<&Path>) {
        let mut fresh = HashMap::with_capacity(state.open_tabs.len());
        for doc in &state.open_tabs {
            let now_dirty = doc.is_dirty();
            let was_dirty = self.last_dirty.get(&doc.path).copied().unwrap_or(now_dirty);
            if was_dirty && !now_dirty && let Some(root) = root {
                self.run(doc.path.clone(), root.to_path_buf());
            }
            fresh.insert(doc.path.clone(), now_dirty);
        }
        self.last_dirty = fresh;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_returns_early_with_nothing_running() {
        let mut diff = DiffState::default();
        let mut state = EditorState::default();
        diff.poll(&mut state); // must not panic with no scans in flight
    }

    #[test]
    fn run_then_poll_applies_a_completed_scan_to_the_matching_open_tab() {
        let (_dir, doc) = test_support::temp_document("A.java", "class A {}");
        let path = doc.path.clone();
        let mut state = EditorState { open_tabs: vec![doc], ..Default::default() };

        let mut diff = DiffState::default();
        // A nonexistent root still exercises the real spawn-and-send path —
        // `git_diff_hunks`/`git_blame` both degrade to an empty result
        // rather than an error, so this exercises "a completed scan with
        // zero hunks/blame lines is still applied," not just the happy
        // path.
        diff.run(path.clone(), PathBuf::from("/nonexistent/root"));

        loop {
            diff.poll(&mut state);
            if !diff.scans.contains_key(&path) {
                break;
            }
        }
        assert!(state.open_tabs[0].diff_hunks.is_empty());
        assert!(state.open_tabs[0].blame.is_empty());
    }

    #[test]
    fn run_then_poll_applies_real_blame_lines_to_the_matching_open_tab() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let file = root.join("A.java");
        std::fs::write(&file, "class A {}\n").unwrap();
        let run = |args: &[&str]| std::process::Command::new("git").current_dir(&root).args(args).output().unwrap();
        run(&["init", "-q"]);
        run(&["config", "user.email", "a@b.com"]);
        run(&["config", "user.name", "test"]);
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);

        let doc = fg_core::Document::open(file.clone()).unwrap();
        let mut state = EditorState { open_tabs: vec![doc], ..Default::default() };

        let mut diff = DiffState::default();
        diff.run(file.clone(), root);
        loop {
            diff.poll(&mut state);
            if !diff.scans.contains_key(&file) {
                break;
            }
        }
        assert_eq!(state.open_tabs[0].blame.len(), 1);
        assert_eq!(state.open_tabs[0].blame[0].summary, "init");
    }

    #[test]
    fn poll_drops_a_result_for_a_path_thats_no_longer_open() {
        let mut diff = DiffState::default();
        diff.run(PathBuf::from("/tmp/gone.java"), PathBuf::from("."));
        let mut state = EditorState::default(); // nothing open

        loop {
            diff.poll(&mut state);
            if diff.scans.is_empty() {
                break;
            }
        }
        assert!(state.open_tabs.is_empty(), "must not panic or fabricate a tab for the dropped result");
    }

    #[test]
    fn check_for_saves_does_not_trigger_on_a_tabs_very_first_sighting() {
        let (_dir, doc) = test_support::temp_document("A.java", "class A {}");
        let mut state = EditorState { open_tabs: vec![doc], ..Default::default() };
        let mut diff = DiffState::default();

        diff.check_for_saves(&state, Some(Path::new(".")));
        assert!(diff.scans.is_empty(), "a freshly-observed clean tab is not a save transition");

        state.open_tabs[0].buffer.insert(0, "// x\n");
        diff.check_for_saves(&state, Some(Path::new(".")));
        assert!(diff.scans.is_empty(), "going dirty is not a save transition either");
    }

    #[test]
    fn check_for_saves_triggers_on_a_dirty_to_clean_transition() {
        let (_dir, mut doc) = test_support::temp_document("A.java", "class A {}");
        doc.buffer.insert(0, "// x\n");
        assert!(doc.is_dirty());
        let path = doc.path.clone();
        let mut state = EditorState { open_tabs: vec![doc], ..Default::default() };
        let mut diff = DiffState::default();

        diff.check_for_saves(&state, Some(Path::new("."))); // observes it already dirty

        state.open_tabs[0].buffer = state.open_tabs[0].saved_buffer.clone(); // simulates a save clearing dirty
        diff.check_for_saves(&state, Some(Path::new(".")));

        assert!(diff.scans.contains_key(&path), "the dirty -> clean transition must kick off a scan");
    }

    #[test]
    fn check_for_saves_is_a_no_op_with_no_project_root() {
        let (_dir, mut doc) = test_support::temp_document("A.java", "class A {}");
        doc.buffer.insert(0, "// x\n");
        let mut state = EditorState { open_tabs: vec![doc], ..Default::default() };
        let mut diff = DiffState::default();

        diff.check_for_saves(&state, None);
        state.open_tabs[0].buffer = state.open_tabs[0].saved_buffer.clone();
        diff.check_for_saves(&state, None);

        assert!(diff.scans.is_empty(), "no project root means nothing to diff against");
    }
}
