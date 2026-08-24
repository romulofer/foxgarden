//! `PLAN.md` Track 23: orchestrates one Java debug session end to end —
//! request a port from jdt.ls (`LspState::request_start_debug_session`),
//! connect `dap_client::DapSession` to it, and drive the DAP `initialize` /
//! `launch` / `configurationDone` handshake through to a real attached
//! process (Phase 1). Phase 2 adds breakpoints (sent both up front, before
//! the debuggee is ever resumed, and live while attached) and
//! stepping/continue, tracking whether the debuggee is currently paused and
//! where.
//!
//! Every step here is non-blocking (`poll` advances whatever's in flight,
//! called once a frame the same way `LspState::sync` already is) for the
//! same reason every other background op in this app is: jdt.ls indexing a
//! real project, or java-debug itself resolving/launching a real JVM, both
//! take real wall-clock time no UI thread should ever wait on directly.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use fg_core::{BuildTool, Document, RunConfig};
use serde_json::{Value, json};

use crate::dap_client::{DapResult, DapSession};
use crate::lsp_client::ResponseError;
use crate::lsp_state::LspState;

/// What `poll`'s caller (a debug toolbar, eventually — `PLAN.md` Phase 2)
/// shows the user right now.
#[derive(Debug, Clone, PartialEq)]
pub enum DebugStatus {
    Idle,
    Starting(&'static str),
    Attached,
    Failed(String),
}

/// One step of the launch handshake in flight. Not `pub` — `DebugState`'s
/// own `status`/`poll` are the only surface a caller needs; the exact
/// sub-step is this module's own bookkeeping, the same "internal enum,
/// public status projection" split `lsp_state::Slot`/`starting_servers`
/// already uses.
enum Phase {
    Idle,
    /// Waiting on `vscode.java.startDebugSession`'s own response (the port
    /// to connect to). Carries the launch arguments assembled at `start`
    /// time so they're ready the instant the socket connects, plus the
    /// snapshot of every open document's breakpoints taken at that same
    /// moment (`PLAN.md` Track 23 Phase 2) — sent once the adapter's own
    /// `initialized` event arrives, before `configurationDone` (see
    /// `Launching`'s own doc comment for why that ordering matters).
    RequestingPort { rx: Receiver<Result<Value, ResponseError>>, launch_args: Value, initial_breakpoints: Vec<(PathBuf, HashSet<usize>)> },
    /// Connected; waiting on the DAP `initialize` response (adapter
    /// capabilities — unused by Phase 1, but required by the protocol
    /// before any other request is valid).
    AwaitingInitializeResponse { session: DapSession, rx: Receiver<DapResult>, launch_args: Value, initial_breakpoints: Vec<(PathBuf, HashSet<usize>)> },
    /// `launch` has been sent; waiting for *both* its own response and the
    /// adapter's `initialized` event (order between the two is not fixed by
    /// the DAP spec) before `configurationDone` is safe to send — the
    /// standard generic-DAP-client sequencing, not something java-debug
    /// documents as its own special case. `initial_breakpoints` are sent
    /// (fire-and-forget — `breakpoints_sent` just guards against resending
    /// every frame this phase is polled) the instant `initialized_event_
    /// seen` first flips `true`, deliberately *before* `configurationDone`
    /// — java-debug may resume the JVM as soon as that lands, so a
    /// breakpoint on an early line would otherwise race the debuggee
    /// actually reaching it.
    Launching {
        session: DapSession,
        launch_rx: Receiver<DapResult>,
        launch_done: bool,
        initialized_event_seen: bool,
        initial_breakpoints: Vec<(PathBuf, HashSet<usize>)>,
        breakpoints_sent: bool,
    },
    /// `configurationDone` sent; waiting on its response. The program is
    /// already running by this point in practice (java-debug starts the
    /// JVM once `launch` lands) — this step just confirms the adapter
    /// considers its own setup finished.
    AwaitingConfigurationDone { session: DapSession, rx: Receiver<DapResult> },
    /// Attached and running (or paused). Kept polling (`poll_events`) so a
    /// real `terminated`/`exited` event — the debuggee finishing on its own
    /// — is noticed rather than left to look like a still-running session
    /// forever. `paused` is `Some` once a real `"stopped"` event's own top
    /// stack frame has been resolved (`PLAN.md` Track 23 Phase 2);
    /// `stack_trace_rx` is the in-flight `stackTrace` request between
    /// seeing `"stopped"` and that frame actually resolving. `var_fetch`
    /// (Phase 3) is the separate, later `scopes`/`variables` chain kicked
    /// off once `paused` itself resolves — a distinct in-flight receiver
    /// because it depends on the top frame's own `id`, which isn't known
    /// until the `stackTrace` response above has already landed.
    Attached {
        session: DapSession,
        paused: Option<PausedFrame>,
        stack_trace_rx: Option<(i64, Receiver<DapResult>)>,
        var_fetch: VarFetch,
    },
    Failed(String),
}

/// The debuggee's current top stack frame while paused — `line` is
/// 0-indexed (converted from DAP's own 1-indexed `line`, matching
/// `Document::breakpoints`' own 0-indexed convention) so `paused_location`
/// can be compared directly against a `Document`'s own line numbers with
/// no further adjustment at any call site. `stack`/`variables` (Track 23
/// Phase 3) start empty and fill in once their own background requests
/// resolve — `paused_location`/`is_paused` (and the current-line highlight
/// they drive) don't wait on either, only on the top frame itself.
struct PausedFrame {
    thread_id: i64,
    frame_id: i64,
    file: PathBuf,
    line: usize,
    stack: Vec<StackFrameSummary>,
    variables: Vec<VariableGroup>,
    /// Distinguishes "the `scopes`/`variables` chain hasn't started yet"
    /// from "it finished and genuinely found zero groups" — both leave
    /// `variables` empty, but only the first should trigger a fetch.
    variables_fetched: bool,
}

/// One frame of a real `stackTrace` response, for the call-stack panel
/// (`PLAN.md` Track 23 Phase 3) — `file` is `None` for a frame with no real
/// source path (e.g. a synthetic/decompiled frame), unlike `PausedFrame`
/// itself which requires one on its own top frame.
pub struct StackFrameSummary {
    pub id: i64,
    pub name: String,
    pub file: Option<PathBuf>,
    pub line: usize,
}

/// One DAP `scopes` entry (e.g. "Locals", "Arguments") together with the
/// real `variables` response fetched for it (`PLAN.md` Track 23 Phase 3).
pub struct VariableGroup {
    pub name: String,
    pub variables: Vec<VariableEntry>,
}

/// One real `variables` response entry — `value`/`kind` are already
/// formatted strings straight from the adapter (java-debug's own `value`/
/// `type` fields), not further parsed here.
pub struct VariableEntry {
    pub name: String,
    pub value: String,
    pub kind: String,
}

/// The `scopes`/`variables` fetch chain that follows a paused frame
/// resolving (`PLAN.md` Track 23 Phase 3) — a second, later chain than
/// `stack_trace_rx` because `scopes` needs the top frame's own `id`, which
/// only exists once that first `stackTrace` response has already landed.
/// `Idle` covers both "not paused" and "already fetched for this pause";
/// reset to `Idle` on every fresh `"stopped"`/`"continued"` event so a new
/// pause always re-fetches rather than showing the previous one's stale
/// values.
enum VarFetch {
    Idle,
    AwaitingScopes { rx: Receiver<DapResult> },
    AwaitingVariables { name: String, remaining: Vec<(String, i64)>, rx: Receiver<DapResult>, collected: Vec<VariableGroup> },
}

/// The DAP `launch` request's own `arguments` object — every field here is
/// `com.microsoft.java.debug.plugin`'s real, documented (or, for the ones
/// undocumented in user-facing `Configuration.md`, decompiled straight out
/// of `vscjava.vscode-java-debug`'s own bundled client code rather than
/// guessed) launch-config shape: `mainClass`/`projectName`/`vmArgs` per that
/// extension's own `Configuration.md`; `classPaths`/`modulePaths`/`args`/
/// `env`/`cwd`/`console` confirmed against its `dist/extension.js`, which
/// resolves and forwards each verbatim. `classPaths` is handed real
/// already-resolved absolute paths (`fg_core::resolve_classpath`, the same
/// Track 21 resolution `Run`/`Track 22` already use) rather than the
/// `"$Auto"` sentinel the real extension supports for its own jdt.ls-side
/// resolution — this app resolves it client-side already, no reason to ask
/// jdt.ls to redo it.
fn build_launch_args(project_root: &Path, run_config: &RunConfig, classpath: &[PathBuf]) -> Value {
    let project_name =
        project_root.file_name().and_then(|name| name.to_str()).unwrap_or("FoxGarden project").to_string();
    let cwd = run_config.working_dir.clone().unwrap_or_else(|| project_root.to_path_buf());
    let env: HashMap<&str, &str> = run_config.env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    json!({
        "type": "java",
        "request": "launch",
        "name": run_config.name,
        "mainClass": run_config.main_class,
        "projectName": project_name,
        "cwd": cwd.display().to_string(),
        "classPaths": classpath.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "modulePaths": Vec::<String>::new(),
        "args": run_config.program_args,
        "vmArgs": run_config.vm_args,
        "env": env,
        "console": "internalConsole",
        "stopOnEntry": false,
    })
}

pub struct DebugState {
    phase: Phase,
    /// The last `breakpoints` set actually sent to the adapter for each
    /// file, keyed by path (`PLAN.md` Track 23 Phase 2) — lets
    /// `sync_breakpoints` re-send only files whose set genuinely changed
    /// since the last send, rather than the whole open-tabs list every
    /// frame. Reset to empty on every `start` (a fresh session has told the
    /// adapter nothing yet, even if the previous session had).
    last_sent_breakpoints: HashMap<PathBuf, HashSet<usize>>,
}

impl Default for DebugState {
    fn default() -> Self {
        Self { phase: Phase::Idle, last_sent_breakpoints: HashMap::new() }
    }
}

impl DebugState {
    /// The current phase, projected down to what a caller can actually act
    /// on (a status line; later, whether `stop` makes sense).
    pub fn status(&self) -> DebugStatus {
        match &self.phase {
            Phase::Idle => DebugStatus::Idle,
            Phase::RequestingPort { .. } => DebugStatus::Starting("requesting a debug port…"),
            Phase::AwaitingInitializeResponse { .. } => DebugStatus::Starting("connected, initializing…"),
            Phase::Launching { .. } => DebugStatus::Starting("launching…"),
            Phase::AwaitingConfigurationDone { .. } => DebugStatus::Starting("finishing setup…"),
            Phase::Attached { .. } => DebugStatus::Attached,
            Phase::Failed(message) => DebugStatus::Failed(message.clone()),
        }
    }

