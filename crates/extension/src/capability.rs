//! What an extension must be allowed to do before the host will register what
//! it contributes (`PLAN.md` Track 24 Phase B).
//!
//! Declared up front, not discovered at the call: an API that assumes every
//! extension may start processes or fetch files has already lost the ability
//! to say no. The registry derives what a set of contributions needs
//! ([`required`]) and refuses an extension that did not declare it, so the
//! declaration is checkable by the user before anything runs.

use crate::Contributions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capability {
    /// Start external programs: language servers, build tools, analyzers,
    /// debug adapters.
    ProcessExec,
    /// Download files from the network, as an analyzer install does.
    DownloadFile,
}

impl Capability {
    /// The name a manifest spells it with.
    pub fn id(self) -> &'static str {
        match self {
            Self::ProcessExec => "process_exec",
            Self::DownloadFile => "download_file",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [Self::ProcessExec, Self::DownloadFile].into_iter().find(|c| c.id() == id)
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// Each capability `contributions` needs, with the kind of contribution that
/// needs it (for the refusal message). Pure data contributions (languages,
/// grammars, icons) need none.
pub fn required(contributions: &Contributions) -> Vec<(Capability, &'static str)> {
    let mut needed = Vec::new();
    if !contributions.language_servers.is_empty() {
        needed.push((Capability::ProcessExec, "a language server"));
    }
    if !contributions.build_tools.is_empty() {
        needed.push((Capability::ProcessExec, "a build tool"));
    }
    if !contributions.analyzers.is_empty() {
        needed.push((Capability::ProcessExec, "an analyzer"));
    }
    if !contributions.debug_adapters.is_empty() {
        needed.push((Capability::ProcessExec, "a debug adapter"));
    }
    if contributions.analyzers.iter().any(|a| a.install.is_some()) {
        needed.push((Capability::DownloadFile, "an analyzer install"));
    }
    needed
}
