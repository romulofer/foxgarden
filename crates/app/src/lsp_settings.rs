/// Settings > Language Server — off by default (`PLAN.md` Track 20 is
/// still Phase 1: the current app-owned lifecycle only establishes a
/// handshake, while later phases add visible language features. Spawning an
/// external JVM process per open Java/Kotlin project is real enough cost
/// that it remains explicit opt-in, the same stance `AutoSaveSettings`
/// already takes. `lsp_state::LspState` checks `enabled` before every spawn,
/// so no external process launches until the user turns this on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LspSettings {
    pub enabled: bool,
    pub jdtls_binary: String,
    pub kotlin_language_server_binary: String,
}
