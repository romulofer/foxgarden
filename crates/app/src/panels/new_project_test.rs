
use super::*;

#[test]
fn open_resets_every_field_and_opens_the_dialog() {
    let mut state = NewProjectWizardState {
        open: false,
        group_id: "stale".to_string(),
        artifact_id: "stale".to_string(),
        location: "/stale".to_string(),
        runtime_version: 8,
        build_tool_id: "gradle".to_string(),
        language_id: "kotlin".to_string(),
        last_error: Some("stale error".to_string()),
        picker: crate::folder_picker::FolderPicker::default(),
    };
    state.open(test_support::languages());

    assert!(state.open);
    assert_eq!(state.group_id, "");
    assert_eq!(state.artifact_id, "");
    assert_eq!(state.location, "");
    // The defaults are the registry's first contributed target and its
    // newest offered release, not anything this panel names itself.
    assert_eq!(state.runtime_version, 21);
    assert_eq!(state.build_tool_id, "maven");
    assert_eq!(state.language_id, "java");
    assert!(state.last_error.is_none());
}

#[test]
fn project_root_joins_location_and_artifact_id() {
    let state = NewProjectWizardState {
        location: "/home/dev/code".to_string(),
        artifact_id: "my-app".to_string(),
        ..Default::default()
    };
    assert_eq!(project_root(&state), Path::new("/home/dev/code/my-app"));
}

#[test]
fn form_is_valid_requires_every_field_non_empty() {
    let mut state = NewProjectWizardState::default();
    assert!(!form_is_valid(&state));

    state.build_tool_id = "maven".to_string();
    state.language_id = "java".to_string();
    assert!(!form_is_valid(&state));

    state.group_id = "com.example".to_string();
    assert!(!form_is_valid(&state));

    state.artifact_id = "app".to_string();
    assert!(!form_is_valid(&state));

    state.location = "/home/dev".to_string();
    assert!(form_is_valid(&state));
}

#[test]
fn form_is_valid_rejects_whitespace_only_fields() {
    let state = NewProjectWizardState {
        group_id: "  ".to_string(),
        artifact_id: "app".to_string(),
        location: "/home/dev".to_string(),
        build_tool_id: "maven".to_string(),
        language_id: "java".to_string(),
        ..Default::default()
    };
    assert!(!form_is_valid(&state));
}

#[test]
fn create_and_open_scaffolds_saves_config_and_opens_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let state = NewProjectWizardState {
        group_id: "com.example".to_string(),
        artifact_id: "my-app".to_string(),
        location: dir.path().display().to_string(),
        runtime_version: 17,
        build_tool_id: "maven".to_string(),
        language_id: "java".to_string(),
        ..Default::default()
    };
    let mut editor_state = test_support::editor_state();

    create_and_open(&state, &mut editor_state).expect("scaffolds and opens");

    let root = dir.path().join("my-app");
    assert!(root.join("pom.xml").exists());
    assert!(root.join("src/main/java/com/example/Main.java").exists());
    assert_eq!(
        editor_state.project.as_ref().map(|p| p.root.clone()),
        Some(root.clone())
    );
    assert_eq!(fg_core::load_project_config(&root).java_release, Some(17));
}

#[test]
fn create_and_open_with_gradle_scaffolds_gradle_files() {
    let dir = tempfile::tempdir().unwrap();
    let state = NewProjectWizardState {
        group_id: "com.example".to_string(),
        artifact_id: "my-app".to_string(),
        location: dir.path().display().to_string(),
        runtime_version: 17,
        build_tool_id: "gradle".to_string(),
        language_id: "java".to_string(),
        ..Default::default()
    };
    let mut editor_state = test_support::editor_state();

    create_and_open(&state, &mut editor_state).expect("scaffolds and opens");

    let root = dir.path().join("my-app");
    assert!(root.join("settings.gradle.kts").exists());
    assert!(root.join("build.gradle.kts").exists());
    assert!(root.join("src/main/java/com/example/Main.java").exists());
    assert_eq!(
        editor_state.project.as_ref().map(|p| p.root.clone()),
        Some(root.clone())
    );
}

