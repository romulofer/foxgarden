//! `PLAN.md` Track 20 Phase 1: the LSP client core — child-process
//! management for an external language server (`jdtls`/`kotlin-language-
//! server`) plus the JSON-RPC-over-stdio framing read/write loop, and the
//! `initialize`/`initialized` handshake. No user-visible feature yet (per
//! this phase's own scope) — later phases (diagnostics, hover, go-to-
//! definition, ...) build on `LspSession::send_request`/`send_notification`/
//! `poll_server_messages` without needing to touch this module's own
//! framing or process-lifecycle code again.
//!
//! Lives in `crates/app`, not `fg-core`, for the same reason `pty_session`
//! does: a live child process and its background reader thread aren't
//! headlessly unit-testable the way `fg-core`'s own pure logic is meant to
//! stay (`AGENTS.md`). Every request is fire-and-forget from the caller's
//! side, mirroring `static_analysis`'s own `spawn_scan`/`poll_scan` shape —
//! `send_request` never blocks, handing back a `Receiver` the caller polls
//! once a frame like every other background op in this app already does,
//! so a slow (or hung) language server never stalls the editor itself.
//!
//! `#![allow(dead_code)]`: genuinely no call site outside this module's own
//! tests yet — by this phase's own design, not an oversight. Phase 2
//! (diagnostics) is the first real caller; remove this once it lands.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use serde_json::Value;

/// A JSON-RPC error object (`{"code": ..., "message": ...}`) — the `Err`
/// half of a request's eventual result.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
}

/// One message read off the server's stdout, already classified enough to
/// route it.
#[derive(Debug, PartialEq)]
enum IncomingMessage {
    /// A response to a request *this* client sent, correlated by `id`.
    Response { id: i64, result: Result<Value, ResponseError> },
    /// A notification (or request) the server itself originated — Phase 1
    /// has no handler for any of these yet (`textDocument/
    /// publishDiagnostics` is Phase 2's job), just the plumbing to not
    /// drop them on the floor unrouted.
    ServerMessage { method: String, params: Value },
    /// Neither of the above (missing both `id`+result/error and `method`)
    /// — not a message shape this client expects; dropped by the reader
    /// loop rather than panicking on a malformed/unexpected server message.
    Unroutable,
}

/// Classifies one already-parsed JSON-RPC message. Pure/no I/O, directly
/// testable against hand-built `Value`s.
fn classify(value: Value) -> IncomingMessage {
    let Some(obj) = value.as_object() else { return IncomingMessage::Unroutable };
    if let Some(method) = obj.get("method").and_then(Value::as_str) {
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        return IncomingMessage::ServerMessage { method: method.to_string(), params };
    }
    if let Some(id) = obj.get("id").and_then(Value::as_i64) {
        if let Some(error) = obj.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
            let message = error.get("message").and_then(Value::as_str).unwrap_or("").to_string();
            return IncomingMessage::Response { id, result: Err(ResponseError { code, message }) };
        }
        let result = obj.get("result").cloned().unwrap_or(Value::Null);
        return IncomingMessage::Response { id, result: Ok(result) };
    }
    IncomingMessage::Unroutable
}

/// Reads one `Content-Length`-framed JSON-RPC message off `reader` — the
/// LSP wire format itself (a small header block, a blank line, then
/// exactly `Content-Length` bytes of UTF-8 JSON; any header other than
/// `Content-Length` — `Content-Type`, in practice — is real but irrelevant
/// here, so skipped rather than rejected). `Ok(None)` on a clean EOF
/// before any header line is read at all (the server process exited
/// between messages); an EOF *mid*-message is a real `Err`, not silently
/// treated the same as a clean one.
fn read_message<R: BufRead>(reader: &mut R) -> std::io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            return if content_length.is_none() {
                Ok(None)
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF mid-header"))
            };
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // blank line: end of headers
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse()
                    .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "malformed Content-Length header"))?,
            );
        }
    }
    let content_length =
        content_length.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length header"))?;
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    let value = serde_json::from_slice(&body).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(value))
}

