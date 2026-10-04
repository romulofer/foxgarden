//! The dockable Build Output panel (`PLAN.md` Track 22 — Build/Run, one
//! shared panel per Phase 1's own "Shared dockable output panel" text):
//! runs whatever compile command the project's own build tool contributes
//! (`BuildToolHandle::command`) on a background thread, streaming its
//! stdout+stderr live into a scrollable log; a row the same tool recognized
//! as a compiler diagnostic is also a click-to-jump entry (`pending_navigation`,
//! the same mechanism the HTTP route map already established — see
//! `app.rs`'s own `jump_to`/`resolve_pending_navigation`).
//!
//! Streaming, not spawn-wait-collect: this codebase's other background-job
//! helpers (`static_analysis`, classpath resolution, ...) all call
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
//! chains straight into the launch the same tool assembles
//! (`BuildToolHandle::run_command`) once that compile succeeds — both stages append to the same
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

use fg_core::{BuildProblem, BuildToolHandle, EditorState, LineCoverage, ProblemSeverity, RunConfig};
use fg_i18n::t;

/// One line of accumulated output, with the `BuildProblem` it parsed to (if
/// any) already computed once, when the line arrived, rather than
/// re-parsed every frame the panel repaints. Always `None` for a Run
/// stage's own program-output lines — a tool only ever recognizes a
/// compiler diagnostic's own line shape.
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
/// `RunLaunched` for that second process. `Test` also stops after one
/// command (a test task already compiles everything it needs on its own,
/// unlike Run's separate launch) but its own `Finished` handling
/// additionally summarizes the tool's own test report once the process exits, appending that summary (and a
/// clickable row per failing test) to the same log.
enum Stage {
    Build,
    RunCompiling {
        project_root: PathBuf,
        tool: BuildToolHandle,
        config: RunConfig,
    },
    RunLaunched,
    Test {
        project_root: PathBuf,
        tool: BuildToolHandle,
    },
    /// Run with Coverage (`PLAN.md` Track 13 Phase 1, Maven-only): one
    /// `mvn` process running `prepare-agent`+`test`+`report` back to back
    /// (`BuildTask::Coverage` — one process, not a chained pair the way
    /// `RunCompiling` chains into a second launch). `poll`'s own `Finished`
    /// handling asks the tool for the resulting report
    /// (`BuildToolHandle::coverage_results`) once this exits successfully.
    Coverage {
        project_root: PathBuf,
        tool: BuildToolHandle,
    },
    /// Docker "Build & Run" (`PLAN.md` Track 14 Phase 1): `docker build`
    /// running against `project_root`'s `Dockerfile`; `poll`'s own
    /// `Finished` handling chains straight into `docker run --rm` of the
    /// image it just built once that succeeds, the same "compile, then
    /// launch" chain `RunCompiling`/`RunLaunched` already establish for
    /// `mvn`/`gradle` + `java`.
    DockerBuild {
        project_root: PathBuf,
    },
    /// The `docker run --rm` launched once `DockerBuild` finishes — no
    /// further chaining, so this needs no fields of its own, same as
    /// `RunLaunched`. The container's own name (for `docker stop`) lives in
    /// `BuildState::docker_teardown`, not here, since it has to outlive the
    /// stage being `take`n in `poll`.
    DockerRun,
    /// Docker Compose "Up" (`PLAN.md` Track 14 Phase 1): one `docker
    /// compose up --build` process covering build-and-start together, no
    /// chained second process — same one-process shape `Coverage` already
    /// uses for a tool that does everything itself.
    DockerCompose,
}

/// What Phase 2 has to tear down for the Docker task currently in flight —
/// the piece killing the local `docker` client `Child` can't do on its own
/// (the container/stack lives on the daemon, not as our child process). Set
/// the moment a `docker run`/`docker compose up` starts, cleared once it
/// ends (either torn down explicitly by `stop`/`shutdown`, or on its own
/// when `poll` sees it finish — a `--rm` container removes itself on exit).
#[derive(Clone)]
enum DockerTeardown {
    /// `docker stop <name>` for a `docker_run_command` container.
    Container(String),
    /// `docker compose -f <file> down` for a `docker_compose_up_command`
    /// stack.
    Compose(PathBuf),
}