    /// Whether a new `start` call is currently valid — anything other than
    /// idle or a previous, already-reported failure means a session is
    /// genuinely in flight or running.
    pub fn can_start(&self) -> bool {
        matches!(self.phase, Phase::Idle | Phase::Failed(_))
    }

    /// Drains a session's own terminal failure exactly once — the same
    /// "returned once, not re-surfaced every frame" contract `LspState::
    /// sync`'s own `Vec<String>` return already holds itself to, needed here
    /// because `poll` runs every frame while `last_error`'s modal is a
    /// one-shot pop-up, not a value it can just read live off `status()`.
    /// Transitions `Failed` back to `Idle` (a state `can_start`/`is_running`
    /// already treat identically) so the Run menu's button isn't left
    /// permanently reading its failed label.
    pub fn take_failure(&mut self) -> Option<String> {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Failed(message) => Some(message),
            other => {
                self.phase = other;
                None
            }
        }
    }

    /// Whether the Run menu's `Debug Project` button should show as busy
    /// (and every other Run/Test/Build action disable itself) — the
    /// complement of `can_start` except a `Failed` session must still read
    /// as "not running" so the user can retry, matching `BuildState::
    /// is_run_running`'s own "only while genuinely in flight" contract.
    pub fn is_running(&self) -> bool {
        !matches!(self.phase, Phase::Idle | Phase::Failed(_))
    }