/// Writes one `Content-Length`-framed JSON-RPC message — the write half of
/// `read_message`'s own format. Header and body are concatenated into one
/// buffer and written via a single `write_all` call, deliberately not two
/// separate writes (a header `write!` followed by a body `write_all`) —
/// a real, observed flake this session (a test's fake server process,
/// which exits right after its first response with no long-running real
/// server behind it, occasionally raced ahead and closed its own stdin
/// between the two writes, a real `BrokenPipe` under heavy parallel `cargo
/// test` load) confirmed two syscalls leaves a real window a fast-exiting
/// reader can close mid-message; one call removes it.
fn write_message<W: Write>(writer: &mut W, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value)?;
    let mut framed = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    framed.extend_from_slice(&body);
    writer.write_all(&framed)?;
    writer.flush()
}

type PendingResponses = Arc<Mutex<HashMap<i64, Sender<Result<Value, ResponseError>>>>>;

/// A running language server: its background writer thread's send half,
/// the child process itself, and the background reader thread's parsed
/// messages routed either to a pending request's own one-shot `Receiver`
/// (a `Response`) or to `server_messages_rx` (a notification/request the
/// server originated).
pub struct LspSession {
    child: Child,
    writer_tx: Sender<Value>,
    next_id: i64,
    pending: PendingResponses,
    server_messages_rx: Receiver<(String, Value)>,
}

