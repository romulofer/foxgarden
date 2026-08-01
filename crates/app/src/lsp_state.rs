//! App-owned lifecycle for the two supported language servers.
//!
//! `lsp_client` intentionally knows only how to speak JSON-RPC to one child
//! process. This module decides *whether* a process should exist for the
//! current project and keeps every wait/poll non-blocking, so an unavailable
//! or slow server never joins the editor's input-to-pixels path.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use fg_core::{Diagnostic, Document, Language, Severity};
use lsp_types::notification::{DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, PublishDiagnostics};
use lsp_types::request::{Completion, HoverRequest};
use lsp_types::{DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, InitializeParams, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, Uri, WorkspaceFolder};
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use serde_json::json;

use crate::lsp_client::{LspSession, ResponseError};
use crate::lsp_settings::LspSettings;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerKind {
    Java,
    Kotlin,
}

impl ServerKind {
    fn language(self) -> Language {
        match self {
            Self::Java => Language::Java,
            Self::Kotlin => Language::Kotlin,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Java => "JDTLS",
            Self::Kotlin => "Kotlin Language Server",
        }
    }

    fn configured_binary(self, settings: &LspSettings) -> &str {
        match self {
            Self::Java => &settings.jdtls_binary,
            Self::Kotlin => &settings.kotlin_language_server_binary,
        }
    }

