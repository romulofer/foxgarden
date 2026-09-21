//! App-owned lifecycle for the two supported language servers.
//!
//! `lsp_client` intentionally knows only how to speak JSON-RPC to one child
//! process. This module decides *whether* a process should exist for the
//! current project and keeps every wait/poll non-blocking, so an unavailable
//! or slow server never joins the editor's input-to-pixels path.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use fg_core::{Diagnostic, Document, Language, Severity};
use lsp_types::notification::Notification as _;
use lsp_types::notification::{DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, PublishDiagnostics};
use lsp_types::request::Request as _;
use lsp_types::request::{
    CodeActionRequest, Completion, ExecuteCommand, GotoDefinition, HoverRequest, References, Rename,
};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, InitializeParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, Uri, WorkspaceFolder,
};
use serde_json::json;

use crate::lsp_client::{LspSession, ResponseError, Waker};
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
    Failed {
        config: SessionConfig,
    },
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
        wake: &Waker,
    ) -> Vec<String> {
        let mut errors = self.poll_retiring();
        let java_needed = documents.iter().any(|doc| doc.language == Some(Language::Java));
        let kotlin_needed = documents.iter().any(|doc| doc.language == Some(Language::Kotlin));

        for kind in [ServerKind::Java, ServerKind::Kotlin] {
            let needed = match kind {
                ServerKind::Java => java_needed,
                ServerKind::Kotlin => kotlin_needed,
            };
            let config = desired_config(kind, settings, project_root, needed, &mut self.java_release);
            let slot = match kind {
                ServerKind::Java => &mut self.java,
                ServerKind::Kotlin => &mut self.kotlin,
            };
            reconcile_slot(slot, config, kind, &mut self.retiring, &mut errors, wake);
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
                Slot::Ready {
                    status_message: Some(message),
                    ..
                } => Some((kind.name(), message.as_str())),
                _ => None,
            })
            .collect()
    }

    /// Whether anything here is still waiting on a background reply — a
    /// handshake in flight, or a shutdown awaiting its response. Both are
    /// bounded waits on a reply that may never come (a server that dies
    /// mid-handshake sends nothing at all), so they keep polling on a timer.
    ///
    /// A `Ready` slot deliberately does *not* count, even though its server
    /// can publish diagnostics unprompted at any time: those arrive on the
    /// reader thread, which wakes the event loop itself via the
    /// `lsp_client::Waker` every session is spawned with. Counting `Ready`
    /// here instead meant a single idle-but-alive language server pinned the
    /// whole app to a permanent 5/second repaint that never settled — one
    /// open Java file was enough to keep it awake indefinitely with nothing
    /// in flight and no input.
    pub fn wants_repaint(&self) -> bool {
        fn slot_pending(slot: &Slot) -> bool {
            matches!(slot, Slot::Starting(_))
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
    pub fn request_completion(
        &mut self,
        doc: &mut Document,
        byte_offset: usize,
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
        let params = lsp_types::CompletionParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: Some(lsp_types::CompletionContext {
                trigger_kind: lsp_types::CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(".".to_string()),
            }),
        };
        session
            .send_request(Completion::METHOD, serde_json::to_value(params).ok()?)
            .ok()
    }

    /// Sends a `textDocument/hover` request at `byte_offset` in `doc` — same
    /// session-lookup/flush-then-send shape as `request_completion` above
    /// (including the same pre-request `sync_one_document` flush, for the
    /// same reason: whichever frame first hovers a just-edited position
    /// must not race ahead of that edit's own `didChange` on the session's
    /// single ordered stdin pipe), just a different LSP method and no
    /// completion-specific trigger context.
    pub fn request_hover(
        &mut self,
        doc: &mut Document,
        byte_offset: usize,
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
        let params = lsp_types::HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: Default::default(),
        };
        session
            .send_request(HoverRequest::METHOD, serde_json::to_value(params).ok()?)
            .ok()
    }

    /// Sends a `textDocument/definition` request at `byte_offset` in `doc` —
    /// same session-lookup/flush-then-send shape as `request_hover` above,
    /// just a different LSP method. `goto_definition::GotoDefinitionState`
    /// owns decoding the reply (`PLAN.md` Track 20 Phase 4).
    pub fn request_definition(
        &mut self,
        doc: &mut Document,
        byte_offset: usize,
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
        let params = lsp_types::GotoDefinitionParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        session
            .send_request(GotoDefinition::METHOD, serde_json::to_value(params).ok()?)
            .ok()
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
    pub fn request_references(
        &mut self,
        doc: &mut Document,
        byte_offset: usize,
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
        let params = lsp_types::ReferenceParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext {
                include_declaration: true,
            },
        };
        session
            .send_request(References::METHOD, serde_json::to_value(params).ok()?)
            .ok()
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
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        };
        session
            .send_request(Rename::METHOD, serde_json::to_value(params).ok()?)
            .ok()
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
            context: lsp_types::CodeActionContext {
                diagnostics: vec![lsp_diagnostic],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        session
            .send_request(CodeActionRequest::METHOD, serde_json::to_value(params).ok()?)
            .ok()
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
    pub fn request_class_file_contents(
        &mut self,
        uri: &str,
    ) -> Option<Receiver<Result<serde_json::Value, ResponseError>>> {
        let Slot::Ready { session, .. } = &mut self.java else {
            return None;
        };
        session
            .send_request("java/classFileContents", json!({ "uri": uri }))
            .ok()
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
        let Slot::Ready { session, .. } = &mut self.java else {
            return None;
        };
        let params = lsp_types::ExecuteCommandParams {
            command: "vscode.java.startDebugSession".to_string(),
            arguments: Vec::new(),
            work_done_progress_params: Default::default(),
        };
        session
            .send_request(ExecuteCommand::METHOD, serde_json::to_value(params).ok()?)
            .ok()
    }

    fn poll_retiring(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        self.retiring
            .retain_mut(|retiring| match retiring.shutdown_rx.try_recv() {
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
        let stale =
            self.root.as_deref() != Some(root) || self.checked.is_none_or(|at| at.elapsed() >= Self::RECHECK_AFTER);
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
    wake: &Waker,
) {
    if !slot_matches(slot, desired.as_ref()) {
        retire_slot(std::mem::take(slot), retiring);
    }
    if !matches!(slot, Slot::Empty) {
        poll_slot(slot, kind, errors);
        return;
    }
    let Some(config) = desired else { return };
    match start_session(kind, config.clone(), wake) {
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
                    let Slot::Starting(running) = running else {
                        unreachable!()
                    };
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
    let Slot::Ready {
        session,
        open_documents,
        ..
    } = slot
    else {
        return;
    };
    let current: HashSet<PathBuf> = documents
        .iter()
        .filter(|doc| doc.language == Some(kind.language()))
        .map(|doc| doc.path.clone())
        .collect();
    let closed: Vec<PathBuf> = open_documents
        .keys()
        .filter(|path| !current.contains(*path))
        .cloned()
        .collect();
    for path in closed {
        match file_uri(&path) {
            Ok(uri) => {
                let params = DidCloseTextDocumentParams {
                    text_document: TextDocumentIdentifier { uri },
                };
                if let Err(error) =
                    session.send_notification(DidCloseTextDocument::METHOD, serde_json::to_value(params).unwrap())
                {
                    errors.push(format!(
                        "failed to close {kind_name} LSP document: {error}",
                        kind_name = kind.name()
                    ));
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
    let Slot::Ready {
        session,
        open_documents,
        sync_kind,
        ..
    } = slot
    else {
        return true;
    };
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
                vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: new_text.clone(),
                }]
            };
            let params = DidChangeTextDocumentParams {
                text_document: lsp_types::VersionedTextDocumentIdentifier {
                    uri,
                    version: doc.lsp_version,
                },
                content_changes,
            };
            (
                session.send_notification(DidChangeTextDocument::METHOD, serde_json::to_value(params).unwrap()),
                new_text,
            )
        }
        None => {
            let language_id = match kind {
                ServerKind::Java => "java",
                ServerKind::Kotlin => "kotlin",
            }
            .to_string();
            let text = doc.buffer.to_string();
            let params = DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id,
                    version: doc.lsp_version,
                    text: text.clone(),
                },
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
            errors.push(format!(
                "failed to synchronize {} with {}: {error}",
                doc.path.display(),
                kind.name()
            ));
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
    let sync = initialize_result
        .get("capabilities")
        .and_then(|c| c.get("textDocumentSync"));
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

    let mut prefix = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
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
    while suffix > 0 && (!old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix)) {
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
    let Slot::Ready {
        session,
        status_message,
        ..
    } = slot
    else {
        return;
    };
    for (method, params) in session.poll_server_messages() {
        match method.as_str() {
            m if m == PublishDiagnostics::METHOD => {
                let Ok(published) = serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(params) else {
                    errors.push("language server sent malformed publishDiagnostics params".to_string());
                    continue;
                };
                let Some(doc) = documents
                    .iter_mut()
                    .find(|doc| file_uri(&doc.path).ok().as_ref() == Some(&published.uri))
                else {
                    continue;
                };
                if published.version.is_some_and(|version| version < doc.lsp_version) {
                    continue;
                }
                let text = doc.buffer.to_string();
                doc.lsp_diagnostics = published
                    .diagnostics
                    .into_iter()
                    .filter_map(|diagnostic| {
                        utf16_range_to_bytes(&text, diagnostic.range).map(|range| Diagnostic {
                            range,
                            severity: match diagnostic.severity {
                                Some(lsp_types::DiagnosticSeverity::ERROR) => Severity::Error,
                                _ => Severity::Warning,
                            },
                            message: diagnostic.message,
                        })
                    })
                    .collect();
            }
            "language/status" => {
                let Ok(status) = serde_json::from_value::<LanguageStatusParams>(params) else {
                    continue;
                };
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
        let line = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .strip_suffix('\r')
            .unwrap_or(line);
        let mut units = 0u32;
        for (byte, character) in line.char_indices() {
            if units == position.character {
                return Some(byte);
            }
            units += character.len_utf16() as u32;
            if units > position.character {
                return None;
            }
        }
        (units == position.character).then_some(line.len())
    };
    let line_start = |line_number: u32| {
        lines
            .iter()
            .take(line_number as usize)
            .map(|line| line.len())
            .sum::<usize>()
    };
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
    let character = text[line_start_byte..byte_offset]
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum();
    lsp_types::Position { line, character }
}

fn retire_slot(slot: Slot, retiring: &mut Vec<RetiringSession>) {
    match slot {
        Slot::Ready { mut session, .. } => {
            if let Ok(shutdown_rx) = session.shutdown() {
                retiring.push(RetiringSession {
                    session,
                    shutdown_rx,
                    started: Instant::now(),
                });
            }
        }
        Slot::Starting(_) | Slot::Failed { .. } | Slot::Empty => {}
    }
}

fn start_session(
    kind: ServerKind,
    config: SessionConfig,
    wake: &Waker,
) -> Result<RunningSession, String> {
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
    let mut session = LspSession::spawn(&config.binary, &args, Some(&config.root), Arc::clone(wake)).map_err(|error| {
        format!(
            "failed to start {} at {}: {error}",
            kind.name(),
            config.binary.display()
        )
    })?;
    let params = initialize_params(kind, &config, &debug_bundles)?;
    let initialize_rx = session
        .initialize(params)
        .map_err(|error| format!("failed to initialize {}: {error}", kind.name()))?;
    Ok(RunningSession {
        config,
        session,
        initialize_rx,
    })
}

fn initialize_params(
    kind: ServerKind,
    config: &SessionConfig,
    debug_bundles: &[String],
) -> Result<InitializeParams, String> {
    let root = config.root.as_path();
    let uri = file_uri(root)?;
    #[allow(deprecated)]
    let params = InitializeParams {
        process_id: Some(std::process::id()),
        root_uri: Some(uri.clone()),
        workspace_folders: Some(vec![WorkspaceFolder {
            uri,
            name: root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("FoxGarden project")
                .to_string(),
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
                    completion_item: Some(lsp_types::CompletionItemCapability {
                        snippet_support: Some(false),
                        ..Default::default()
                    }),
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
        client_info: Some(lsp_types::ClientInfo {
            name: "FoxGarden".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
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
static FILE_URI_CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, Uri>>> =
    std::sync::OnceLock::new();

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
    let text = path
        .to_str()
        .ok_or_else(|| format!("LSP project root is not UTF-8: {}", path.display()))?;
    let normalized = if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.to_string()
    };
    let encoded: String = normalized
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'.' | b'_' | b'~' | b':' => vec![byte as char],
            other => format!("%{other:02X}").chars().collect(),
        })
        .collect();
    let uri = if cfg!(windows) {
        format!("file:///{encoded}")
    } else {
        format!("file://{encoded}")
    };
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
    let rest = if cfg!(windows) {
        rest.strip_prefix('/').unwrap_or(rest)
    } else {
        rest
    };
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
#[path = "lsp_state_test.rs"]
mod lsp_state_test;
