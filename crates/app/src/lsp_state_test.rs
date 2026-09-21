
use super::*;

/// A `Waker` for tests that don't assert on wakeups — nothing here runs a
/// UI event loop, so there is nothing to wake.
fn noop_waker() -> Waker {
    Arc::new(|| {})
}

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
    let settings = LspSettings {
        enabled: true,
        jdtls_binary: "jdtls".to_string(),
        ..Default::default()
    };
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
    let settings = LspSettings {
        enabled: true,
        jdtls_binary: "jdtls".to_string(),
        ..Default::default()
    };

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
            lsp_manager::JavaRuntime {
                major: 21,
                name: "JavaSE-21".to_string(),
                path: PathBuf::from("/jdk21"),
            },
            lsp_manager::JavaRuntime {
                major: 8,
                name: "JavaSE-1.8".to_string(),
                path: PathBuf::from("/jdk8"),
            },
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
    assert_eq!(
        params.initialization_options.unwrap()["extendedClientCapabilities"]["classFileContentsSupport"],
        true
    );
}

/// `debug_bundles` is a plain argument specifically so this stays a
/// pure, no-I/O test of the plumbing (`PLAN.md` Track 23 Phase 1) rather
/// than one that has to resolve `lsp_manager::ensure_debug_plugin_jar`
/// for real against this machine's actual cache directory.
#[test]
fn initialize_params_carries_the_debug_bundle_path_through() {
    let params = initialize_params(
        ServerKind::Java,
        &test_config(Path::new(".")),
        &["/cache/java-debug-plugin.jar".to_string()],
    )
    .unwrap();
    assert_eq!(
        params.initialization_options.unwrap()["bundles"],
        serde_json::json!(["/cache/java-debug-plugin.jar"])
    );
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
    assert_eq!(
        advertised_sync_kind(&numeric),
        lsp_types::TextDocumentSyncKind::INCREMENTAL
    );

    let options = serde_json::json!({ "capabilities": { "textDocumentSync": { "openClose": true, "change": 2 } } });
    assert_eq!(
        advertised_sync_kind(&options),
        lsp_types::TextDocumentSyncKind::INCREMENTAL
    );

    let full = serde_json::json!({ "capabilities": { "textDocumentSync": { "change": 1 } } });
    assert_eq!(advertised_sync_kind(&full), lsp_types::TextDocumentSyncKind::FULL);

    assert_eq!(
        advertised_sync_kind(&serde_json::json!({})),
        lsp_types::TextDocumentSyncKind::FULL
    );
}

#[test]
fn byte_to_utf16_position_at_the_very_start_is_zero_zero() {
    assert_eq!(
        byte_to_utf16_position("class Foo {\n}\n", 0),
        lsp_types::Position { line: 0, character: 0 }
    );
}

#[test]
fn byte_to_utf16_position_mid_line() {
    // byte 4 is the 'F' of "Foo", still line 0.
    assert_eq!(
        byte_to_utf16_position("class Foo {\n}\n", 6),
        lsp_types::Position { line: 0, character: 6 }
    );
}

#[test]
fn byte_to_utf16_position_right_after_a_lines_own_trailing_newline() {
    let text = "class Foo {\n}\n";
    // byte 12 is right after the first '\n', the '}' on line 1.
    assert_eq!(
        byte_to_utf16_position(text, 12),
        lsp_types::Position { line: 1, character: 0 }
    );
}

#[test]
fn byte_to_utf16_position_handles_crlf_line_endings() {
    let text = "ab\r\ncd";
    // byte 5 is the 'd': "ab\r\nc" is 5 bytes, so 5 lands right after 'c'.
    assert_eq!(
        byte_to_utf16_position(text, 5),
        lsp_types::Position { line: 1, character: 1 }
    );
}

#[test]
fn byte_to_utf16_position_counts_utf16_units_not_bytes_across_a_non_bmp_character() {
    // '\u{1F600}' (😀) is 4 UTF-8 bytes but 2 UTF-16 code units (a
    // surrogate pair) — a wrong byte-for-unit conflation here would
    // put every completion request after an emoji at the wrong column.
    let text = "a\u{1F600}b";
    let byte_offset = 'a'.len_utf8() + '\u{1F600}'.len_utf8();
    assert_eq!(
        byte_to_utf16_position(text, byte_offset),
        lsp_types::Position { line: 0, character: 3 }
    );
}

#[test]
fn byte_to_utf16_position_clamps_an_out_of_range_offset_to_the_end_of_text() {
    let text = "ab";
    assert_eq!(
        byte_to_utf16_position(text, 50),
        lsp_types::Position { line: 0, character: 2 }
    );
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
    let session = LspSession::spawn(Path::new("python3"), &["-c".to_string(), script.to_string()], None, noop_waker())
        .expect("python3 is always available");
    let (_dir, mut doc) = test_support::temp_document("Foo.java", "class Foo {}");
    let root = doc.path.parent().unwrap().to_path_buf();
    let mut state = LspState {
        java: Slot::Ready {
            config: SessionConfig {
                binary: PathBuf::from("python3"),
                ..test_config(&root)
            },
            session,
            open_documents: HashMap::new(),
            sync_kind: lsp_types::TextDocumentSyncKind::INCREMENTAL,
            status_message: None,
        },
        kotlin: Slot::Empty,
        retiring: Vec::new(),
        java_release: JavaReleaseCache::default(),
    };

    let rx = state
        .request_completion(&mut doc, 0)
        .expect("a Ready Java session should accept the request");
    let value = rx
        .recv()
        .expect("the fake server's response arrives")
        .expect("the fake server replied successfully");
    assert_eq!(value[0]["label"], "add(E e) : boolean");
}

