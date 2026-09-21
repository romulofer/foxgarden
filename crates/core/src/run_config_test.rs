use super::*;

fn sample() -> RunConfig {
    RunConfig {
        name: "Run App".to_string(),
        main_class: "com.example.Main".to_string(),
        vm_args: "-Xmx512m".to_string(),
        program_args: "--debug".to_string(),
        env: vec![
            ("DEBUG".to_string(), "true".to_string()),
            ("PORT".to_string(), "8080".to_string()),
        ],
        working_dir: Some(PathBuf::from("/home/user/project")),
    }
}

#[test]
fn a_single_config_round_trips() {
    let configs = vec![sample()];
    let serialized = serialize_run_configs(&configs);
    assert_eq!(parse_run_configs(&serialized), configs);
}

#[test]
fn multiple_configs_round_trip() {
    let mut second = sample();
    second.name = "Run Tests".to_string();
    second.working_dir = None;
    second.env.clear();
    let configs = vec![sample(), second];

    let serialized = serialize_run_configs(&configs);
    assert_eq!(parse_run_configs(&serialized), configs);
}

#[test]
fn an_env_value_containing_equals_signs_round_trips() {
    let mut config = RunConfig::default();
    config.env.push((
        "JDBC_URL".to_string(),
        "jdbc:postgresql://host/db?user=a&pass=b".to_string(),
    ));

    let serialized = serialize_run_configs(&[config.clone()]);
    assert_eq!(parse_run_configs(&serialized), vec![config]);
}

#[test]
fn no_working_dir_round_trips_as_none() {
    let mut config = sample();
    config.working_dir = None;

    let serialized = serialize_run_configs(&[config.clone()]);
    assert_eq!(parse_run_configs(&serialized), vec![config]);
}

#[test]
fn empty_input_parses_to_no_configs() {
    assert_eq!(parse_run_configs(""), vec![]);
    assert_eq!(parse_run_configs("   \n\n  "), vec![]);
}

#[test]
fn unrecognized_keys_are_skipped_rather_than_erroring() {
    let input = r#"[{"name": "Foo", "future_field": "something new", "main_class": "Foo"}]"#;
    let configs = parse_run_configs(input);
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].name, "Foo");
    assert_eq!(configs[0].main_class, "Foo");
}

#[test]
fn a_missing_field_defaults_rather_than_failing_the_whole_parse() {
    let input = r#"[{"name": "Foo"}]"#;
    let configs = parse_run_configs(input);
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].name, "Foo");
    assert_eq!(configs[0].main_class, "");
    assert_eq!(configs[0].env, vec![]);
    assert_eq!(configs[0].working_dir, None);
}

#[test]
fn load_run_configs_with_no_saved_file_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load_run_configs(dir.path()), vec![]);
}

#[test]
fn load_run_configs_with_malformed_json_returns_empty_rather_than_panicking() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".foxgarden")).unwrap();
    std::fs::write(dir.path().join(".foxgarden").join("run_configs.json"), "not json").unwrap();
    assert_eq!(load_run_configs(dir.path()), vec![]);
}

#[test]
fn save_then_load_round_trips_through_the_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    let configs = vec![sample()];

    save_run_configs(dir.path(), &configs).unwrap();
    let loaded = load_run_configs(dir.path());

    assert_eq!(loaded, configs);
    assert!(dir.path().join(".foxgarden").join("run_configs.json").exists());
}
