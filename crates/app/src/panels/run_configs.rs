use fg_i18n::t;
use std::path::{Path, PathBuf};

use fg_core::RunConfig;

use crate::widgets::modal::show_modal;

/// Transient state for the Run > Edit Configurations… dialog, owned by the
/// caller across frames. `configs` is loaded from disk (`fg_core::
/// load_run_configs`) once, when the dialog opens (`open`), and written
/// back (`fg_core::save_run_configs`) once, when it closes — every field
/// edit in between just mutates this in-memory copy directly, the same
/// "no explicit save step, the fields themselves are the source of truth
/// while open" shape the side panel's rename field already uses, rather
/// than re-writing the file on every keystroke.
#[derive(Default)]
pub struct RunConfigsDialogState {
    open: bool,
    configs: Vec<RunConfig>,
    selected: usize,
    /// A new environment-variable row being typed, not yet added to the
    /// selected config's `env` list.
    new_env_key: String,
    new_env_value: String,
    /// The working-directory "Browse…" dialog, when one is up — on its own
    /// thread (`crate::folder_picker`) so a portal dialog that takes its
    /// time can't freeze the editor underneath this one.
    working_dir_picker: crate::folder_picker::FolderPicker,
}

impl RunConfigsDialogState {
    /// Loads whatever's already saved for `project_root` and opens the
    /// dialog on it.
    pub fn open(&mut self, project_root: &Path) {
        self.configs = fg_core::load_run_configs(project_root);
        self.selected = 0;
        self.new_env_key.clear();
        self.new_env_value.clear();
        self.open = true;
    }
}

/// Draws the dialog if `state.open`, saving `state.configs` back to
/// `project_root` and closing it once "Close" is clicked or Escape is
/// pressed. Any save failure is reported through `last_error`, same as
/// every other file-writing failure in this app.
pub fn show(ui: &egui::Ui, project_root: &Path, state: &mut RunConfigsDialogState, last_error: &mut Option<String>) {
    let mut close_clicked = false;

    let outcome = show_modal(ui, "run_configs_dialog", state.open.then_some(()), |ui, ()| {
        ui.set_min_width(560.0);
        ui.heading(t().run_configs.heading);

        ui.horizontal(|ui| {
            show_config_list(ui, state);
            ui.separator();
            show_selected_config_fields(ui, project_root, state);
        });

        ui.separator();
        close_clicked = ui.button(t().common.close).clicked();
    });

    let escape_pressed = outcome.is_some_and(|(_, escape)| escape);
    if close_clicked || escape_pressed {
        state.open = false;
        if let Err(err) = fg_core::save_run_configs(project_root, &state.configs) {
            crate::errors::report(last_error, format!("failed to save run configurations: {err}"));
        }
    }
}

fn show_config_list(ui: &mut egui::Ui, state: &mut RunConfigsDialogState) {
    ui.vertical(|ui| {
        ui.set_width(160.0);
        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
            for (index, config) in state.configs.iter().enumerate() {
                let label = if config.name.is_empty() {
                    "(unnamed)"
                } else {
                    config.name.as_str()
                };
                if ui.selectable_label(index == state.selected, label).clicked() {
                    state.selected = index;
                }
            }
        });

        ui.horizontal(|ui| {
            if ui.button(t().run_configs.new).clicked() {
                state.configs.push(RunConfig::default());
                state.selected = state.configs.len() - 1;
            }
            let has_selection = !state.configs.is_empty();
            if ui.add_enabled(has_selection, egui::Button::new(t().run_configs.duplicate)).clicked() {
                let mut copy = state.configs[state.selected].clone();
                copy.name = format!("{} (copy)", copy.name);
                state.configs.insert(state.selected + 1, copy);
                state.selected += 1;
            }
            if ui.add_enabled(has_selection, egui::Button::new(t().common.delete)).clicked() {
                state.configs.remove(state.selected);
                state.selected = state.selected.min(state.configs.len().saturating_sub(1));
            }
        });
    });
}

fn show_selected_config_fields(ui: &mut egui::Ui, project_root: &Path, state: &mut RunConfigsDialogState) {
    let Some(config) = state.configs.get_mut(state.selected) else {
        ui.weak(t().run_configs.empty_hint);
        return;
    };

    ui.vertical(|ui| {
        egui::Grid::new("run_config_fields").num_columns(2).show(ui, |ui| {
            ui.label(t().run_configs.name);
            ui.text_edit_singleline(&mut config.name);
            ui.end_row();

            ui.label(t().run_configs.main_class);
            ui.text_edit_singleline(&mut config.main_class);
            ui.end_row();

            ui.label(t().run_configs.vm_args);
            ui.text_edit_singleline(&mut config.vm_args);
            ui.end_row();

            ui.label(t().run_configs.program_args);
            ui.text_edit_singleline(&mut config.program_args);
            ui.end_row();

            ui.label(t().run_configs.working_dir);
            ui.horizontal(|ui| {
                let mut dir_text = config
                    .working_dir
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                if ui.text_edit_singleline(&mut dir_text).changed() {
                    config.working_dir = (!dir_text.is_empty()).then(|| PathBuf::from(&dir_text));
                }
                if ui
                    .add_enabled(!state.working_dir_picker.is_open(), egui::Button::new(t().run_configs.browse))
                    .clicked()
                {
                    state.working_dir_picker.open(Some(project_root.to_path_buf()));
                }
                if let Some(dir) = state.working_dir_picker.poll() {
                    config.working_dir = Some(dir);
                }
            });
            ui.end_row();
        });

        ui.separator();
        ui.label(t().run_configs.env_vars);
        let mut remove_index = None;
        for (index, (key, value)) in config.env.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(key);
                ui.label("=");
                ui.text_edit_singleline(value);
                if ui.small_button("✗").clicked() {
                    remove_index = Some(index);
                }
            });
        }
        if let Some(index) = remove_index {
            config.env.remove(index);
        }

        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut state.new_env_key);
            ui.label("=");
            ui.text_edit_singleline(&mut state.new_env_value);
            if ui.button(t().run_configs.add).clicked() && !state.new_env_key.is_empty() {
                let key = std::mem::take(&mut state.new_env_key);
                let value = std::mem::take(&mut state.new_env_value);
                config.env.push((key, value));
            }
        });
    });
}
