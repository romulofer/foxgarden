use super::*;

/// No tool claims the directory, so there is no classpath to resolve: the
/// root counts as scanned, and no thread is started for a scan that could
/// only ever come back empty.
#[test]
fn a_project_no_build_tool_claims_is_never_scanned() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = SpringConfigState::default();
    state.observe(test_support::languages(), Some(dir.path()));

    state.ensure_scanning(dir.path());

    assert!(!state.scanning());
    assert_eq!(state.scanned_root.as_deref(), Some(dir.path()));
}

/// The completion trigger can run for a project `observe` has not seen yet
/// (opened earlier the same frame). That must not count as scanned, or the
/// project would never get its properties.
#[test]
fn a_project_not_yet_observed_is_left_for_a_later_call() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = SpringConfigState::default();

    state.ensure_scanning(dir.path());

    assert!(!state.scanning());
    assert!(state.scanned_root.is_none());
}

#[test]
fn the_build_tool_is_detected_again_when_the_project_changes() {
    let plain = tempfile::tempdir().unwrap();
    let maven = tempfile::tempdir().unwrap();
    std::fs::write(maven.path().join("pom.xml"), "<project/>").unwrap();
    let mut state = SpringConfigState::default();

    state.observe(test_support::languages(), Some(plain.path()));
    assert!(state.build_tool.as_ref().is_some_and(|(_, tool)| tool.is_none()));

    state.observe(test_support::languages(), Some(maven.path()));
    let (root, tool) = state.build_tool.as_ref().unwrap();
    assert_eq!(root, maven.path());
    assert_eq!(tool.as_ref().map(|t| t.id), Some("maven"));

    state.observe(test_support::languages(), None);
    assert!(state.build_tool.is_none());
}
