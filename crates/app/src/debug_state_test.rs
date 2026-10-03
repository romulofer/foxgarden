
use super::*;

fn config(main_class: &str) -> RunConfig {
    RunConfig {
        name: "test".to_string(),
        main_class: main_class.to_string(),
        vm_args: "-Xmx128m".to_string(),
        program_args: "foo".to_string(),
        env: vec![("MY_ENV".to_string(), "hello".to_string())],
        working_dir: None,
    }
}

#[test]
fn build_launch_args_carries_every_field_the_real_extension_forwards() {
    let root = Path::new("/projects/demo-app");
    let classpath = vec![
        PathBuf::from("/projects/demo-app/target/classes"),
        PathBuf::from("/home/.m2/x.jar"),
    ];
    let args = build_launch_args(root, &config("com.example.Main"), &classpath);

    assert_eq!(args["mainClass"], "com.example.Main");
    assert_eq!(args["projectName"], "demo-app");
    assert_eq!(args["cwd"], "/projects/demo-app");
    assert_eq!(
        args["classPaths"],
        serde_json::json!(["/projects/demo-app/target/classes", "/home/.m2/x.jar"])
    );
    assert_eq!(args["modulePaths"], serde_json::json!([]));
    assert_eq!(args["args"], "foo");
    assert_eq!(args["vmArgs"], "-Xmx128m");
    assert_eq!(args["env"]["MY_ENV"], "hello");
    assert_eq!(args["console"], "internalConsole");
}

#[test]
fn build_launch_args_prefers_working_dir_over_project_root() {
    let root = Path::new("/projects/demo-app");
    let mut cfg = config("com.example.Main");
    cfg.working_dir = Some(PathBuf::from("/projects/demo-app/sub"));
    let args = build_launch_args(root, &cfg, &[]);
    assert_eq!(args["cwd"], "/projects/demo-app/sub");
}

#[test]
fn debug_state_starts_idle_and_reports_attached_status_correctly() {
    let state = DebugState::default();
    assert_eq!(state.status(), DebugStatus::Idle);
    assert!(state.can_start());
}

#[test]
fn stop_from_idle_is_a_harmless_no_op() {
    let mut state = DebugState::default();
    state.stop();
    assert_eq!(state.status(), DebugStatus::Idle);
}

#[test]
fn step_and_continue_from_idle_are_harmless_no_ops() {
    let mut state = DebugState::default();
    state.continue_();
    state.step_over();
    state.step_into();
    state.step_out();
    assert_eq!(state.status(), DebugStatus::Idle);
    assert!(!state.is_paused());
    assert_eq!(state.paused_location(), None);
}

#[test]
fn parse_stopped_thread_id_reads_a_real_stopped_event_body() {
    let body = serde_json::json!({ "reason": "breakpoint", "threadId": 7, "allThreadsStopped": true });
    assert_eq!(parse_stopped_thread_id(&body), Some(7));
}

#[test]
fn parse_stopped_thread_id_is_none_without_one() {
    assert_eq!(
        parse_stopped_thread_id(&serde_json::json!({ "reason": "breakpoint" })),
        None
    );
}

#[test]
fn parse_top_stack_frame_reads_the_first_frames_file_and_converts_to_0_indexed() {
    let body = serde_json::json!({
        "stackFrames": [
            { "id": 1, "name": "main", "line": 12, "column": 1, "source": { "name": "Main.java", "path": "/projects/demo-app/src/main/java/com/example/Main.java" } },
            { "id": 2, "name": "caller", "line": 40, "column": 1, "source": { "name": "Other.java", "path": "/projects/demo-app/src/main/java/com/example/Other.java" } },
        ],
        "totalFrames": 2,
    });
    let (id, file, line) = parse_top_stack_frame(&body).expect("a real top frame with a file source parses");
    assert_eq!(id, 1);
    assert_eq!(
        file,
        PathBuf::from("/projects/demo-app/src/main/java/com/example/Main.java")
    );
    assert_eq!(line, 11);
}

