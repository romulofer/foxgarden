//! File > New Project… — scaffolds a brand-new Maven+Java project through
//! `fg_core::scaffold`/`project_config` (`PLAN.md` Track 29 Phase 3) and
//! opens it, the same `EditorState::open_project` "Open Folder…" already
//! calls. Built on `widgets::modal::show_modal`, `run_configs.rs`'s own
//! template — the right one here since this wizard has a real terminal
//! "Create" action, unlike a settings dialog that just edits fields in
//! place.

use std::path::{Path, PathBuf};

use fg_core::{BuildTool, EditorState, ProjectConfig, ProjectLanguage, ScaffoldSpec};
use fg_i18n::{msg, t};

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
    build_tool: BuildTool,
    language: ProjectLanguage,
    last_error: Option<String>,
    picker: crate::folder_picker::FolderPicker,
}

impl NewProjectWizardState {
    pub fn open(&mut self) {
        self.group_id.clear();
        self.artifact_id.clear();
        self.location.clear();
        self.java_release = 21;
        self.build_tool = BuildTool::Maven;
        self.language = ProjectLanguage::Java;
        self.last_error = None;
        self.open = true;
    }

    pub fn picker_running(&self) -> bool {
        self.picker.is_open()
    }

    /// Same shape as `panels::jdk_registry::JdkRegistryState::poll_picker`.
    pub fn poll_picker(&mut self) -> Option<PathBuf> {
        self.picker.poll()
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
///
/// Cancel/Escape close the dialog without creating anything — driven by
/// `show_modal`'s own `(result, escape_pressed)` outcome, same as every
/// other dialog in this app (`jdk_registry.rs`'s own `show_settings` is the
/// closest template). An earlier version of this function called
/// `show_modal` for its side effects only and never looked at that return
/// value at all, so Cancel's own `clicked()` was computed and then
/// silently discarded — the dialog only ever closed via a successful
/// Create. Regression test: `new_project_dialog_opens_and_cancel_closes_it`
/// in `app::e2e_test::menus_test`.
pub fn show(ui: &egui::Ui, state: &mut NewProjectWizardState, editor_state: &mut EditorState) {
    if let Some(folder) = state.poll_picker() {
        state.location = folder.display().to_string();
    }
    if state.picker_running() {
        ui.ctx().request_repaint();
    }

    let mut created = false;
    let outcome = show_modal(ui, "new_project_wizard", state.open.then_some(()), |ui, ()| {
        ui.set_min_width(480.0);
        ui.heading(t().new_project.heading);
        ui.label(egui::RichText::new(t().new_project.description).weak());
        ui.separator();

        egui::Grid::new("new_project_form")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(t().new_project.group_id);
                ui.add(egui::TextEdit::singleline(&mut state.group_id).hint_text(t().new_project.group_id_hint));
                ui.end_row();

                ui.label(t().new_project.artifact_id);
                ui.add(egui::TextEdit::singleline(&mut state.artifact_id).hint_text(t().new_project.artifact_id_hint));
                ui.end_row();

                ui.label(t().new_project.location);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut state.location).hint_text(t().new_project.location_hint));
                    if ui
                        .add_enabled(!state.picker_running(), egui::Button::new(t().new_project.browse))
                        .clicked()
                    {
                        state.picker.open(None);
                    }
                });
                ui.end_row();

                ui.label(t().new_project.java_release);
                egui::ComboBox::new("new_project_java_release", "")
                    .selected_text(state.java_release.to_string())
                    .show_ui(ui, |ui| {
                        for release in JAVA_RELEASES {
                            ui.selectable_value(&mut state.java_release, release, release.to_string());
                        }
                    });
                ui.end_row();

                ui.label(t().new_project.build_tool);
                egui::ComboBox::new("new_project_build_tool", "")
                    .selected_text(build_tool_label(state.build_tool))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut state.build_tool,
                            BuildTool::Maven,
                            t().new_project.build_tool_maven,
                        );
                        ui.selectable_value(
                            &mut state.build_tool,
                            BuildTool::Gradle,
                            t().new_project.build_tool_gradle,
                        );
                    });
                ui.end_row();

                ui.label(t().new_project.language);
                egui::ComboBox::new("new_project_language", "")
                    .selected_text(language_label(state.language))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut state.language,
                            ProjectLanguage::Java,
                            t().new_project.language_java,
                        );
                        ui.selectable_value(
                            &mut state.language,
                            ProjectLanguage::Kotlin,
                            t().new_project.language_kotlin,
                        );
                    });
                ui.end_row();
            });

        if state.build_tool == BuildTool::Gradle {
            ui.label(egui::RichText::new(t().new_project.gradle_no_wrapper_hint).weak());
        }

        if !state.location.trim().is_empty() && !state.artifact_id.trim().is_empty() {
            let preview = msg::will_create_project(&project_root(state).display().to_string());
            ui.label(egui::RichText::new(preview).weak());
        }

        if let Some(error) = &state.last_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(form_is_valid(state), egui::Button::new(t().new_project.create))
                .clicked()
            {
                created = true;
            }
            ui.button(t().common.cancel).clicked()
        })
        .inner
    });

    if created {
        match create_and_open(state, editor_state) {
            Ok(()) => state.open = false,
            Err(error) => state.last_error = Some(error),
        }
    }

    if let Some((cancel_clicked, escape_pressed)) = outcome
        && (cancel_clicked || escape_pressed)
    {
        state.open = false;
    }
}

fn build_tool_label(build_tool: BuildTool) -> &'static str {
    match build_tool {
        BuildTool::Maven => t().new_project.build_tool_maven,
        BuildTool::Gradle => t().new_project.build_tool_gradle,
    }
}

fn language_label(language: ProjectLanguage) -> &'static str {
    match language {
        ProjectLanguage::Java => t().new_project.language_java,
        ProjectLanguage::Kotlin => t().new_project.language_kotlin,
    }
}

fn create_and_open(state: &NewProjectWizardState, editor_state: &mut EditorState) -> Result<(), String> {
    let root = project_root(state);
    let spec = ScaffoldSpec {
        group_id: state.group_id.trim().to_string(),
        artifact_id: state.artifact_id.trim().to_string(),
        java_release: state.java_release,
        build_tool: state.build_tool,
        language: state.language,
    };
    fg_core::write_scaffold(&root, &fg_core::scaffold_files(&spec)).map_err(|e| msg::scaffold_failed(&e))?;

    let config = ProjectConfig {
        java_release: Some(state.java_release),
        jdk_home: None,
    };
    fg_core::save_project_config(&root, &config)
        .map_err(|e| msg::scaffolded_but_config_save_failed(&root.display().to_string(), &e.to_string()))?;

    editor_state
        .open_project(root.clone())
        .map_err(|e| msg::scaffolded_but_open_failed(&root.display().to_string(), &e.to_string()))
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
#[path = "new_project_test.rs"]
mod new_project_test;
