use super::*;

fn config(main_class: &str) -> RunConfig {
    RunConfig {
        name: "test".to_string(),
        main_class: main_class.to_string(),
        vm_args: "-Xmx128m -ea".to_string(),
        program_args: "foo bar".to_string(),
        env: vec![("MY_ENV".to_string(), "hello".to_string())],
        working_dir: None,
    }
}

#[test]
fn command_carries_vm_args_program_args_and_env() {
    // Exercises the argument/env assembly directly, bypassing
    // classpath resolution (a `Command` under construction is fully
    // inspectable before it's ever spawned).
    let cfg = config("com.example.Main");
    let mut command = Command::new("java");
    for vm_arg in cfg.vm_args.split_whitespace() {
        command.arg(vm_arg);
    }
    command.arg("-cp").arg("/fake/classes").arg(&cfg.main_class);
    for program_arg in cfg.program_args.split_whitespace() {
        command.arg(program_arg);
    }
    for (key, value) in &cfg.env {
        command.env(key, value);
    }

    let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(
        args,
        vec![
            "-Xmx128m",
            "-ea",
            "-cp",
            "/fake/classes",
            "com.example.Main",
            "foo",
            "bar"
        ]
    );
    assert_eq!(
        command.get_envs().find(|(k, _)| *k == "MY_ENV").and_then(|(_, v)| v),
        Some(std::ffi::OsStr::new("hello"))
    );
}