    /// Kicks off a new debug session for `run_config` against `tool`'s own
    /// resolved classpath. `lsp` must have a `Ready` Java session — the
    /// same precondition `LspState::request_start_debug_session` already
    /// enforces, surfaced here as a real error message instead of a silent
    /// no-op.
    pub fn start(
        &mut self,
        lsp: &mut LspState,
        project_root: &Path,
        tool: BuildTool,
        run_config: &RunConfig,
        initial_breakpoints: Vec<(PathBuf, HashSet<usize>)>,
    ) -> Result<(), String> {
        if !self.can_start() {
            return Err("a debug session is already starting or running".to_string());
        }
        let classpath = fg_core::resolve_classpath(project_root, tool).map_err(|error| error.to_string())?;
        let launch_args = build_launch_args(project_root, run_config, &classpath);
        let rx = lsp
            .request_start_debug_session()
            .ok_or_else(|| "the Java language server isn't ready yet".to_string())?;
        self.last_sent_breakpoints.clear();
        self.phase = Phase::RequestingPort { rx, launch_args, initial_breakpoints };
        Ok(())
    }

    /// Ends whatever's in flight or running. A `disconnect` request
    /// (`terminateDebuggee: true`, per the DAP spec — without it a real
    /// adapter is allowed to leave the debuggee running detached) is sent
    /// best-effort when a real session exists; dropping the `DapSession`
    /// right after closes the socket outright either way (`DapSession`'s
    /// own `Drop`), so this never leaves a stuck phase behind even if the
    /// adapter never answers.
    pub fn stop(&mut self) {
        if let Phase::Attached { mut session, .. }
        | Phase::AwaitingConfigurationDone { mut session, .. }
        | Phase::Launching { mut session, .. }
        | Phase::AwaitingInitializeResponse { mut session, .. } = std::mem::replace(&mut self.phase, Phase::Idle)
        {
            let _ = session.send_request("disconnect", json!({ "terminateDebuggee": true }));
        }
    }

