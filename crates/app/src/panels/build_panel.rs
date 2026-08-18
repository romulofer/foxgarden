//! The dockable Build Output panel (`PLAN.md` Track 22 — Build/Run, one
//! shared panel per Phase 1's own "Shared dockable output panel" text):
//! runs `mvn compile`/`gradle compileJava` (`fg_core::build_command`) on a
//! background thread, streaming its stdout+stderr live into a scrollable
//! log; a row whose text `fg_core::parse_build_output_line` recognized as a
//! compiler diagnostic is also a click-to-jump entry (`pending_navigation`,
//! the same mechanism the Spring endpoint map already established — see
//! `app.rs`'s own `jump_to`/`resolve_pending_navigation`).
//!
//! Streaming, not spawn-wait-collect: `fg_core`'s existing background-job
//! helpers (`static_analysis`, `gradle_classpaths`, ...) all call
//! `Command::output()` and hand back one finished `Result` — fine for a
//! scan with no user-facing progress, wrong for a build/run a user is
//! actively watching. This instead mirrors `pty_session::PtySession`'s own
//! shape (a background reader thread pushing into an `mpsc` channel,
//! drained non-blockingly once a frame) — line-buffered rather than raw
//! bytes (no terminal emulation needed here), and polling `ctx.
//! request_repaint()` from the panel's own `show` (like `git_stage`) rather
//! than from inside the thread (like `pty_session`), since that avoids
//! capturing an `egui::Context` into the reader threads for no real benefit
//! here.
//!
//! `PLAN.md` Track 22 Phase 2 ("Run") reuses this exact same streaming
//! plumbing rather than growing its own copy: `start_run` spawns the same
//! compile command `start_build` does, and `poll`'s own `Finished` handling
//! chains straight into a second, `fg_core::run_command`-assembled `java`
//! launch once that compile succeeds — both stages append to the same
//! `rows`, so a Run's own compiler-error and program-output lines share one
//! continuous, scrollable log. `Stop` (Phase 2's own checkpoint) needs a
//! live handle to whichever child is *currently* running to kill it, which
//! `start_build`'s original single-shot design didn't keep around at all —
//! `child` is now an `Arc<Mutex<Option<Child>>>` shared with the
//! completion-waiting thread (which locks it to call `wait()` rather than
//! owning the `Child` outright), so `stop()` can reach in and kill it from
//! the UI thread at any time, mid-compile or mid-run alike.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use fg_core::{BuildProblem, BuildTool, RunConfig, Severity};
use fg_i18n::t;

/// One line of accumulated output, with the `BuildProblem` it parsed to (if
/// any) already computed once, when the line arrived, rather than
/// re-parsed every frame the panel repaints. Always `None` for a Run
/// stage's own program-output lines (`parse_build_output_line` only ever
/// matches a compiler diagnostic's own line shape).
struct BuildRow {
    text: String,
    problem: Option<BuildProblem>,
}

enum BuildEvent {
    Line(String),
    Finished { success: bool },
}

/// What the currently in-flight process (if any) is for. `Build` stops
/// after that one command; `RunCompiling` chains a second `java` launch
/// once the compile it's currently streaming succeeds, transitioning to
/// `RunLaunched` for that second process.
enum Stage {
    Build,
    RunCompiling {
        project_root: PathBuf,
        tool: BuildTool,
        config: RunConfig,
    },
    RunLaunched,
}

#[derive(Default)]
pub struct BuildState {
    rows: Vec<BuildRow>,
    rx: Option<Receiver<BuildEvent>>,
    child: Arc<Mutex<Option<Child>>>,
    stage: Option<Stage>,
    last_success: Option<bool>,
}

impl BuildState {
    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    /// Distinguishes which of the two menu actions is currently in flight
    /// (they share this one panel/state, but Run > Build and Run > Run
    /// Project each need their own "am I busy" answer to disable/relabel
    /// only themselves, not both, while the other's job runs).
    pub fn is_build_running(&self) -> bool {
        self.running() && matches!(self.stage, Some(Stage::Build))
    }

    pub fn is_run_running(&self) -> bool {
        self.running() && matches!(self.stage, Some(Stage::RunCompiling { .. }) | Some(Stage::RunLaunched))
    }

    /// Starts a plain Build: the project's own compile command, stopping
    /// once it finishes either way.
    pub fn start_build(&mut self, project_root: &Path, tool: BuildTool) -> std::io::Result<()> {
        self.reset();
        self.spawn_process(fg_core::build_command(project_root, tool))?;
        self.stage = Some(Stage::Build);
        Ok(())
    }

