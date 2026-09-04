//! App-owned lifecycle for the two supported language servers.
//!
//! `lsp_client` intentionally knows only how to speak JSON-RPC to one child
//! process. This module decides *whether* a process should exist for the
//! current project and keeps every wait/poll non-blocking, so an unavailable
//! or slow server never joins the editor's input-to-pixels path.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use fg_core::{Diagnostic, Document, Language, Severity};
use lsp_types::notification::{DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, PublishDiagnostics};
use lsp_types::request::{CodeActionRequest, Completion, ExecuteCommand, GotoDefinition, HoverRequest, References, Rename};
use lsp_types::{DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, InitializeParams, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, Uri, WorkspaceFolder};
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use serde_json::json;

use crate::lsp_client::{LspSession, ResponseError};
use crate::lsp_manager;
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

    /// `debug_bundles` is a plain parameter, not read off `config`, so this
    /// stays a pure function of its own arguments — directly unit-testable
    /// (`initialize_params_has_one_root_workspace_and_jdtls_capabilities`)
    /// with no real filesystem I/O against this machine's actual cache
    /// directory. `start_session` is the only real caller and is the one
    /// place that resolves `lsp_manager::ensure_debug_plugin_jar` for real,
    /// mirroring `ensure_kotlin_stdlib_override_for`'s own "prepared right
    /// before spawning, not earlier" placement.
    fn initialization_options(self, config: &SessionConfig, debug_bundles: &[String]) -> serde_json::Value {
        match self {
            // The local Zed Java reference establishes these as useful,
            // conservative JDTLS capabilities. Classpath injection remains
            // deliberately absent until a real JDTLS probe establishes its
            // documented configuration contract (PLAN.md Track 20).
            Self::Java => json!({
                "extendedClientCapabilities": {
                    "classFileContentsSupport": true,
                    "resolveAdditionalTextEditsSupport": true,
                },
                "settings": { "java": { "configuration": { "runtimes": jdtls_runtimes(config) } } },
                "bundles": debug_bundles,
            }),
            // kotlin-language-server has usable workspace settings, but no
            // project-specific setting is safe to invent here.
            Self::Kotlin => json!({}),
        }
    }
}

/// The `bundles` list a Java session's `initializationOptions` carries —
/// jdt.ls loads each entry as an extra OSGi bundle on top of its own core
/// (`PLAN.md` Track 23), which is how `com.microsoft.java.debug.plugin`
/// (real DAP support, not a jdt.ls-native feature) gets into a running
/// session at all. Only ever called for `ServerKind::Java` (`start_session`'s
/// own match arm gates it, mirroring the Kotlin-only `ensure_kotlin_stdlib_
/// override_for` call in the arm right next to it) — real filesystem I/O
/// against this machine's actual cache directory, so it must never run from
/// anywhere a fast/non-`#[ignore]`d unit test can reach, unlike the rest of
/// `initialize_params`'s own construction. Best-effort, mirroring
/// `ensure_kotlin_stdlib_override_for`'s own "never blocks/fails the
/// caller" shape: a failure here (a read-only cache directory, say)
/// degrades to "no debugger available this session" rather than breaking
/// hover/diagnostics/completion, which have nothing to do with debugging.
/// Track 23's own "Debug" action is where that degradation becomes a real,
/// user-visible error — `vscode.java.startDebugSession` against a session
/// with no debug bundle loaded fails with jdt.ls' own "unknown command"
/// response, not a silent no-op.
fn debug_plugin_bundles() -> Vec<String> {
    match lsp_manager::ensure_debug_plugin_jar() {
        Ok(path) => vec![path.display().to_string()],
        Err(error) => {
            eprintln!("java-debug plugin unavailable, Debug will not work this session: {error}");
            Vec::new()
        }
    }
}

/// jdt.ls' own `java.configuration.runtimes` list: every JDK this machine
/// has, named by its Eclipse execution environment, with the one matching
/// the project's declared release marked `default`.
///
/// This is what makes older projects diagnosable at all. jdt.ls itself only
/// runs on a JDK 21, and left to itself it compiles against *that* — so a
/// Java 8 codebase gets no error on `var`, no error on a `record`, and no
/// warning where its real compiler would reject the file outright. Handing
/// it the JDK 8 install and telling it that's the project's environment
/// makes its diagnostics match the build. A project whose declared release
/// has no matching JDK installed still gets the full list (jdt.ls can then
/// at least report the mismatch itself) but no `default` — claiming an
/// environment that isn't there produces worse errors than saying nothing.
fn jdtls_runtimes(config: &SessionConfig) -> Vec<serde_json::Value> {
    config
        .runtimes
        .iter()
        .map(|runtime| {
            json!({
                "name": runtime.name,
                "path": runtime.path.display().to_string(),
                "default": Some(runtime.major) == config.java_release,
            })
        })
        .collect()
}

