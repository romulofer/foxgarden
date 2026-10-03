
use super::*;
use std::io::Write;

#[test]
fn read_capture_folds_a_real_collapsed_file_into_a_tree() {
    let dir = test_support::tempdir();
    let path = dir.path().join("p.collapsed");
    let mut f = std::fs::File::create(&path).unwrap();
    write!(f, "a;b;c 5\na;b;d 3\n").unwrap();

    let tree = read_capture(&path).expect("a non-empty profile parses");
    assert_eq!(tree.total, 8);
    assert_eq!(tree.children[0].name, "a");
}

#[test]
fn read_capture_rejects_an_empty_profile() {
    let dir = test_support::tempdir();
    let path = dir.path().join("empty.collapsed");
    std::fs::File::create(&path).unwrap();

    let error = read_capture(&path).expect_err("no samples must be an error");
    assert!(error.contains("no samples"), "{error}");
}

#[test]
fn read_capture_errors_when_the_file_is_missing() {
    let error = read_capture(Path::new("/no/such/profile.collapsed")).expect_err("a missing file must error");
    assert!(error.contains("couldn't read"), "{error}");
}

#[test]
fn capture_output_path_is_unique_per_call() {
    let a = capture_output_path(42);
    let b = capture_output_path(42);
    assert_ne!(a, b, "two captures of the same PID must not collide");
    assert!(a.to_string_lossy().contains("foxgarden-profile-42-"));
}

#[test]
fn state_starts_idle() {
    let state = ProfilerState::default();
    assert!(!state.is_capturing());
    assert!(state.last_profile.is_none());
}
