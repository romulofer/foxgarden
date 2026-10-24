use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: Range<usize>,
    pub severity: Severity,
    pub message: String,
    /// What a language server attached to the diagnostic, kept so a later
    /// request about it can hand it back unchanged (see [`ServerDiagnostic`]).
    /// `None` for diagnostics the editor computed itself.
    pub server: Option<ServerDiagnostic>,
}

/// The parts of an LSP diagnostic beyond range, severity and message.
/// Servers key follow-up requests on them: jdtls finds a diagnostic's quick
/// fixes by its `code`, and answers a `textDocument/codeAction` with nothing
/// when the diagnostic comes back without one. Each is the raw JSON text of
/// the value the server sent (`code` may be a number or a string; `data` is
/// opaque), so the core needs no JSON dependency.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerDiagnostic {
    pub code: Option<String>,
    pub source: Option<String>,
    pub data: Option<String>,
}
