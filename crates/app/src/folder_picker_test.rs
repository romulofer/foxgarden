
use super::*;

#[test]
fn a_fresh_picker_is_closed_and_has_nothing_to_report() {
    let mut picker = FolderPicker::default();
    assert!(!picker.is_open());
    assert_eq!(picker.poll(), None);
}

#[test]
fn poll_returns_the_picked_folder_once_and_then_closes() {
    // Stands in for the dialog thread: the real one is a native window
    // no test can answer, but everything this type does with its reply
    // is on this side of the channel.
    let (tx, rx) = channel();
    let mut picker = FolderPicker { rx: Some(rx) };
    assert!(picker.is_open());
    assert_eq!(picker.poll(), None, "nothing answered yet");

    tx.send(Some(PathBuf::from("/tmp/project"))).unwrap();

    assert_eq!(picker.poll(), Some(PathBuf::from("/tmp/project")));
    assert!(!picker.is_open());
    assert_eq!(picker.poll(), None, "the same answer is never delivered twice");
}

#[test]
fn a_cancelled_dialog_closes_the_picker_without_a_folder() {
    let (tx, rx) = channel();
    let mut picker = FolderPicker { rx: Some(rx) };

    tx.send(None).unwrap();

    assert_eq!(picker.poll(), None);
    assert!(!picker.is_open());
}

#[test]
fn a_dialog_thread_that_dies_stops_being_waited_on() {
    let (tx, rx) = channel::<Option<PathBuf>>();
    let mut picker = FolderPicker { rx: Some(rx) };
    drop(tx);

    assert_eq!(picker.poll(), None);
    assert!(!picker.is_open());
}
