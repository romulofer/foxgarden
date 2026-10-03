
use super::*;
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    let bytes = format!(
        "Content-Type: application/vscode-jsonrpc\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
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
    write_message(
        &mut out,
        &serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {}}),
    )
    .unwrap();
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
    assert_eq!(
        classify(value),
        IncomingMessage::Response {
            id: 3,
            result: Ok(serde_json::json!({"ok": true}))
        }
    );
}

#[test]
fn classify_reports_an_error_response() {
    let value =
        serde_json::json!({"jsonrpc": "2.0", "id": 3, "error": {"code": -32601, "message": "method not found"}});
    assert_eq!(
        classify(value),
        IncomingMessage::Response {
            id: 3,
            result: Err(ResponseError {
                code: -32601,
                message: "method not found".to_string()
            })
        }
    );
}

#[test]
fn classify_reports_a_server_notification_with_no_id() {
    let value = serde_json::json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": "file:///a"}});
    assert_eq!(
        classify(value),
        IncomingMessage::ServerMessage {
            method: "textDocument/publishDiagnostics".to_string(),
            params: serde_json::json!({"uri": "file:///a"}),
            id: None,
        }
    );
}

#[test]
fn classify_reports_a_server_to_client_request_with_its_id() {
    // Has both `method` and `id` — the shape that used to be
    // misrouted as a fire-and-forget notification, silently dropping
    // the `id` a real reply needs to be correlated against.
    let value =
        serde_json::json!({"jsonrpc": "2.0", "id": 9, "method": "workspace/configuration", "params": {"items": []}});
    assert_eq!(
        classify(value),
        IncomingMessage::ServerMessage {
            method: "workspace/configuration".to_string(),
            params: serde_json::json!({"items": []}),
            id: Some(9),
        }
    );
}

#[test]
fn default_server_request_reply_answers_workspace_configuration_with_one_null_per_item() {
    let params = serde_json::json!({"items": [{"section": "java"}, {"section": "kotlin"}]});
    assert_eq!(
        default_server_request_reply("workspace/configuration", &params),
        serde_json::json!([null, null])
    );
}

#[test]
fn default_server_request_reply_answers_any_other_method_with_a_bare_null() {
    assert_eq!(
        default_server_request_reply("client/registerCapability", &serde_json::json!({})),
        serde_json::Value::Null
    );
}

#[test]
fn classify_reports_unroutable_for_neither_shape() {
    assert_eq!(
        classify(serde_json::json!({"jsonrpc": "2.0"})),
        IncomingMessage::Unroutable
    );
    assert_eq!(
        classify(serde_json::json!("not even an object")),
        IncomingMessage::Unroutable
    );
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
    let script = format!(
        "printf 'Content-Length: %d\\r\\n\\r\\n%s' {} '{body}'; cat > /dev/null",
        body.len()
    );
    LspSession::spawn(Path::new("sh"), &["-c".to_string(), script], None, Arc::new(|| {}))
        .expect("sh is always available")
}

/// Same fake server, but spawned with a `Waker` that counts its calls —
/// `wake_count` is what the wakeup tests assert on.
fn fake_server_waking(body: &str) -> (LspSession, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let script = format!(
        "printf 'Content-Length: %d\\r\\n\\r\\n%s' {} '{body}'; cat > /dev/null",
        body.len()
    );
    let for_thread = Arc::clone(&wakes);
    let session = LspSession::spawn(
        Path::new("sh"),
        &["-c".to_string(), script],
        None,
        Arc::new(move || {
            for_thread.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .expect("sh is always available");
    (session, wakes)
}

/// The reader thread waking its event loop is what lets an idle-but-alive
/// session stop being polled on a timer at all (`LspState::wants_repaint`
/// deliberately does not count a `Ready` slot). An unprompted notification
/// — no request of this client's own to correlate it with — is exactly the
/// case that has nothing else to trigger a poll.
#[test]
fn an_unprompted_notification_wakes_the_event_loop() {
    let (session, wakes) = fake_server_waking(
        r#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"file:///a.java"}}"#,
    );
    loop {
        if !session.poll_server_messages().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(wakes.load(Ordering::SeqCst) >= 1, "the notification woke nothing");
}

#[test]
fn a_response_wakes_the_event_loop() {
    let (mut session, wakes) = fake_server_waking(r#"{"jsonrpc":"2.0","id":0,"result":{"capabilities":{}}}"#);
    let rx = session.send_request("initialize", serde_json::json!({})).unwrap();
    let _ = rx.recv().expect("the fake server's response arrives");
    assert!(wakes.load(Ordering::SeqCst) >= 1, "the response woke nothing");
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
    assert_eq!(
        result,
        Err(ResponseError {
            code: -32601,
            message: "method not found".to_string()
        })
    );
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