impl DockerTeardown {
    fn command(&self) -> std::process::Command {
        match self {
            DockerTeardown::Container(name) => fg_core::docker_stop_command(name),
            DockerTeardown::Compose(file) => fg_core::docker_compose_down_command(file),
        }
    }
}

/// One coverage run's own outcome: every source file JaCoCo reported data
/// for, paired with its per-line marks — or the error reading/parsing
/// `jacoco.xml` hit. Named alias purely to keep `BuildState`'s own field
/// and `take_coverage_result`'s signature legible (`clippy::
/// type_complexity` otherwise flags the inline nested-generic spelling).
type CoverageResult = Result<Vec<(PathBuf, Vec<LineCoverage>)>, String>;

#[derive(Default)]
pub struct BuildState {
    rows: Vec<BuildRow>,
    rx: Option<Receiver<BuildEvent>>,
    child: Arc<Mutex<Option<Child>>>,
    stage: Option<Stage>,
    last_success: Option<bool>,
    /// Set once by `poll`'s own `Coverage` `Finished` handling, drained by
    /// `take_coverage_result` — a getter separate from `show`'s own return
    /// value (which only carries a click target) since growing that
    /// signature for a second, unrelated payload would be a worse fit than
    /// a dedicated drain method.
    coverage_result: Option<CoverageResult>,
    /// The running Docker container/stack this state is responsible for
    /// stopping (`PLAN.md` Track 14 Phase 2), or `None` when no Docker task
    /// is live. Read by `stop` (fire-and-forget teardown) and `shutdown`
    /// (blocking teardown on app close).
    docker_teardown: Option<DockerTeardown>,
    /// Whichever build tool produced the output currently streaming in — the
    /// only thing that can say whether a line names a compiler diagnostic.
    /// Kept beside `stage` rather than inside it because a Docker task has
    /// output but no build tool, and `Stage` would then need the field on
    /// every arm.
    output_tool: Option<BuildToolHandle>,
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

    pub fn is_test_running(&self) -> bool {
        self.running() && matches!(self.stage, Some(Stage::Test { .. }))
    }

    /// The PID of the launched `java` program, once a Run has reached its
    /// `RunLaunched` stage — the profiler (`PLAN.md` Track 26) attaches to
    /// this. `None` during the `RunCompiling` stage (the child is then the
    /// `mvn`/`gradle` compiler, not the user's own JVM worth profiling) and
    /// whenever nothing is running. Reads the shared `child` handle
    /// `spawn_process`/`stop` also hold, so it reflects the process actually
    /// alive right now.
    pub fn run_pid(&self) -> Option<u32> {
        if !matches!(self.stage, Some(Stage::RunLaunched)) {
            return None;
        }
        self.child.lock().ok()?.as_ref().map(|child| child.id())
    }

    pub fn is_coverage_running(&self) -> bool {
        self.running() && matches!(self.stage, Some(Stage::Coverage { .. }))
    }

    /// Distinguishes Docker "Build & Run" from Docker Compose "Up" the same
    /// way `is_build_running`/`is_run_running` distinguish Build from Run —
    /// each menu entry needs its own "am I busy" answer.
    pub fn is_docker_build_run_running(&self) -> bool {
        self.running() && matches!(self.stage, Some(Stage::DockerBuild { .. }) | Some(Stage::DockerRun))
    }

    pub fn is_docker_compose_running(&self) -> bool {
        self.running() && matches!(self.stage, Some(Stage::DockerCompose))
    }

    /// Starts a plain Build: the project's own compile command, stopping
    /// once it finishes either way.
    pub fn start_build(&mut self, project_root: &Path, tool: &BuildToolHandle) -> std::io::Result<()> {
        self.reset();
        self.spawn_task(project_root, tool, fg_core::BuildTask::Compile)?;
        self.stage = Some(Stage::Build);
        Ok(())
    }

