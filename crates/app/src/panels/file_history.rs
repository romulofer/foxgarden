//! Tab context-menu "File History…" (`PLAN.md` Track 4 Phase 2): a floating
//! window listing every `.foxgarden/history/` snapshot `fg_core::file_
//! history::write_snapshot` (Phase 1) has written for one file, each row's
//! own "+N -M" (`widgets::diff_view::diff_stat`) against that file's
//! current *live* buffer (not its last save — a snapshot vs. an unsaved
//! edit is exactly the comparison a "what have I changed since this
//! version" browse wants), a selected row's full diff below via Track 18's
//! `widgets::diff_view::show_diff`, and a Revert button. Same background-
//! thread-plus-`Receiver` shape `git_stage.rs`'s own "Full Diff" window
//! already established for the identical "fetch real file content off the
//! UI thread, poll once a frame" problem.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use fg_i18n::t;

use crate::style::fonts::EditorFont;
use crate::widgets::diff_view::{self, DiffMode};

/// One snapshot row, already carrying its own content and diff stat against
/// the live buffer captured at `open` time — computed once, in the
/// background, rather than re-reading/re-diffing 50 files a frame while the
/// window stays open (see this module's own doc comment).
struct SnapshotRow {
    timestamp_nanos: u128,
    content: String,
    added: usize,
    removed: usize,
}

type RowsResult = Vec<SnapshotRow>;

fn spawn<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Receiver<T> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx
}

#[derive(Default)]
pub struct FileHistoryState {
    /// The file currently shown, plus the `SideBySide`/`Inline` toggle's
    /// current value — `None` means the window is closed. Same one-`Option`
    /// "there's only ever one of these open" shape `GitStageState::full_
    /// diff` already uses for its own floating window.
    open_for: Option<(PathBuf, DiffMode)>,
    /// The live buffer's text at the moment `open` was called — every row's
    /// own diff stat, and the selected row's full diff, compares against
    /// this, not a re-read each frame (see the module doc comment).
    live_content: String,
    rows: Vec<SnapshotRow>,
    rows_rx: Option<Receiver<RowsResult>>,
    /// Index into `rows` of whichever one the list currently has expanded,
    /// if any.
    selected: Option<usize>,
}

impl FileHistoryState {
    /// Opens the window for `file_path`, kicking off the background scan.
    /// `live_content` is the file's *current* editor buffer content — the
    /// caller (`panels::tabs`, which already has the live `Document` in
    /// hand at the context-menu click site) is what makes this "vs. live
    /// buffer", not "vs. last save", the same distinction `Document::
    /// project_root`'s own doc comment draws for why snapshotting can't
    /// just read `saved_buffer`. Re-opening while already open for the same
    /// file leaves the mode toggle wherever the user left it, same as
    /// `GitStageState::open_full_diff`.
    pub fn open(&mut self, project_root: PathBuf, file_path: PathBuf, live_content: String) {
        let mode = match &self.open_for {
            Some((existing, mode)) if existing == &file_path => *mode,
            _ => DiffMode::SideBySide,
        };
        self.open_for = Some((file_path.clone(), mode));
        self.live_content = live_content.clone();
        self.rows = Vec::new();
        self.selected = None;
        self.rows_rx = Some(spawn(move || {
            fg_core::list_snapshots(&project_root, &file_path)
                .into_iter()
                .filter_map(|snapshot| {
                    let content = std::fs::read_to_string(&snapshot.path).ok()?;
                    let (added, removed) = diff_view::diff_stat(&content, &live_content);
                    Some(SnapshotRow {
                        timestamp_nanos: snapshot.timestamp_nanos,
                        content,
                        added,
                        removed,
                    })
                })
                .collect()
        }));
    }

    pub fn close(&mut self) {
        *self = Self::default();
    }

    /// The file the window is currently open for, if any — `panels::tabs`'
    /// own revert application needs this to look the matching open tab back
    /// up (`EditorState::find_tab`) once `show` hands back a Revert click's
    /// content.
    pub fn open_path(&self) -> Option<&Path> {
        self.open_for.as_ref().map(|(path, _)| path.as_path())
    }

