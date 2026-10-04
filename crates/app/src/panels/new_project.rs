//! File > New Project… — scaffolds a brand-new project and opens it, the
//! same `EditorState::open_project` "Open Folder…" already calls. *What*
//! files a project of the chosen kind needs comes from whichever extension
//! contributes that build tool/language pair (`Registry::scaffold_files`);
//! this panel only collects the answers and writes the result out through
//! `fg_core::write_scaffold`. Built on `widgets::modal::show_modal`, `run_configs.rs`'s own
//! template — the right one here since this wizard has a real terminal
//! "Create" action, unlike a settings dialog that just edits fields in
//! place.

use std::path::{Path, PathBuf};

use fg_core::{EditorState, ProjectConfig, ScaffoldSpec};
use fg_extension::Registry;
use fg_i18n::{msg, t};

use crate::jdk_registry::JdkRegistry;
use crate::widgets::modal::show_modal;

/// One scaffoldable project kind, flattened out of the registry for the
/// wizard's own combo boxes: ids to put in a `ScaffoldSpec`, display names
/// to show, and whichever runtime versions that target offers.
struct ScaffoldOption {
    build_tool_id: String,
    build_tool_name: String,
    language_id: String,
    language_name: String,
    runtime_versions: Vec<u32>,
}

/// Every target any registered extension scaffolds. Built once per frame
/// while the dialog is open (and not at all while it is closed) rather than
/// held in the state: nothing is registered
/// after startup, and a `Registry` borrow cannot live inside the modal's
/// own closure next to a mutable `EditorState`.
fn scaffold_options(languages: &Registry) -> Vec<ScaffoldOption> {
    languages
        .scaffolds()
        .into_iter()
        .map(|scaffold| ScaffoldOption {
            build_tool_id: scaffold.build_tool_id.clone(),
            build_tool_name: languages
                .build_tool(&scaffold.build_tool_id)
                .map(|tool| tool.display_name)
                .unwrap_or_else(|| scaffold.build_tool_id.clone()),
            language_id: scaffold.language_id.clone(),
            language_name: languages
                .language(&scaffold.language_id)
                .map(|language| language.language.display_name.clone())
                .unwrap_or_else(|| scaffold.language_id.clone()),
            runtime_versions: scaffold.runtime_versions.clone(),
        })
        .collect()
}

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
    runtime_version: u32,
    build_tool_id: String,
    language_id: String,
    last_error: Option<String>,
    picker: crate::folder_picker::FolderPicker,
}

impl NewProjectWizardState {
    /// Opens the dialog, defaulting to the first target the registry offers
    /// — "the first registered extension's first scaffold" rather than a
    /// hardcoded Maven+Java, which this panel is no longer allowed to name.
    pub fn open(&mut self, languages: &Registry) {
        self.group_id.clear();
        self.artifact_id.clear();
        self.location.clear();
        self.last_error = None;
        self.open = true;

        let first = scaffold_options(languages).into_iter().next();
        match first {
            Some(option) => {
                self.runtime_version = option.runtime_versions.last().copied().unwrap_or_default();
                self.build_tool_id = option.build_tool_id;
                self.language_id = option.language_id;
            }
            None => {
                self.runtime_version = 0;
                self.build_tool_id.clear();
                self.language_id.clear();
            }
        }
    }

    pub fn picker_running(&self) -> bool {
        self.picker.is_open()
    }

    /// Same shape as `panels::jdk_registry::JdkRegistryState::poll_picker`.
    pub fn poll_picker(&mut self) -> Option<PathBuf> {
        self.picker.poll()
    }
}

/// The runtime versions the chosen build tool/language pair offers — empty
/// when it offers none, or when no target matches the pair at all.
fn runtime_versions_for<'a>(
    scaffolds: impl IntoIterator<Item = (&'a str, &'a str, &'a [u32])>,
    build_tool_id: &str,
    language_id: &str,
) -> Vec<u32> {
    scaffolds
        .into_iter()
        .find(|(tool, language, _)| *tool == build_tool_id && *language == language_id)
        .map(|(_, _, versions)| versions.to_vec())
        .unwrap_or_default()
}

/// Keeps `runtime_version` one that `offered` actually lists: the newest
/// offered one when the current pick is not among them (switching to a
/// target that does not offer it), and 0 when the target offers none.
fn sync_runtime_version(state: &mut NewProjectWizardState, offered: &[u32]) {
    match offered.last() {
        None => state.runtime_version = 0,
        Some(&newest) if !offered.contains(&state.runtime_version) => state.runtime_version = newest,
        Some(_) => {}
    }
}

fn project_root(state: &NewProjectWizardState) -> PathBuf {
    Path::new(&state.location).join(&state.artifact_id)
}