    /// Advances whatever step is currently in flight — called once a frame,
    /// same "poll everything non-blocking" shape `LspState::sync` already
    /// uses for its own handshakes.
    pub fn poll(&mut self) {
        self.phase = match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::RequestingPort { rx, launch_args, initial_breakpoints } => {
                poll_requesting_port(rx, launch_args, initial_breakpoints)
            }
            Phase::AwaitingInitializeResponse { session, rx, launch_args, initial_breakpoints } => {
                poll_awaiting_initialize(session, rx, launch_args, initial_breakpoints)
            }
            Phase::Launching { session, launch_rx, launch_done, initialized_event_seen, initial_breakpoints, breakpoints_sent } => {
                poll_launching(
                    session,
                    launch_rx,
                    launch_done,
                    initialized_event_seen,
                    initial_breakpoints,
                    breakpoints_sent,
                    &mut self.last_sent_breakpoints,
                )
            }
            Phase::AwaitingConfigurationDone { session, rx } => poll_awaiting_configuration_done(session, rx),
            Phase::Attached { session, paused, stack_trace_rx, var_fetch } => {
                poll_attached(session, paused, stack_trace_rx, var_fetch)
            }
            other @ (Phase::Idle | Phase::Failed(_)) => other,
        };
    }

    /// Where the debuggee is currently paused, if it is — `None` while
    /// running, starting, idle, or failed. The caller (`widget.rs`'s
    /// current-line highlight) compares `.0` against its own document's
    /// path itself; this doesn't filter by document since it has no
    /// per-document context to filter with.
    pub fn paused_location(&self) -> Option<(&Path, usize)> {
        match &self.phase {
            Phase::Attached { paused: Some(frame), .. } => Some((frame.file.as_path(), frame.line)),
            _ => None,
        }
    }

    /// The full call stack of the frame the debuggee is currently paused
    /// at (`PLAN.md` Track 23 Phase 3) — empty while not paused, or while
    /// paused but the `stackTrace` response hasn't resolved yet (both are
    /// the same "nothing to show" case to a caller).
    pub fn call_stack(&self) -> &[StackFrameSummary] {
        match &self.phase {
            Phase::Attached { paused: Some(frame), .. } => &frame.stack,
            _ => &[],
        }
    }

    /// The current pause's local variables, grouped by DAP scope (e.g.
    /// "Locals", "Arguments") — `PLAN.md` Track 23 Phase 3. Empty until the
    /// `scopes`/`variables` chain kicked off after the pause itself
    /// resolves finishes fetching.
    pub fn variables(&self) -> &[VariableGroup] {
        match &self.phase {
            Phase::Attached { paused: Some(frame), .. } => &frame.variables,
            _ => &[],
        }
    }

    /// Whether the debug toolbar's Continue/Step buttons should be enabled
    /// — only meaningful while genuinely paused at a real stack frame.
    pub fn is_paused(&self) -> bool {
        matches!(self.phase, Phase::Attached { paused: Some(_), .. })
    }

    /// Resumes a paused debuggee (`"continue"`). A no-op outside a paused
    /// `Attached` phase, matching `stop`'s own "harmless when there's
    /// nothing to act on" contract — clears `paused`/`stack_trace_rx`
    /// immediately rather than waiting on the request's own response or a
    /// `"continued"` event, so the current-line highlight disappears the
    /// instant the user clicks, not after a network round-trip.
    pub fn continue_(&mut self) {
        self.send_paused_thread_command("continue");
    }

    /// `"next"` — steps over the current line.
    pub fn step_over(&mut self) {
        self.send_paused_thread_command("next");
    }

    /// `"stepIn"` — steps into a call on the current line.
    pub fn step_into(&mut self) {
        self.send_paused_thread_command("stepIn");
    }

    /// `"stepOut"` — runs until the current function returns.
    pub fn step_out(&mut self) {
        self.send_paused_thread_command("stepOut");
    }

    /// Shared body for `continue_`/`step_over`/`step_into`/`step_out`: all
    /// four are `{"threadId": <paused thread>}` DAP requests that only make
    /// sense while genuinely paused, and all four optimistically clear
    /// `paused` the same way.
    fn send_paused_thread_command(&mut self, command: &str) {
        let Phase::Attached { session, paused, stack_trace_rx, var_fetch } = &mut self.phase else { return };
        let Some(frame) = paused.take() else { return };
        *stack_trace_rx = None;
        *var_fetch = VarFetch::Idle;
        let _ = session.send_request(command, json!({ "threadId": frame.thread_id }));
    }

    /// Re-sends `setBreakpoints` for every open document whose breakpoint
    /// set has changed since it was last sent (`PLAN.md` Track 23 Phase
    /// 2) — a no-op outside `Attached` (nothing to send to), and a no-op
    /// per-file when nothing about that file's own set changed
    /// (`breakpoints_to_resend`'s own diffing). Called once a frame from
    /// `app.rs`, same "cheap enough to recompute every frame, no dirty
    /// flag needed" shape `folding`'s own fresh-every-frame recomputation
    /// already establishes.
    pub fn sync_breakpoints<'a>(&mut self, docs: impl Iterator<Item = &'a Document>) {
        let current: Vec<(&'a Path, &'a HashSet<usize>)> = docs.map(|d| (d.path.as_path(), &d.breakpoints)).collect();
        let to_send = breakpoints_to_resend(&self.last_sent_breakpoints, current.into_iter());
        if to_send.is_empty() {
            return;
        }
        let Phase::Attached { session, .. } = &mut self.phase else { return };
        for (path, lines) in to_send {
            let _ = session.send_request("setBreakpoints", set_breakpoints_args(&path, &lines));
            self.last_sent_breakpoints.insert(path, lines);
        }
    }
}

/// `setBreakpoints`' own DAP request arguments for one source file. Each
/// line is sent one-past its own value because `initialize`'s own
/// `linesStartAt1: true` (Phase 1) makes DAP line numbers 1-indexed, while
/// `lines` (from `Document::breakpoints`) stays 0-indexed like every other
/// line-number field in this codebase.
fn set_breakpoints_args(path: &Path, lines: &HashSet<usize>) -> Value {
    let mut sorted: Vec<usize> = lines.iter().copied().collect();
    sorted.sort_unstable();
    json!({
        "source": { "path": path.display().to_string() },
        "breakpoints": sorted.iter().map(|line| json!({ "line": line + 1 })).collect::<Vec<_>>(),
    })
}

/// Pure diffing logic behind `DebugState::sync_breakpoints`, split out so
/// it's directly unit-testable with no real `DapSession` — decides which
/// files actually need a fresh `setBreakpoints` call. A file is included
/// when its current set differs from what was last sent, but a file that
/// has never had breakpoints and still doesn't is deliberately excluded
/// even though `last_sent` has no entry for it either (`None != Some(&
/// empty_set)` would otherwise be true) — sending an empty `setBreakpoints`
/// for every open, breakpoint-free tab every frame would be pure noise the
/// adapter never needed telling.
fn breakpoints_to_resend<'a>(
    last_sent: &HashMap<PathBuf, HashSet<usize>>,
    current: impl Iterator<Item = (&'a Path, &'a HashSet<usize>)>,
) -> Vec<(PathBuf, HashSet<usize>)> {
    current
        .filter(|(path, lines)| {
            let previously_sent = last_sent.get(*path);
            (!lines.is_empty() || previously_sent.is_some()) && previously_sent != Some(*lines)
        })
        .map(|(path, lines)| (path.to_path_buf(), lines.clone()))
        .collect()
}