    /// Starts a Run: the same compile command `start_build` uses, but
    /// `poll`'s own `Finished` handling chains into launching `config` via
    /// `fg_core::run_command` once (and only if) that compile succeeds —
    /// matching every mainstream IDE's own "Run always builds first"
    /// behavior, and matching Track 22's own inter-phase framing (Run
    /// "reusing `RunConfig`" on top of the same Build this phase already
    /// established).
    pub fn start_run(&mut self, project_root: &Path, tool: BuildTool, config: RunConfig) -> std::io::Result<()> {
        self.reset();
        self.spawn_process(fg_core::build_command(project_root, tool))?;
        self.stage = Some(Stage::RunCompiling {
            project_root: project_root.to_path_buf(),
            tool,
            config,
        });
        Ok(())
    }

    /// Kills whichever process is currently in flight (the compile, or the
    /// launched `java` run) — real termination via `Child::kill`, not just
    /// dropping our own handle to it (which would leave it running,
    /// detached). A no-op if nothing is running.
    pub fn stop(&mut self) {
        if let Ok(mut guard) = self.child.lock()
            && let Some(child) = guard.as_mut()
        {
            let _ = child.kill();
        }
    }

    fn reset(&mut self) {
        self.stop();
        self.rows.clear();
        self.rx = None;
        self.stage = None;
        self.last_success = None;
    }

    /// Spawns `command` with both stdout and stderr piped, one reader
    /// thread per stream (a real, found-not-assumed detail: Maven's own
    /// `[ERROR]` lines land on **stdout**, Gradle's plain javac ones land
    /// on **stderr** — verified against real `mvn`/`gradle` runs, so both
    /// streams genuinely need reading, not just one merged into the other).
    /// A third thread joins both readers (guaranteeing every already-
    /// buffered line is drained before `Finished` is sent, not just that
    /// the process itself exited) and then waits on the child, through the
    /// shared `Arc<Mutex<..>>` `stop()` also reaches into, for the real
    /// exit status.
    fn spawn_process(&mut self, mut command: std::process::Command) -> std::io::Result<()> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn()?;

        let (tx, rx) = channel();
        let stdout_thread = child.stdout.take().map(|s| spawn_line_reader(s, tx.clone()));
        let stderr_thread = child.stderr.take().map(|s| spawn_line_reader(s, tx.clone()));

        let child_slot = Arc::new(Mutex::new(Some(child)));
        self.child = child_slot.clone();

        std::thread::spawn(move || {
            if let Some(t) = stdout_thread {
                let _ = t.join();
            }
            if let Some(t) = stderr_thread {
                let _ = t.join();
            }
            let success = match child_slot.lock() {
                Ok(mut guard) => matches!(guard.as_mut().map(|c| c.wait()), Some(Ok(status)) if status.success()),
                Err(_) => false,
            };
            let _ = tx.send(BuildEvent::Finished { success });
        });

        self.rx = Some(rx);
        Ok(())
    }

    fn poll(&mut self) {
        let Some(rx) = self.rx.take() else { return };
        let mut still_running = true;
        loop {
            match rx.try_recv() {
                Ok(BuildEvent::Line(text)) => {
                    let problem = fg_core::parse_build_output_line(&text);
                    self.rows.push(BuildRow { text, problem });
                }
                Ok(BuildEvent::Finished { success }) => {
                    still_running = false;
                    match self.stage.take() {
                        Some(Stage::RunCompiling {
                            project_root,
                            tool,
                            config,
                        }) if success => match fg_core::run_command(&project_root, tool, &config) {
                            Ok(command) => match self.spawn_process(command) {
                                Ok(()) => self.stage = Some(Stage::RunLaunched),
                                Err(err) => {
                                    self.rows.push(BuildRow {
                                        text: format!("Failed to launch: {err}"),
                                        problem: None,
                                    });
                                    self.last_success = Some(false);
                                }
                            },
                            Err(err) => {
                                self.rows.push(BuildRow {
                                    text: format!("Failed to resolve run classpath: {err}"),
                                    problem: None,
                                });
                                self.last_success = Some(false);
                            }
                        },
                        _ => self.last_success = Some(success),
                    }
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    still_running = false;
                    break;
                }
            }
        }
        if still_running {
            self.rx = Some(rx);
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

/// Draws the panel and polls its background job for this frame. Returns
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

    if state.running() {
        ui.horizontal(|ui| {
            if ui.button(t().common.stop).clicked() {
                state.stop();
            }
        });
        ui.separator();
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
