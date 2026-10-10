use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Extension;

/// A kind of language runtime the user keeps an inventory of — JDKs for the
/// JVM, say. The app lists the inventory and its dialog; what a runtime *is*
/// (how to recognize an install, where to look for them) is the extension's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeContribution {
    /// Stable key — the inventory is persisted under it, so it must never
    /// change once shipped. Lowercase, like `"jdk"`.
    pub id: String,
    /// The plural the UI names it with: `"JDKs"`.
    pub display_name: String,
    /// One install, singular, for buttons: `"JDK"`.
    pub item_name: String,
    /// What the inventory is for, shown at the top of its dialog.
    pub description: String,
}

/// One installed runtime an extension recognized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInstall {
    pub home: PathBuf,
    /// How the inventory shows it: `"Java 21"`.
    pub label: String,
    pub major_version: u32,
}

/// A runtime kind bundled with the extension that recognizes its installs.
/// Owns an `Arc` for the same reason [`crate::AnalyzerHandle`] does: both
/// questions run a process, so they belong on a background thread that
/// cannot borrow the registry.
#[derive(Clone)]
pub struct RuntimeHandle {
    pub contribution: RuntimeContribution,
    extension: Arc<dyn Extension>,
}

impl std::fmt::Debug for RuntimeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeHandle")
            .field("id", &self.contribution.id)
            .finish_non_exhaustive()
    }
}

impl RuntimeHandle {
    pub(crate) fn new(contribution: RuntimeContribution, extension: Arc<dyn Extension>) -> Self {
        Self { contribution, extension }
    }

    pub fn id(&self) -> &str {
        &self.contribution.id
    }

    pub fn display_name(&self) -> &str {
        &self.contribution.display_name
    }

    /// Verifies that `home` holds a working install, by running it. `Err` is
    /// a message for the user.
    pub fn detect(&self, home: &Path) -> Result<RuntimeInstall, String> {
        self.extension.detect_runtime(&self.contribution.id, home)
    }

    /// Every install found on this machine. Blocking: run it off the UI
    /// thread.
    pub fn discover(&self) -> Vec<RuntimeInstall> {
        self.extension.discover_runtimes(&self.contribution.id)
    }
}