fn poll_requesting_port(
    rx: Receiver<Result<Value, ResponseError>>,
    launch_args: Value,
    initial_breakpoints: Vec<(PathBuf, HashSet<usize>)>,
) -> Phase {
    match rx.try_recv() {
        // Verified live against a real jdt.ls 1.60.0 + java-debug 0.53.2:
        // `vscode.java.startDebugSession`'s response body is a bare JSON
        // number (`33267`), not the quoted string the real `vscode-java-
        // debug` extension's own TypeScript types (`Promise<string>`)
        // implied before this was actually run — `as_u64` first, `as_str`
        // kept as a fallback in case a different server version really
        // does quote it, rather than assuming either shape from docs alone.
        Ok(Ok(body)) => match body.as_u64().or_else(|| body.as_str().and_then(|s| s.parse().ok())).and_then(|port| u16::try_from(port).ok()) {
            Some(port) => match DapSession::connect(port) {
                Ok(mut session) => match session.send_request(
                    "initialize",
                    json!({ "clientID": "foxgarden", "adapterID": "java", "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path" }),
                ) {
                    Ok(rx) => Phase::AwaitingInitializeResponse { session, rx, launch_args, initial_breakpoints },
                    Err(error) => Phase::Failed(format!("failed to send the DAP initialize request: {error}")),
                },
                Err(error) => Phase::Failed(format!("failed to connect to the debug adapter on port {port}: {error}")),
            },
            None => Phase::Failed(format!("jdt.ls returned an unusable debug port: {body}")),
        },
        Ok(Err(error)) => Phase::Failed(format!("vscode.java.startDebugSession failed: {}", error.message)),
        Err(TryRecvError::Empty) => Phase::RequestingPort { rx, launch_args, initial_breakpoints },
        Err(TryRecvError::Disconnected) => Phase::Failed("the Java language server session ended".to_string()),
    }
}

fn poll_awaiting_initialize(
    session: DapSession,
    rx: Receiver<DapResult>,
    launch_args: Value,
    initial_breakpoints: Vec<(PathBuf, HashSet<usize>)>,
) -> Phase {
    match rx.try_recv() {
        Ok(Ok(_capabilities)) => launch(session, launch_args, initial_breakpoints),
        Ok(Err(message)) => Phase::Failed(format!("DAP initialize failed: {message}")),
        Err(TryRecvError::Empty) => Phase::AwaitingInitializeResponse { session, rx, launch_args, initial_breakpoints },
        Err(TryRecvError::Disconnected) => Phase::Failed("the debug adapter closed the connection".to_string()),
    }
}

fn launch(mut session: DapSession, launch_args: Value, initial_breakpoints: Vec<(PathBuf, HashSet<usize>)>) -> Phase {
    match session.send_request("launch", launch_args) {
        Ok(launch_rx) => Phase::Launching {
            session,
            launch_rx,
            launch_done: false,
            initialized_event_seen: false,
            initial_breakpoints,
            breakpoints_sent: false,
        },
        Err(error) => Phase::Failed(format!("failed to send the DAP launch request: {error}")),
    }
}

#[allow(clippy::too_many_arguments)]
fn poll_launching(
    mut session: DapSession,
    launch_rx: Receiver<DapResult>,
    mut launch_done: bool,
    mut initialized_event_seen: bool,
    initial_breakpoints: Vec<(PathBuf, HashSet<usize>)>,
    mut breakpoints_sent: bool,
    last_sent_breakpoints: &mut HashMap<PathBuf, HashSet<usize>>,
) -> Phase {
    for (event, _body) in session.poll_events() {
        if event == "initialized" {
            initialized_event_seen = true;
        }
    }
    // Sent the instant `initialized` first arrives — deliberately before
    // `configurationDone` (see `Phase::Launching`'s own doc comment for
    // why the ordering matters), and only once (`breakpoints_sent` guards
    // against resending on every later poll of this same phase).
    if initialized_event_seen && !breakpoints_sent {
        for (path, lines) in &initial_breakpoints {
            let _ = session.send_request("setBreakpoints", set_breakpoints_args(path, lines));
            last_sent_breakpoints.insert(path.clone(), lines.clone());
        }
        breakpoints_sent = true;
    }
    match launch_rx.try_recv() {
        Ok(Ok(_)) => launch_done = true,
        Ok(Err(message)) => return Phase::Failed(format!("launch failed: {message}")),
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => return Phase::Failed("the debug adapter closed the connection".to_string()),
    }
    if launch_done && initialized_event_seen {
        return configuration_done(session);
    }
    Phase::Launching { session, launch_rx, launch_done, initialized_event_seen, initial_breakpoints, breakpoints_sent }
}

fn configuration_done(mut session: DapSession) -> Phase {
    // `Value::Null`, not an empty object, is what a DAP request with no
    // real arguments would naively send — real java-debug 0.53.2 rejects
    // that outright (verified live: `Expected a JsonObject but was
    // JsonNull; at path $.arguments`, its own server-side Gson deserializer
    // choking on a `null` where its Java model expects an always-present,
    // possibly-empty object). `json!({})` is what actually works.
    match session.send_request("configurationDone", json!({})) {
        Ok(rx) => Phase::AwaitingConfigurationDone { session, rx },
        Err(error) => Phase::Failed(format!("failed to send configurationDone: {error}")),
    }
}

fn poll_awaiting_configuration_done(session: DapSession, rx: Receiver<DapResult>) -> Phase {
    match rx.try_recv() {
        Ok(Ok(_)) => Phase::Attached { session, paused: None, stack_trace_rx: None, var_fetch: VarFetch::Idle },
        Ok(Err(message)) => Phase::Failed(format!("configurationDone failed: {message}")),
        Err(TryRecvError::Empty) => Phase::AwaitingConfigurationDone { session, rx },
        Err(TryRecvError::Disconnected) => Phase::Failed("the debug adapter closed the connection".to_string()),
    }
}