    fn set_mode(&mut self, mode: DiffMode) {
        if let Some(open_for) = &mut self.open_for {
            open_for.1 = mode;
        }
    }

    fn running(&self) -> bool {
        self.rows_rx.is_some()
    }

    /// Drains a completed scan, if any. Mirrors `GitStageState::poll_full_
    /// diff`'s own shape — `app.rs`/`tabs.rs` just needs to call this once a
    /// frame so a repaint keeps happening while the scan is in flight (see
    /// `show`'s own request_repaint call).
    fn poll(&mut self) {
        let Some(rx) = &self.rows_rx else { return };
        match rx.try_recv() {
            Ok(rows) => {
                self.rows = rows;
                self.rows_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.rows_rx = None,
        }
    }
}

/// Draws the "File History…" window, if `state` has one open. Returns the
/// content to restore if the Revert button under the selected row was
/// clicked this frame — applying it (through the normal buffer-replace +
/// reparse path `workspace_edit::apply_file_edits` already established for
/// "a real, undoable content change from outside typing") is left to the
/// caller, the same "this widget only reports the click, the caller owns
/// `EditorState`" split `git_stage.rs`'s own hunk-stage buttons already use.
pub fn show(
    ctx: &egui::Context,
    state: &mut FileHistoryState,
    editor_font: EditorFont,
    font_size: f32,
    dark_mode: bool,
) -> Option<String> {
    state.poll();
    if state.running() {
        ctx.request_repaint();
    }

    let (file_path, mut mode) = state.open_for.clone()?;
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut open = true;
    let mut revert_content = None;

    egui::Window::new(format!("{}{file_name}", t().file_history.heading_prefix))
        .id(egui::Id::new("file_history_window"))
        .open(&mut open)
        .default_size([700.0, 500.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut mode, DiffMode::SideBySide, t().file_history.side_by_side);
                ui.selectable_value(&mut mode, DiffMode::Inline, t().file_history.inline);
            });
            ui.separator();

            if state.running() && state.rows.is_empty() {
                ui.label(egui::RichText::new(t().file_history.loading).weak());
                return;
            }
            if state.rows.is_empty() {
                ui.label(egui::RichText::new(t().file_history.empty).weak());
                return;
            }

            egui::ScrollArea::vertical()
                .max_height(150.0)
                .id_salt("file_history_rows")
                .show(ui, |ui| {
                    for (index, row) in state.rows.iter().enumerate() {
                        let is_selected = state.selected == Some(index);
                        let label = format!(
                            "{}   +{} -{}",
                            format_timestamp(row.timestamp_nanos),
                            row.added,
                            row.removed
                        );
                        if ui.selectable_label(is_selected, label).clicked() {
                            state.selected = if is_selected { None } else { Some(index) };
                        }
                    }
                });

            let Some(row) = state.selected.and_then(|index| state.rows.get(index)) else {
                return;
            };
            ui.separator();
            if ui.button(t().file_history.revert).clicked() {
                revert_content = Some(row.content.clone());
            }
            egui::ScrollArea::both().id_salt("file_history_diff").show(ui, |ui| {
                diff_view::show_diff(
                    ui,
                    &row.content,
                    &state.live_content,
                    mode,
                    editor_font,
                    font_size,
                    dark_mode,
                );
            });
        });

    state.set_mode(mode);
    if !open {
        state.close();
    }
    revert_content
}

/// A coarse, bucketed "how long ago" string for a snapshot's own nanosecond
/// timestamp (`fg_core::Snapshot::timestamp_nanos`) — same bucketing
/// `widgets::editor::painting::relative_time` already uses for blame
/// annotations, duplicated in miniature here rather than reused across that
/// module boundary (it's `pub(super)` to `widgets::editor`, and 15 lines of
/// integer division isn't worth widening that visibility for).
fn format_timestamp(timestamp_nanos: u128) -> String {
    let then_unix = (timestamp_nanos / 1_000_000_000) as i64;
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(then_unix);
    let secs = (now_unix - then_unix).max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else if secs < 30 * 86_400 {
        format!("{}d ago", secs / 86_400)
    } else {
        format!("{}mo ago", secs / (30 * 86_400))
    }
}

#[cfg(test)]
#[path = "file_history_test.rs"]
mod file_history_test;
