
use super::*;

#[test]
fn open_settings_opens_the_dialog_and_clears_the_previous_error() {
    let mut state = JdkRegistryState {
        settings_open: false,
        last_add_error: Some("stale".to_string()),
        picker: crate::folder_picker::FolderPicker::default(),
        auto_detect_rx: None,
    };
    state.open_settings();
    assert!(state.settings_open);
    assert!(state.last_add_error.is_none());
}

// The picker's own "in flight vs. resolved" mechanics are tested
// where they now live, in `crate::folder_picker` — this dialog only
// forwards to it.

#[test]
fn poll_auto_detect_drains_a_completed_scan_without_blocking_the_caller() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = JdkRegistryState::default();
    state.auto_detect_rx = Some(rx);
    assert!(state.auto_detect_running());

    // Nothing sent yet — still running, not a false "found nothing"
    // result (same "in flight" vs. "resolved" distinction poll_picker
    // has to make).
    assert!(state.poll_auto_detect().is_empty());
    assert!(state.auto_detect_running());

    let found = vec![JavaRuntime {
        major: 21,
        name: "JavaSE-21".to_string(),
        path: PathBuf::from("/jdk21"),
    }];
    tx.send(found.clone()).unwrap();
    assert_eq!(state.poll_auto_detect(), found);
    assert!(!state.auto_detect_running());
}

#[test]
fn poll_auto_detect_on_a_disconnected_sender_clears_the_slot_without_a_result() {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<JavaRuntime>>();
    let mut state = JdkRegistryState::default();
    state.auto_detect_rx = Some(rx);
    drop(tx);

    assert!(state.poll_auto_detect().is_empty());
    assert!(!state.auto_detect_running());
}