fn poll_attached(
    mut session: DapSession,
    mut paused: Option<PausedFrame>,
    mut stack_trace_rx: Option<(i64, Receiver<DapResult>)>,
    mut var_fetch: VarFetch,
) -> Phase {
    for (event, body) in session.poll_events() {
        match event.as_str() {
            "terminated" | "exited" => return Phase::Idle,
            "stopped" => {
                if let Some(thread_id) = parse_stopped_thread_id(&body) {
                    // No `levels` field: DAP treats an absent/zero `levels`
                    // as "every remaining frame from `startFrame`", which is
                    // exactly what the Phase 3 call-stack panel needs —
                    // Phase 2 only ever asked for the top frame.
                    let args = json!({ "threadId": thread_id, "startFrame": 0 });
                    if let Ok(rx) = session.send_request("stackTrace", args) {
                        stack_trace_rx = Some((thread_id, rx));
                    }
                    var_fetch = VarFetch::Idle;
                }
            }
            // Covers both an explicit Continue and any step command
            // already having resumed execution — the debuggee is running
            // again either way, so any in-flight stack-trace/variables
            // lookup for the frame it was just paused at is now stale.
            "continued" => {
                paused = None;
                stack_trace_rx = None;
                var_fetch = VarFetch::Idle;
            }
            _ => {}
        }
    }
    if let Some((thread_id, rx)) = &stack_trace_rx {
        match rx.try_recv() {
            Ok(Ok(body)) => {
                if let Some((frame_id, file, line)) = parse_top_stack_frame(&body) {
                    let stack = parse_call_stack(&body);
                    paused = Some(PausedFrame {
                        thread_id: *thread_id,
                        frame_id,
                        file,
                        line,
                        stack,
                        variables: Vec::new(),
                        variables_fetched: false,
                    });
                }
                stack_trace_rx = None;
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => stack_trace_rx = None,
            Err(TryRecvError::Empty) => {}
        }
    }
    if let Some(frame) = paused.as_mut() {
        // The `scopes`/`variables` chain (`PLAN.md` Track 23 Phase 3) only
        // starts once the top frame itself has resolved (it needs that
        // frame's own `id`), and only once per pause — `variables_fetched`
        // (not `var_fetch == Idle`, which is also true right after the
        // chain finishes) is what tells "not started" apart from "already
        // finished, no more requests needed".
        if matches!(var_fetch, VarFetch::Idle) && !frame.variables_fetched {
            match session.send_request("scopes", json!({ "frameId": frame.frame_id })) {
                Ok(rx) => var_fetch = VarFetch::AwaitingScopes { rx },
                Err(_) => frame.variables_fetched = true,
            }
        }
        var_fetch = poll_var_fetch(&mut session, var_fetch, frame);
    }
    Phase::Attached { session, paused, stack_trace_rx, var_fetch }
}

/// Advances the `scopes`/`variables` chain by exactly one step, writing the
/// finished result straight into `frame.variables`/`frame.
/// variables_fetched` the moment the last scope's own `variables` response
/// lands (or the chain gives up early) — pure state-machine plumbing,
/// mirrors every other `poll_*` helper's "take the old state, return the
/// new one" shape.
fn poll_var_fetch(session: &mut DapSession, var_fetch: VarFetch, frame: &mut PausedFrame) -> VarFetch {
    match var_fetch {
        VarFetch::AwaitingScopes { rx } => match rx.try_recv() {
            Ok(Ok(body)) => advance_variables_fetch(session, frame, parse_scopes(&body), Vec::new()),
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                frame.variables_fetched = true;
                VarFetch::Idle
            }
            Err(TryRecvError::Empty) => VarFetch::AwaitingScopes { rx },
        },
        VarFetch::AwaitingVariables { name, remaining, rx, mut collected } => match rx.try_recv() {
            Ok(Ok(body)) => {
                collected.push(VariableGroup { name, variables: parse_variables(&body) });
                advance_variables_fetch(session, frame, remaining, collected)
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                collected.push(VariableGroup { name, variables: Vec::new() });
                frame.variables = collected;
                frame.variables_fetched = true;
                VarFetch::Idle
            }
            Err(TryRecvError::Empty) => VarFetch::AwaitingVariables { name, remaining, rx, collected },
        },
        VarFetch::Idle => VarFetch::Idle,
    }
}

/// Sends the next pending scope's own `variables` request, or — once
/// `remaining` is empty — writes the finished `collected` groups straight
/// into `frame` and reports the chain as idle again.
fn advance_variables_fetch(
    session: &mut DapSession,
    frame: &mut PausedFrame,
    mut remaining: Vec<(String, i64)>,
    collected: Vec<VariableGroup>,
) -> VarFetch {
    if remaining.is_empty() {
        frame.variables = collected;
        frame.variables_fetched = true;
        return VarFetch::Idle;
    }
    let (name, variables_reference) = remaining.remove(0);
    match session.send_request("variables", json!({ "variablesReference": variables_reference })) {
        Ok(rx) => VarFetch::AwaitingVariables { name, remaining, rx, collected },
        Err(_) => {
            frame.variables = collected;
            frame.variables_fetched = true;
            VarFetch::Idle
        }
    }
}

/// A `"stopped"` event's own `threadId` — pure/no-I/O, mirrors `dap_client::
/// classify`'s own testable-without-a-socket shape.
fn parse_stopped_thread_id(body: &Value) -> Option<i64> {
    body.get("threadId").and_then(Value::as_i64)
}

/// A `stackTrace` response's top frame — its own `id` (needed by `scopes`),
/// file, and line, converted to this codebase's own 0-indexed line
/// convention (`- 1`, undoing `linesStartAt1: true`'s DAP-side 1-indexing).
/// `None` for a frame with no real file `source.path` (e.g. a synthetic/
/// decompiled source `stackTrace` can legitimately return) — Phase 2's own
/// stated scope is a file-based current-line highlight, not every possible
/// DAP source kind.
fn parse_top_stack_frame(body: &Value) -> Option<(i64, PathBuf, usize)> {
    let frame = body.get("stackFrames")?.as_array()?.first()?;
    let id = frame.get("id")?.as_i64()?;
    let path = frame.get("source")?.get("path")?.as_str()?;
    let line = frame.get("line")?.as_u64()?;
    Some((id, PathBuf::from(path), line.saturating_sub(1) as usize))
}