#[test]
fn parse_call_stack_keeps_every_frame_including_ones_without_a_file() {
    let body = serde_json::json!({
        "stackFrames": [
            { "id": 1, "name": "main", "line": 12, "column": 1, "source": { "name": "Main.java", "path": "/projects/demo-app/src/main/java/com/example/Main.java" } },
            { "id": 2, "name": "decompiled", "line": 3, "column": 1, "source": { "name": "Foo.class", "sourceReference": 42 } },
        ],
    });
    let stack = parse_call_stack(&body);
    assert_eq!(stack.len(), 2);
    assert_eq!(stack[0].id, 1);
    assert_eq!(stack[0].name, "main");
    assert_eq!(
        stack[0].file,
        Some(PathBuf::from("/projects/demo-app/src/main/java/com/example/Main.java"))
    );
    assert_eq!(stack[0].line, 11);
    assert_eq!(stack[1].id, 2);
    assert_eq!(stack[1].file, None);
    assert_eq!(stack[1].line, 2);
}

#[test]
fn parse_call_stack_is_empty_for_no_frames() {
    assert!(parse_call_stack(&serde_json::json!({ "stackFrames": [] })).is_empty());
}

#[test]
fn parse_scopes_drops_expensive_and_empty_scopes() {
    let body = serde_json::json!({
        "scopes": [
            { "name": "Locals", "variablesReference": 100, "expensive": false },
            { "name": "Arguments", "variablesReference": 101, "expensive": false },
            { "name": "Static", "variablesReference": 102, "expensive": true },
            { "name": "Empty", "variablesReference": 0, "expensive": false },
        ],
    });
    assert_eq!(
        parse_scopes(&body),
        vec![("Locals".to_string(), 100), ("Arguments".to_string(), 101)]
    );
}

#[test]
fn parse_variables_reads_name_value_and_type() {
    let body = serde_json::json!({
        "variables": [
            { "name": "count", "value": "3", "type": "int", "variablesReference": 0 },
            { "name": "self", "value": "Foo@1 (id=2)", "type": "Foo", "variablesReference": 5 },
        ],
    });
    let variables = parse_variables(&body);
    assert_eq!(variables.len(), 2);
    assert_eq!(variables[0].name, "count");
    assert_eq!(variables[0].value, "3");
    assert_eq!(variables[0].kind, "int");
    assert_eq!(variables[1].name, "self");
    assert_eq!(variables[1].value, "Foo@1 (id=2)");
}

#[test]
fn parse_top_stack_frame_is_none_for_an_empty_stack() {
    assert_eq!(
        parse_top_stack_frame(&serde_json::json!({ "stackFrames": [], "totalFrames": 0 })),
        None
    );
}

#[test]
fn parse_top_stack_frame_is_none_without_a_real_file_source() {
    let body = serde_json::json!({
        "stackFrames": [{ "id": 1, "name": "decompiled", "line": 3, "column": 1, "source": { "name": "Foo.class", "sourceReference": 42 } }],
    });
    assert_eq!(parse_top_stack_frame(&body), None);
}

#[test]
fn breakpoints_to_resend_includes_a_file_with_a_new_or_changed_set() {
    let mut last_sent = HashMap::new();
    last_sent.insert(PathBuf::from("/a/Main.java"), HashSet::from([3]));
    let unchanged = HashSet::from([3]);
    let changed = HashSet::from([5, 6]);
    let brand_new = HashSet::from([1]);
    let current = vec![
        (Path::new("/a/Main.java"), &unchanged),
        (Path::new("/a/Other.java"), &changed),
        (Path::new("/a/New.java"), &brand_new),
    ];
    let mut to_send = breakpoints_to_resend(&last_sent, current.into_iter());
    to_send.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        to_send,
        vec![
            (PathBuf::from("/a/New.java"), HashSet::from([1])),
            (PathBuf::from("/a/Other.java"), HashSet::from([5, 6])),
        ]
    );
}

#[test]
fn breakpoints_to_resend_excludes_a_file_that_never_had_breakpoints_and_still_doesnt() {
    let last_sent = HashMap::new();
    let empty = HashSet::new();
    let current = vec![(Path::new("/a/Untouched.java"), &empty)];
    assert!(breakpoints_to_resend(&last_sent, current.into_iter()).is_empty());
}

#[test]
fn breakpoints_to_resend_includes_a_file_whose_breakpoints_were_all_cleared() {
    let mut last_sent = HashMap::new();
    last_sent.insert(PathBuf::from("/a/Main.java"), HashSet::from([3]));
    let empty = HashSet::new();
    let current = vec![(Path::new("/a/Main.java"), &empty)];
    assert_eq!(
        breakpoints_to_resend(&last_sent, current.into_iter()),
        vec![(PathBuf::from("/a/Main.java"), HashSet::new())]
    );
}
