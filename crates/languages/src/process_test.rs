#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use super::*;

/// A program that is `body` run by `sh`, in a fresh directory.
fn script(body: &str) -> (tempfile::TempDir, ProcessCommand) {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("ext.sh");
    std::fs::write(&program, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let command = ProcessCommand {
        program,
        args: Vec::new(),
        dir: dir.path().to_path_buf(),
    };
    (dir, command)
}

#[test]
fn a_request_carries_the_protocol_version_method_and_params() {
    let (_dir, command) = script(
        r#"read line
case "$line" in
  *'"protocol":1'*'"method":"ping"'*'"params":{"n":7}'*) echo '{"result":"pong"}' ;;
  *) echo '{"error":"bad envelope"}' ;;
esac"#,
    );
    let reply: String = command.call("ping", &serde_json::json!({ "n": 7 })).unwrap();
    assert_eq!(reply, "pong");
}

#[test]
fn an_error_reply_is_the_extensions_own_message() {
    let (_dir, command) = script(r#"read line; echo '{"error":"tool not configured"}'"#);
    let reply: Result<String, String> = command.call("ping", &serde_json::json!({}));
    assert_eq!(reply, Err("tool not configured".to_string()));
}

#[test]
fn a_failing_program_reports_its_stderr() {
    let (_dir, command) = script(r#"echo 'boom: no config' >&2; exit 3"#);
    let error = command.call::<_, String>("ping", &serde_json::json!({})).unwrap_err();
    assert!(error.contains("boom: no config"), "{error}");
}

#[test]
fn an_unreadable_or_empty_reply_is_an_error_not_a_guess() {
    let (_dir, garbage) = script(r#"read line; echo 'not json'"#);
    assert!(garbage.call::<_, String>("ping", &serde_json::json!({})).unwrap_err().contains("unreadable reply"));

    let (_dir, empty) = script(r#"read line; echo '{}'"#);
    assert!(empty.call::<_, String>("ping", &serde_json::json!({})).unwrap_err().contains("neither"));
}

#[test]
fn a_program_that_cannot_start_is_reported() {
    let command = ProcessCommand {
        program: PathBuf::from("/nonexistent/ext"),
        args: Vec::new(),
        dir: PathBuf::from("/"),
    };
    assert!(command.call::<_, String>("ping", &serde_json::json!({})).unwrap_err().contains("could not start"));
}

#[test]
fn the_program_runs_in_the_extension_directory() {
    let (dir, command) = script(r#"read line; printf '{"result":"%s"}\n' "$(pwd)""#);
    let cwd: String = command.call("where", &serde_json::json!({})).unwrap();
    assert_eq!(std::fs::canonicalize(cwd).unwrap(), std::fs::canonicalize(dir.path()).unwrap());
}

#[test]
fn a_path_leaving_the_directory_is_refused() {
    assert!(inside(Path::new("/ext"), Path::new("../x")).is_err());
    assert!(inside(Path::new("/ext"), Path::new("/bin/sh")).is_err());
    assert_eq!(inside(Path::new("/ext"), Path::new("bin/x")).unwrap(), PathBuf::from("/ext/bin/x"));
}
