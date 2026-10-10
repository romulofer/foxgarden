
use super::*;

#[test]
fn open_settings_opens_the_dialog_for_that_kind_and_clears_the_previous_error() {
    let mut state = RuntimesState {
        last_add_error: Some("stale".to_string()),
        ..Default::default()
    };
    assert!(state.open.is_none());
    state.open_settings("jdk");
    assert_eq!(state.open.as_deref(), Some("jdk"));
    assert!(state.last_add_error.is_none());
}

// The picker's own "in flight vs. resolved" mechanics are tested
// where they now live, in `crate::folder_picker` — this dialog only
// forwards to it.

#[test]
fn poll_auto_detect_drains_a_completed_scan_without_blocking_the_caller() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = RuntimesState {
        auto_detect_rx: Some(rx),
        ..Default::default()
    };
    assert!(state.auto_detect_running());

    // Nothing sent yet — still running, not a false "found nothing"
    // result (same "in flight" vs. "resolved" distinction poll_picker
    // has to make).
    assert!(state.poll_auto_detect().is_empty());
    assert!(state.auto_detect_running());

    let found = vec![RuntimeInstall {
        home: std::path::PathBuf::from("/jdk21"),
        label: "Java 21".to_string(),
        major_version: 21,
    }];
    tx.send(found.clone()).unwrap();
    assert_eq!(state.poll_auto_detect(), found);
    assert!(!state.auto_detect_running());
}

#[test]
fn poll_auto_detect_on_a_disconnected_sender_clears_the_slot_without_a_result() {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<RuntimeInstall>>();
    let mut state = RuntimesState {
        auto_detect_rx: Some(rx),
        ..Default::default()
    };
    drop(tx);

    assert!(state.poll_auto_detect().is_empty());
    assert!(!state.auto_detect_running());
}
