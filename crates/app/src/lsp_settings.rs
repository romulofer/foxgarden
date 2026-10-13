use std::collections::BTreeMap;

use crate::lsp_manager::Installed;

/// Settings > Language Servers… (`panels::lsp_servers`) — off by default:
/// spawning an external JVM process per open Java/Kotlin project is real
/// enough cost that it stays explicit opt-in, the same stance
/// `AutoSaveSettings` already takes. `lsp_state::LspState` checks `enabled`
/// before every spawn, so no external process launches until the user turns
/// this on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LspSettings {
    pub enabled: bool,
    /// One entry per language server, keyed by its registered id.
    pub servers: BTreeMap<String, ServerSettings>,
}

/// What the user configured for one language server. `installed_version`
/// records what `lsp_manager` last installed into its own cache directory —
/// `""` means "not installed through FoxGarden", which is also the right
/// value for someone who pointed `binary` at a server they installed
/// themselves. Same shape (and same reason) as
/// `static_analysis::ExternalToolPaths`' own installed-version fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerSettings {
    pub binary: String,
    pub installed_version: String,
    /// The home of the runtime the server runs on (`HostRuntime`),
    /// overriding the ambient lookup the extension would otherwise fall back
    /// to — for the common case where the machine's default runtime is too
    /// old, e.g. an sdkman/asdf-managed one that isn't the active one. Empty
    /// means "auto-detect", same as leaving it unset.
    pub runtime_home: String,
}

impl LspSettings {
    /// Writes a completed install's launcher path into the matching server's
    /// binary field, overwriting whatever was there. An explicit
    /// Install/Update click is exactly the case where overwriting a
    /// hand-typed path is right: the user just asked this app to go fetch a
    /// copy and use it.
    pub fn apply_installed(&mut self, installed: &Installed) {
        let server = self.servers.entry(installed.server.clone()).or_default();
        server.binary = installed.binary.display().to_string();
        server.installed_version = installed.version.clone();
    }

    /// This server's configured binary path and last-installed version, as
    /// the settings dialog's own per-server section needs them — mutable,
    /// since that section edits the path in place.
    pub fn fields_for(&mut self, server_id: &str) -> (&mut String, &mut String) {
        let server = self.servers.entry(server_id.to_string()).or_default();
        (&mut server.binary, &mut server.installed_version)
    }

    /// The user-configured binary path for a server looked up by its
    /// registered id. Returns `""` for an unconfigured server; the extension
    /// will reject an empty binary rather than guessing.
    pub fn binary_for(&self, server_id: &str) -> &str {
        self.servers.get(server_id).map_or("", |server| server.binary.as_str())
    }

    /// The runtime home override for a server that declares a `HostRuntime`.
    /// Returns `""` when none is configured (auto-detect applies).
    pub fn runtime_home_for(&self, server_id: &str) -> &str {
        self.servers.get(server_id).map_or("", |server| server.runtime_home.as_str())
    }

    /// The runtime home field, mutable — the dialog edits it in place and a
    /// finished scan writes into it.
    pub fn runtime_home_mut(&mut self, server_id: &str) -> &mut String {
        &mut self.servers.entry(server_id.to_string()).or_default().runtime_home
    }

    /// `settings` with `server_id`'s binary set — the shape tests want.
    #[cfg(test)]
    pub fn with_binary(mut self, server_id: &str, binary: &str) -> Self {
        *self.fields_for(server_id).0 = binary.to_string();
        self
    }
}

#[cfg(test)]
#[path = "lsp_settings_test.rs"]
mod lsp_settings_test;
