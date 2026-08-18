//! File > New Project… — scaffolds a brand-new Maven+Java project through
//! `fg_core::scaffold`/`project_config` (`PLAN.md` Track 29 Phase 3) and
//! opens it, the same `EditorState::open_project` "Open Folder…" already
//! calls. Built on `widgets::modal::show_modal`, `run_configs.rs`'s own
//! template — the right one here since this wizard has a real terminal
//! "Create" action, unlike a settings dialog that just edits fields in
//! place.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use fg_core::{BuildTool, EditorState, ProjectConfig, ProjectLanguage, ScaffoldSpec};

use crate::jdk_registry::JdkRegistry;
use crate::widgets::modal::show_modal;

/// The Java releases offered in the wizard's own picker — every one
/// `lsp_manager::execution_environment_name` (Track 29 Phase 2) already
/// names correctly for jdt.ls, from the oldest release a real project still
/// targets in practice to the newest LTS.
const JAVA_RELEASES: [u32; 4] = [8, 11, 17, 21];

#[derive(Default)]
pub struct NewProjectWizardState {
    open: bool,
    group_id: String,
    artifact_id: String,
    /// The parent directory the new project folder (named after
    /// `artifact_id`) is created under — text field plus a Browse… button,
    /// same shape `run_configs.rs`'s own working-dir field uses, except the
    /// folder-picker call itself runs on a background thread (see
    /// `picker_rx`) rather than blocking the UI thread the way `run_
    /// configs.rs`'s own Browse… still does (TECHNICAL_DEBT.md #23 — no
    /// reason to add a fifth instance of a bug this session already fixed
    /// once).
    location: String,
    java_release: u32,
    last_error: Option<String>,
    picker_rx: Option<Receiver<Option<PathBuf>>>,
}

impl NewProjectWizardState {
    pub fn open(&mut self) {
        self.group_id.clear();
        self.artifact_id.clear();
        self.location.clear();
        self.java_release = 21;
        self.last_error = None;
        self.open = true;
    }

    pub fn picker_running(&self) -> bool {
        self.picker_rx.is_some()
    }

    /// Same shape as `panels::jdk_registry::JdkRegistryState::poll_picker`.
    pub fn poll_picker(&mut self) -> Option<PathBuf> {
        let rx = self.picker_rx.as_ref()?;
        match rx.try_recv() {
            Ok(folder) => {
                self.picker_rx = None;
                folder
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.picker_rx = None;
                None
            }
        }
    }
}

fn project_root(state: &NewProjectWizardState) -> PathBuf {
    Path::new(&state.location).join(&state.artifact_id)
}

/// `true` once every field holds something `scaffold_files`/`write_scaffold`
/// could actually act on — checked before enabling Create rather than
/// after clicking it, so a half-filled form can't produce a confusing
/// mid-air failure.
fn form_is_valid(state: &NewProjectWizardState) -> bool {
    !state.group_id.trim().is_empty() && !state.artifact_id.trim().is_empty() && !state.location.trim().is_empty()
}

