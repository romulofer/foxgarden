use super::*;

fn sample() -> ProjectConfig {
    ProjectConfig {
        runtime_release: Some(17),
        runtime_home: Some(PathBuf::from("/opt/jdk17")),
    }
}

#[test]
fn a_config_round_trips() {
    let config = sample();
    let serialized = serialize_project_config(&config);
    assert_eq!(parse_project_config(&serialized), config);
}

#[test]
fn no_runtime_home_round_trips_as_none() {
    let config = ProjectConfig {
        runtime_home: None,
        ..sample()
    };
    let serialized = serialize_project_config(&config);
    assert_eq!(parse_project_config(&serialized), config);
}

#[test]
fn empty_input_parses_to_the_default_config() {
    assert_eq!(parse_project_config(""), ProjectConfig::default());
    assert_eq!(parse_project_config("not json"), ProjectConfig::default());
}

#[test]
fn a_missing_field_defaults_rather_than_failing_the_whole_parse() {
    let config = parse_project_config(r#"{"java_release": 21}"#);
    assert_eq!(config.runtime_release, Some(21));
    assert_eq!(config.runtime_home, None);
}

#[test]
fn unrecognized_keys_are_skipped_rather_than_erroring() {
    let config = parse_project_config(r#"{"java_release": 21, "future_field": "something new"}"#);
    assert_eq!(config.runtime_release, Some(21));
}

#[test]
fn load_project_config_with_no_saved_file_returns_the_default() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load_project_config(dir.path()), ProjectConfig::default());
}

#[test]
fn load_project_config_with_malformed_json_returns_the_default_rather_than_panicking() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".foxgarden")).unwrap();
    std::fs::write(dir.path().join(".foxgarden").join("project.json"), "not json").unwrap();
    assert_eq!(load_project_config(dir.path()), ProjectConfig::default());
}

#[test]
fn save_then_load_round_trips_through_the_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    let config = sample();

    save_project_config(dir.path(), &config).unwrap();
    let loaded = load_project_config(dir.path());

    assert_eq!(loaded, config);
    assert!(dir.path().join(".foxgarden").join("project.json").exists());
}

#[test]
fn a_file_written_with_the_current_key_names_reads_back_and_the_old_names_still_do() {
    let current = parse_project_config(r#"{"runtime_release": 17, "runtime_home": "/opt/rt"}"#);
    assert_eq!((current.runtime_release, current.runtime_home), (Some(17), Some(PathBuf::from("/opt/rt"))));

    let legacy = parse_project_config(r#"{"java_release": 11, "jdk_home": "/opt/jdk11"}"#);
    assert_eq!((legacy.runtime_release, legacy.runtime_home), (Some(11), Some(PathBuf::from("/opt/jdk11"))));
}