    /// Starts a Run: the same compile command `start_build` uses, but
    /// `poll`'s own `Finished` handling chains into launching `config` via
    /// the tool's own launch command once (and only if) that compile succeeds —
    /// matching every mainstream IDE's own "Run always builds first"
    /// behavior, and matching Track 22's own inter-phase framing (Run
    /// "reusing `RunConfig`" on top of the same Build this phase already
    /// established).
    pub fn start_run(&mut self, project_root: &Path, tool: &BuildToolHandle, config: RunConfig) -> std::io::Result<()> {
        self.reset();
        self.spawn_task(project_root, tool, fg_core::BuildTask::Compile)?;
        self.stage = Some(Stage::RunCompiling {
            project_root: project_root.to_path_buf(),
            tool: tool.clone(),
            config,
        });
        Ok(())
    }

    /// Starts a Test: the tool's own test task (which already compiles
    /// everything it needs, unlike Run — no chained second process here).
    /// `poll`'s own `Finished` handling summarizes the tool's own test
    /// report once this exits, regardless of exit status (a test
    /// failure makes the process itself exit non-zero, but the report is
    /// still written and is what actually answers "which tests failed",
    /// not the exit code).
    pub fn start_test(&mut self, project_root: &Path, tool: &BuildToolHandle) -> std::io::Result<()> {
        self.reset();
        self.spawn_task(project_root, tool, fg_core::BuildTask::Test)?;
        self.stage = Some(Stage::Test {
            project_root: project_root.to_path_buf(),
            tool: tool.clone(),
        });
        Ok(())
    }

    /// Starts "Run with Coverage" (`PLAN.md` Track 13 Phase 1, Maven-only):
    /// one process covering instrumentation, test run and report conversion
    /// together — no chained second process, unlike Run's compile-then-launch.
    /// Only tools that contribute a `Coverage` task can start this.
    pub fn start_coverage(&mut self, project_root: &Path, tool: &BuildToolHandle) -> std::io::Result<()> {
        self.reset();
        self.spawn_task(project_root, tool, fg_core::BuildTask::Coverage)?;
        self.stage = Some(Stage::Coverage {
            project_root: project_root.to_path_buf(),
            tool: tool.clone(),
        });
        Ok(())
    }

    /// Starts Docker "Build & Run" (`PLAN.md` Track 14 Phase 1):
    /// `docker build`, chaining into `docker run --rm` of the built image
    /// once that succeeds (`poll`'s own `Finished` handling).
    pub fn start_docker_build_and_run(&mut self, project_root: &Path) -> std::io::Result<()> {
        self.reset();
        self.spawn_process(fg_core::docker_build_command(project_root))?;
        self.stage = Some(Stage::DockerBuild {
            project_root: project_root.to_path_buf(),
        });
        Ok(())
    }

    /// Starts Docker Compose "Up" (`PLAN.md` Track 14 Phase 1): one
    /// `docker compose up --build` process, no chaining.
    pub fn start_docker_compose_up(&mut self, compose_file: &Path) -> std::io::Result<()> {
        self.reset();
        self.spawn_process(fg_core::docker_compose_up_command(compose_file))?;
        self.stage = Some(Stage::DockerCompose);
        // The stack is now (being) brought up, so `stop`/`shutdown` owe it a
        // `docker compose down` — set before returning, not on first output,
        // so even an immediate Stop tears it down.
        self.docker_teardown = Some(DockerTeardown::Compose(compose_file.to_path_buf()));
        Ok(())
    }

    /// Drains a completed coverage run's parsed result, if any finished
    /// since the last poll — called once a frame from `FoxGardenApp::ui`,
    /// right after `build_panel::show` (the only place `poll` actually
    /// runs).
    pub fn take_coverage_result(&mut self) -> Option<CoverageResult> {
        self.coverage_result.take()
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
        // Killing the local `docker` client above doesn't stop the
        // container/stack it launched (Track 14 Phase 2) — that runs on the
        // daemon. Tear it down explicitly, on a background thread so
        // `docker stop`'s own up-to-10s graceful shutdown never freezes the
        // UI. Fire-and-forget: the daemon does the work regardless of
        // whether we wait on the client.
        if let Some(teardown) = self.docker_teardown.take() {
            let mut command = teardown.command();
            command.stdout(Stdio::null()).stderr(Stdio::null());
            std::thread::spawn(move || {
                let _ = command.status();
            });
        }
    }

