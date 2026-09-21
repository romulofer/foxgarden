//! `PLAN.md` Track 23 Phase 1: the DAP (Debug Adapter Protocol) client core.
//!
//! DAP is a real *second* protocol, not a thin skin over LSP (`SPEC.md`
//! §23) — its own message shape (`type`: `request`/`response`/`event`,
//! correlated by `seq`/`request_seq` rather than JSON-RPC's `method`/`id`),
//! so `classify`/`IncomingMessage` here are new, not reused from
//! `lsp_client`. What genuinely *is* shared: the wire framing itself — DAP
//! specifies the exact same `Content-Length`-header-then-JSON-body shape LSP
//! uses, so `lsp_client::read_message`/`write_message` (already generic over
//! `serde_json::Value`, no LSP-specific parsing inside either) are reused
//! verbatim rather than reimplemented — the "shared framing code with Track
//! 20's LSP client where genuinely reusable" `PLAN.md` itself calls for.
//!
//! The other real difference from `lsp_client::LspSession`: there is no
//! child process here. `java-debug` (`com.microsoft.java.debug.plugin`) runs
//! *inside* the already-running jdt.ls JVM as a loaded OSGi bundle; a
//! `workspace/executeCommand` call for `vscode.java.startDebugSession`
//! (`lsp_state`) is what actually starts its DAP server and hands back the
//! TCP port to connect to — verified concretely against the real
//! `vscode-java-debug` extension's own bundled client code (`extension.js`'s
//! `startDebugSession`/`DebugAdapterServer`, not assumed), per `SPEC.md`
//! §23's own explicit caution not to assume protocol details unverified.
//! `DapSession::connect` therefore takes a `port`, not a `binary`+`args`.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::lsp_client::{read_message, write_message};

/// One message read off the adapter's socket, already classified enough to
/// route it. Unlike LSP's `id` (present on every request/response), DAP
/// gives every message a `seq` but only a *response* carries `request_seq`
/// (the `seq` of the request it answers) — that's the correlation key.
#[derive(Debug, PartialEq)]
enum IncomingMessage {
    /// A response to a request this client sent, correlated by
    /// `request_seq`. `success: false` carries `message` (a human-readable
    /// failure reason, per the DAP spec) rather than a structured error
    /// object the way LSP's `error.code` does.
    Response {
        request_seq: i64,
        success: bool,
        body: Value,
        message: Option<String>,
    },
    /// An event the adapter originated on its own (`initialized`, `output`,
    /// `stopped`, `terminated`, ...) — Phase 1 has no handler for any
    /// specific one yet, just the plumbing to not drop them unrouted; later
    /// phases (breakpoints/stepping, Phase 2; variables/call stack, Phase 3)
    /// read these off `poll_events`.
    Event { event: String, body: Value },
    /// A reverse request (adapter-to-client, e.g. `runInTerminal`) — real
    /// per the DAP spec, but java-debug's own default launch console
    /// (`internalConsole`) never sends one, so Phase 1 has no reply logic
    /// for this shape yet, same "plumbing without a handler" scope
    /// `lsp_client`'s own Phase 1 held itself to for server-to-client
    /// notifications it didn't yet need to act on.
    ReverseRequest {
        seq: i64,
        command: String,
        arguments: Value,
    },
    /// Not a message shape this client expects — dropped by the reader loop
    /// rather than panicking on a malformed/unexpected adapter message.
    Unroutable,
}

/// Classifies one already-parsed DAP message. Pure/no I/O, directly
/// testable against hand-built `Value`s — mirrors `lsp_client::classify`'s
/// own shape for the same reason: keep the protocol-shape decision testable
/// without a real socket.
fn classify(value: Value) -> IncomingMessage {
    let Some(obj) = value.as_object() else {
        return IncomingMessage::Unroutable;
    };
    match obj.get("type").and_then(Value::as_str) {
        Some("response") => {
            let Some(request_seq) = obj.get("request_seq").and_then(Value::as_i64) else {
                return IncomingMessage::Unroutable;
            };
            let success = obj.get("success").and_then(Value::as_bool).unwrap_or(false);
            let body = obj.get("body").cloned().unwrap_or(Value::Null);
            let message = obj.get("message").and_then(Value::as_str).map(str::to_string);
            IncomingMessage::Response {
                request_seq,
                success,
                body,
                message,
            }
        }
        Some("event") => {
            let Some(event) = obj.get("event").and_then(Value::as_str) else {
                return IncomingMessage::Unroutable;
            };
            let body = obj.get("body").cloned().unwrap_or(Value::Null);
            IncomingMessage::Event {
                event: event.to_string(),
                body,
            }
        }
        Some("request") => {
            let (Some(seq), Some(command)) = (
                obj.get("seq").and_then(Value::as_i64),
                obj.get("command").and_then(Value::as_str),
            ) else {
                return IncomingMessage::Unroutable;
            };
            let arguments = obj.get("arguments").cloned().unwrap_or(Value::Null);
            IncomingMessage::ReverseRequest {
                seq,
                command: command.to_string(),
                arguments,
            }
        }
        _ => IncomingMessage::Unroutable,
    }
}