#[test]
fn create_and_open_with_kotlin_scaffolds_kotlin_sources() {
    let dir = tempfile::tempdir().unwrap();
    let state = NewProjectWizardState {
        group_id: "com.example".to_string(),
        artifact_id: "my-app".to_string(),
        location: dir.path().display().to_string(),
        runtime_version: 17,
        build_tool_id: "gradle".to_string(),
        language_id: "kotlin".to_string(),
        ..Default::default()
    };
    let mut editor_state = test_support::editor_state();

    create_and_open(&state, &mut editor_state).expect("scaffolds and opens");

    let root = dir.path().join("my-app");
    assert!(root.join("build.gradle.kts").exists());
    assert!(root.join("src/main/kotlin/com/example/Main.kt").exists());
    assert!(!root.join("src/main/java/com/example/Main.java").exists());
    assert_eq!(
        editor_state.project.as_ref().map(|p| p.root.clone()),
        Some(root.clone())
    );
}

#[test]
fn create_and_open_reports_a_scaffold_failure_without_touching_editor_state() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("my-app"), "a file, not a directory").unwrap();
    let state = NewProjectWizardState {
        group_id: "com.example".to_string(),
        artifact_id: "my-app".to_string(),
        location: dir.path().display().to_string(),
        runtime_version: 17,
        build_tool_id: "maven".to_string(),
        language_id: "java".to_string(),
        ..Default::default()
    };
    let mut editor_state = test_support::editor_state();

    let error = create_and_open(&state, &mut editor_state).unwrap_err();
    assert!(!error.is_empty());
    assert!(editor_state.project.is_none());
}

/// A pair no extension scaffolds is a real possibility now that the targets
/// come from the registry — it has to fail with a message rather than
/// writing a half-made project.
#[test]
fn create_and_open_refuses_a_target_no_extension_scaffolds() {
    let dir = tempfile::tempdir().unwrap();
    let state = NewProjectWizardState {
        group_id: "com.example".to_string(),
        artifact_id: "my-app".to_string(),
        location: dir.path().display().to_string(),
        runtime_version: 17,
        build_tool_id: "maven".to_string(),
        language_id: "yaml".to_string(),
        ..Default::default()
    };
    let mut editor_state = test_support::editor_state();

    let error = create_and_open(&state, &mut editor_state).unwrap_err();
    assert!(!error.is_empty());
    assert!(!dir.path().join("my-app").exists());
    assert!(editor_state.project.is_none());
}

#[test]
fn runtime_versions_are_those_of_the_matching_pair_only() {
    let scaffolds = [("anvil", "khuzdul", &[2u32, 7][..]), ("anvil", "elvish", &[][..])];
    assert_eq!(runtime_versions_for(scaffolds, "anvil", "khuzdul"), vec![2, 7]);
    assert!(runtime_versions_for(scaffolds, "anvil", "elvish").is_empty());
    assert!(runtime_versions_for(scaffolds, "kiln", "khuzdul").is_empty());
}

/// Switching to a target that does not offer the current pick moves it to
/// that target's newest version, and to 0 for a target with none — a stale
/// pick from the previous target must never survive into a `ScaffoldSpec`.
#[test]
fn sync_runtime_version_keeps_the_pick_among_the_offered_versions() {
    let mut state = NewProjectWizardState {
        runtime_version: 8,
        ..Default::default()
    };
    sync_runtime_version(&mut state, &[8, 11, 17]);
    assert_eq!(state.runtime_version, 8, "an offered pick is kept");

    sync_runtime_version(&mut state, &[17, 21]);
    assert_eq!(state.runtime_version, 21);

    sync_runtime_version(&mut state, &[]);
    assert_eq!(state.runtime_version, 0);
}

/// A version the chosen target does not offer reaches neither the extension
/// nor the saved project config.
#[test]
fn create_and_open_drops_a_runtime_version_the_target_does_not_offer() {
    let dir = tempfile::tempdir().unwrap();
    let state = NewProjectWizardState {
        group_id: "com.example".to_string(),
        artifact_id: "my-app".to_string(),
        location: dir.path().display().to_string(),
        runtime_version: 0,
        build_tool_id: "maven".to_string(),
        language_id: "java".to_string(),
        ..Default::default()
    };
    let mut editor_state = test_support::editor_state();

    create_and_open(&state, &mut editor_state).expect("scaffolds and opens");

    let root = dir.path().join("my-app");
    assert_eq!(fg_core::load_project_config(&root).java_release, None);
    let pom = std::fs::read_to_string(root.join("pom.xml")).unwrap();
    assert!(!pom.contains(">0<"), "{pom}");
}