    fn initialization_options(self) -> serde_json::Value {
        match self {
            // The local Zed Java reference establishes these as useful,
            // conservative JDTLS capabilities. Classpath injection remains
            // deliberately absent until a real JDTLS probe establishes its
            // documented configuration contract (PLAN.md Track 20).
            Self::Java => json!({
                "extendedClientCapabilities": {
                    "classFileContentsSupport": true,
                    "resolveAdditionalTextEditsSupport": true,
                }
            }),
            // kotlin-language-server has usable workspace settings, but no
            // project-specific setting is safe to invent here.
            Self::Kotlin => json!({}),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct SessionConfig {
    root: PathBuf,
    binary: PathBuf,
}

struct RunningSession {
    config: SessionConfig,
    session: LspSession,
    initialize_rx: Receiver<Result<serde_json::Value, ResponseError>>,
}

struct RetiringSession {
    session: LspSession,
    shutdown_rx: Receiver<Result<serde_json::Value, ResponseError>>,
    started: Instant,
}

#[derive(Default)]
enum Slot {
    #[default]
    Empty,
    Starting(RunningSession),
    Ready { config: SessionConfig, session: LspSession, open_documents: HashSet<PathBuf> },
    Failed { config: SessionConfig },
}

/// At most one session per supported language for FoxGarden's one project.
/// Failed configurations stay failed until their project/binary setting
/// changes, preventing an invalid executable from being spawned every frame.
#[derive(Default)]
pub struct LspState {
    java: Slot,
    kotlin: Slot,
    retiring: Vec<RetiringSession>,
}

impl LspState {
    /// Reconciles the desired sessions with the open project/documents and
    /// advances every non-blocking handshake/shutdown poll. Returned strings
    /// are one-shot lifecycle failures suitable for the app's existing error
    /// modal; no error is returned repeatedly for an unchanged bad setting.
    pub fn sync(
        &mut self,
        settings: &LspSettings,
        project_root: Option<&Path>,
        documents: &mut [Document],
    ) -> Vec<String> {
        let mut errors = self.poll_retiring();
        let java_needed = documents.iter().any(|doc| doc.language == Some(Language::Java));
        let kotlin_needed = documents.iter().any(|doc| doc.language == Some(Language::Kotlin));

        for kind in [ServerKind::Java, ServerKind::Kotlin] {
            let needed = match kind { ServerKind::Java => java_needed, ServerKind::Kotlin => kotlin_needed };
            let config = desired_config(kind, settings, project_root, needed);
            let slot = match kind {
                ServerKind::Java => &mut self.java,
                ServerKind::Kotlin => &mut self.kotlin,
            };
            reconcile_slot(slot, config, kind, &mut self.retiring, &mut errors);
            sync_documents(slot, kind, documents, &mut errors);
            apply_diagnostics(slot, documents, &mut errors);
        }
        errors
    }

    /// Whether anything here is still waiting on a background reply — a
    /// handshake in flight, a live session whose server can publish
    /// diagnostics unprompted at any time (real servers do, right after
    /// `didOpen`, with no further client action to react to), or a shutdown
    /// awaiting its response. The caller's own event loop only calls back
    /// into `sync` in reaction to a repaint (an input event or an explicit
    /// request) — with nothing requesting one, a session sitting idle
    /// between user keystrokes would have its own async replies (the
    /// `initialize` response, a `publishDiagnostics` the server sent on its
    /// own initiative) queue up unread until the next unrelated repaint,
    /// exactly the failure this app's own `pty_session`/`terminal_widget`
    /// already guard their own background work against.
    pub fn wants_repaint(&self) -> bool {
        fn slot_pending(slot: &Slot) -> bool {
            matches!(slot, Slot::Starting(_) | Slot::Ready { .. })
        }
        slot_pending(&self.java) || slot_pending(&self.kotlin) || !self.retiring.is_empty()
    }

    /// Sends a `textDocument/completion` request at `byte_offset` in
    /// `doc`, for whichever session matches `doc.language` — `None` if
    /// that's neither Java nor Kotlin, or no session for it is `Ready`.
    ///
    /// Flushes `doc`'s own pending edit first, via `sync_one_document`
    /// (the same per-document logic `sync`'s own `sync_documents` loop
    /// uses), rather than assuming it's already been sent. This matters on
    /// the exact frame a dot-completion trigger fires: this frame's own
    /// `sync` call already ran earlier (`FoxGardenApp::ui`'s own ordering),
    /// *before* the keystroke that both landed the triggering `.` and
    /// called this method — so without an explicit flush right here, this
    /// request would race ahead of its own document's `didChange` on a
    /// single, strictly-ordered JSON-RPC stdin pipe, and the server would
    /// compute completions against content missing the just-typed `.`.
    pub fn request_completion(&mut self, doc: &mut Document, byte_offset: usize) -> Option<Receiver<Result<serde_json::Value, ResponseError>>> {
        let kind = match doc.language {
            Some(Language::Java) => ServerKind::Java,
            Some(Language::Kotlin) => ServerKind::Kotlin,
            _ => return None,
        };
        let slot = match kind {
            ServerKind::Java => &mut self.java,
            ServerKind::Kotlin => &mut self.kotlin,
        };
        let mut errors = Vec::new();
        if !sync_one_document(slot, kind, doc, &mut errors) {
            return None;
        }
        let Slot::Ready { session, .. } = slot else { return None };
        let uri = file_uri(&doc.path).ok()?;
        let position = byte_to_utf16_position(&doc.buffer.to_string(), byte_offset);
        let params = lsp_types::CompletionParams {
            text_document_position: lsp_types::TextDocumentPositionParams { text_document: TextDocumentIdentifier { uri }, position },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: Some(lsp_types::CompletionContext {
                trigger_kind: lsp_types::CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(".".to_string()),
            }),
        };
        session.send_request(Completion::METHOD, serde_json::to_value(params).ok()?).ok()
    }

    /// Sends a `textDocument/hover` request at `byte_offset` in `doc` — same
    /// session-lookup/flush-then-send shape as `request_completion` above
    /// (including the same pre-request `sync_one_document` flush, for the
    /// same reason: whichever frame first hovers a just-edited position
    /// must not race ahead of that edit's own `didChange` on the session's
    /// single ordered stdin pipe), just a different LSP method and no
    /// completion-specific trigger context.
    pub fn request_hover(&mut self, doc: &mut Document, byte_offset: usize) -> Option<Receiver<Result<serde_json::Value, ResponseError>>> {
        let kind = match doc.language {
            Some(Language::Java) => ServerKind::Java,
            Some(Language::Kotlin) => ServerKind::Kotlin,
            _ => return None,
        };
        let slot = match kind {
            ServerKind::Java => &mut self.java,
            ServerKind::Kotlin => &mut self.kotlin,
        };
        let mut errors = Vec::new();
        if !sync_one_document(slot, kind, doc, &mut errors) {
            return None;
        }
        let Slot::Ready { session, .. } = slot else { return None };
        let uri = file_uri(&doc.path).ok()?;
        let position = byte_to_utf16_position(&doc.buffer.to_string(), byte_offset);
        let params = lsp_types::HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams { text_document: TextDocumentIdentifier { uri }, position },
            work_done_progress_params: Default::default(),
        };
        session.send_request(HoverRequest::METHOD, serde_json::to_value(params).ok()?).ok()
    }

    fn poll_retiring(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        self.retiring.retain_mut(|retiring| match retiring.shutdown_rx.try_recv() {
            Ok(Ok(_)) => {
                let _ = retiring.session.exit();
                false
            }
            Ok(Err(error)) => {
                errors.push(format!("language server rejected shutdown: {}", error.message));
                false
            }
            Err(TryRecvError::Disconnected) => false,
            Err(TryRecvError::Empty) if retiring.started.elapsed() >= SHUTDOWN_TIMEOUT => false,
            Err(TryRecvError::Empty) => true,
        });
        errors
    }
}

impl Drop for LspState {
    fn drop(&mut self) {
        retire_slot(std::mem::take(&mut self.java), &mut self.retiring);
        retire_slot(std::mem::take(&mut self.kotlin), &mut self.retiring);
        // App shutdown cannot rely on another UI frame to receive shutdown
        // responses. `LspSession::Drop` kills whatever remains as the
        // bounded fallback documented by the Phase 1 plan.
    }
}

fn desired_config(kind: ServerKind, settings: &LspSettings, root: Option<&Path>, needed: bool) -> Option<SessionConfig> {
    if !settings.enabled || !needed {
        return None;
    }
    let binary = kind.configured_binary(settings).trim();
    let root = root?;
    (!binary.is_empty()).then(|| SessionConfig { root: root.to_path_buf(), binary: PathBuf::from(binary) })
}

fn reconcile_slot(
    slot: &mut Slot,
    desired: Option<SessionConfig>,
    kind: ServerKind,
    retiring: &mut Vec<RetiringSession>,
    errors: &mut Vec<String>,
) {
    if !slot_matches(slot, desired.as_ref()) {
        retire_slot(std::mem::take(slot), retiring);
    }
    if !matches!(slot, Slot::Empty) {
        poll_slot(slot, kind, errors);
        return;
    }
    let Some(config) = desired else { return };
    match start_session(kind, config.clone()) {
        Ok(starting) => *slot = Slot::Starting(starting),
        Err(message) => {
            errors.push(message.clone());
            *slot = Slot::Failed { config };
        }
    }
}

fn slot_matches(slot: &Slot, desired: Option<&SessionConfig>) -> bool {
    match (slot, desired) {
        (Slot::Empty, None) => true,
        (Slot::Starting(running), Some(config)) => running.config == *config,
        (Slot::Ready { config: existing, .. }, Some(config)) => existing == config,
        (Slot::Failed { config: existing, .. }, Some(config)) => existing == config,
        _ => false,
    }
}

fn poll_slot(slot: &mut Slot, kind: ServerKind, errors: &mut Vec<String>) {
    match slot {
        Slot::Starting(running) => match running.initialize_rx.try_recv() {
            Ok(Ok(_)) => {
                if let Err(error) = running.session.initialized() {
                    let config = running.config.clone();
                    let message = format!("failed to finish {} initialization: {error}", kind.name());
                    errors.push(message.clone());
                    *slot = Slot::Failed { config };
                } else {
                    let running = std::mem::replace(slot, Slot::Empty);
                    let Slot::Starting(running) = running else { unreachable!() };
                    *slot = Slot::Ready { config: running.config, session: running.session, open_documents: HashSet::new() };
                }
            }
            Ok(Err(error)) => {
                let config = running.config.clone();
                let message = format!("{} initialization failed: {}", kind.name(), error.message);
                errors.push(message.clone());
                *slot = Slot::Failed { config };
            }
            Err(TryRecvError::Disconnected) => {
                let config = running.config.clone();
                let message = format!("{} exited before initialization completed", kind.name());
                errors.push(message.clone());
                *slot = Slot::Failed { config };
            }
            Err(TryRecvError::Empty) => {}
        },
        Slot::Ready { config, session, .. } => match session.try_wait() {
            Ok(Some(status)) => {
                let config = config.clone();
                let message = format!("{} exited unexpectedly ({status})", kind.name());
                errors.push(message.clone());
                *slot = Slot::Failed { config };
            }
            Ok(None) => {
                // Phase 2 owns notifications such as publishDiagnostics;
                // drain none here so it can consume them with their document
                // version/URI lifecycle rules rather than losing them.
            }
            Err(error) => {
                let config = config.clone();
                let message = format!("failed to poll {}: {error}", kind.name());
                errors.push(message.clone());
                *slot = Slot::Failed { config };
            }
        },
        Slot::Empty | Slot::Failed { .. } => {}
    }
}

/// Full-text synchronization is deliberately the first implementation: it
/// avoids getting byte/UTF-16 incremental edit bookkeeping wrong while still
/// sending only documents whose edit path marked them pending. Servers that
/// advertise incremental sync can be optimized later without changing the
/// document lifecycle.
fn sync_documents(slot: &mut Slot, kind: ServerKind, documents: &mut [Document], errors: &mut Vec<String>) {
    let Slot::Ready { session, open_documents, .. } = slot else { return };
    let current: HashSet<PathBuf> = documents
        .iter()
        .filter(|doc| doc.language == Some(kind.language()))
        .map(|doc| doc.path.clone())
        .collect();
    let closed: Vec<PathBuf> = open_documents.difference(&current).cloned().collect();
    for path in closed {
        match file_uri(&path) {
            Ok(uri) => {
                let params = DidCloseTextDocumentParams { text_document: TextDocumentIdentifier { uri } };
                if let Err(error) = session.send_notification(DidCloseTextDocument::METHOD, serde_json::to_value(params).unwrap()) {
                    errors.push(format!("failed to close {kind_name} LSP document: {error}", kind_name = kind.name()));
                    return;
                }
            }
            // Can't send a well-formed `didClose` without a valid URI (the
            // file's own path no longer canonicalizes, e.g. deleted out
            // from under an open tab) — surfaced rather than silently
            // dropped, same as the open/change loop below does for the
            // same failure.
            Err(error) => errors.push(error),
        }
        open_documents.remove(&path);
    }
    for doc in documents.iter_mut().filter(|doc| doc.language == Some(kind.language())) {
        if !sync_one_document(slot, kind, doc, errors) {
            return;
        }
    }
}

/// Sends whatever `doc` itself still needs — an initial `didOpen` if this
/// session has never seen its path, otherwise a `didChange` only if
/// `lsp_sync_pending` is actually set. Extracted out of `sync_documents`'s
/// own per-document loop so `LspState::request_completion` can flush one
/// specific document's own pending edit on demand (immediately before a
/// completion request, so the server sees it in time — see that function's
/// own doc comment) without reimplementing this same open-vs-change
/// branching, and without the bug a naive `sync_documents(slot, kind,
/// std::slice::from_mut(doc), ...)` call would have: that function's own
/// *closed*-document detection compares `open_documents` against whatever
/// slice it's given, so a 1-element slice would misdetect every other
/// currently-open document of this language as newly closed.
///
/// Returns `false` on a notification-send failure (a broken pipe — no
/// point this frame's `sync_documents` loop trying any later document,
/// same abort-the-rest-of-this-frame behavior as before this was
/// extracted); `true` otherwise, including the common "nothing to send"
/// case.
fn sync_one_document(slot: &mut Slot, kind: ServerKind, doc: &mut Document, errors: &mut Vec<String>) -> bool {
    let Slot::Ready { session, open_documents, .. } = slot else { return true };
    let uri = match file_uri(&doc.path) {
        Ok(uri) => uri,
        Err(error) => {
            errors.push(error);
            return true;
        }
    };
    let result = if open_documents.contains(&doc.path) {
        if !doc.lsp_sync_pending {
            return true;
        }
        let params = DidChangeTextDocumentParams {
            text_document: lsp_types::VersionedTextDocumentIdentifier { uri, version: doc.lsp_version },
            content_changes: vec![TextDocumentContentChangeEvent { range: None, range_length: None, text: doc.buffer.to_string() }],
        };
        session.send_notification(DidChangeTextDocument::METHOD, serde_json::to_value(params).unwrap())
    } else {
        let language_id = match kind { ServerKind::Java => "java", ServerKind::Kotlin => "kotlin" }.to_string();
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem { uri, language_id, version: doc.lsp_version, text: doc.buffer.to_string() },
        };
        session.send_notification(DidOpenTextDocument::METHOD, serde_json::to_value(params).unwrap())
    };
    match result {
        Ok(()) => {
            open_documents.insert(doc.path.clone());
            doc.lsp_sync_pending = false;
            true
        }
        Err(error) => {
            errors.push(format!("failed to synchronize {} with {}: {error}", doc.path.display(), kind.name()));
            false
        }
    }
}

fn apply_diagnostics(slot: &mut Slot, documents: &mut [Document], errors: &mut Vec<String>) {
    let Slot::Ready { session, .. } = slot else { return };
    for (method, params) in session.poll_server_messages() {
        if method != PublishDiagnostics::METHOD {
            continue;
        }
        let Ok(published) = serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(params) else {
            errors.push("language server sent malformed publishDiagnostics params".to_string());
            continue;
        };
        let Some(doc) = documents.iter_mut().find(|doc| file_uri(&doc.path).ok().as_ref() == Some(&published.uri)) else {
            continue;
        };
        if published.version.is_some_and(|version| version < doc.lsp_version) {
            continue;
        }
        let text = doc.buffer.to_string();
        doc.lsp_diagnostics = published.diagnostics.into_iter().filter_map(|diagnostic| {
            utf16_range_to_bytes(&text, diagnostic.range).map(|range| Diagnostic {
                range,
                severity: match diagnostic.severity {
                    Some(lsp_types::DiagnosticSeverity::ERROR) => Severity::Error,
                    _ => Severity::Warning,
                },
                message: diagnostic.message,
            })
        }).collect();
    }
}

fn utf16_range_to_bytes(text: &str, range: lsp_types::Range) -> Option<std::ops::Range<usize>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let point = |position: lsp_types::Position| -> Option<usize> {
        // `{line: lines.len(), character: 0}` is LSP's own legal way to
        // point just past the final line's `\n` — the shape a server
        // reports for an EOF-anchored diagnostic (unterminated block/
        // string, unexpected EOF) — so it's an empty virtual last line,
        // not an out-of-range lookup to reject.
        let line = match lines.get(position.line as usize) {
            Some(line) => *line,
            None if position.line as usize == lines.len() && position.character == 0 => "",
            None => return None,
        };
        let line = line.strip_suffix('\n').unwrap_or(line).strip_suffix('\r').unwrap_or(line);
        let mut units = 0u32;
        for (byte, character) in line.char_indices() {
            if units == position.character { return Some(byte); }
            units += character.len_utf16() as u32;
            if units > position.character { return None; }
        }
        (units == position.character).then_some(line.len())
    };
    let line_start = |line_number: u32| lines.iter().take(line_number as usize).map(|line| line.len()).sum::<usize>();
    let start = line_start(range.start.line) + point(range.start)?;
    let end = line_start(range.end.line) + point(range.end)?;
    (start <= end).then_some(start..end)
}

