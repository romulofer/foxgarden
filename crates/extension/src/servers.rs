//! Installing a language server the extension ships with: what the archive
//! is and where its launcher ends up. The extension owns the bytes (it is the
//! only thing that knows the server exists); the app owns extracting them
//! into its cache and the settings that point at the result.

use std::sync::Arc;

use crate::{Extension, LanguageServerContribution};

/// How a server's bundled archive is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
}

/// Where "what is the newest upstream version?" is answered. Both shapes are
/// GitHub's, because that is where servers publish; a repository that cuts
/// real releases uses `Releases`, one that only tags uses `Tags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatestVersionSource {
    Releases,
    Tags,
}

/// A server FoxGarden can install from bytes the extension carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInstall {
    /// The one version bundled — the only one that can be installed.
    pub version: String,
    pub archive: ArchiveKind,
    /// The launcher's path inside the extracted archive, one component per
    /// entry (so no separator is baked in).
    pub launcher: Vec<String>,
    /// `owner/name` on GitHub, for the update check and the install tooltip.
    pub github_repo: String,
    pub latest_version_source: LatestVersionSource,
    /// What installing costs and what the server needs to run, for the dialog.
    pub note: String,
}

/// A server with an installer, bundled with the extension that carries its
/// archive. Owns an `Arc` like [`crate::RuntimeHandle`], so an install can
/// run on a background thread that cannot borrow the registry.
#[derive(Clone)]
pub struct ServerInstallHandle {
    pub server: LanguageServerContribution,
    pub install: ServerInstall,
    extension: Arc<dyn Extension>,
}

impl std::fmt::Debug for ServerInstallHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerInstallHandle")
            .field("id", &self.server.id)
            .finish_non_exhaustive()
    }
}

impl ServerInstallHandle {
    pub(crate) fn new(server: LanguageServerContribution, install: ServerInstall, extension: Arc<dyn Extension>) -> Self {
        Self { server, install, extension }
    }

    pub fn id(&self) -> &str {
        &self.server.id
    }

    pub fn display_name(&self) -> &str {
        &self.server.display_name
    }

    /// The bundled archive's bytes, or `None` if the extension has none.
    pub fn archive(&self) -> Option<&'static [u8]> {
        self.extension.server_archive(&self.server.id)
    }

    /// Whether the machine can run the server, checked before installing so
    /// a missing runtime surfaces in one sentence. Blocking (it may run a
    /// process): call it off the UI thread.
    pub fn check_prerequisites(&self) -> Result<(), String> {
        self.extension.check_server_prerequisites(&self.server.id)
    }
}