/// `true` once every field holds something the chosen extension's scaffold
/// could actually act on — checked before enabling Create rather than
/// after clicking it, so a half-filled form can't produce a confusing
/// mid-air failure.
fn form_is_valid(state: &NewProjectWizardState) -> bool {
    !state.group_id.trim().is_empty()
        && !state.artifact_id.trim().is_empty()
        && !state.location.trim().is_empty()
        && !state.build_tool_id.is_empty()
        && !state.language_id.is_empty()
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
    // Called every frame from `app.rs`; everything below costs allocations
    // that only an open dialog has any use for.
    if !state.open {
        return;
    }
    // Read out of the registry before the modal's closure borrows
    // `editor_state` mutably to open the created project.
    let options = scaffold_options(&editor_state.languages);

    // Switching build tool can strand a language that tool has no scaffold
    // for, and switching target can strand a runtime version the new one
    // does not offer; both fall back before anything is drawn, rather than
    // leaving Create enabled on a choice nothing generates.
    let languages_for_tool: Vec<(String, String)> = unique_by(
        options.iter().filter(|o| o.build_tool_id == state.build_tool_id),
        |o| (o.language_id.clone(), o.language_name.clone()),
    );
    if !languages_for_tool.iter().any(|(id, _)| *id == state.language_id)
        && let Some((id, _)) = languages_for_tool.first()
    {
        state.language_id = id.clone();
    }
    let runtime_versions = runtime_versions_for(
        options
            .iter()
            .map(|o| (o.build_tool_id.as_str(), o.language_id.as_str(), o.runtime_versions.as_slice())),
        &state.build_tool_id,
        &state.language_id,
    );
    sync_runtime_version(state, &runtime_versions);

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

                if !runtime_versions.is_empty() {
                    ui.label(t().new_project.java_release);
                    egui::ComboBox::new("new_project_java_release", "")
                        .selected_text(state.runtime_version.to_string())
                        .show_ui(ui, |ui| {
                            for release in &runtime_versions {
                                ui.selectable_value(&mut state.runtime_version, *release, release.to_string());
                            }
                        });
                    ui.end_row();
                }

                ui.label(t().new_project.build_tool);
                egui::ComboBox::new("new_project_build_tool", "")
                    .selected_text(label_for(&options, |o| {
                        (o.build_tool_id == state.build_tool_id).then(|| o.build_tool_name.clone())
                    }))
                    .show_ui(ui, |ui| {
                        for (id, name) in unique_by(&options, |o| (o.build_tool_id.clone(), o.build_tool_name.clone()))
                        {
                            ui.selectable_value(&mut state.build_tool_id, id, name);
                        }
                    });
                ui.end_row();

                ui.label(t().new_project.language);
                egui::ComboBox::new("new_project_language", "")
                    .selected_text(label_for(&options, |o| {
                        (o.language_id == state.language_id).then(|| o.language_name.clone())
                    }))
                    .show_ui(ui, |ui| {
                        for (id, name) in languages_for_tool {
                            ui.selectable_value(&mut state.language_id, id, name);
                        }
                    });
                ui.end_row();
            });

        // Still keyed by tool id: the hint is localized here, and an
        // extension has no locale to supply its own notes in yet — a Phase B
        // question (see `SPEC.md` §24 on deferring a UI contribution API).
        if state.build_tool_id == "gradle" {
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

/// The display name for whichever option `pick` matches, or an empty label
/// when nothing is selected yet (no extension contributes a scaffold at all).
fn label_for(options: &[ScaffoldOption], pick: impl Fn(&ScaffoldOption) -> Option<String>) -> String {
    options.iter().find_map(pick).unwrap_or_default()
}

/// `(id, display name)` pairs in registration order, first occurrence
/// winning — a build tool appears once in its own picker even though it has
/// one scaffold target per language.
fn unique_by<'a>(
    options: impl IntoIterator<Item = &'a ScaffoldOption>,
    key: impl Fn(&ScaffoldOption) -> (String, String),
) -> Vec<(String, String)> {
    let mut seen = Vec::new();
    for option in options {
        let pair = key(option);
        if !seen.iter().any(|(id, _)| *id == pair.0) {
            seen.push(pair);
        }
    }
    seen
}

fn create_and_open(state: &NewProjectWizardState, editor_state: &mut EditorState) -> Result<(), String> {
    let root = project_root(state);
    // Only a version the chosen target offers is passed on: anything else
    // (0 for a target with none, or one left over from another target) is
    // `None`, which leaves the extension to its own default.
    let offered = runtime_versions_for(
        editor_state
            .languages
            .scaffolds()
            .into_iter()
            .map(|s| (s.build_tool_id.as_str(), s.language_id.as_str(), s.runtime_versions.as_slice())),
        &state.build_tool_id,
        &state.language_id,
    );
    let runtime_version = offered.contains(&state.runtime_version).then_some(state.runtime_version);
    let spec = ScaffoldSpec {
        namespace: state.group_id.trim().to_string(),
        name: state.artifact_id.trim().to_string(),
        runtime_version,
        build_tool_id: state.build_tool_id.clone(),
        language_id: state.language_id.clone(),
    };
    let files = editor_state
        .languages
        .scaffold_files(&spec)
        .ok_or_else(|| msg::scaffold_failed(t().new_project.no_scaffold_for_target))?;
    fg_core::write_scaffold(&root, &files).map_err(|e| msg::scaffold_failed(&e))?;

    let config = ProjectConfig {
        java_release: runtime_version,
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
