
use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

#[test]
fn classify_reports_a_successful_response() {
    let value = serde_json::json!({
        "seq": 2, "type": "response", "request_seq": 1, "success": true, "command": "initialize", "body": {"ok": true}
    });
    assert_eq!(
        classify(value),
        IncomingMessage::Response {
            request_seq: 1,
            success: true,
            body: serde_json::json!({"ok": true}),
            message: None
        }
    );
}

#[test]
fn classify_reports_a_failed_response_with_its_message() {
    let value = serde_json::json!({
        "seq": 2, "type": "response", "request_seq": 1, "success": false, "command": "launch", "message": "main class not found"
    });
    assert_eq!(
        classify(value),
        IncomingMessage::Response {
            request_seq: 1,
            success: false,
            body: Value::Null,
            message: Some("main class not found".to_string())
        }
    );
}

#[test]
fn classify_reports_an_event_with_no_request_seq_at_all() {
    let value = serde_json::json!({"seq": 5, "type": "event", "event": "initialized"});
    assert_eq!(
        classify(value),
        IncomingMessage::Event {
            event: "initialized".to_string(),
            body: Value::Null
        }
    );
}

#[test]
fn classify_reports_an_output_event_with_its_body() {
    let value = serde_json::json!({"seq": 6, "type": "event", "event": "output", "body": {"category": "stdout", "output": "hi\n"}});
    assert_eq!(
        classify(value),
        IncomingMessage::Event {
            event: "output".to_string(),
            body: serde_json::json!({"category": "stdout", "output": "hi\n"})
        }
    );
}

#[test]
fn classify_reports_a_reverse_request() {
    let value =
        serde_json::json!({"seq": 3, "type": "request", "command": "runInTerminal", "arguments": {"cwd": "/tmp"}});
    assert_eq!(
        classify(value),
        IncomingMessage::ReverseRequest {
            seq: 3,
            command: "runInTerminal".to_string(),
            arguments: serde_json::json!({"cwd": "/tmp"})
        }
    );
}

#[test]
fn classify_reports_unroutable_for_an_unknown_type() {
    assert_eq!(
        classify(serde_json::json!({"seq": 1, "type": "bogus"})),
        IncomingMessage::Unroutable
    );
    assert_eq!(
        classify(serde_json::json!("not even an object")),
        IncomingMessage::Unroutable
    );
}

/// A genuine `TcpListener` fake adapter, mirroring `lsp_client`'s own
/// "one real fake-server-process test" discipline — proves
/// `DapSession::connect`/`send_request`'s actual socket/thread/channel
/// wiring, not just `classify`'s pure logic already covered above.
fn fake_adapter_returning(body: &str) -> (DapSession, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binding a loopback port always succeeds");
    let port = listener.local_addr().unwrap().port();
    let framed = {
        let mut buf = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        buf.extend_from_slice(body.as_bytes());
        buf
    };
    let handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("the client connects");
        socket.write_all(&framed).expect("writing the canned reply");
        // Stays connected (discarding whatever the client writes)
        // rather than closing right after — the same real observed-flake
        // avoidance `lsp_client::tests::fake_server_returning` already
        // documents for its own script: closing immediately can race a
        // `send_request` write that hasn't happened on the wire yet.
        let mut sink = Vec::new();
        let _ = socket.read_to_end(&mut sink);
    });
    let session = DapSession::connect(port).expect("connecting to the fake adapter");
    (session, handle)
}

#[test]
fn send_request_against_a_real_fake_adapter_receives_its_response() {
    let (mut session, _handle) = fake_adapter_returning(
        r#"{"seq":1,"type":"response","request_seq":1,"success":true,"command":"initialize","body":{"supportsConfigurationDoneRequest":true}}"#,
    );
    let rx = session
        .send_request("initialize", serde_json::json!({"clientID": "foxgarden"}))
        .unwrap();
    let result = rx.recv().expect("the fake adapter's response arrives");
    assert_eq!(
        result,
        Ok(serde_json::json!({"supportsConfigurationDoneRequest": true}))
    );
}

#[test]
fn send_request_against_a_real_fake_adapter_receives_its_failure_message() {
    let (mut session, _handle) = fake_adapter_returning(
        r#"{"seq":1,"type":"response","request_seq":1,"success":false,"command":"launch","message":"main class not found"}"#,
    );
    let rx = session.send_request("launch", serde_json::json!({})).unwrap();
    let result = rx.recv().expect("the fake adapter's response arrives");
    assert_eq!(result, Err("main class not found".to_string()));
}

#[test]
fn poll_events_receives_a_real_event_from_a_real_socket() {
    let (session, _handle) = fake_adapter_returning(r#"{"seq":1,"type":"event","event":"initialized"}"#);
    loop {
        let events = session.poll_events();
        if let Some((event, body)) = events.into_iter().next() {
            assert_eq!(event, "initialized");
            assert_eq!(body, Value::Null);
            break;
        }
    }
}