/// The outcome of one request this client sent — `Ok` carries the
/// response's own `body` (the adapter's `success: false` `message`, when
/// present, becomes the `Err` string; a bare `Err("<command> failed")`
/// fallback when the adapter didn't explain).
pub type DapResult = Result<Value, String>;

type PendingResponses = Arc<Mutex<HashMap<i64, Sender<DapResult>>>>;

/// A running debug-adapter connection: the background writer thread's send
/// half, plus the background reader thread's parsed messages routed either
/// to a pending request's own one-shot `Receiver` (a `Response`) or to
/// `events_rx` (an `Event`) — the same split `lsp_client::LspSession`
/// already uses for its own response/server-message routing.
pub struct DapSession {
    stream: TcpStream,
    writer_tx: Sender<Value>,
    next_seq: i64,
    pending: PendingResponses,
    events_rx: Receiver<(String, Value)>,
}

impl DapSession {
    /// Connects to `java-debug`'s DAP server at `127.0.0.1:<port>` — the
    /// port `lsp_state`'s `vscode.java.startDebugSession` call hands back —
    /// and starts the background reader/writer threads. No handshake is
    /// sent here (the DAP `initialize`/`launch`/`configurationDone`
    /// sequence is a *caller* concern, `debug_state`'s own job, mirroring
    /// `LspSession::spawn` leaving `initialize`/`initialized` to its own
    /// caller too).
    pub fn connect(port: u16) -> std::io::Result<Self> {
        let stream = TcpStream::connect(("127.0.0.1", port))?;
        let read_stream = stream.try_clone()?;
        let mut write_stream = stream.try_clone()?;

        let pending: PendingResponses = Arc::new(Mutex::new(HashMap::new()));
        let (events_tx, events_rx) = mpsc::channel();
        let pending_for_thread = Arc::clone(&pending);

        std::thread::spawn(move || {
            let mut reader = BufReader::new(read_stream);
            loop {
                match read_message(&mut reader) {
                    Ok(Some(value)) => match classify(value) {
                        IncomingMessage::Response {
                            request_seq,
                            success,
                            body,
                            message,
                        } => {
                            if let Some(tx) = pending_for_thread.lock().unwrap().remove(&request_seq) {
                                let result = if success {
                                    Ok(body)
                                } else {
                                    Err(message.unwrap_or_else(|| "request failed".to_string()))
                                };
                                let _ = tx.send(result);
                            }
                        }
                        IncomingMessage::Event { event, body } => {
                            if events_tx.send((event, body)).is_err() {
                                break; // DapSession (and its Receiver) dropped
                            }
                        }
                        // No reply logic yet (this struct's own doc comment) —
                        // dropped rather than left to stall a real adapter
                        // waiting on one, same trade-off as `Unroutable`.
                        IncomingMessage::ReverseRequest { .. } | IncomingMessage::Unroutable => {}
                    },
                    Ok(None) => break, // adapter closed the socket
                    Err(_) => break,
                }
            }
        });

        let (writer_tx, writer_rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            for message in writer_rx {
                if write_message(&mut write_stream, &message).is_err() {
                    break; // adapter closed the socket or the write failed
                }
            }
        });

        Ok(Self {
            stream,
            writer_tx,
            next_seq: 1,
            pending,
            events_rx,
        })
    }

    /// Sends a DAP request, returning a `Receiver` for its eventual
    /// response — never blocks (queues onto the writer thread's channel
    /// exactly like `LspSession::send_request`, and for the identical
    /// reason: a stalled adapter must never freeze the caller's thread).
    pub fn send_request(&mut self, command: &str, arguments: Value) -> std::io::Result<Receiver<DapResult>> {
        let seq = self.next_seq;
        self.next_seq += 1;
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(seq, tx);
        let message = serde_json::json!({ "seq": seq, "type": "request", "command": command, "arguments": arguments });
        if self.writer_tx.send(message).is_err() {
            self.pending.lock().unwrap().remove(&seq);
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "debug adapter's writer thread has exited",
            ));
        }
        Ok(rx)
    }

    /// Drains every event the adapter has sent since the last poll — called
    /// once a frame, same shape `LspSession::poll_server_messages` already
    /// uses for its own per-frame drain.
    pub fn poll_events(&self) -> Vec<(String, Value)> {
        self.events_rx.try_iter().collect()
    }
}

impl Drop for DapSession {
    /// Closing a session shuts the socket down outright — no orderly
    /// `disconnect`/`terminate` DAP request here (`debug_state` owns that
    /// sequence for a session it's deliberately ending before ever dropping
    /// one, mirroring `lsp_state::LspState`'s own shutdown-before-drop
    /// discipline for `LspSession`).
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(test)]
#[path = "dap_client_test.rs"]
mod dap_client_test;
