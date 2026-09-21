
use super::*;

#[test]
fn open_resets_every_field_and_opens_the_dialog() {
    let mut state = NewProjectWizardState {
        open: false,
        group_id: "stale".to_string(),
        artifact_id: "stale".to_string(),
        location: "/stale".to_string(),
        java_release: 8,
        build_tool: BuildTool::Gradle,
        language: ProjectLanguage::Kotlin,
        last_error: Some("stale error".to_string()),
        picker: crate::folder_picker::FolderPicker::default(),
    };
    state.open();

    assert!(state.open);
    assert_eq!(state.group_id, "");
    assert_eq!(state.artifact_id, "");
    assert_eq!(state.location, "");
    assert_eq!(state.java_release, 21);
    assert_eq!(state.build_tool, BuildTool::Maven);
    assert_eq!(state.language, ProjectLanguage::Java);
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
        java_release: 17,
        ..Default::default()
    };
    let mut editor_state = EditorState::default();

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
        java_release: 17,
        build_tool: BuildTool::Gradle,
        ..Default::default()
    };
    let mut editor_state = EditorState::default();

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
        java_release: 17,
        build_tool: BuildTool::Gradle,
        language: ProjectLanguage::Kotlin,
        ..Default::default()
    };
    let mut editor_state = EditorState::default();

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
        java_release: 17,
        ..Default::default()
    };
    let mut editor_state = EditorState::default();

    let error = create_and_open(&state, &mut editor_state).unwrap_err();
    assert!(!error.is_empty());
    assert!(editor_state.project.is_none());
}
