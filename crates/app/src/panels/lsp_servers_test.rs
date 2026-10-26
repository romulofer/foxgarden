
use super::*;

#[test]
fn open_settings_opens_the_dialog() {
    let mut state = LspServersState::default();
    assert!(!state.settings_open);
    state.open_settings(&LspSettings::default(), None, test_support::languages());
    assert!(state.settings_open);
}

/// Opening with no runtime home configured is what kicks the scan that
/// fills the field in; opening with one already set must not, since that
/// scan's result overwrites the field. A server with no host runtime never
/// scans.
#[test]
fn opening_scans_for_a_runtime_only_when_its_home_is_blank() {
    let mut state = LspServersState::default();
    state.open_settings(&LspSettings::default(), None, test_support::languages());
    assert!(state.manager.detecting_runtime_home("jdtls"));
    assert!(!state.manager.detecting_runtime_home("kotlin-language-server"));

    let mut state = LspServersState::default();
    let mut settings = LspSettings::default();
    *settings.runtime_home_mut("jdtls") = "/usr/lib/jvm/java-21".to_string();
    state.open_settings(&settings, None, test_support::languages());
    assert!(!state.manager.detecting_runtime_home("jdtls"));
}

#[test]
fn a_scan_that_found_nothing_is_what_shows_the_missing_runtime_note() {
    let mut state = LspServersState::default();
    assert!(!state.no_runtime_found.contains("jdtls"));
    state.record_runtime_detection("jdtls".to_string(), false);
    assert!(state.no_runtime_found.contains("jdtls"));
    state.record_runtime_detection("jdtls".to_string(), true);
    assert!(!state.no_runtime_found.contains("jdtls"));
}

#[test]
fn recording_a_check_result_keeps_the_two_servers_separate() {
    let mut state = LspServersState::default();
    state.record_latest_version("jdtls".to_string(), Ok("1.60.0".to_string()));
    assert_eq!(state.latest_version("jdtls"), Some(&Ok("1.60.0".to_string())));
    assert!(state.latest_version("kotlin-language-server").is_none());
}

/// A later check replaces the earlier answer rather than accumulating
/// stale ones — the dialog only ever shows the most recent.
#[test]
fn a_later_check_replaces_the_previous_result() {
    let mut state = LspServersState::default();
    state.record_latest_version("jdtls".to_string(), Err("offline".to_string()));
    state.record_latest_version("jdtls".to_string(), Ok("1.60.0".to_string()));
    assert_eq!(state.latest_version("jdtls"), Some(&Ok("1.60.0".to_string())));
}

#[test]
fn a_bare_name_is_found_on_the_path_and_a_missing_one_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let tool = dir.path().join("jedi-language-server");
    std::fs::write(&tool, "").unwrap();
    let path_var = std::env::join_paths([other.path(), dir.path()]).unwrap();

    assert_eq!(find_program_in(Path::new("jedi-language-server"), Some(&path_var)), Some(tool));
    assert_eq!(find_program_in(Path::new("absent-server"), Some(&path_var)), None);
    assert_eq!(find_program_in(Path::new("jedi-language-server"), None), None);
}

#[test]
fn a_path_to_a_file_is_found_only_when_the_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let tool = dir.path().join("server");
    std::fs::write(&tool, "").unwrap();

    assert_eq!(find_program_in(&tool, None), Some(tool.clone()));
    assert_eq!(find_program_in(&dir.path().join("missing"), None), None);
}

/// A server with no installer that names a program is listed for the dialog;
/// the installed ones (jdtls, Kotlin) are not, they have their own sections.
#[test]
fn opening_lists_the_servers_that_are_found_rather_than_installed() {
    let mut state = LspServersState::default();
    state.open_settings(&LspSettings::default(), None, test_support::languages());
    let ids: Vec<&str> = state.provided.iter().map(|p| p.server.id.as_str()).collect();
    assert_eq!(ids, ["jedi"]);
    assert_eq!(state.provided[0].default_binary, PathBuf::from("jedi-language-server"));
}

#[cfg(unix)]
#[test]
fn a_server_in_the_projects_environment_is_what_the_dialog_reports() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join(".venv/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("python"), "").unwrap();
    std::fs::write(bin.join("jedi-language-server"), "").unwrap();

    let mut state = LspServersState::default();
    state.open_settings(&LspSettings::default(), Some(dir.path()), test_support::languages());
    assert_eq!(state.provided[0].default_binary, bin.join("jedi-language-server"));
}
