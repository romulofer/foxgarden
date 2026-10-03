
use super::*;

#[test]
fn open_settings_opens_the_dialog() {
    let mut state = LspServersState::default();
    assert!(!state.settings_open);
    state.open_settings(&LspSettings::default(), None);
    assert!(state.settings_open);
}

/// Opening with no Java Home configured is what kicks the JDK scan that
/// fills the field in; opening with one already set must not, since
/// that scan's result overwrites the field.
#[test]
fn opening_scans_for_a_jdk_only_when_java_home_is_blank() {
    let mut state = LspServersState::default();
    state.open_settings(&LspSettings::default(), None);
    assert!(state.manager.detecting_java_home());

    let mut state = LspServersState::default();
    state.open_settings(
        &LspSettings {
            jdtls_java_home: "/usr/lib/jvm/java-21".to_string(),
            ..Default::default()
        },
        None,
    );
    assert!(!state.manager.detecting_java_home());
}

#[test]
fn a_scan_that_found_nothing_is_what_shows_the_missing_jdk_note() {
    let mut state = LspServersState::default();
    assert!(!state.no_java_home_found);
    state.record_java_home_detection(false);
    assert!(state.no_java_home_found);
    state.record_java_home_detection(true);
    assert!(!state.no_java_home_found);
}

#[test]
fn recording_a_check_result_keeps_the_two_servers_separate() {
    let mut state = LspServersState::default();
    state.record_latest_version(Server::Jdtls, Ok("1.60.0".to_string()));
    assert_eq!(state.latest_version(Server::Jdtls), Some(&Ok("1.60.0".to_string())));
    assert!(state.latest_version(Server::KotlinLanguageServer).is_none());
}

/// A later check replaces the earlier answer rather than accumulating
/// stale ones — the dialog only ever shows the most recent.
#[test]
fn a_later_check_replaces_the_previous_result() {
    let mut state = LspServersState::default();
    state.record_latest_version(Server::Jdtls, Err("offline".to_string()));
    state.record_latest_version(Server::Jdtls, Ok("1.60.0".to_string()));
    assert_eq!(state.latest_version(Server::Jdtls), Some(&Ok("1.60.0".to_string())));
}