/// `utf16_range_to_bytes`'s own reverse direction — a byte offset (always
/// one this app's own cursor/anchor math produced, so always a valid char
/// boundary) to the UTF-16 `Position` a completion request's own
/// `textDocument/completion` needs. Walks every *complete* (newline-
/// terminated) line strictly before `byte_offset` to find `line`/its own
/// start byte, then counts UTF-16 units from there to `byte_offset` for
/// `character` — the same per-line UTF-16-counting approach
/// `utf16_range_to_bytes` already uses, just measuring forward instead of
/// resolving a target position backward.
fn byte_to_utf16_position(text: &str, byte_offset: usize) -> lsp_types::Position {
    let byte_offset = byte_offset.min(text.len());
    let mut line = 0u32;
    let mut line_start_byte = 0usize;
    for l in text[..byte_offset].split_inclusive('\n') {
        if l.ends_with('\n') {
            line += 1;
            line_start_byte += l.len();
        }
    }
    let character = text[line_start_byte..byte_offset].chars().map(|c| c.len_utf16() as u32).sum();
    lsp_types::Position { line, character }
}

fn retire_slot(slot: Slot, retiring: &mut Vec<RetiringSession>) {
    match slot {
        Slot::Ready { mut session, .. } => {
            if let Ok(shutdown_rx) = session.shutdown() {
                retiring.push(RetiringSession { session, shutdown_rx, started: Instant::now() });
            }
        }
        Slot::Starting(_) | Slot::Failed { .. } | Slot::Empty => {}
    }
}

