//! Sampling profilers an extension contributes (`PLAN.md` Track 26, moved out
//! of the app by Track 24).
//!
//! A profiler attaches to a running process by pid, samples it for a fixed
//! time and writes collapsed stacks (`frame1;frame2;… <count>`, one stack per
//! line, the folded format FlameGraph established) to a file. The editor reads
//! that file into its flame graph; which tool produces it, how it is
//! downloaded and how it is invoked is the extension's.

use std::path::PathBuf;
use std::sync::Arc;

use crate::{ArchiveKind, CommandSpec, Extension};

/// What to sample. Each tool has its own name for these; the extension maps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileEvent {
    /// CPU time.
    Cpu,
    /// Heap allocation.
    Alloc,
    /// Lock contention.
    Lock,
    /// Wall-clock time: every thread, whether running or blocked.
    Wall,
}

/// One capture being asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRequest {
    pub pid: u32,
    pub event: ProfileEvent,
    pub duration_secs: u32,
    /// Where the profiler must write its collapsed stacks.
    pub output_path: PathBuf,
}

/// A pinned, verifiable download of a profiler, for the platform this build
/// runs on. Like [`crate::AnalyzerInstall`], `sha256` is required: what is
/// downloaded becomes native code the editor starts and injects into a
/// running program, so a download that does not hash to it is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilerInstall {
    pub version: String,
    pub download_url: String,
    /// Lowercase hex SHA-256 of exactly what `download_url` serves.
    pub sha256: String,
    pub archive: ArchiveKind,
    /// The archive's single top-level directory.
    pub dir_name: String,
    /// The launcher's path inside that directory (`bin/asprof`).
    pub launcher: String,
}

/// A profiler and how to get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilerContribution {
    pub id: String,
    pub display_name: String,
    /// `None` when the profiler has no build for this platform: the editor
    /// then says so instead of offering a download.
    pub install: Option<ProfilerInstall>,
}

/// A registered profiler together with the extension that runs it, for the
/// same reason as [`crate::AnalyzerHandle`]: a capture outlasts any borrow of
/// the registry.
#[derive(Clone)]
pub struct ProfilerHandle {
    pub contribution: ProfilerContribution,
    extension: Arc<dyn Extension>,
}

impl std::fmt::Debug for ProfilerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfilerHandle").field("id", &self.contribution.id).finish_non_exhaustive()
    }
}

impl ProfilerHandle {
    pub(crate) fn new(contribution: ProfilerContribution, extension: Arc<dyn Extension>) -> Self {
        Self { contribution, extension }
    }

    pub fn id(&self) -> &str {
        &self.contribution.id
    }

    pub fn display_name(&self) -> &str {
        &self.contribution.display_name
    }

    /// The process that runs one capture with the installed `launcher`, or
    /// `None` when the extension cannot profile this way.
    pub fn command(&self, launcher: &std::path::Path, request: &ProfileRequest) -> Option<CommandSpec> {
        self.extension.profiler_command(&self.contribution.id, launcher, request)
    }
}
