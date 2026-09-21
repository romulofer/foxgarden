use super::*;

fn sample() -> ProjectConfig {
    ProjectConfig {
        java_release: Some(17),
        jdk_home: Some(PathBuf::from("/opt/jdk17")),
    }
}

#[test]
fn a_config_round_trips() {
    let config = sample();
    let serialized = serialize_project_config(&config);
    assert_eq!(parse_project_config(&serialized), config);
}

#[test]
fn no_jdk_home_round_trips_as_none() {
    let config = ProjectConfig {
        jdk_home: None,
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
    assert_eq!(config.java_release, Some(21));
    assert_eq!(config.jdk_home, None);
}

#[test]
fn unrecognized_keys_are_skipped_rather_than_erroring() {
    let config = parse_project_config(r#"{"java_release": 21, "future_field": "something new"}"#);
    assert_eq!(config.java_release, Some(21));
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