/// A `stackTrace` response's full frame list, for the call-stack panel
/// (`PLAN.md` Track 23 Phase 3) — unlike `parse_top_stack_frame`, a frame
/// with no real file source is kept (`file: None`) rather than dropped,
/// since a native/synthetic frame still belongs in the stack the user sees,
/// it just isn't a click-to-jump target.
fn parse_call_stack(body: &Value) -> Vec<StackFrameSummary> {
    let Some(frames) = body.get("stackFrames").and_then(Value::as_array) else { return Vec::new() };
    frames
        .iter()
        .filter_map(|frame| {
            let id = frame.get("id")?.as_i64()?;
            let name = frame.get("name")?.as_str()?.to_string();
            let line = frame.get("line")?.as_u64()?;
            let file = frame.get("source").and_then(|s| s.get("path")).and_then(Value::as_str).map(PathBuf::from);
            Some(StackFrameSummary { id, name, file, line: line.saturating_sub(1) as usize })
        })
        .collect()
}

/// A `scopes` response, reduced to `(name, variablesReference)` pairs —
/// deliberately excludes a scope whose `variablesReference` is `0` (the DAP
/// spec's own "this scope has no variables" marker) and any scope reported
/// `expensive: true` (real java-debug reports a synthetic "Static" or
/// "Static Variables" scope this way, matching the real `vscode-java-debug`
/// extension's own "Locals panel skips expensive scopes" behavior) — a
/// static-fields dump is rarely what "local variables" means to a user
/// stepping through code, and can legitimately be large.
fn parse_scopes(body: &Value) -> Vec<(String, i64)> {
    let Some(scopes) = body.get("scopes").and_then(Value::as_array) else { return Vec::new() };
    scopes
        .iter()
        .filter(|scope| !scope.get("expensive").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|scope| {
            let name = scope.get("name")?.as_str()?.to_string();
            let variables_reference = scope.get("variablesReference")?.as_i64()?;
            if variables_reference == 0 { None } else { Some((name, variables_reference)) }
        })
        .collect()
}

