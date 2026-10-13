
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
