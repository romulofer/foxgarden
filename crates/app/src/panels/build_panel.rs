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

use fg_core::{BuildProblem, BuildTool, EditorState, LineCoverage, RunConfig, Severity};
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
/// `RunLaunched` for that second process. `Test` also stops after one
/// command (`mvn test`/`gradle test` already compile everything they need
/// on their own, unlike Run's separate `java` launch) but its own
/// `Finished` handling additionally scans and summarizes the tool's own
/// JUnit-XML report once the process exits, appending that summary (and a
/// clickable row per failing test) to the same log.
enum Stage {
    Build,
    RunCompiling {
        project_root: PathBuf,
        tool: BuildTool,
        config: RunConfig,
    },
    RunLaunched,
    Test {
        project_root: PathBuf,
        tool: BuildTool,
    },
    /// Run with Coverage (`PLAN.md` Track 13 Phase 1, Maven-only): one
    /// `mvn` process running `prepare-agent`+`test`+`report` back to back
    /// (`fg_core::coverage_command` — see its own doc comment for why this
    /// is one process, not a chained pair the way `RunCompiling` chains
    /// into a second launch). `poll`'s own `Finished` handling reads and
    /// parses `fg_core::coverage_report_path`'s own file once this exits
    /// successfully.
    Coverage { project_root: PathBuf },
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

    pub fn is_coverage_running(&self) -> bool {
        self.running() && matches!(self.stage, Some(Stage::Coverage { .. }))
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

    /// Starts a Test: `mvn test`/`gradle test` (already compile everything
    /// they need themselves, unlike Run — no chained second process here).
    /// `poll`'s own `Finished` handling scans and summarizes the tool's own
    /// JUnit-XML report once this exits, regardless of exit status (a test
    /// failure makes the process itself exit non-zero, but the report is
    /// still written and is what actually answers "which tests failed",
    /// not the exit code).
    pub fn start_test(&mut self, project_root: &Path, tool: BuildTool) -> std::io::Result<()> {
        self.reset();
        self.spawn_process(fg_core::test_command(project_root, tool))?;
        self.stage = Some(Stage::Test {
            project_root: project_root.to_path_buf(),
            tool,
        });
        Ok(())
    }

    /// Starts "Run with Coverage" (`PLAN.md` Track 13 Phase 1, Maven-only):
    /// one `mvn` process (`fg_core::coverage_command`) covering agent-
    /// attach, test run, and XML conversion together — no chained second
    /// process, unlike Run's compile-then-launch.
    pub fn start_coverage(&mut self, project_root: &Path) -> std::io::Result<()> {
        self.reset();
        self.spawn_process(fg_core::coverage_command(project_root))?;
        self.stage = Some(Stage::Coverage {
            project_root: project_root.to_path_buf(),
        });
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
    }

    fn reset(&mut self) {
        self.stop();
        self.rows.clear();
        self.rx = None;
        self.stage = None;
        self.last_success = None;
        self.coverage_result = None;
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

    /// Scans `tool`'s own JUnit-XML report (`fg_core::scan_test_reports`)
    /// and appends a pass/fail summary line plus one clickable row per
    /// failing/errored test to the log — reusing `BuildRow`'s own
    /// `BuildProblem` shape (`column: 1`, since a test failure's own
    /// "where" is a line, the same as Gradle's own column-less compiler
    /// errors already use) rather than growing a second, parallel
    /// click-to-jump row type just for this.
    fn append_test_summary(&mut self, project_root: &Path, tool: BuildTool) {
        let cases = fg_core::scan_test_reports(project_root, tool);
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
            let line = case
                .detail
                .as_deref()
                .and_then(|detail| fg_core::failure_line(detail, &case.classname))
                .unwrap_or(1);
            let problem = fg_core::test_source_file(project_root, &case.classname).map(|path| BuildProblem {
                path,
                line,
                column: 1,
                severity: Severity::Error,
                message: label.clone(),
            });
            self.rows.push(BuildRow {
                text: format!("FAILED  {label}"),
                problem,
            });
        }
    }

    /// Reads and parses `fg_core::coverage_report_path`'s own file,
    /// resolving each entry to a real path via `fg_core::
    /// resolve_coverage_paths`, and appends a one-line summary to the log
    /// either way — same "summarize the batch result into the log"
    /// pattern `append_test_summary` already established for `Stage::
    /// Test`.
    fn finish_coverage(&mut self, project_root: &Path) {
        let report_path = fg_core::coverage_report_path(project_root);
        let result = std::fs::read_to_string(&report_path)
            .map_err(|e| e.to_string())
            .and_then(|xml| fg_core::parse_jacoco_xml(&xml))
            .map(|parsed| fg_core::resolve_coverage_paths(project_root, parsed));
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
                        Some(Stage::Test { project_root, tool }) => {
                            self.last_success = Some(success);
                            self.append_test_summary(&project_root, tool);
                        }
                        Some(Stage::Coverage { project_root }) => {
                            self.last_success = Some(success);
                            if success {
                                self.finish_coverage(&project_root);
                            } else {
                                self.rows.push(BuildRow {
                                    text: "Coverage run failed to complete — see log above.".into(),
                                    problem: None,
                                });
                            }
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
mod tests {
    use fg_core::CoverageStatus;

    use super::*;

    fn line(n: usize, status: CoverageStatus) -> LineCoverage {
        LineCoverage { line: n, status }
    }

    #[test]
    fn apply_coverage_results_sets_matching_docs_and_clears_the_rest() {
        let (_dir_a, doc_a) = test_support::temp_document("A.java", "class A {}");
        let (_dir_b, doc_b) = test_support::temp_document("B.java", "class B {}");
        let path_a = doc_a.path.clone();
        let mut state = EditorState { open_tabs: vec![doc_a, doc_b], ..Default::default() };

        let results = vec![(path_a, vec![line(0, CoverageStatus::Covered)])];
        apply_coverage_results(&mut state, &results);

        assert_eq!(state.open_tabs[0].coverage_lines.len(), 1);
        assert_eq!(state.open_tabs[0].coverage_lines[0].status, CoverageStatus::Covered);
        assert!(state.open_tabs[1].coverage_lines.is_empty());
    }

    #[test]
    fn apply_coverage_results_replaces_rather_than_accumulates() {
        let (_dir, mut doc) = test_support::temp_document("A.java", "class A {}");
        doc.coverage_lines = vec![line(9, CoverageStatus::Missed)];
        let path = doc.path.clone();
        let mut state = EditorState { open_tabs: vec![doc], ..Default::default() };

        apply_coverage_results(&mut state, &[(path, vec![line(0, CoverageStatus::Covered)])]);

        assert_eq!(state.open_tabs[0].coverage_lines.len(), 1);
        assert_eq!(state.open_tabs[0].coverage_lines[0].line, 0);
    }

    #[test]
    fn a_run_with_no_results_for_a_doc_clears_its_stale_marks() {
        let (_dir, mut doc) = test_support::temp_document("A.java", "class A {}");
        doc.coverage_lines = vec![line(0, CoverageStatus::Covered)];
        let mut state = EditorState { open_tabs: vec![doc], ..Default::default() };

        apply_coverage_results(&mut state, &[]);

        assert!(state.open_tabs[0].coverage_lines.is_empty());
    }
}