/// A `variables` response's own entries — `value`/`type` are already
/// adapter-formatted display strings, used as-is (java-debug's own `value`
/// for an object is already e.g. `"Foo@1 (id=2)"`, not a reference this app
/// would need to resolve further for Phase 3's stated scope).
fn parse_variables(body: &Value) -> Vec<VariableEntry> {
    let Some(variables) = body.get("variables").and_then(Value::as_array) else { return Vec::new() };
    variables
        .iter()
        .filter_map(|variable| {
            let name = variable.get("name")?.as_str()?.to_string();
            let value = variable.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
            let kind = variable.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
            Some(VariableEntry { name, value, kind })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(main_class: &str) -> RunConfig {
        RunConfig {
            name: "test".to_string(),
            main_class: main_class.to_string(),
            vm_args: "-Xmx128m".to_string(),
            program_args: "foo".to_string(),
            env: vec![("MY_ENV".to_string(), "hello".to_string())],
            working_dir: None,
        }
    }

    #[test]
    fn build_launch_args_carries_every_field_the_real_extension_forwards() {
        let root = Path::new("/projects/demo-app");
        let classpath = vec![PathBuf::from("/projects/demo-app/target/classes"), PathBuf::from("/home/.m2/x.jar")];
        let args = build_launch_args(root, &config("com.example.Main"), &classpath);

        assert_eq!(args["mainClass"], "com.example.Main");
        assert_eq!(args["projectName"], "demo-app");
        assert_eq!(args["cwd"], "/projects/demo-app");
        assert_eq!(args["classPaths"], serde_json::json!(["/projects/demo-app/target/classes", "/home/.m2/x.jar"]));
        assert_eq!(args["modulePaths"], serde_json::json!([]));
        assert_eq!(args["args"], "foo");
        assert_eq!(args["vmArgs"], "-Xmx128m");
        assert_eq!(args["env"]["MY_ENV"], "hello");
        assert_eq!(args["console"], "internalConsole");
    }

    #[test]
    fn build_launch_args_prefers_working_dir_over_project_root() {
        let root = Path::new("/projects/demo-app");
        let mut cfg = config("com.example.Main");
        cfg.working_dir = Some(PathBuf::from("/projects/demo-app/sub"));
        let args = build_launch_args(root, &cfg, &[]);
        assert_eq!(args["cwd"], "/projects/demo-app/sub");
    }

    #[test]
    fn debug_state_starts_idle_and_reports_attached_status_correctly() {
        let state = DebugState::default();
        assert_eq!(state.status(), DebugStatus::Idle);
        assert!(state.can_start());
    }

    #[test]
    fn stop_from_idle_is_a_harmless_no_op() {
        let mut state = DebugState::default();
        state.stop();
        assert_eq!(state.status(), DebugStatus::Idle);
    }

    #[test]
    fn step_and_continue_from_idle_are_harmless_no_ops() {
        let mut state = DebugState::default();
        state.continue_();
        state.step_over();
        state.step_into();
        state.step_out();
        assert_eq!(state.status(), DebugStatus::Idle);
        assert!(!state.is_paused());
        assert_eq!(state.paused_location(), None);
    }

    #[test]
    fn parse_stopped_thread_id_reads_a_real_stopped_event_body() {
        let body = serde_json::json!({ "reason": "breakpoint", "threadId": 7, "allThreadsStopped": true });
        assert_eq!(parse_stopped_thread_id(&body), Some(7));
    }

    #[test]
    fn parse_stopped_thread_id_is_none_without_one() {
        assert_eq!(parse_stopped_thread_id(&serde_json::json!({ "reason": "breakpoint" })), None);
    }

    #[test]
    fn parse_top_stack_frame_reads_the_first_frames_file_and_converts_to_0_indexed() {
        let body = serde_json::json!({
            "stackFrames": [
                { "id": 1, "name": "main", "line": 12, "column": 1, "source": { "name": "Main.java", "path": "/projects/demo-app/src/main/java/com/example/Main.java" } },
                { "id": 2, "name": "caller", "line": 40, "column": 1, "source": { "name": "Other.java", "path": "/projects/demo-app/src/main/java/com/example/Other.java" } },
            ],
            "totalFrames": 2,
        });
        let (id, file, line) = parse_top_stack_frame(&body).expect("a real top frame with a file source parses");
        assert_eq!(id, 1);
        assert_eq!(file, PathBuf::from("/projects/demo-app/src/main/java/com/example/Main.java"));
        assert_eq!(line, 11);
    }

    #[test]
    fn parse_call_stack_keeps_every_frame_including_ones_without_a_file() {
        let body = serde_json::json!({
            "stackFrames": [
                { "id": 1, "name": "main", "line": 12, "column": 1, "source": { "name": "Main.java", "path": "/projects/demo-app/src/main/java/com/example/Main.java" } },
                { "id": 2, "name": "decompiled", "line": 3, "column": 1, "source": { "name": "Foo.class", "sourceReference": 42 } },
            ],
        });
        let stack = parse_call_stack(&body);
        assert_eq!(stack.len(), 2);
        assert_eq!(stack[0].id, 1);
        assert_eq!(stack[0].name, "main");
        assert_eq!(stack[0].file, Some(PathBuf::from("/projects/demo-app/src/main/java/com/example/Main.java")));
        assert_eq!(stack[0].line, 11);
        assert_eq!(stack[1].id, 2);
        assert_eq!(stack[1].file, None);
        assert_eq!(stack[1].line, 2);
    }

    #[test]
    fn parse_call_stack_is_empty_for_no_frames() {
        assert!(parse_call_stack(&serde_json::json!({ "stackFrames": [] })).is_empty());
    }

    #[test]
    fn parse_scopes_drops_expensive_and_empty_scopes() {
        let body = serde_json::json!({
            "scopes": [
                { "name": "Locals", "variablesReference": 100, "expensive": false },
                { "name": "Arguments", "variablesReference": 101, "expensive": false },
                { "name": "Static", "variablesReference": 102, "expensive": true },
                { "name": "Empty", "variablesReference": 0, "expensive": false },
            ],
        });
        assert_eq!(parse_scopes(&body), vec![("Locals".to_string(), 100), ("Arguments".to_string(), 101)]);
    }

    #[test]
    fn parse_variables_reads_name_value_and_type() {
        let body = serde_json::json!({
            "variables": [
                { "name": "count", "value": "3", "type": "int", "variablesReference": 0 },
                { "name": "self", "value": "Foo@1 (id=2)", "type": "Foo", "variablesReference": 5 },
            ],
        });
        let variables = parse_variables(&body);
        assert_eq!(variables.len(), 2);
        assert_eq!(variables[0].name, "count");
        assert_eq!(variables[0].value, "3");
        assert_eq!(variables[0].kind, "int");
        assert_eq!(variables[1].name, "self");
        assert_eq!(variables[1].value, "Foo@1 (id=2)");
    }

    #[test]
    fn parse_top_stack_frame_is_none_for_an_empty_stack() {
        assert_eq!(parse_top_stack_frame(&serde_json::json!({ "stackFrames": [], "totalFrames": 0 })), None);
    }

    #[test]
    fn parse_top_stack_frame_is_none_without_a_real_file_source() {
        let body = serde_json::json!({
            "stackFrames": [{ "id": 1, "name": "decompiled", "line": 3, "column": 1, "source": { "name": "Foo.class", "sourceReference": 42 } }],
        });
        assert_eq!(parse_top_stack_frame(&body), None);
    }

    #[test]
    fn breakpoints_to_resend_includes_a_file_with_a_new_or_changed_set() {
        let mut last_sent = HashMap::new();
        last_sent.insert(PathBuf::from("/a/Main.java"), HashSet::from([3]));
        let unchanged = HashSet::from([3]);
        let changed = HashSet::from([5, 6]);
        let brand_new = HashSet::from([1]);
        let current = vec![
            (Path::new("/a/Main.java"), &unchanged),
            (Path::new("/a/Other.java"), &changed),
            (Path::new("/a/New.java"), &brand_new),
        ];
        let mut to_send = breakpoints_to_resend(&last_sent, current.into_iter());
        to_send.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            to_send,
            vec![
                (PathBuf::from("/a/New.java"), HashSet::from([1])),
                (PathBuf::from("/a/Other.java"), HashSet::from([5, 6])),
            ]
        );
    }

    #[test]
    fn breakpoints_to_resend_excludes_a_file_that_never_had_breakpoints_and_still_doesnt() {
        let last_sent = HashMap::new();
        let empty = HashSet::new();
        let current = vec![(Path::new("/a/Untouched.java"), &empty)];
        assert!(breakpoints_to_resend(&last_sent, current.into_iter()).is_empty());
    }

    #[test]
    fn breakpoints_to_resend_includes_a_file_whose_breakpoints_were_all_cleared() {
        let mut last_sent = HashMap::new();
        last_sent.insert(PathBuf::from("/a/Main.java"), HashSet::from([3]));
        let empty = HashSet::new();
        let current = vec![(Path::new("/a/Main.java"), &empty)];
        assert_eq!(
            breakpoints_to_resend(&last_sent, current.into_iter()),
            vec![(PathBuf::from("/a/Main.java"), HashSet::new())]
        );
    }
}