    /// The app-close counterpart to `stop` (`PLAN.md` Track 14 Phase 2:
    /// "quitting the app stops everything still tracked"): same teardown,
    /// but run to completion rather than fire-and-forget — once the process
    /// exits, a background thread wouldn't survive to finish the `docker
    /// stop`, so this blocks (briefly) to guarantee the container/stack is
    /// actually stopped before FoxGarden goes away, not left orphaned on the
    /// daemon.
    pub fn shutdown(&mut self) {
        if let Ok(mut guard) = self.child.lock()
            && let Some(child) = guard.as_mut()
        {
            let _ = child.kill();
        }
        if let Some(teardown) = self.docker_teardown.take() {
            let mut command = teardown.command();
            command.stdout(Stdio::null()).stderr(Stdio::null());
            let _ = command.status();
        }
    }

    fn reset(&mut self) {
        self.stop();
        self.rows.clear();
        self.rx = None;
        self.stage = None;
        self.last_success = None;
        self.coverage_result = None;
        self.output_tool = None;
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
    /// Spawns `task` as `tool` describes it, remembering the tool so every
    /// output line can be parsed by whoever produced it. A task the tool
    /// doesn't support surfaces as an ordinary `io::Error`, the same way a
    /// missing binary already does.
    fn spawn_task(
        &mut self,
        project_root: &Path,
        tool: &BuildToolHandle,
        task: fg_core::BuildTask,
    ) -> std::io::Result<()> {
        let spec = tool.command(project_root, task).ok_or_else(|| {
            std::io::Error::other(format!("{} does not support this task", tool.display_name))
        })?;
        self.output_tool = Some(tool.clone());
        self.spawn_process(spec.to_command())
    }

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
            let success = wait_without_holding_the_lock(&child_slot);
            let _ = tx.send(BuildEvent::Finished { success });
        });

        self.rx = Some(rx);
        Ok(())
    }

    /// Asks `tool` for its last test run's results and appends a pass/fail
    /// summary line plus one clickable row per
    /// failing/errored test to the log — reusing `BuildRow`'s own
    /// `BuildProblem` shape (`column: 1`, since a test failure's own
    /// "where" is a line, the same as Gradle's own column-less compiler
    /// errors already use) rather than growing a second, parallel
    /// click-to-jump row type just for this.
    fn append_test_summary(&mut self, project_root: &Path, tool: &BuildToolHandle) {
        let cases = tool.test_results(project_root);
        let summary = fg_core::summarize(&cases);
        self.rows.push(BuildRow {
            text: String::new(),
            problem: None,
        });
        self.rows.push(BuildRow {
            text: format!(
                "Tests: {} total, {} passed, {} failed, {} errored, {} skipped",
                summary.total,
                summary.passed(),
                summary.failed,
                summary.errored,
                summary.skipped
            ),
            problem: None,
        });
        for case in cases
            .iter()
            .filter(|c| matches!(c.outcome, fg_core::TestOutcome::Failed | fg_core::TestOutcome::Errored))
        {
            let label = format!("{}.{}", case.classname, case.name);
            let problem = tool.test_failure_location(project_root, case).map(|at| BuildProblem {
                path: at.path,
                line: at.line,
                column: 1,
                severity: ProblemSeverity::Error,
                message: label.clone(),
            });
            self.rows.push(BuildRow {
                text: format!("FAILED  {label}"),
                problem,
            });
        }
    }

    /// Asks `tool` for the coverage its last run recorded, already resolved
    /// to real files, and appends a one-line summary to the log either way — same "summarize the batch result into the log"
    /// pattern `append_test_summary` already established for `Stage::
    /// Test`.
    fn finish_coverage(&mut self, project_root: &Path, tool: &BuildToolHandle) {
        let result = tool
            .coverage_results(project_root)
            .unwrap_or_else(|| Err(format!("{} reports no coverage data", tool.display_name)));
        self.rows.push(BuildRow {
            text: String::new(),
            problem: None,
        });
        match &result {
            Ok(files) => self.rows.push(BuildRow {
                text: format!("Coverage: {} source file(s) with data", files.len()),
                problem: None,
            }),
            Err(err) => self.rows.push(BuildRow {
                text: format!("Coverage report couldn't be read: {err}"),
                problem: None,
            }),
        }
        self.coverage_result = Some(result);
    }

    fn poll(&mut self) {
        let Some(rx) = self.rx.take() else { return };
        let mut still_running = true;
        loop {
            match rx.try_recv() {
                Ok(BuildEvent::Line(text)) => {
                    let problem = self.output_tool.as_ref().and_then(|tool| tool.parse_output_line(&text));
                    self.rows.push(BuildRow { text, problem });
                }
                Ok(BuildEvent::Finished { success }) => {
                    still_running = false;
                    match self.stage.take() {
                        Some(Stage::RunCompiling {
                            project_root,
                            tool,
                            config,
                        }) if success => match tool
                            .run_command(&project_root, &config.to_run_spec())
                            .unwrap_or_else(|| Err(format!("{} cannot launch a program", tool.display_name)))
                        {
                            Ok(spec) => match self.spawn_process(spec.to_command()) {
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
                        Some(Stage::Test { project_root, tool }) => {
                            self.last_success = Some(success);
                            self.append_test_summary(&project_root, &tool);
                        }
                        Some(Stage::Coverage { project_root, tool }) => {
                            self.last_success = Some(success);
                            if success {
                                self.finish_coverage(&project_root, &tool);
                            } else {
                                self.rows.push(BuildRow {
                                    text: "Coverage run failed to complete — see log above.".into(),
                                    problem: None,
                                });
                            }
                        }
                        Some(Stage::DockerBuild { project_root }) if success => {
                            let name = fg_core::container_name(&project_root);
                            match self.spawn_process(fg_core::docker_run_command(&project_root, &name)) {
                                Ok(()) => {
                                    self.stage = Some(Stage::DockerRun);
                                    // The container is now (being) started under
                                    // `name`, so `stop`/`shutdown` owe it a
                                    // `docker stop`.
                                    self.docker_teardown = Some(DockerTeardown::Container(name));
                                }
                                Err(err) => {
                                    self.rows.push(BuildRow {
                                        text: format!("Failed to run container: {err}"),
                                        problem: None,
                                    });
                                    self.last_success = Some(false);
                                }
                            }
                        }
                        // A Docker container/stack that finished on its own —
                        // a `--rm` container removes itself on exit, and a
                        // `compose up` that returns has already stopped — so
                        // there's nothing left to tear down.
                        Some(Stage::DockerRun) | Some(Stage::DockerCompose) => {
                            self.docker_teardown = None;
                            self.last_success = Some(success);
                        }
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

/// Waits for the child in `slot` to exit and reports whether it succeeded.
///
/// Polls `try_wait`, taking the lock only for each check. A blocking `wait`
/// inside the lock kept it for the child's whole remaining life, and
/// `stop`, `shutdown` and the per-frame `run_pid` all lock the same slot on
/// the UI thread — so a program that closed its output but kept running
/// (a daemonizing server) froze the editor, and Stop could never kill it.
fn wait_without_holding_the_lock(slot: &Mutex<Option<Child>>) -> bool {
    loop {
        let status = match slot.lock() {
            Ok(mut guard) => match guard.as_mut() {
                Some(child) => child.try_wait(),
                None => return false,
            },
            Err(_) => return false,
        };
        match status {
            Ok(Some(status)) => return status.success(),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return false,
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
/// the HTTP route map's own call site already uses.
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
                ProblemSeverity::Error => ui.visuals().error_fg_color,
                ProblemSeverity::Warning => ui.visuals().warn_fg_color,
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

/// Applies a completed "Run with Coverage" run's per-file results to every
/// currently open tab, keyed by path — same wholesale-replace-including-
/// to-empty shape `static_analysis::apply_results` already established for
/// Checkstyle/PMD/SpotBugs. Only open tabs get results (the same
/// "this is the squiggle-pipeline's own existing scope" rule those three
/// already follow) — a file opened *after* this run simply has no marks
/// until the next "Run with Coverage."
pub fn apply_coverage_results(state: &mut EditorState, results: &[(PathBuf, Vec<LineCoverage>)]) {
    for doc in &mut state.open_tabs {
        doc.coverage_lines = results
            .iter()
            .find(|(path, _)| *path == doc.path)
            .map(|(_, lines)| lines.clone())
            .unwrap_or_default();
    }
}

#[cfg(test)]
#[path = "build_panel_test.rs"]
mod build_panel_test;