/// Draws the dialog if `state.open`. On a successful Create, scaffolds the
/// project, saves its `ProjectConfig`, opens it via `editor_state::
/// open_project` (mutating `editor_state` in place, the exact function
/// "Open Folder…" already calls), and closes the dialog. Any failure
/// (scaffold refused a non-empty directory, the write itself failed, the
/// subsequent open failed) is reported inline in the dialog rather than
/// through the app-wide error banner — the user is still looking right at
/// the form that needs fixing.
pub fn show(ui: &egui::Ui, state: &mut NewProjectWizardState, editor_state: &mut EditorState) {
    if let Some(folder) = state.poll_picker() {
        state.location = folder.display().to_string();
    }
    if state.picker_running() {
        ui.ctx().request_repaint();
    }

    let mut created = false;
    show_modal(ui, "new_project_wizard", state.open.then_some(()), |ui, ()| {
        ui.set_min_width(480.0);
        ui.heading("New Project");
        ui.label(
            egui::RichText::new("Creates a new Maven + Java project and opens it. Gradle and Kotlin aren't supported by this wizard yet.")
                .weak(),
        );
        ui.separator();

        egui::Grid::new("new_project_form").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Group ID");
            ui.text_edit_singleline(&mut state.group_id);
            ui.end_row();

            ui.label("Artifact ID");
            ui.text_edit_singleline(&mut state.artifact_id);
            ui.end_row();

            ui.label("Location");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut state.location);
                if ui.add_enabled(!state.picker_running(), egui::Button::new("Browse…")).clicked() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(rfd::FileDialog::new().pick_folder());
                    });
                    state.picker_rx = Some(rx);
                }
            });
            ui.end_row();

            ui.label("Java release");
            egui::ComboBox::new("new_project_java_release", "")
                .selected_text(state.java_release.to_string())
                .show_ui(ui, |ui| {
                    for release in JAVA_RELEASES {
                        ui.selectable_value(&mut state.java_release, release, release.to_string());
                    }
                });
            ui.end_row();
        });

        if !state.location.trim().is_empty() && !state.artifact_id.trim().is_empty() {
            ui.label(egui::RichText::new(format!("Will create: {}", project_root(state).display())).weak());
        }

        if let Some(error) = &state.last_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(form_is_valid(state), egui::Button::new("Create")).clicked() {
                created = true;
            }
            ui.button("Cancel").clicked()
        })
        .inner
    });

    if created {
        match create_and_open(state, editor_state) {
            Ok(()) => state.open = false,
            Err(error) => state.last_error = Some(error),
        }
    }
}

fn create_and_open(state: &NewProjectWizardState, editor_state: &mut EditorState) -> Result<(), String> {
    let root = project_root(state);
    let spec = ScaffoldSpec {
        group_id: state.group_id.trim().to_string(),
        artifact_id: state.artifact_id.trim().to_string(),
        java_release: state.java_release,
        build_tool: BuildTool::Maven,
        language: ProjectLanguage::Java,
    };
    fg_core::write_scaffold(&root, &fg_core::scaffold_files(&spec))?;

    let config = ProjectConfig { java_release: Some(state.java_release), jdk_home: None };
    fg_core::save_project_config(&root, &config).map_err(|e| format!("scaffolded {} but couldn't save its project config: {e}", root.display()))?;

    editor_state
        .open_project(root.clone())
        .map_err(|e| format!("scaffolded {} but couldn't open it: {e}", root.display()))
}

/// Not wired into `show`'s own UI yet — `JdkRegistry` only gains a real
/// caller once the wizard offers targeting a specific registered JDK
/// (`jdk_home` in `ProjectConfig`) rather than leaving it to auto-detection
/// the way `create_and_open` does today. Kept and exported so the intent is
/// visible in the panel's own signature rather than a silent gap.
#[allow(dead_code)]
pub fn registered_jdks_for(java_release: u32, registry: &JdkRegistry) -> Option<PathBuf> {
    registry.closest_for(java_release).map(|jdk| jdk.home.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_resets_every_field_and_opens_the_dialog() {
        let mut state = NewProjectWizardState {
            open: false,
            group_id: "stale".to_string(),
            artifact_id: "stale".to_string(),
            location: "/stale".to_string(),
            java_release: 8,
            last_error: Some("stale error".to_string()),
            picker_rx: None,
        };
        state.open();

        assert!(state.open);
        assert_eq!(state.group_id, "");
        assert_eq!(state.artifact_id, "");
        assert_eq!(state.location, "");
        assert_eq!(state.java_release, 21);
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
        assert_eq!(editor_state.project.as_ref().map(|p| p.root.clone()), Some(root.clone()));
        assert_eq!(fg_core::load_project_config(&root).java_release, Some(17));
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
}
