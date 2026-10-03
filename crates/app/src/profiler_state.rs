//! Orchestrating one async-profiler capture (`PLAN.md` Track 26 Phase 1 —
//! "Profiler integration"): spawn `asprof` against a live JVM's PID, wait for
//! its fixed-duration sample to finish, then read the collapsed-stack file it
//! wrote and fold it into a `fg_core::FlameNode` tree.
//!
//! Same background-thread-plus-`Receiver` shape every other long-running
//! action in this app uses (`build_panel`'s own process streaming,
//! `tool_manager`'s installs): `asprof -d N` blocks for the whole sample
//! duration, so running it on the UI thread would freeze the app for N
//! seconds — instead one thread runs it to completion and sends the parsed
//! result back, polled once a frame. The parsed `FlameNode` is what Track 26
//! Phase 2's own flame-graph widget paints; this phase only proves the
//! capture produces a non-empty tree.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use fg_core::{FlameNode, ProfileEvent, parse_collapsed, profiler_command};

/// A finished capture's parsed profile, or the reason it failed.
pub type CaptureResult = Result<FlameNode, String>;

/// The default sample duration, in seconds — long enough to collect a
/// meaningful CPU profile from a running program, short enough that the user
/// isn't left waiting. Interactive profilers commonly default to this
/// ballpark; it's a plain constant rather than a setting for this phase.
pub const DEFAULT_DURATION_SECS: u32 = 10;

/// Reads the collapsed-stack file `asprof` wrote at `output_path` and folds it
/// into a `FlameNode` tree, erroring if the file is missing/unreadable or the
/// profile came back empty (no samples — e.g. the target did nothing during
/// the window, or the attach silently produced nothing). Split out of
/// `start_capture`'s background thread so the read-and-parse step is testable
/// against a real on-disk collapsed file without spawning `asprof`.
fn read_capture(output_path: &Path) -> CaptureResult {
    let text = std::fs::read_to_string(output_path)
        .map_err(|e| format!("couldn't read the profiler's output at {}: {e}", output_path.display()))?;
    let tree = parse_collapsed(&text);
    if tree.total == 0 {
        return Err(
            "the profiler captured no samples — the process may have been idle, or the attach failed".to_string(),
        );
    }
    Ok(tree)
}

/// A unique output path for one capture, under the OS temp dir, keyed by the
/// target PID and a nanosecond timestamp so two captures (even of the same
/// PID) never collide on the same file.
fn capture_output_path(pid: u32) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("foxgarden-profile-{pid}-{nanos}.collapsed"))
}

/// Drives async-profiler: the installer plus at most one in-flight capture and
/// the most recent result. `manager` is public so the UI can offer Install
/// and poll it directly, the same way the External Tools dialog reaches
/// `ToolManagerState`.
#[derive(Default)]
pub struct ProfilerState {
    pub manager: crate::profiler_manager::ProfilerManagerState,
    capture: Option<Receiver<CaptureResult>>,
    /// The most recent successful capture's tree — kept for Track 26 Phase 2's
    /// flame-graph widget to render.
    pub last_profile: Option<FlameNode>,
}

impl ProfilerState {
    pub fn is_capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// Spawns `asprof` against `pid` on a background thread, sampling `event`
    /// for `duration_secs`, and arranges for its parsed result to arrive via
    /// `poll_capture`. A no-op if a capture's already in flight (the button
    /// that calls this is disabled meanwhile, but guard anyway).
    pub fn start_capture(&mut self, asprof: PathBuf, pid: u32, event: ProfileEvent, duration_secs: u32) {
        if self.is_capturing() {
            return;
        }
        let output_path = capture_output_path(pid);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut command = profiler_command(&asprof, pid, event, duration_secs, &output_path);
            let result = match command.status() {
                Ok(status) if status.success() => read_capture(&output_path),
                Ok(status) => Err(format!(
                    "asprof exited with {status} — is the target a JVM this profiler can attach to?"
                )),
                Err(e) => Err(format!("couldn't launch asprof: {e}")),
            };
            // Best-effort cleanup: the parsed tree is what matters now, not
            // the raw file. A leftover temp file is harmless if this fails.
            let _ = std::fs::remove_file(&output_path);
            let _ = tx.send(result);
        });
        self.capture = Some(rx);
    }

    /// Returns a finished capture's result once its background thread sends
    /// it, clearing the in-flight slot. On success the tree is also stashed in
    /// `last_profile` for the flame-graph widget; the returned value lets the
    /// caller surface a one-line summary/toast either way.
    pub fn poll_capture(&mut self) -> Option<CaptureResult> {
        let rx = self.capture.as_ref()?;
        match rx.try_recv() {
            Ok(result) => {
                self.capture = None;
                if let Ok(tree) = &result {
                    self.last_profile = Some(tree.clone());
                }
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.capture = None;
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "profiler_state_test.rs"]
mod profiler_state_test;