fn start_session(kind: ServerKind, config: SessionConfig) -> Result<RunningSession, String> {
    let mut session = LspSession::spawn(&config.binary, &[], Some(&config.root))
        .map_err(|error| format!("failed to start {} at {}: {error}", kind.name(), config.binary.display()))?;
    let params = initialize_params(kind, &config.root)?;
    let initialize_rx = session
        .initialize(params)
        .map_err(|error| format!("failed to initialize {}: {error}", kind.name()))?;
    Ok(RunningSession { config, session, initialize_rx })
}

fn initialize_params(kind: ServerKind, root: &Path) -> Result<InitializeParams, String> {
    let uri = file_uri(root)?;
    #[allow(deprecated)]
    let params = InitializeParams {
        process_id: Some(std::process::id()),
        root_uri: Some(uri.clone()),
        workspace_folders: Some(vec![WorkspaceFolder {
            uri,
            name: root.file_name().and_then(|name| name.to_str()).unwrap_or("FoxGarden project").to_string(),
        }]),
        initialization_options: Some(kind.initialization_options()),
        // `snippet_support: Some(false)` — Phase 5's own completion
        // candidates are inserted as plain text (`completion::
        // bare_label_and_has_params`), never a real tab-stop-navigable
        // snippet; declaring this lets a compliant server send `PlainText`-
        // formatted items itself rather than relying solely on this
        // client's own defensive label-parsing to undo a `Snippet` one.
        capabilities: lsp_types::ClientCapabilities {
            text_document: Some(lsp_types::TextDocumentClientCapabilities {
                completion: Some(lsp_types::CompletionClientCapabilities {
                    completion_item: Some(lsp_types::CompletionItemCapability { snippet_support: Some(false), ..Default::default() }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        client_info: Some(lsp_types::ClientInfo { name: "FoxGarden".to_string(), version: Some(env!("CARGO_PKG_VERSION").to_string()) }),
        ..Default::default()
    };
    Ok(params)
}

fn file_uri(path: &Path) -> Result<Uri, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("cannot resolve LSP project root {}: {error}", path.display()))?;
    let text = path.to_str().ok_or_else(|| format!("LSP project root is not UTF-8: {}", path.display()))?;
    let normalized = if cfg!(windows) { text.replace('\\', "/") } else { text.to_string() };
    let encoded: String = normalized
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'.' | b'_' | b'~' | b':' => vec![byte as char],
            other => format!("%{other:02X}").chars().collect(),
        })
        .collect();
    let uri = if cfg!(windows) { format!("file:///{encoded}") } else { format!("file://{encoded}") };
    Uri::from_str(&uri).map_err(|error| format!("invalid LSP file URI: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_config_requires_opt_in_root_language_and_binary() {
        let settings = LspSettings { enabled: true, jdtls_binary: "jdtls".to_string(), kotlin_language_server_binary: String::new() };
        let root = Path::new(".");
        assert!(desired_config(ServerKind::Java, &settings, Some(root), true).is_some());
        assert!(desired_config(ServerKind::Java, &settings, Some(root), false).is_none());
        assert!(desired_config(ServerKind::Kotlin, &settings, Some(root), true).is_none());
    }

    #[test]
    fn initialize_params_has_one_root_workspace_and_jdtls_capabilities() {
        let params = initialize_params(ServerKind::Java, Path::new(".")).unwrap();
        assert_eq!(params.workspace_folders.as_ref().unwrap().len(), 1);
        assert_eq!(params.initialization_options.unwrap()["extendedClientCapabilities"]["classFileContentsSupport"], true);
    }

    #[test]
    fn file_uri_percent_encodes_a_space() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project with space");
        std::fs::create_dir(&root).unwrap();
        assert!(file_uri(&root).unwrap().as_str().contains("project%20with%20space"));
    }

    #[test]
    fn utf16_range_to_bytes_resolves_a_mid_file_position() {
        let text = "class Foo {\n}\n";
        let range = lsp_types::Range {
            start: lsp_types::Position { line: 1, character: 0 },
            end: lsp_types::Position { line: 1, character: 1 },
        };
        assert_eq!(utf16_range_to_bytes(text, range), Some(12..13));
    }

    #[test]
    fn utf16_range_to_bytes_clamps_an_eof_anchored_position_instead_of_dropping_it() {
        // A trailing-newline-terminated file has exactly 2 `split_inclusive`
        // segments; a diagnostic reported at `{line: 2, character: 0}` is
        // LSP's own legal way to point just past the final line, the shape
        // a real server uses for e.g. an unterminated-block/unexpected-EOF
        // error — it must resolve to the end of the buffer, not `None`.
        let text = "class Foo {\n}\n";
        let range = lsp_types::Range {
            start: lsp_types::Position { line: 2, character: 0 },
            end: lsp_types::Position { line: 2, character: 0 },
        };
        assert_eq!(utf16_range_to_bytes(text, range), Some(text.len()..text.len()));
    }

    #[test]
    fn utf16_range_to_bytes_rejects_a_genuinely_out_of_range_line() {
        let text = "class Foo {\n}\n";
        let range = lsp_types::Range {
            start: lsp_types::Position { line: 5, character: 0 },
            end: lsp_types::Position { line: 5, character: 0 },
        };
        assert_eq!(utf16_range_to_bytes(text, range), None);
    }

    #[test]
    fn byte_to_utf16_position_at_the_very_start_is_zero_zero() {
        assert_eq!(byte_to_utf16_position("class Foo {\n}\n", 0), lsp_types::Position { line: 0, character: 0 });
    }

    #[test]
    fn byte_to_utf16_position_mid_line() {
        // byte 4 is the 'F' of "Foo", still line 0.
        assert_eq!(byte_to_utf16_position("class Foo {\n}\n", 6), lsp_types::Position { line: 0, character: 6 });
    }

    #[test]
    fn byte_to_utf16_position_right_after_a_lines_own_trailing_newline() {
        let text = "class Foo {\n}\n";
        // byte 12 is right after the first '\n', the '}' on line 1.
        assert_eq!(byte_to_utf16_position(text, 12), lsp_types::Position { line: 1, character: 0 });
    }

    #[test]
    fn byte_to_utf16_position_handles_crlf_line_endings() {
        let text = "ab\r\ncd";
        // byte 5 is the 'd': "ab\r\nc" is 5 bytes, so 5 lands right after 'c'.
        assert_eq!(byte_to_utf16_position(text, 5), lsp_types::Position { line: 1, character: 1 });
    }

    #[test]
    fn byte_to_utf16_position_counts_utf16_units_not_bytes_across_a_non_bmp_character() {
        // '\u{1F600}' (😀) is 4 UTF-8 bytes but 2 UTF-16 code units (a
        // surrogate pair) — a wrong byte-for-unit conflation here would
        // put every completion request after an emoji at the wrong column.
        let text = "a\u{1F600}b";
        let byte_offset = 'a'.len_utf8() + '\u{1F600}'.len_utf8();
        assert_eq!(byte_to_utf16_position(text, byte_offset), lsp_types::Position { line: 0, character: 3 });
    }

    #[test]
    fn byte_to_utf16_position_clamps_an_out_of_range_offset_to_the_end_of_text() {
        let text = "ab";
        assert_eq!(byte_to_utf16_position(text, 50), lsp_types::Position { line: 0, character: 2 });
    }

    /// The one real fake-server-*process* test this phase's own checkpoint
    /// asks for (mirroring `lsp_client`'s own such test): a genuine child
    /// process, real stdio pipes end to end, proving `request_completion`'s
    /// actual wiring — not just the pure `byte_to_utf16_position` logic
    /// already covered in isolation above. Skips simulating `initialize`/
    /// `initialized` entirely (already proven by `lsp_client`'s own tests)
    /// by constructing `Slot::Ready` directly rather than going through
    /// `sync`/`reconcile_slot` — this test's only job is `request_completion`
    /// itself: does it flush `didOpen` and send a well-formed completion
    /// request, in that order, and does the real response come back
    /// through the returned `Receiver` correctly.
    #[test]
    fn request_completion_round_trips_against_a_real_fake_server_process() {
        // `next_id` starts at 0 on a fresh `LspSession`, and this session
        // never has `initialize()` called on it (per the doc comment
        // above) — so the completion request `request_completion` itself
        // sends is genuinely this session's very first request, id 0.
        // `didOpen` is a notification, so it never consumes an id.
        let script = r#"
import sys

def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if line in (b"\r\n", b"\n", b""):
            break
        if b":" in line:
            k, v = line.split(b":", 1)
            headers[k.strip()] = v.strip()
    length = int(headers[b"Content-Length"])
    return sys.stdin.buffer.read(length)

def write_message(body):
    header = ("Content-Length: %d\r\n\r\n" % len(body)).encode()
    sys.stdout.buffer.write(header + body)
    sys.stdout.buffer.flush()

read_message()
read_message()
write_message(b'{"jsonrpc":"2.0","id":0,"result":[{"label":"add(E e) : boolean","kind":2}]}')
sys.stdin.buffer.read()
"#;
        let session = LspSession::spawn(Path::new("python3"), &["-c".to_string(), script.to_string()], None)
            .expect("python3 is always available");
        let (_dir, mut doc) = test_support::temp_document("Foo.java", "class Foo {}");
        let root = doc.path.parent().unwrap().to_path_buf();
        let mut state = LspState {
            java: Slot::Ready { config: SessionConfig { root, binary: PathBuf::from("python3") }, session, open_documents: HashSet::new() },
            kotlin: Slot::Empty,
            retiring: Vec::new(),
        };

        let rx = state.request_completion(&mut doc, 0).expect("a Ready Java session should accept the request");
        let value = rx.recv().expect("the fake server's response arrives").expect("the fake server replied successfully");
        assert_eq!(value[0]["label"], "add(E e) : boolean");
    }
}
