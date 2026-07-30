/// Settings > Language Server — off by default (`PLAN.md` Track 20 is
/// still Phase 1, no user-visible feature wired up yet, and even once
/// later phases add one, spawning an external JVM process per open Java/
/// Kotlin project is real enough cost that it should be an explicit
/// opt-in, the same "off by default, opt in" stance `AutoSaveSettings`
/// already takes). Every later phase that actually calls `lsp_client::
/// LspSession::spawn` must check `enabled` first — this flag exists
/// specifically as the user's own guarantee that nothing here launches an
/// external process unless they've turned it on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LspSettings {
    pub enabled: bool,
    pub jdtls_binary: String,
    pub kotlin_language_server_binary: String,
}