/// A real child process (`sh -c printf`) that writes one canned
/// notification then blocks reading its own stdin — `lsp_client`'s own
/// `fake_server_returning` shape, reimplemented here since that helper
/// is private to `lsp_client`'s test module.
fn fake_server_sending(body: &str) -> LspSession {
    let script = format!(
        "printf 'Content-Length: %d\\r\\n\\r\\n%s' {} '{body}'; cat > /dev/null",
        body.len()
    );
    LspSession::spawn(Path::new("sh"), &["-c".to_string(), script], None, noop_waker()).expect("sh is always available")
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
        let Slot::Ready { status_message, .. } = &slot else {
            unreachable!()
        };
        if status_message.is_some() {
            assert_eq!(
                status_message.as_deref(),
                Some("Importing project br.ufsc.bridge.pec-backend")
            );
            assert!(
                errors.is_empty(),
                "a non-Error status must never be reported as a lifecycle failure"
            );
            break;
        }
    }
}

/// `"ServiceReady"` — jdt.ls's own terminal "the import this status was
/// tracking is actually done" signal — clears `status_message` back to
/// `None` rather than leaving the last progress line stuck on screen.
#[test]
fn a_service_ready_status_clears_the_status_message() {
    let session = fake_server_sending(
        r#"{"jsonrpc":"2.0","method":"language/status","params":{"type":"ServiceReady","message":"ServiceReady"}}"#,
    );
    let mut slot = ready_slot_around(session);
    let Slot::Ready { status_message, .. } = &mut slot else {
        unreachable!()
    };
    *status_message = Some("Importing project br.ufsc.bridge.pec-backend".to_string());
    let mut documents = [];
    let mut errors = Vec::new();
    loop {
        apply_server_messages(&mut slot, ServerKind::Java, &mut documents, &mut errors);
        let Slot::Ready { status_message, .. } = &slot else {
            unreachable!()
        };
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
            let Slot::Ready { status_message, .. } = &slot else {
                unreachable!()
            };
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
    let cached = lsp_manager::cache_dir()
        .expect("a cache directory")
        .join(cache_relative);
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
        let errors = state.sync(settings, Some(root), std::slice::from_mut(doc), &noop_waker());
        assert!(errors.is_empty(), "language server lifecycle errors: {errors:?}");
        assert!(
            Instant::now() < deadline,
            "kotlin-language-server never finished its handshake"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// `sync_until_kotlin_ready`'s Java counterpart — same loop, same
/// hard-failure stance, just the other slot.
fn sync_until_java_ready(state: &mut LspState, settings: &LspSettings, root: &Path, doc: &mut Document) {
    let deadline = Instant::now() + Duration::from_secs(240);
    while !matches!(state.java, Slot::Ready { .. }) {
        let errors = state.sync(settings, Some(root), std::slice::from_mut(doc), &noop_waker());
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
    let mut doc = Document::open(path, test_support::languages()).expect("open the fixture");
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

    let mut doc = Document::open(main_dir.join("Main.java"), test_support::languages()).expect("open the fixture");
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
            let _ = state.sync(&settings, Some(root), std::slice::from_mut(doc), &noop_waker());
            if let Some((file, line)) = debug.paused_location() {
                break (file.to_path_buf(), line);
            }
            match debug.status() {
                crate::debug_state::DebugStatus::Failed(message) => panic!("debug session failed: {message}"),
                status => {
                    assert!(
                        Instant::now() < deadline,
                        "debug session never paused; stuck at {status:?}"
                    );
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
    assert!(
        !stack.is_empty(),
        "a real stackTrace response should report at least one frame"
    );
    // Real java-debug reports a frame's own `name` as `"Main.main(String[])"`
    // — the declaring class and full signature, not the bare method
    // name a first guess from the DAP spec alone might expect.
    assert_eq!(
        stack[0].name, "Main.main(String[])",
        "the top frame should be the fixture's own main()"
    );
    assert_eq!(
        stack[0].file.as_deref().and_then(|p| p.canonicalize().ok()),
        main_java.canonicalize().ok(),
        "the top frame's own file should be the fixture's Main.java"
    );

    let variables_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        debug.poll();
        let _ = state.sync(&settings, Some(root), std::slice::from_mut(&mut doc), &noop_waker());
        if !debug.variables().is_empty() {
            break;
        }
        assert!(
            Instant::now() < variables_deadline,
            "the scopes/variables chain never finished"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let has_args = debug
        .variables()
        .iter()
        .any(|group| group.variables.iter().any(|v| v.name == "args"));
    assert!(
        has_args,
        "main(String[] args)'s own parameter should show up among the real fetched variables"
    );

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
        let _ = state.sync(&settings, Some(root), std::slice::from_mut(&mut doc), &noop_waker());
        match debug.status() {
            crate::debug_state::DebugStatus::Idle => break,
            crate::debug_state::DebugStatus::Failed(message) => panic!("debug session failed: {message}"),
            status => {
                assert!(
                    Instant::now() < deadline,
                    "debuggee never terminated after Continue; stuck at {status:?}"
                );
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
    let mut doc = Document::open(path, test_support::languages()).expect("open the fixture");
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
