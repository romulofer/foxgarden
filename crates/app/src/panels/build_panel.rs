//! The dockable Build Output panel (`PLAN.md` Track 22 Phase 1): runs `mvn
//! compile`/`gradle compileJava` (`fg_core::build_command`) on a background
//! thread, streaming its stdout+stderr live into a scrollable log; a row
//! whose text `fg_core::parse_build_output_line` recognized as a compiler
//! diagnostic is also a click-to-jump entry (`pending_navigation`, the same
//! mechanism the Spring endpoint map already established — see `app.rs`'s
//! own `jump_to`/`resolve_pending_navigation`).
//!
//! Streaming, not spawn-wait-collect: `fg_core`'s existing background-job
//! helpers (`static_analysis`, `gradle_classpaths`, ...) all call
//! `Command::output()` and hand back one finished `Result` — fine for a
//! scan with no user-facing progress, wrong for a build a user is actively
//! watching. This instead mirrors `pty_session::PtySession`'s own shape (a
//! background reader thread pushing into an `mpsc` channel, drained
//! non-blockingly once a frame) — line-buffered rather than raw bytes (no
//! terminal emulation needed here), and polling `ctx.request_repaint()`
//! from the panel's own `show` (like `git_stage`) rather than from inside
//! the thread (like `pty_session`), since that avoids capturing an
//! `egui::Context` into the reader threads for no real benefit here.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::JoinHandle;

use fg_core::{BuildProblem, BuildTool, Severity};

/// One line of accumulated build output, with the `BuildProblem` it parsed
/// to (if any) already computed once, when the line arrived, rather than
/// re-parsed every frame the panel repaints.
struct BuildRow {
    text: String,
    problem: Option<BuildProblem>,
}

enum BuildEvent {
    Line(String),
    Finished { success: bool },
}

#[derive(Default)]
pub struct BuildState {
    rows: Vec<BuildRow>,
    rx: Option<Receiver<BuildEvent>>,
    last_success: Option<bool>,
}

impl BuildState {
    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    /// Spawns the real build (`fg_core::build_command`) with both stdout
    /// and stderr piped, one reader thread per stream (a real, found-not-
    /// assumed detail: Maven's own `[ERROR]` lines land on **stdout**,
    /// Gradle's plain javac ones land on **stderr** — verified against real
    /// `mvn`/`gradle` runs, so both streams genuinely need reading, not
    /// just one merged into the other). A third thread joins both readers
    /// (guaranteeing every already-buffered line is drained before
    /// `Finished` is sent, not just that the process itself exited) and
    /// then waits on the child for the real exit status.
    pub fn start(&mut self, project_root: &Path, tool: BuildTool) -> std::io::Result<()> {
        self.rows.clear();
        self.last_success = None;

        let mut command = fg_core::build_command(project_root, tool);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn()?;

        let (tx, rx) = channel();
        let stdout_thread = child.stdout.take().map(|s| spawn_line_reader(s, tx.clone()));
        let stderr_thread = child.stderr.take().map(|s| spawn_line_reader(s, tx.clone()));
        std::thread::spawn(move || {
            if let Some(t) = stdout_thread {
                let _ = t.join();
            }
            if let Some(t) = stderr_thread {
                let _ = t.join();
            }
            let success = matches!(child.wait(), Ok(status) if status.success());
            let _ = tx.send(BuildEvent::Finished { success });
        });

        self.rx = Some(rx);
        Ok(())
    }

    fn poll(&mut self) {
        let Some(rx) = self.rx.as_ref() else { return };
        loop {
            match rx.try_recv() {
                Ok(BuildEvent::Line(text)) => {
                    let problem = fg_core::parse_build_output_line(&text);
                    self.rows.push(BuildRow { text, problem });
                }
                Ok(BuildEvent::Finished { success }) => {
                    self.last_success = Some(success);
                    self.rx = None;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.rx = None;
                    break;
                }
            }
        }
    }
}

fn spawn_line_reader<R>(reader: R, tx: Sender<BuildEvent>) -> JoinHandle<()>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(text) => {
                    if tx.send(BuildEvent::Line(text)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}

/// Draws the panel and polls its background build for this frame. Returns
/// `Some((path, line, column))` — 1-based, straight from the compiler, not
/// yet a byte offset — the frame a problem row is clicked; `app.rs`'s own
/// call site opens that path and converts through its now-live buffer
/// before setting `pending_navigation`, the same deferred-conversion shape
/// the Spring endpoint map's own call site already uses.
pub fn show(ui: &mut egui::Ui, state: &mut BuildState) -> Option<(PathBuf, usize, usize)> {
    state.poll();
    if state.running() {
        ui.ctx().request_repaint();
    }

    let mut clicked = None;
    egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
        for row in &state.rows {
            let Some(problem) = &row.problem else {
                ui.monospace(&row.text);
                continue;
            };
            let color = match problem.severity {
                Severity::Error => ui.visuals().error_fg_color,
                Severity::Warning => ui.visuals().warn_fg_color,
            };
            let response = ui.add(
                egui::Label::new(egui::RichText::new(&row.text).monospace().color(color)).sense(egui::Sense::click()),
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                clicked = Some((problem.path.clone(), problem.line, problem.column));
            }
        }
    });
    clicked
}