#[derive(Clone, PartialEq, Eq)]
struct SessionConfig {
    root: PathBuf,
    binary: PathBuf,
    /// `LspSettings::jdtls_java_home` at the time this config was built —
    /// `""` for `ServerKind::Kotlin`, which has no such setting. Part of the
    /// config (not read fresh at spawn time) so editing it in Settings >
    /// Language Servers… changes this, `slot_matches` sees a different
    /// `SessionConfig`, and the running session restarts under the newly
    /// chosen JVM the same way changing the binary path already does.
    java_home: String,
    /// The Java release this project declares (`fg_core::detect_java_release`
    /// — `pom.xml`/Gradle/`.java-version`), or `None` when it declares none;
    /// always `None` for `ServerKind::Kotlin`. Part of the config for the
    /// same reason `java_home` is: a server told at `initialize` time which
    /// execution environment is the default never revisits it, so changing
    /// a `pom.xml`'s compiler release has to restart the session.
    java_release: Option<u32>,
    /// Every JDK found on this machine (`lsp_manager::runtimes_snapshot`) —
    /// what lets a project be linted at a release *older* than the JVM
    /// jdt.ls itself runs on. Empty for `ServerKind::Kotlin`, and empty for
    /// Java until the background scan lands, at which point this differs and
    /// the session restarts with the real list.
    runtimes: Vec<lsp_manager::JavaRuntime>,
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
    Ready {
        config: SessionConfig,
        session: LspSession,
        /// Every document this session has been sent a `didOpen` for,
        /// mapped to the exact text the server currently believes it has.
        /// Keeping that copy is what makes incremental `didChange` possible
        /// (`incremental_change` diffs against it); it costs one extra copy
        /// per open document, which the previous full-text sync was already
        /// allocating on *every* keystroke anyway.
        open_documents: HashMap<PathBuf, String>,
        /// What the server said it wants in its own `initialize` reply.
        /// A server that asks for `Full` still gets whole-document
        /// `didChange`s — sending it ranges it never agreed to parse would
        /// silently corrupt its copy of the file.
        sync_kind: lsp_types::TextDocumentSyncKind,
        /// The most recent `language/status` message jdt.ls sent since its
        /// `initialize` response landed, if any — `None` once a
        /// `"ServiceReady"` status arrives (the project import this
        /// message tracks is actually done by then) or for a server (every
        /// `ServiceKind::Kotlin` session) that never sends this jdt.ls-only
        /// notification at all.
        ///
        /// This is the gap `starting_servers` alone leaves: this app's own
        /// handshake (`initialize` request/response) finishes in a couple
        /// of seconds regardless of project size, well before jdt.ls's own
        /// project import does — on a real multi-module Maven reactor that
        /// import alone measured 24s (`br.ufsc.bridge/pec`, ~40 modules).
        /// Once `initialize` answers, this `Slot` is already `Ready`
        /// (correctly — `didOpen`/requests are legal to send), so without
        /// this field the status bar's "Starting…" line simply vanishes for
        /// however long that import keeps running with nothing anywhere
        /// to say completions/diagnostics are about to be wrong or empty.
        status_message: Option<String>,
    },
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
    java_release: JavaReleaseCache,
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
            let config = desired_config(kind, settings, project_root, needed, &mut self.java_release);
            let slot = match kind {
                ServerKind::Java => &mut self.java,
                ServerKind::Kotlin => &mut self.kotlin,
            };
            reconcile_slot(slot, config, kind, &mut self.retiring, &mut errors);
            sync_documents(slot, kind, documents, &mut errors);
            apply_server_messages(slot, kind, documents, &mut errors);
        }
        errors
    }

    /// The display names of every server whose `initialize` handshake is
    /// still in flight, for the status bar to report.
    ///
    /// Starting a language server is this app's longest-running unprompted
    /// job by far — jdt.ls indexes a real project for tens of seconds
    /// before it answers anything — and until it lands, an open `.java`
    /// file simply has no completions, no hovers, and no diagnostics, with
    /// nothing anywhere to say why. This is what the bar says instead.
    pub fn starting_servers(&self) -> Vec<&'static str> {
        [(ServerKind::Java, &self.java), (ServerKind::Kotlin, &self.kotlin)]
            .into_iter()
            .filter(|(_, slot)| matches!(slot, Slot::Starting(_)))
            .map(|(kind, _)| kind.name())
            .collect()
    }

    /// The display name plus latest `language/status` message of every
    /// `Ready` server still importing its project, for the status bar to
    /// report — `starting_servers`'s own blind spot: this app's `initialize`
    /// handshake (what `starting_servers` tracks) finishes in a couple of
    /// seconds regardless of project size, well before jdt.ls's own project
    /// import does (24s measured on a real ~40-module Maven reactor), and
    /// that gap used to run with nothing on screen to say completions/
    /// diagnostics were still about to be wrong or missing.
    pub fn indexing_servers(&self) -> Vec<(&'static str, &str)> {
        [(ServerKind::Java, &self.java), (ServerKind::Kotlin, &self.kotlin)]
            .into_iter()
            .filter_map(|(kind, slot)| match slot {
                Slot::Ready { status_message: Some(message), .. } => Some((kind.name(), message.as_str())),
                _ => None,
            })
            .collect()
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

    /// Sends a `textDocument/definition` request at `byte_offset` in `doc` —
    /// same session-lookup/flush-then-send shape as `request_hover` above,
    /// just a different LSP method. `goto_definition::GotoDefinitionState`
    /// owns decoding the reply (`PLAN.md` Track 20 Phase 4).
    pub fn request_definition(&mut self, doc: &mut Document, byte_offset: usize) -> Option<Receiver<Result<serde_json::Value, ResponseError>>> {
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
        let params = lsp_types::GotoDefinitionParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams { text_document: TextDocumentIdentifier { uri }, position },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        session.send_request(GotoDefinition::METHOD, serde_json::to_value(params).ok()?).ok()
    }

    /// Sends a `textDocument/references` request at `byte_offset` in
    /// `doc` — same session-lookup/flush-then-send shape as
    /// `request_definition` above, just a different LSP method and params
    /// type. `include_declaration: true` (`PLAN.md` Track 20 Phase 6): a
    /// symbol's own declaration is itself a legitimate "place this is
    /// used", and a real server's own reply list either includes or omits
    /// it consistently based on this flag rather than always including
    /// it, so it has to be requested explicitly to get a genuinely
    /// complete list. `references::FindReferencesState` owns decoding the
    /// reply.
    pub fn request_references(&mut self, doc: &mut Document, byte_offset: usize) -> Option<Receiver<Result<serde_json::Value, ResponseError>>> {
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
        let params = lsp_types::ReferenceParams {
            text_document_position: lsp_types::TextDocumentPositionParams { text_document: TextDocumentIdentifier { uri }, position },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext { include_declaration: true },
        };
        session.send_request(References::METHOD, serde_json::to_value(params).ok()?).ok()
    }

    /// Sends a `textDocument/rename` request at `byte_offset` in `doc`,
    /// asking the server to rename that symbol to `new_name` — same
    /// session-lookup/flush-then-send shape as `request_references`
    /// above, just a different LSP method and an extra plain-string
    /// argument. `crate::rename::RenameState` owns decoding the reply
    /// (an `Option<WorkspaceEdit>`) and actually applying it.
    pub fn request_rename(
        &mut self,
        doc: &mut Document,
        byte_offset: usize,
        new_name: &str,
    ) -> Option<Receiver<Result<serde_json::Value, ResponseError>>> {
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
        let params = lsp_types::RenameParams {
            text_document_position: lsp_types::TextDocumentPositionParams { text_document: TextDocumentIdentifier { uri }, position },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        };
        session.send_request(Rename::METHOD, serde_json::to_value(params).ok()?).ok()
    }

    /// Sends a `textDocument/codeAction` request for `diagnostic`'s own
    /// range in `doc` — same session-lookup/flush-then-send shape as
    /// `request_rename` above. `context.diagnostics` carries `diagnostic`
    /// reconstructed as a real `lsp_types::Diagnostic`: this app's own
    /// `fg_core::Diagnostic` only keeps range/severity/message (not
    /// `code`/`source`/`related_information`), which is what every quick
    /// fix observed in practice keys off; a server that needs the fuller
    /// shape to propose a fix would need those retained too, not attempted
    /// here (`PLAN.md` Track 15 Phase 1's own stated scope).
    /// `widgets::editor::code_action::CodeActionGutter` owns decoding the
    /// reply and offering it.
    pub fn request_code_action(
        &mut self,
        doc: &mut Document,
        diagnostic: &Diagnostic,
    ) -> Option<Receiver<Result<serde_json::Value, ResponseError>>> {
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
        let text = doc.buffer.to_string();
        let range = lsp_types::Range {
            start: byte_to_utf16_position(&text, diagnostic.range.start),
            end: byte_to_utf16_position(&text, diagnostic.range.end),
        };
        let lsp_diagnostic = lsp_types::Diagnostic {
            range,
            severity: Some(match diagnostic.severity {
                Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
                Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
            }),
            message: diagnostic.message.clone(),
            ..Default::default()
        };
        let params = lsp_types::CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range,
            context: lsp_types::CodeActionContext { diagnostics: vec![lsp_diagnostic], only: None, trigger_kind: None },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        session.send_request(CodeActionRequest::METHOD, serde_json::to_value(params).ok()?).ok()
    }

    /// Sends jdt.ls' own `java/classFileContents` extension request — the
    /// only way to read a JDK/library type's decompiled source, since a
    /// `jdt://` URI (what a `textDocument/definition` reply points a
    /// project-external Java symbol at) names no real file on disk.
    /// Java-only: `kotlin-language-server` never emits this scheme, and
    /// jdt.ls is the only server this app declares `classFileContentsSupport`
    /// to (`ServerKind::initialization_options`). `None` whenever the Java
    /// session isn't `Ready` — same best-effort degrade every other LSP path
    /// here already has.
    pub fn request_class_file_contents(&mut self, uri: &str) -> Option<Receiver<Result<serde_json::Value, ResponseError>>> {
        let Slot::Ready { session, .. } = &mut self.java else { return None };
        session.send_request("java/classFileContents", json!({ "uri": uri })).ok()
    }

    /// `vscode.java.startDebugSession` (`PLAN.md` Track 23) — a jdt.ls
    /// workspace command `com.microsoft.java.debug.plugin` registers once
    /// loaded as a bundle (`SessionConfig::debug_bundles`), not a real LSP
    /// method. Starts the plugin's own DAP server *inside* the running
    /// jdt.ls JVM and hands back the TCP port it's listening on — verified
    /// concretely against the real `vscode-java-debug` extension's own
    /// client code, `dap_client`'s own header comment. No document/
    /// byte-offset to flush first (unlike every `request_*` above), same
    /// "just needs the session `Ready`" shape `request_class_file_contents`
    /// already uses. `None` with no debug bundle loaded (`SessionConfig::
    /// debug_bundles` empty) is possible in principle but not special-cased
    /// here — jdt.ls' own "unknown command" error response covers that case
    /// with a real, inspectable message rather than a client-side guess.
    pub fn request_start_debug_session(&mut self) -> Option<Receiver<Result<serde_json::Value, ResponseError>>> {
        let Slot::Ready { session, .. } = &mut self.java else { return None };
        let params = lsp_types::ExecuteCommandParams {
            command: "vscode.java.startDebugSession".to_string(),
            arguments: Vec::new(),
            work_done_progress_params: Default::default(),
        };
        session.send_request(ExecuteCommand::METHOD, serde_json::to_value(params).ok()?).ok()
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

/// The project's declared Java release, remembered between frames.
/// `LspState::sync` runs every frame and a config is rebuilt each time, but
/// the answer only changes when a build file does — so it's re-read on a
/// new project root and otherwise at most once every `RECHECK_AFTER`,
/// which picks up an edited `pom.xml` without stat-ing four paths per frame.
#[derive(Default)]
struct JavaReleaseCache {
    root: Option<PathBuf>,
    release: Option<u32>,
    checked: Option<Instant>,
}

impl JavaReleaseCache {
    const RECHECK_AFTER: Duration = Duration::from_secs(2);

    fn release_for(&mut self, root: &Path) -> Option<u32> {
        let stale = self.root.as_deref() != Some(root)
            || self.checked.is_none_or(|at| at.elapsed() >= Self::RECHECK_AFTER);
        if stale {
            self.root = Some(root.to_path_buf());
            self.release = fg_core::detect_java_release(root).map(|found| found.major);
            self.checked = Some(Instant::now());
        }
        self.release
    }
}

fn desired_config(
    kind: ServerKind,
    settings: &LspSettings,
    root: Option<&Path>,
    needed: bool,
    java_release: &mut JavaReleaseCache,
) -> Option<SessionConfig> {
    if !settings.enabled || !needed {
        return None;
    }
    let binary = kind.configured_binary(settings).trim();
    let root = root?;
    let (java_home, release, runtimes) = match kind {
        ServerKind::Java => (
            settings.jdtls_java_home.trim().to_string(),
            java_release.release_for(root),
            lsp_manager::runtimes_snapshot(),
        ),
        ServerKind::Kotlin => (String::new(), None, Vec::new()),
    };
    (!binary.is_empty()).then(|| SessionConfig {
        root: root.to_path_buf(),
        binary: PathBuf::from(binary),
        java_home,
        java_release: release,
        runtimes,
    })
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
            Ok(Ok(result)) => {
                let sync_kind = advertised_sync_kind(&result);
                if let Err(error) = running.session.initialized() {
                    let config = running.config.clone();
                    let message = format!("failed to finish {} initialization: {error}", kind.name());
                    errors.push(message.clone());
                    *slot = Slot::Failed { config };
                } else {
                    let running = std::mem::replace(slot, Slot::Empty);
                    let Slot::Starting(running) = running else { unreachable!() };
                    *slot = Slot::Ready {
                        config: running.config,
                        session: running.session,
                        open_documents: HashMap::new(),
                        sync_kind,
                        status_message: None,
                    };
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

/// Documents are synchronized incrementally against servers that ask for
/// it (`advertised_sync_kind`) and whole-document against the rest — see
/// `incremental_change`, which derives the edit by diffing against the text
/// the server was last actually sent rather than trusting the editor's edit
/// paths to report themselves.
fn sync_documents(slot: &mut Slot, kind: ServerKind, documents: &mut [Document], errors: &mut Vec<String>) {
    let Slot::Ready { session, open_documents, .. } = slot else { return };
    let current: HashSet<PathBuf> = documents
        .iter()
        .filter(|doc| doc.language == Some(kind.language()))
        .map(|doc| doc.path.clone())
        .collect();
    let closed: Vec<PathBuf> = open_documents.keys().filter(|path| !current.contains(*path)).cloned().collect();
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
    let Slot::Ready { session, open_documents, sync_kind, .. } = slot else { return true };
    let uri = match file_uri(&doc.path) {
        Ok(uri) => uri,
        Err(error) => {
            errors.push(error);
            return true;
        }
    };
    let (result, synced_text) = match open_documents.get(&doc.path) {
        Some(last_synced) => {
            if !doc.lsp_sync_pending {
                return true;
            }
            let new_text = doc.buffer.to_string();
            let content_changes = if *sync_kind == lsp_types::TextDocumentSyncKind::INCREMENTAL {
                match incremental_change(last_synced, &new_text) {
                    Some(change) => vec![change],
                    // The buffer ended up back at what the server already
                    // has (an edit and its undo between two syncs) — there
                    // is nothing to report, so this only clears the pending
                    // flag.
                    None => {
                        doc.lsp_sync_pending = false;
                        return true;
                    }
                }
            } else {
                vec![TextDocumentContentChangeEvent { range: None, range_length: None, text: new_text.clone() }]
            };
            let params = DidChangeTextDocumentParams {
                text_document: lsp_types::VersionedTextDocumentIdentifier { uri, version: doc.lsp_version },
                content_changes,
            };
            (
                session.send_notification(DidChangeTextDocument::METHOD, serde_json::to_value(params).unwrap()),
                new_text,
            )
        }
        None => {
            let language_id = match kind { ServerKind::Java => "java", ServerKind::Kotlin => "kotlin" }.to_string();
            let text = doc.buffer.to_string();
            let params = DidOpenTextDocumentParams {
                text_document: TextDocumentItem { uri, language_id, version: doc.lsp_version, text: text.clone() },
            };
            (
                session.send_notification(DidOpenTextDocument::METHOD, serde_json::to_value(params).unwrap()),
                text,
            )
        }
    };
    match result {
        Ok(()) => {
            // Recorded only on a successful send: a failed notification
            // leaves the server's copy at whatever it had, and the next
            // sync must diff against *that*, not against text the server
            // never received.
            open_documents.insert(doc.path.clone(), synced_text);
            doc.lsp_sync_pending = false;
            true
        }
        Err(error) => {
            errors.push(format!("failed to synchronize {} with {}: {error}", doc.path.display(), kind.name()));
            false
        }
    }
}

/// What `initialize`'s reply says about how this server wants document
/// changes delivered. LSP allows either a bare number
/// (`textDocumentSync: 2`) or the full options object
/// (`textDocumentSync: { "change": 2, ... }`) — real servers use both
/// shapes, so both are read here. Anything missing or unrecognized falls
/// back to `Full`, the conservative choice: a server that gets a whole
/// document when it expected one is merely doing redundant work, whereas
/// one that gets a range it never agreed to apply ends up with a corrupt
/// copy of the file and starts reporting diagnostics for text that isn't
/// there.
fn advertised_sync_kind(initialize_result: &serde_json::Value) -> lsp_types::TextDocumentSyncKind {
    let sync = initialize_result.get("capabilities").and_then(|c| c.get("textDocumentSync"));
    let change = match sync {
        Some(serde_json::Value::Number(n)) => n.as_u64(),
        Some(serde_json::Value::Object(_)) => sync.and_then(|s| s.get("change")).and_then(serde_json::Value::as_u64),
        _ => None,
    };
    match change {
        Some(2) => lsp_types::TextDocumentSyncKind::INCREMENTAL,
        _ => lsp_types::TextDocumentSyncKind::FULL,
    }
}

/// The one `TextDocumentContentChangeEvent` that turns `old` into `new`:
/// the shared prefix and suffix are skipped, and only what's between them
/// is sent, as a range into `old`.
///
/// This is what keeps typing in a large file cheap. A whole-document
/// `didChange` per keystroke means serializing the entire file — hundreds
/// of kilobytes on a real Spring service class — to tell the server about
/// one inserted character, on every keystroke, forever. Diffing here rather
/// than instrumenting every edit path in the editor keeps that bookkeeping
/// in one place (and correct by construction: the result is computed
/// *from* the two texts, so it can't drift out of sync with an edit path
/// that forgot to report itself).
///
/// `None` when the texts are identical — nothing to tell the server.
fn incremental_change(old: &str, new: &str) -> Option<TextDocumentContentChangeEvent> {
    if old == new {
        return None;
    }

    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    // Byte-wise scanning can stop in the middle of a multi-byte character
    // (typing an accent onto an existing letter, say); backing up to a
    // boundary keeps every slice below a valid `str`.
    while prefix > 0 && (!old.is_char_boundary(prefix) || !new.is_char_boundary(prefix)) {
        prefix -= 1;
    }

    let max_suffix = old.len().min(new.len()) - prefix;
    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(max_suffix);
    while suffix > 0
        && (!old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix))
    {
        suffix -= 1;
    }

    let old_end = old.len() - suffix;
    Some(TextDocumentContentChangeEvent {
        range: Some(lsp_types::Range {
            start: byte_to_utf16_position(old, prefix),
            end: byte_to_utf16_position(old, old_end),
        }),
        range_length: None,
        text: new[prefix..new.len() - suffix].to_string(),
    })
}

/// jdt.ls' own custom `language/status` notification's params — `type` is
/// one of `Starting`/`Started`/`Error`/`ServiceReady`/`ProjectStatus` in
/// practice, none of them part of `lsp_types` since this is jdt.ls-specific,
/// not standard LSP. `kotlin-language-server` never sends this at all, so a
/// Kotlin session's `status_message` simply stays `None` throughout.
#[derive(serde::Deserialize)]
struct LanguageStatusParams {
    #[serde(rename = "type")]
    kind: String,
    message: String,
}

fn apply_server_messages(slot: &mut Slot, kind: ServerKind, documents: &mut [Document], errors: &mut Vec<String>) {
    let Slot::Ready { session, status_message, .. } = slot else { return };
    for (method, params) in session.poll_server_messages() {
        match method.as_str() {
            m if m == PublishDiagnostics::METHOD => {
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
            "language/status" => {
                let Ok(status) = serde_json::from_value::<LanguageStatusParams>(params) else { continue };
                match status.kind.as_str() {
                    // The project import this message was tracking is done
                    // (successfully or not — a prior `"Error"` status
                    // already reported the failure) — nothing left to show.
                    "ServiceReady" => *status_message = None,
                    "Error" => {
                        errors.push(format!("{}: {}", kind.name(), status.message));
                        *status_message = Some(status.message);
                    }
                    _ => *status_message = Some(status.message),
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn utf16_range_to_bytes(text: &str, range: lsp_types::Range) -> Option<std::ops::Range<usize>> {
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
    // jdt.ls' own `bin/jdtls` launcher otherwise falls back to whatever
    // `java` its own `JAVA_HOME`/`PATH` resolves to at spawn time, which may
    // not meet its Java 21 runtime minimum at all — pinning it here via its
    // own documented `--java-executable` flag makes every JDTLS session use
    // the exact JVM `lsp_manager::resolve_jdtls_java` already verified,
    // rather than risking a second, unverified resolution.
    let mut debug_bundles = Vec::new();
    let args = match kind {
        ServerKind::Java => {
            let java = lsp_manager::resolve_jdtls_java(&config.java_home)?;
            // Best-effort (`debug_plugin_bundles`'s own doc comment) — a
            // Java session still starts, with hover/diagnostics/completion
            // fully working, even if this fails.
            debug_bundles = debug_plugin_bundles();
            vec!["--java-executable".to_string(), java.display().to_string()]
        }
        ServerKind::Kotlin => {
            // See `lsp_manager::ensure_kotlin_stdlib_override_for`'s own doc
            // comment (TECHNICAL_DEBT.md #17) — pins kotlin-language-server
            // to a version-matched stdlib regardless of what's on `PATH`,
            // best-effort so a failure here never blocks starting the
            // session itself.
            lsp_manager::ensure_kotlin_stdlib_override_for(&config.binary);
            Vec::new()
        }
    };
    let mut session = LspSession::spawn(&config.binary, &args, Some(&config.root))
        .map_err(|error| format!("failed to start {} at {}: {error}", kind.name(), config.binary.display()))?;
    let params = initialize_params(kind, &config, &debug_bundles)?;
    let initialize_rx = session
        .initialize(params)
        .map_err(|error| format!("failed to initialize {}: {error}", kind.name()))?;
    Ok(RunningSession { config, session, initialize_rx })
}

fn initialize_params(kind: ServerKind, config: &SessionConfig, debug_bundles: &[String]) -> Result<InitializeParams, String> {
    let root = config.root.as_path();
    let uri = file_uri(root)?;
    #[allow(deprecated)]
    let params = InitializeParams {
        process_id: Some(std::process::id()),
        root_uri: Some(uri.clone()),
        workspace_folders: Some(vec![WorkspaceFolder {
            uri,
            name: root.file_name().and_then(|name| name.to_str()).unwrap_or("FoxGarden project").to_string(),
        }]),
        initialization_options: Some(kind.initialization_options(config, debug_bundles)),
        // `snippet_support: Some(false)` — Phase 5's own completion
        // candidates are inserted as plain text (`completion::
        // bare_label_and_has_params`), never a real tab-stop-navigable
        // snippet; declaring this lets a compliant server send `PlainText`-
        // formatted items itself rather than relying solely on this
        // client's own defensive label-parsing to undo a `Snippet` one.
        //
        // `hover.content_format` is `[PlainText, Markdown]` — the order is
        // the client's own stated *preference*, per the protocol, and this
        // client's tooltip is a plain `ui.label` (`hover::HoverState::
        // paint`) with no markdown renderer behind it, so a server left to
        // its own default (JDTLS picks Markdown) would have its `**bold**`
        // and fenced code blocks painted as literal punctuation. Markdown
        // stays listed as the accepted fallback for servers that only
        // speak it.
        capabilities: lsp_types::ClientCapabilities {
            text_document: Some(lsp_types::TextDocumentClientCapabilities {
                completion: Some(lsp_types::CompletionClientCapabilities {
                    completion_item: Some(lsp_types::CompletionItemCapability { snippet_support: Some(false), ..Default::default() }),
                    ..Default::default()
                }),
                hover: Some(lsp_types::HoverClientCapabilities {
                    dynamic_registration: Some(false),
                    content_format: Some(vec![lsp_types::MarkupKind::PlainText, lsp_types::MarkupKind::Markdown]),
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

/// Memo table for `file_uri` — every successful `path -> Uri` resolution,
/// so the `canonicalize` syscall inside it is paid once per file instead of
/// once per call. It's called in a linear scan over every open document on
/// *every* `publishDiagnostics` a server sends (jdt.ls sends them in bursts
/// while indexing), which turned a background server's chatter into
/// hundreds of stat calls per frame.
///
/// Only successes are cached: a failure means the path didn't resolve
/// *yet* (a file being written out from under an open tab), and must be
/// retried rather than remembered. Capped, and cleared wholesale when full,
/// since the working set is "files open in this session" — a bounded
/// number in practice, and a rebuild costs one syscall per file.
static FILE_URI_CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, Uri>>> = std::sync::OnceLock::new();

/// How many resolved URIs `FILE_URI_CACHE` keeps before it's cleared.
const FILE_URI_CACHE_CAP: usize = 1024;

fn file_uri(path: &Path) -> Result<Uri, String> {
    let cache = FILE_URI_CACHE.get_or_init(Default::default);
    if let Ok(cached) = cache.lock()
        && let Some(uri) = cached.get(path)
    {
        return Ok(uri.clone());
    }
    let uri = resolve_file_uri(path)?;
    if let Ok(mut cached) = cache.lock() {
        if cached.len() >= FILE_URI_CACHE_CAP {
            cached.clear();
        }
        cached.insert(path.to_path_buf(), uri.clone());
    }
    Ok(uri)
}

fn resolve_file_uri(path: &Path) -> Result<Uri, String> {
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

/// `file_uri`'s own reverse direction — decodes a `file`-scheme URI (as a
/// real server's `textDocument/definition` reply carries) back to a
/// filesystem `PathBuf`. `None` for anything not `file`-scheme: a `jdt://`
/// decompiled-class reference is `goto_definition`'s own job to resolve via
/// `request_class_file_contents` instead, never a real path here.
pub(crate) fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let text = uri.as_str();
    let rest = text.strip_prefix("file://")?;
    let rest = if cfg!(windows) { rest.strip_prefix('/').unwrap_or(rest) } else { rest };
    Some(PathBuf::from(percent_decode(rest)))
}

/// `file_uri`'s own percent-*encode* step, reversed. Malformed `%XX`
/// escapes (a stray `%` not followed by two hex digits) pass through
/// verbatim rather than erroring — real servers only ever emit escapes
/// `file_uri` itself could have produced, so this only needs to undo those,
/// not validate arbitrary input.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 3 <= bytes.len()
            && let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `SessionConfig` for tests that only care about the fields they set
    /// — the Java release/runtimes ones default to "nothing detected", which
    /// is exactly a machine with no JDK scan finished and a project that
    /// declares no release.
    fn test_config(root: &Path) -> SessionConfig {
        SessionConfig {
            root: root.to_path_buf(),
            binary: PathBuf::from("jdtls"),
            java_home: String::new(),
            java_release: None,
            runtimes: Vec::new(),
        }
    }

    #[test]
    fn desired_config_requires_opt_in_root_language_and_binary() {
        let settings = LspSettings { enabled: true, jdtls_binary: "jdtls".to_string(), ..Default::default() };
        let root = Path::new(".");
        let cache = &mut JavaReleaseCache::default();
        assert!(desired_config(ServerKind::Java, &settings, Some(root), true, cache).is_some());
        assert!(desired_config(ServerKind::Java, &settings, Some(root), false, cache).is_none());
        assert!(desired_config(ServerKind::Kotlin, &settings, Some(root), true, cache).is_none());
    }

    /// The project's declared release travels in the config, so a session
    /// started before a `pom.xml` said "Java 8" is replaced by one that
    /// knows — `slot_matches` compares whole configs.
    #[test]
    fn desired_config_carries_the_projects_declared_java_release() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pom.xml"),
            "<project><properties><maven.compiler.source>1.8</maven.compiler.source></properties></project>",
        )
        .unwrap();
        let settings = LspSettings { enabled: true, jdtls_binary: "jdtls".to_string(), ..Default::default() };

        let config = desired_config(
            ServerKind::Java,
            &settings,
            Some(dir.path()),
            true,
            &mut JavaReleaseCache::default(),
        )
        .expect("a Java session is wanted");

        assert_eq!(config.java_release, Some(8));
    }

    /// jdt.ls is handed every installed JDK, with the project's own release
    /// marked default — that pairing is what makes an old project's
    /// diagnostics match its real compiler instead of jdt.ls' own JVM.
    #[test]
    fn jdtls_runtimes_name_every_jdk_and_default_to_the_projects_release() {
        let config = SessionConfig {
            java_release: Some(8),
            runtimes: vec![
                lsp_manager::JavaRuntime { major: 21, name: "JavaSE-21".to_string(), path: PathBuf::from("/jdk21") },
                lsp_manager::JavaRuntime { major: 8, name: "JavaSE-1.8".to_string(), path: PathBuf::from("/jdk8") },
            ],
            ..test_config(Path::new("."))
        };

        let runtimes = jdtls_runtimes(&config);

        assert_eq!(runtimes.len(), 2);
        assert_eq!(runtimes[0]["name"], "JavaSE-21");
        assert_eq!(runtimes[0]["default"], false);
        assert_eq!(runtimes[1]["name"], "JavaSE-1.8");
        assert_eq!(runtimes[1]["path"], "/jdk8");
        assert_eq!(runtimes[1]["default"], true);
    }

    /// A project whose declared release isn't installed anywhere must not
    /// have some *other* JDK declared its default — jdt.ls reporting the
    /// missing environment itself beats silently linting at the wrong one.
    #[test]
    fn no_runtime_is_default_when_the_projects_release_is_not_installed() {
        let config = SessionConfig {
            java_release: Some(8),
            runtimes: vec![lsp_manager::JavaRuntime {
                major: 21,
                name: "JavaSE-21".to_string(),
                path: PathBuf::from("/jdk21"),
            }],
            ..test_config(Path::new("."))
        };

        let runtimes = jdtls_runtimes(&config);

        assert_eq!(runtimes.len(), 1);
        assert_eq!(runtimes[0]["default"], false);
    }

    #[test]
    fn initialize_params_has_one_root_workspace_and_jdtls_capabilities() {
        let params = initialize_params(ServerKind::Java, &test_config(Path::new(".")), &[]).unwrap();
        assert_eq!(params.workspace_folders.as_ref().unwrap().len(), 1);
        assert_eq!(params.initialization_options.unwrap()["extendedClientCapabilities"]["classFileContentsSupport"], true);
    }

    /// `debug_bundles` is a plain argument specifically so this stays a
    /// pure, no-I/O test of the plumbing (`PLAN.md` Track 23 Phase 1) rather
    /// than one that has to resolve `lsp_manager::ensure_debug_plugin_jar`
    /// for real against this machine's actual cache directory.
    #[test]
    fn initialize_params_carries_the_debug_bundle_path_through() {
        let params =
            initialize_params(ServerKind::Java, &test_config(Path::new(".")), &["/cache/java-debug-plugin.jar".to_string()])
                .unwrap();
        assert_eq!(params.initialization_options.unwrap()["bundles"], serde_json::json!(["/cache/java-debug-plugin.jar"]));
    }

    /// A server picks its hover content format from what the client says
    /// it prefers; with no `hover` capability declared at all, JDTLS
    /// defaults to Markdown and the tooltip paints raw `**`/``` ``` ```
    /// punctuation (`HoverState::paint` is a plain label, not a markdown
    /// renderer). PlainText must therefore be *first*, not merely present.
    #[test]
    fn initialize_params_prefers_plain_text_hover_content() {
        let params = initialize_params(ServerKind::Java, &test_config(Path::new(".")), &[]).unwrap();
        let hover = params.capabilities.text_document.unwrap().hover.unwrap();
        assert_eq!(
            hover.content_format,
            Some(vec![lsp_types::MarkupKind::PlainText, lsp_types::MarkupKind::Markdown])
        );
    }

    #[test]
    fn file_uri_percent_encodes_a_space() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project with space");
        std::fs::create_dir(&root).unwrap();
        assert!(file_uri(&root).unwrap().as_str().contains("project%20with%20space"));
    }

    #[test]
    fn uri_to_path_round_trips_through_file_uri_including_a_percent_encoded_space() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project with space");
        std::fs::create_dir_all(&root).unwrap();
        let canonical = root.canonicalize().unwrap();
        let uri = file_uri(&root).unwrap();
        assert_eq!(uri_to_path(&uri).unwrap(), canonical);
    }

    #[test]
    fn uri_to_path_rejects_a_non_file_scheme() {
        let uri = Uri::from_str("jdt://contents/rt.jar/java.lang/String.class?=x").unwrap();
        assert!(uri_to_path(&uri).is_none());
    }

    #[test]
    fn percent_decode_undoes_file_uris_own_encoding() {
        assert_eq!(percent_decode("project%20with%20space"), "project with space");
    }

    #[test]
    fn percent_decode_leaves_a_malformed_escape_verbatim() {
        assert_eq!(percent_decode("100%_off"), "100%_off");
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

    /// Applies `change` to `old` the way a real server would, so a test can
    /// assert the server ends up with exactly `new` instead of only
    /// asserting the range arithmetic looks plausible.
    fn apply(old: &str, change: &TextDocumentContentChangeEvent) -> String {
        let range = change.range.expect("an incremental change always carries a range");
        let start = utf16_range_to_bytes(old, range).expect("range resolves against the text it was built from");
        format!("{}{}{}", &old[..start.start], change.text, &old[start.end..])
    }

    #[test]
    fn incremental_change_is_none_when_nothing_changed() {
        assert!(incremental_change("class A {}", "class A {}").is_none());
    }

    #[test]
    fn incremental_change_sends_only_the_inserted_text() {
        let old = "class A {\n    int x;\n}\n";
        let new = "class A {\n    int xy;\n}\n";

        let change = incremental_change(old, new).expect("a change");

        assert_eq!(change.text, "y");
        assert_eq!(apply(old, &change), new);
    }

    #[test]
    fn incremental_change_sends_an_empty_text_for_a_deletion() {
        let old = "class A {\n    int xy;\n}\n";
        let new = "class A {\n    int x;\n}\n";

        let change = incremental_change(old, new).expect("a change");

        assert!(change.text.is_empty());
        assert_eq!(apply(old, &change), new);
    }

    #[test]
    fn incremental_change_handles_a_multi_line_replacement() {
        let old = "class A {\n    int x;\n    int y;\n}\n";
        let new = "class A {\n    long z;\n}\n";

        let change = incremental_change(old, new).expect("a change");

        assert_eq!(apply(old, &change), new);
    }

    #[test]
    fn incremental_change_never_splits_a_multi_byte_character() {
        // The shared prefix ends mid-`ç` byte-wise: "café" and "caçé" share
        // `ca` plus the first byte of the two-byte `f`/`ç`... which is only
        // true for characters that happen to share a lead byte, so this
        // uses two that do.
        let old = "// año\nclass A {}\n";
        let new = "// añô\nclass A {}\n";

        let change = incremental_change(old, new).expect("a change");

        assert_eq!(apply(old, &change), new);
    }

    #[test]
    fn incremental_change_handles_an_edit_at_the_very_end() {
        let old = "class A {}";
        let new = "class A {}\n";

        let change = incremental_change(old, new).expect("a change");

        assert_eq!(change.text, "\n");
        assert_eq!(apply(old, &change), new);
    }

    #[test]
    fn incremental_change_handles_an_edit_at_the_very_start() {
        let old = "class A {}\n";
        let new = "// hi\nclass A {}\n";

        let change = incremental_change(old, new).expect("a change");

        assert_eq!(apply(old, &change), new);
    }

    #[test]
    fn advertised_sync_kind_reads_both_shapes_and_defaults_to_full() {
        let numeric = serde_json::json!({ "capabilities": { "textDocumentSync": 2 } });
        assert_eq!(advertised_sync_kind(&numeric), lsp_types::TextDocumentSyncKind::INCREMENTAL);

        let options = serde_json::json!({ "capabilities": { "textDocumentSync": { "openClose": true, "change": 2 } } });
        assert_eq!(advertised_sync_kind(&options), lsp_types::TextDocumentSyncKind::INCREMENTAL);

        let full = serde_json::json!({ "capabilities": { "textDocumentSync": { "change": 1 } } });
        assert_eq!(advertised_sync_kind(&full), lsp_types::TextDocumentSyncKind::FULL);

        assert_eq!(advertised_sync_kind(&serde_json::json!({})), lsp_types::TextDocumentSyncKind::FULL);
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
            java: Slot::Ready {
                config: SessionConfig { binary: PathBuf::from("python3"), ..test_config(&root) },
                session,
                open_documents: HashMap::new(),
            sync_kind: lsp_types::TextDocumentSyncKind::INCREMENTAL,
                status_message: None,
            },
            kotlin: Slot::Empty,
            retiring: Vec::new(),
            java_release: JavaReleaseCache::default(),
        };

        let rx = state.request_completion(&mut doc, 0).expect("a Ready Java session should accept the request");
        let value = rx.recv().expect("the fake server's response arrives").expect("the fake server replied successfully");
        assert_eq!(value[0]["label"], "add(E e) : boolean");
    }

    /// A real child process (`sh -c printf`) that writes one canned
    /// notification then blocks reading its own stdin — `lsp_client`'s own
    /// `fake_server_returning` shape, reimplemented here since that helper
    /// is private to `lsp_client`'s test module.
    fn fake_server_sending(body: &str) -> LspSession {
        let script = format!("printf 'Content-Length: %d\\r\\n\\r\\n%s' {} '{body}'; cat > /dev/null", body.len());
        LspSession::spawn(Path::new("sh"), &["-c".to_string(), script], None).expect("sh is always available")
    }

    fn ready_slot_around(session: LspSession) -> Slot {
        Slot::Ready {
            config: test_config(Path::new(".")),
            session,
            open_documents: HashMap::new(),
            sync_kind: lsp_types::TextDocumentSyncKind::INCREMENTAL,
            status_message: None,
        }
    }

    /// A plain (non-`"Error"`, non-`"ServiceReady"`) `language/status` —
    /// what jdt.ls sends throughout a project import (`"Starting"`,
    /// `"Started"`, `"ProjectStatus"`, ...) — lands as `status_message`,
    /// polled until the background reader thread has actually delivered it
    /// (same "loop until it shows up" shape `poll_server_messages_receives_
    /// a_real_notification_from_a_real_process` in `lsp_client` already
    /// uses for a bare notification with no response to block on instead).
    #[test]
    fn a_language_status_notification_becomes_the_slots_status_message() {
        let session = fake_server_sending(
            r#"{"jsonrpc":"2.0","method":"language/status","params":{"type":"Starting","message":"Importing project br.ufsc.bridge.pec-backend"}}"#,
        );
        let mut slot = ready_slot_around(session);
        let mut documents = [];
        let mut errors = Vec::new();
        loop {
            apply_server_messages(&mut slot, ServerKind::Java, &mut documents, &mut errors);
            let Slot::Ready { status_message, .. } = &slot else { unreachable!() };
            if status_message.is_some() {
                assert_eq!(status_message.as_deref(), Some("Importing project br.ufsc.bridge.pec-backend"));
                assert!(errors.is_empty(), "a non-Error status must never be reported as a lifecycle failure");
                break;
            }
        }
    }

    /// `"ServiceReady"` — jdt.ls's own terminal "the import this status was
    /// tracking is actually done" signal — clears `status_message` back to
    /// `None` rather than leaving the last progress line stuck on screen.
    #[test]
    fn a_service_ready_status_clears_the_status_message() {
        let session = fake_server_sending(r#"{"jsonrpc":"2.0","method":"language/status","params":{"type":"ServiceReady","message":"ServiceReady"}}"#);
        let mut slot = ready_slot_around(session);
        let Slot::Ready { status_message, .. } = &mut slot else { unreachable!() };
        *status_message = Some("Importing project br.ufsc.bridge.pec-backend".to_string());
        let mut documents = [];
        let mut errors = Vec::new();
        loop {
            apply_server_messages(&mut slot, ServerKind::Java, &mut documents, &mut errors);
            let Slot::Ready { status_message, .. } = &slot else { unreachable!() };
            if status_message.is_none() {
                break;
            }
        }
    }

    /// `"Error"` — jdt.ls's own report that a project import actually
    /// failed (real, observed case: a stale `.project` file referencing an
    /// unregistered `org.jetbrains.kotlin.core.filesystem` linked-resource
    /// scheme aborts the whole reactor import) — is the one status type
    /// that must also reach `errors`, this app's existing one-shot
    /// lifecycle-failure channel (`sync`'s own doc comment), or it's
    /// invisible: the JSON-RPC handshake still succeeds and the slot stays
    /// `Ready` regardless, so nothing else here would ever surface it.
    #[test]
    fn an_error_status_is_reported_through_errors_too() {
        let session = fake_server_sending(
            r#"{"jsonrpc":"2.0","method":"language/status","params":{"type":"Error","message":"Failed to import projects"}}"#,
        );
        let mut slot = ready_slot_around(session);
        let mut documents = [];
        let mut errors = Vec::new();
        loop {
            apply_server_messages(&mut slot, ServerKind::Java, &mut documents, &mut errors);
            if !errors.is_empty() {
                assert_eq!(errors, vec!["JDTLS: Failed to import projects".to_string()]);
                let Slot::Ready { status_message, .. } = &slot else { unreachable!() };
                assert_eq!(status_message.as_deref(), Some("Failed to import projects"));
                break;
            }
        }
    }

    /// Where the real-server tests below look for a binary: an environment
    /// override first (a developer's own install, wherever it lives), then
    /// whatever `lsp_manager` last installed into its own cache directory.
    /// Panics rather than silently passing — these tests are `#[ignore]`d,
    /// so reaching one at all means a developer explicitly asked for it and
    /// deserves to be told why it can't run.
    fn real_server_binary(env_var: &str, cache_relative: &str) -> PathBuf {
        if let Some(configured) = std::env::var_os(env_var) {
            return PathBuf::from(configured);
        }
        let cached = lsp_manager::cache_dir().expect("a cache directory").join(cache_relative);
        assert!(
            cached.is_file(),
            "no language server at {} — install one through Settings > Language Servers…, \
             or point {env_var} at your own",
            cached.display()
        );
        cached
    }

    /// Drives `sync` until this state's Kotlin session finishes its
    /// handshake, the same way the app's own update loop would across
    /// frames. Any lifecycle error is a hard failure: a real-server test
    /// that quietly proceeds with no session would "pass" by asserting
    /// nothing.
    fn sync_until_kotlin_ready(state: &mut LspState, settings: &LspSettings, root: &Path, doc: &mut Document) {
        let deadline = Instant::now() + Duration::from_secs(240);
        while !matches!(state.kotlin, Slot::Ready { .. }) {
            let errors = state.sync(settings, Some(root), std::slice::from_mut(doc));
            assert!(errors.is_empty(), "language server lifecycle errors: {errors:?}");
            assert!(Instant::now() < deadline, "kotlin-language-server never finished its handshake");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// `sync_until_kotlin_ready`'s Java counterpart — same loop, same
    /// hard-failure stance, just the other slot.
    fn sync_until_java_ready(state: &mut LspState, settings: &LspSettings, root: &Path, doc: &mut Document) {
        let deadline = Instant::now() + Duration::from_secs(240);
        while !matches!(state.java, Slot::Ready { .. }) {
            let errors = state.sync(settings, Some(root), std::slice::from_mut(doc));
            assert!(errors.is_empty(), "language server lifecycle errors: {errors:?}");
            assert!(Instant::now() < deadline, "jdtls never finished its handshake");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// `TECHNICAL_DEBT.md` #20: Track 20 Phase 3's own live-verify only ever
    /// got a blank `contents` back, for what was almost certainly a JDK
    /// type with no sources attached — leaving "hover is genuinely broken"
    /// and "this install simply has no Javadoc for `List`" indistinguishable.
    /// A symbol the *project itself* declares separates them: jdtls resolves
    /// that from its own compiled bindings, no external sources needed, so
    /// blank content here would be a real bug in `request_hover`.
    ///
    /// Retries rather than asking once: jdtls answers a hover long before
    /// it has finished building the project model, and its early answers
    /// are legitimately empty.
    ///
    /// `#[ignore]`d for the same reasons as the Kotlin test above; jdtls
    /// additionally needs a JDK 21 (`FOXGARDEN_JDTLS_JAVA_HOME`).
    #[test]
    #[ignore = "needs a real jdtls install and a JDK 21; run with --ignored"]
    fn java_hover_against_a_real_server_documents_a_project_owned_symbol() {
        let binary = real_server_binary("FOXGARDEN_JDTLS", "jdtls-1.60.0/bin/jdtls");
        let source = concat!(
            "public class Sample {\n",
            "    /** Returns the answer to everything. */\n",
            "    int answer() {\n",
            "        return 42;\n",
            "    }\n",
            "\n",
            "    void run() {\n",
            "        int value = answer();\n",
            "    }\n",
            "}\n",
        );
        let dir = test_support::tempdir();
        let path = test_support::write_file(dir.path(), "Sample.java", source);
        let mut doc = Document::open(path).expect("open the fixture");
        let settings = LspSettings {
            enabled: true,
            jdtls_binary: binary.display().to_string(),
            jdtls_java_home: std::env::var("FOXGARDEN_JDTLS_JAVA_HOME").unwrap_or_default(),
            ..Default::default()
        };

        let mut state = LspState::default();
        sync_until_java_ready(&mut state, &settings, dir.path(), &mut doc);

        // The `answer()` *call site*, not its declaration — the ordinary
        // "what is this thing I'm reading" hover, and the one that has to
        // resolve a binding rather than just read the token under the
        // pointer.
        let call_site = source.rfind("answer()").expect("the fixture calls its own method");
        let deadline = Instant::now() + Duration::from_secs(180);
        let content = loop {
            let rx = state
                .request_hover(&mut doc, call_site)
                .expect("a Ready Java session should accept the request");
            let value = rx
                .recv_timeout(Duration::from_secs(60))
                .expect("the server answers the hover request")
                .expect("the server replied successfully");
            let text = value["contents"].to_string();
            if text.contains("answer") {
                break text;
            }
            assert!(
                Instant::now() < deadline,
                "jdtls never resolved a project-owned symbol; last hover contents: {text}"
            );
            std::thread::sleep(Duration::from_secs(2));
        };
        assert!(
            content.contains("Returns the answer to everything"),
            "hover resolved the symbol but dropped its Javadoc: {content}"
        );
    }

    /// `PLAN.md` Track 23 Phase 1's own checkpoint: launching a real Java
    /// program under the debugger successfully attaches. Drives the real
    /// chain end to end — a real `mvn compile`, a real jdt.ls session with
    /// `com.microsoft.java.debug.plugin` loaded as a bundle
    /// (`lsp_state::debug_plugin_bundles`), a real `vscode.java.
    /// startDebugSession` call, a real socket connection, and the real DAP
    /// `initialize`/`launch`/`configurationDone` handshake — not a mocked
    /// adapter, since `dap_client`'s own fake-socket tests already cover the
    /// framing/classification logic in isolation and what's unverified here
    /// is everything downstream of it: whether jdt.ls actually starts the
    /// plugin's DAP server on a real port, and whether java-debug actually
    /// accepts this app's own hand-assembled `launch` arguments.
    ///
    /// `#[ignore]`d for the same reasons every real-server test here is:
    /// needs a real jdtls + JDK 21 install, plus a real `mvn` on `PATH` to
    /// compile the fixture project this test writes itself. Run with
    /// `--ignored`, `FOXGARDEN_JDTLS`/`FOXGARDEN_JDTLS_JAVA_HOME` set the
    /// same way the hover test above needs them.
    #[test]
    #[ignore = "needs a real jdtls install, a JDK 21, and mvn on PATH; run with --ignored"]
    fn java_debug_launch_against_a_real_server_attaches_to_a_real_process() {
        let binary = real_server_binary("FOXGARDEN_JDTLS", "jdtls-1.60.0/bin/jdtls");
        let dir = test_support::tempdir();
        let root = dir.path();
        std::fs::write(
            root.join("pom.xml"),
            concat!(
                "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n",
                "  <modelVersion>4.0.0</modelVersion>\n",
                "  <groupId>com.example</groupId>\n",
                "  <artifactId>debug-fixture</artifactId>\n",
                "  <version>1.0.0</version>\n",
                "  <properties>\n",
                "    <maven.compiler.release>17</maven.compiler.release>\n",
                "    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>\n",
                "  </properties>\n",
                "</project>\n",
            ),
        )
        .expect("write the fixture pom.xml");
        let main_dir = root.join("src/main/java/com/example");
        std::fs::create_dir_all(&main_dir).expect("create the fixture's source tree");
        std::fs::write(
            main_dir.join("Main.java"),
            concat!(
                "package com.example;\n",
                "public class Main {\n",
                "    public static void main(String[] args) throws InterruptedException {\n",
                "        System.out.println(\"debug fixture running\");\n",
                "        Thread.sleep(5000);\n",
                "        System.out.println(\"debug fixture done\");\n",
                "    }\n",
                "}\n",
            ),
        )
        .expect("write the fixture Main.java");
        let compile = std::process::Command::new("mvn")
            .args(["-q", "compile"])
            .current_dir(root)
            .status()
            .expect("mvn is on PATH");
        assert!(compile.success(), "real mvn compile of the fixture project failed");

        let mut doc = Document::open(main_dir.join("Main.java")).expect("open the fixture");
        let settings = LspSettings {
            enabled: true,
            jdtls_binary: binary.display().to_string(),
            jdtls_java_home: std::env::var("FOXGARDEN_JDTLS_JAVA_HOME").unwrap_or_default(),
            ..Default::default()
        };
        let mut state = LspState::default();
        sync_until_java_ready(&mut state, &settings, root, &mut doc);

        let run_config = fg_core::RunConfig {
            name: "debug fixture".to_string(),
            main_class: "com.example.Main".to_string(),
            vm_args: String::new(),
            program_args: String::new(),
            env: Vec::new(),
            working_dir: None,
        };
        // `PLAN.md` Track 23 Phase 2: a real breakpoint on the fixture's own
        // `System.out.println("debug fixture running")` line (0-indexed
        // line 3 — `package`/`class`/`main` signature/println are lines
        // 0-3) exercises the *initial* breakpoint path (`start`'s own
        // `initial_breakpoints`, sent before `configurationDone`), not the
        // live-toggle path (`sync_breakpoints`) — the point of this test is
        // proving a breakpoint set up front actually halts execution there.
        let main_java = main_dir.join("Main.java");
        let mut debug = crate::debug_state::DebugState::default();
        debug
            .start(
                &mut state,
                root,
                fg_core::BuildTool::Maven,
                &run_config,
                vec![(main_java.clone(), HashSet::from([3]))],
            )
            .expect("starting a debug session against a Ready Java session should succeed");

        // Draining the Java session's own diagnostics/hover plumbing
        // (`sync_until_java_ready`'s own discipline) keeps the connection
        // genuinely alive rather than starving it of the polling every
        // other real-server test in this file already relies on — reused
        // across every wait loop below, not just the first one.
        let wait_for_pause = |debug: &mut crate::debug_state::DebugState, state: &mut LspState, doc: &mut Document| {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                debug.poll();
                let _ = state.sync(&settings, Some(root), std::slice::from_mut(doc));
                if let Some((file, line)) = debug.paused_location() {
                    break (file.to_path_buf(), line);
                }
                match debug.status() {
                    crate::debug_state::DebugStatus::Failed(message) => panic!("debug session failed: {message}"),
                    status => {
                        assert!(Instant::now() < deadline, "debug session never paused; stuck at {status:?}");
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        };

        let (paused_file, paused_line) = wait_for_pause(&mut debug, &mut state, &mut doc);
        assert_eq!(
            paused_file.canonicalize().expect("java-debug reports a real file path"),
            main_java.canonicalize().expect("the fixture file exists"),
            "should pause in the exact file the breakpoint was set on"
        );
        assert_eq!(paused_line, 3, "should pause exactly at the breakpoint's own line");
        assert_eq!(debug.status(), crate::debug_state::DebugStatus::Attached);

        // `PLAN.md` Track 23 Phase 3: the call stack and the `scopes`/
        // `variables` chain both need extra polling past `paused_location`
        // itself resolving (the former is fetched in the same `stackTrace`
        // response, so it's already there; the latter is a separate,
        // slower background chain — see `debug_state::VarFetch`'s own doc
        // comment for why).
        let stack = debug.call_stack();
        assert!(!stack.is_empty(), "a real stackTrace response should report at least one frame");
        // Real java-debug reports a frame's own `name` as `"Main.main(String[])"`
        // — the declaring class and full signature, not the bare method
        // name a first guess from the DAP spec alone might expect.
        assert_eq!(stack[0].name, "Main.main(String[])", "the top frame should be the fixture's own main()");
        assert_eq!(
            stack[0].file.as_deref().and_then(|p| p.canonicalize().ok()),
            main_java.canonicalize().ok(),
            "the top frame's own file should be the fixture's Main.java"
        );

        let variables_deadline = Instant::now() + Duration::from_secs(30);
        loop {
            debug.poll();
            let _ = state.sync(&settings, Some(root), std::slice::from_mut(&mut doc));
            if !debug.variables().is_empty() {
                break;
            }
            assert!(Instant::now() < variables_deadline, "the scopes/variables chain never finished");
            std::thread::sleep(Duration::from_millis(100));
        }
        let has_args = debug.variables().iter().any(|group| group.variables.iter().any(|v| v.name == "args"));
        assert!(has_args, "main(String[] args)'s own parameter should show up among the real fetched variables");

        // Step Over: from the println line, the next line reached in the
        // same frame is the `Thread.sleep(5000)` call right after it.
        debug.step_over();
        let (_, line_after_step) = wait_for_pause(&mut debug, &mut state, &mut doc);
        assert_eq!(line_after_step, 4, "Step Over should land on the next source line");

        // Continue: nothing else stops it, so the debuggee runs to
        // completion — `Thread.sleep(5000)` makes this genuinely take a
        // few seconds, hence the generous deadline below.
        debug.continue_();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            debug.poll();
            let _ = state.sync(&settings, Some(root), std::slice::from_mut(&mut doc));
            match debug.status() {
                crate::debug_state::DebugStatus::Idle => break,
                crate::debug_state::DebugStatus::Failed(message) => panic!("debug session failed: {message}"),
                status => {
                    assert!(Instant::now() < deadline, "debuggee never terminated after Continue; stuck at {status:?}");
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }

        debug.stop();
    }

    /// `TECHNICAL_DEBT.md` #18: a raw JSON-RPC probe already proved
    /// `kotlin-language-server` itself answers a `list.` completion with
    /// the receiver's own members, but the same action through the real
    /// GUI showed generic top-level candidates instead — leaving it
    /// unknown whether *this codebase's* own client path (URI encoding,
    /// `didOpen`/`didChange` ordering, `byte_to_utf16_position`) was at
    /// fault. This drives exactly that path — no GUI, no raw probe — so
    /// the answer is attributable to one side or the other.
    ///
    /// `#[ignore]`d: needs a real server binary on disk and takes tens of
    /// seconds of real handshake/indexing time. Run it with
    /// `cargo test -p foxgarden --bin foxgarden -- --ignored kotlin_completion`.
    #[test]
    #[ignore = "needs a real kotlin-language-server install; run with --ignored"]
    fn kotlin_completion_against_a_real_server_returns_the_receivers_own_members() {
        let binary = real_server_binary(
            "FOXGARDEN_KOTLIN_LANGUAGE_SERVER",
            "kotlin-language-server-1.3.13/server/bin/kotlin-language-server",
        );
        let before_dot = "fun main() {\n    val list = mutableListOf<String>()\n    list\n}\n";
        let dir = test_support::tempdir();
        let path = test_support::write_file(dir.path(), "src/main/kotlin/Sample.kt", before_dot);
        let mut doc = Document::open(path).expect("open the fixture");
        let settings = LspSettings {
            enabled: true,
            kotlin_language_server_binary: binary.display().to_string(),
            ..Default::default()
        };

        let mut state = LspState::default();
        // The handshake (and the `didOpen` `sync` sends with it) completes
        // *before* the dot is typed — the GUI's own ordering, and the one
        // that makes the `didChange` below a real mid-session edit rather
        // than part of the document's very first `didOpen`.
        sync_until_kotlin_ready(&mut state, &settings, dir.path(), &mut doc);

        let typed = before_dot.replace("    list\n", "    list.\n");
        // Exactly what `widgets::editor::widget::apply_edit` does for a
        // typed character, minus the tree-sitter reparse this doesn't need.
        doc.buffer.replace(ropey::Rope::from_str(&typed));
        doc.lsp_version += 1;
        doc.lsp_sync_pending = true;
        let anchor = typed.find("list.").expect("the fixture contains the receiver") + "list.".len();

        let rx = state
            .request_completion(&mut doc, anchor)
            .expect("a Ready Kotlin session should accept the request");
        let value = rx
            .recv_timeout(Duration::from_secs(120))
            .expect("the server answers the completion request")
            .expect("the server replied successfully");
        let labels: Vec<String> = serde_json::from_value::<lsp_types::CompletionResponse>(value)
            .map(|response| match response {
                lsp_types::CompletionResponse::Array(items) => items,
                lsp_types::CompletionResponse::List(list) => list.items,
            })
            .expect("a well-formed completion response")
            .into_iter()
            .map(|item| item.label)
            .collect();

        // `MutableList<String>`'s own members, not the bare-keyword set a
        // server falls back to when it can't resolve the receiver at the
        // requested position — that fallback (`by`/`out`/`set`/…) is
        // precisely the degraded result #18 recorded from the GUI.
        for member in ["add", "get", "size", "clear"] {
            assert!(
                labels.iter().any(|label| label.split('(').next() == Some(member)),
                "no `{member}` among {} completions: {labels:?}",
                labels.len()
            );
        }
    }
}