impl LspSession {
    /// Spawns `binary` (`args`, `cwd` if given — the project root) as a
    /// language server and starts its background reader *and* writer
    /// threads. The server's own stderr is inherited (not piped) — `jdtls`/
    /// `kotlin-language-server` both log real diagnostics there, and this
    /// phase has no UI to surface it through yet (`PLAN.md` Phase 1's own
    /// "inspectable via logging" checkpoint wording), so letting it flow
    /// straight to this app's own stderr is the simplest way to actually
    /// inspect it.
    pub fn spawn(binary: &Path, args: &[String], cwd: Option<&Path>) -> std::io::Result<Self> {
        let mut command = Command::new(binary);
        command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut child = command.spawn()?;
        let mut stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");

        let pending: PendingResponses = Arc::new(Mutex::new(HashMap::new()));
        let (server_tx, server_messages_rx) = mpsc::channel();
        let pending_for_thread = Arc::clone(&pending);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader) {
                    Ok(Some(value)) => match classify(value) {
                        IncomingMessage::Response { id, result } => {
                            if let Some(tx) = pending_for_thread.lock().unwrap().remove(&id) {
                                let _ = tx.send(result);
                            }
                        }
                        IncomingMessage::ServerMessage { method, params } => {
                            if server_tx.send((method, params)).is_err() {
                                break; // LspSession (and its Receiver) dropped
                            }
                        }
                        IncomingMessage::Unroutable => {}
                    },
                    Ok(None) => break, // server exited, stdout closed
                    Err(_) => break,
                }
            }
        });

        // A dedicated writer thread, mirroring the reader thread above, is
        // what actually makes `send_request`/`send_notification`'s own
        // "never blocks" doc comment true rather than aspirational: a
        // *single* small JSON-RPC message essentially never fills the OS
        // pipe buffer on its own, but a server that stalls reading its own
        // stdin (busy indexing, or — the real case this was caught against,
        // Track 20 Phase 3's own hover live-verify — seemingly wedged
        // handling a `textDocument/hover` against a project with no
        // attached JDK sources) lets every subsequent full-buffer
        // `didChange` (sent on nearly every keystroke) queue up until the
        // buffer *does* fill, at which point a direct `write_all` on the
        // caller's own thread — the UI thread — blocks for as long as the
        // server stays stuck, freezing the whole editor. Routing every
        // write through this channel instead means the caller's `send`
        // only ever pushes onto an unbounded in-process queue (which
        // cannot block), while this thread absorbs whatever blocking the
        // real pipe write needs.
        let (writer_tx, writer_rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            for message in writer_rx {
                if write_message(&mut stdin, &message).is_err() {
                    break; // server exited or its stdin pipe broke
                }
            }
        });

        Ok(Self { child, writer_tx, next_id: 0, pending, server_messages_rx })
    }

    /// Sends a JSON-RPC request, returning a `Receiver` for its eventual
    /// response — never blocks (queues onto the writer thread's channel
    /// rather than writing to the pipe directly — see `spawn`'s own doc
    /// comment on why that distinction matters), so a caller polls the
    /// returned `Receiver` from wherever it already polls everything else.
    pub fn send_request(&mut self, method: &str, params: Value) -> std::io::Result<Receiver<Result<Value, ResponseError>>> {
        let id = self.next_id;
        self.next_id += 1;
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let message = serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if self.writer_tx.send(message).is_err() {
            // The writer thread only ever exits after a real write failure
            // (spawn's own loop) — the server is already dead or dying.
            // Leaving this request's sender in `pending` would turn that
            // into an unbounded leak for any later caller that retries.
            self.pending.lock().unwrap().remove(&id);
            return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "language server's writer thread has exited"));
        }
        Ok(rx)
    }

    /// Sends a JSON-RPC notification — no response expected, ever (per the
    /// protocol itself), so no `Receiver` to hand back.
    pub fn send_notification(&mut self, method: &str, params: Value) -> std::io::Result<()> {
        let message = serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.writer_tx
            .send(message)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "language server's writer thread has exited"))
    }

    /// The `initialize` request, via `lsp_types::request::Initialize`'s own
    /// `METHOD`/`Params`/`Result` types rather than a hand-typed method
    /// string and a bare `Value` — a typo or a wrong-shaped params object
    /// is a compile error this way, not a real server silently rejecting
    /// the handshake at runtime. The first request any real language
    /// server expects.
    pub fn initialize(&mut self, params: lsp_types::InitializeParams) -> std::io::Result<Receiver<Result<Value, ResponseError>>> {
        let params = serde_json::to_value(params).expect("InitializeParams always serializes");
        self.send_request(lsp_types::request::Initialize::METHOD, params)
    }

    /// The `initialized` notification — sent once, right after the
    /// `initialize` response arrives, completing the handshake (`PLAN.md`
    /// Phase 1's own scope). No other request/notification is safe to send
    /// before this one, per the LSP spec itself.
    pub fn initialized(&mut self) -> std::io::Result<()> {
        let params = serde_json::to_value(lsp_types::InitializedParams {}).expect("InitializedParams always serializes");
        self.send_notification(lsp_types::notification::Initialized::METHOD, params)
    }

    /// Begins LSP's orderly shutdown sequence. The caller owns the returned
    /// receiver and must wait for the successful `shutdown` response before
    /// calling [`Self::exit`]; doing both here would make `exit` race ahead
    /// of the server's response and violate the protocol.
    pub fn shutdown(&mut self) -> std::io::Result<Receiver<Result<Value, ResponseError>>> {
        self.send_request("shutdown", Value::Null)
    }

    /// The final notification in an orderly shutdown, sent only after the
    /// receiver from [`Self::shutdown`] has completed.
    pub fn exit(&mut self) -> std::io::Result<()> {
        self.send_notification("exit", Value::Null)
    }

    /// Whether the child process has exited. This is deliberately polled by
    /// the UI-thread owner rather than waited on: a stalled language server
    /// must never hold up input or painting.
    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    /// Drains every notification/request the server itself has sent since
    /// the last poll — called once a frame, same shape as every other
    /// background op this app already polls. No client-side handler exists
    /// for any of these yet (that's Phase 2 onward's job); this just keeps
    /// them from piling up unread in the channel.
    pub fn poll_server_messages(&self) -> Vec<(String, Value)> {
        self.server_messages_rx.try_iter().collect()
    }
}

impl Drop for LspSession {
    /// Closing a session kills the real server process outright — no
    /// graceful `shutdown`/`exit` handshake yet (`lsp_state::LspState` owns
    /// that sequence for its own long-lived sessions before ever dropping
    /// one). `wait()` after `kill()` is mandatory, not optional cleanup: the
    /// standard library never reaps a child on its own, so a killed-but-
    /// unwaited process stays a zombie in the process table until this
    /// app's own exit — real cost given `lsp_state::LspState` spawns/kills
    /// sessions repeatedly over one run (settings changes, doc close/
    /// reopen, crash-restarts). `wait()` returns essentially immediately
    /// after a `kill()`, so this stays effectively non-blocking in
    /// practice.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn framed(body: &str) -> Vec<u8> {
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
    }

    #[test]
    fn read_message_parses_a_single_framed_message() {
        let bytes = framed(r#"{"jsonrpc":"2.0","id":0,"result":{}}"#);
        let mut reader = BufReader::new(Cursor::new(bytes));
        let value = read_message(&mut reader).unwrap().unwrap();
        assert_eq!(value["id"], 0);
    }

    #[test]
    fn read_message_skips_an_unrelated_header() {
        let body = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
        let bytes = format!("Content-Type: application/vscode-jsonrpc\r\nContent-Length: {}\r\n\r\n{}", body.len(), body)
            .into_bytes();
        let mut reader = BufReader::new(Cursor::new(bytes));
        let value = read_message(&mut reader).unwrap().unwrap();
        assert_eq!(value["method"], "initialized");
    }

    #[test]
    fn read_message_reads_two_consecutive_messages_off_the_same_stream() {
        let mut bytes = framed(r#"{"jsonrpc":"2.0","id":0,"result":1}"#);
        bytes.extend(framed(r#"{"jsonrpc":"2.0","id":1,"result":2}"#));
        let mut reader = BufReader::new(Cursor::new(bytes));
        assert_eq!(read_message(&mut reader).unwrap().unwrap()["id"], 0);
        assert_eq!(read_message(&mut reader).unwrap().unwrap()["id"], 1);
    }

    #[test]
    fn read_message_on_a_clean_eof_before_any_header_is_none_not_an_error() {
        let mut reader = BufReader::new(Cursor::new(Vec::<u8>::new()));
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn read_message_eof_mid_header_is_a_real_error() {
        let mut reader = BufReader::new(Cursor::new(b"Content-Length: 10\r\n".to_vec()));
        assert!(read_message(&mut reader).is_err());
    }

    #[test]
    fn read_message_missing_content_length_is_a_real_error() {
        let mut reader = BufReader::new(Cursor::new(b"Content-Type: application/json\r\n\r\n{}".to_vec()));
        assert!(read_message(&mut reader).is_err());
    }

    #[test]
    fn write_message_produces_the_exact_wire_format() {
        let mut out = Vec::new();
        write_message(&mut out, &serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {}})).unwrap();
        let body = r#"{"id":0,"jsonrpc":"2.0","method":"initialize","params":{}}"#;
        assert_eq!(out, framed(body));
    }

    #[test]
    fn write_then_read_round_trips_through_the_same_framing() {
        let original = serde_json::json!({"jsonrpc": "2.0", "id": 7, "method": "foo", "params": {"x": 1}});
        let mut buf = Vec::new();
        write_message(&mut buf, &original).unwrap();
        let mut reader = BufReader::new(Cursor::new(buf));
        assert_eq!(read_message(&mut reader).unwrap().unwrap(), original);
    }

    #[test]
    fn classify_reports_a_successful_response() {
        let value = serde_json::json!({"jsonrpc": "2.0", "id": 3, "result": {"ok": true}});
        assert_eq!(classify(value), IncomingMessage::Response { id: 3, result: Ok(serde_json::json!({"ok": true})) });
    }

    #[test]
    fn classify_reports_an_error_response() {
        let value = serde_json::json!({"jsonrpc": "2.0", "id": 3, "error": {"code": -32601, "message": "method not found"}});
        assert_eq!(
            classify(value),
            IncomingMessage::Response { id: 3, result: Err(ResponseError { code: -32601, message: "method not found".to_string() }) }
        );
    }

    #[test]
    fn classify_reports_a_server_notification() {
        let value = serde_json::json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": "file:///a"}});
        assert_eq!(
            classify(value),
            IncomingMessage::ServerMessage {
                method: "textDocument/publishDiagnostics".to_string(),
                params: serde_json::json!({"uri": "file:///a"})
            }
        );
    }

    #[test]
    fn classify_reports_unroutable_for_neither_shape() {
        assert_eq!(classify(serde_json::json!({"jsonrpc": "2.0"})), IncomingMessage::Unroutable);
        assert_eq!(classify(serde_json::json!("not even an object")), IncomingMessage::Unroutable);
    }

    /// The one real fake-server-*process* test this phase's own checkpoint
    /// asks for: a genuine child process (`sh -c`, universally available on
    /// this platform — no compiled test-fixture binary needed just to emit
    /// one canned reply), immediately writing a real `Content-Length`-framed
    /// response with `id: 0` and exiting, real stdio pipes end to end. Proves
    /// `LspSession::spawn`/`send_request`'s actual process/thread/channel
    /// wiring, not just the pure `read_message`/`write_message`/`classify`
    /// logic the tests above already cover in isolation.
    fn fake_server_returning(body: &str) -> LspSession {
        // `printf`s its one canned reply immediately, then blocks
        // (`cat > /dev/null`, discarding whatever it reads) instead of
        // exiting — a real, observed flake this session: a script that
        // exits right after `printf` can race a `send_request`/
        // `initialize` write that hasn't happened yet, closing its own
        // stdin first (`write_message`'s own doc comment, real `BrokenPipe`
        // under heavy parallel `cargo test` load), and a test that never
        // writes anything at all (`poll_server_messages`'s own notification
        // test) needs this script to reply without waiting on input in the
        // first place, ruling out gating the reply on a `read`. Staying
        // alive keeps this process' stdin pipe valid regardless of *when*
        // (or whether) the client ever writes to it; `LspSession::drop`
        // kills it once the test's own session goes out of scope.
        let script = format!("printf 'Content-Length: %d\\r\\n\\r\\n%s' {} '{body}'; cat > /dev/null", body.len());
        LspSession::spawn(Path::new("sh"), &["-c".to_string(), script], None).expect("sh is always available")
    }

    #[test]
    fn send_request_against_a_real_fake_server_process_receives_its_response() {
        let mut session = fake_server_returning(r#"{"jsonrpc":"2.0","id":0,"result":{"capabilities":{}}}"#);
        let rx = session.send_request("initialize", serde_json::json!({})).unwrap();
        let result = rx.recv().expect("the fake server's response arrives");
        assert_eq!(result, Ok(serde_json::json!({"capabilities": {}})));
    }

    #[test]
    fn send_request_against_a_real_fake_server_process_receives_its_error() {
        let mut session =
            fake_server_returning(r#"{"jsonrpc":"2.0","id":0,"error":{"code":-32601,"message":"method not found"}}"#);
        let rx = session.send_request("bogusMethod", serde_json::json!({})).unwrap();
        let result = rx.recv().expect("the fake server's response arrives");
        assert_eq!(result, Err(ResponseError { code: -32601, message: "method not found".to_string() }));
    }

    #[test]
    fn poll_server_messages_receives_a_real_notification_from_a_real_process() {
        let session = fake_server_returning(
            r#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"file:///a.java"}}"#,
        );
        // The notification has no `id`/request of its own to await a
        // response for — poll until it shows up, same "loop until the
        // background thread's result lands" shape every other real-process
        // test in this codebase already uses (e.g. `panels::git_stage`'s).
        loop {
            let messages = session.poll_server_messages();
            if let Some((method, params)) = messages.into_iter().next() {
                assert_eq!(method, "textDocument/publishDiagnostics");
                assert_eq!(params, serde_json::json!({"uri": "file:///a.java"}));
                break;
            }
        }
    }

    #[test]
    fn initialize_sends_the_real_initialize_method_name() {
        // Verified via a real fake server that only needs to reply once,
        // id 0 — `initialize` is always this session's very first request,
        // so it's always id 0; this exercises `initialize`'s own
        // `lsp_types`-typed params actually serializing and reaching the
        // wire correctly, not just `send_request`'s generic path (already
        // covered above).
        let mut session = fake_server_returning(r#"{"jsonrpc":"2.0","id":0,"result":{"capabilities":{}}}"#);
        let params = lsp_types::InitializeParams::default();
        let rx = session.initialize(params).unwrap();
        assert!(rx.recv().expect("the fake server's response arrives").is_ok());
    }
}
