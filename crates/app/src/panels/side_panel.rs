use std::path::PathBuf;

use fg_core::{EditorState, FileKind, FileNode};

/// Transient UI state for the side panel, owned by the caller across frames.
#[derive(Default)]
pub struct SidePanelState {
    /// (target directory, typed name so far).
    new_file_draft: Option<(PathBuf, String)>,
    rename_draft: Option<(PathBuf, String)>,
    pending_delete: Option<PathBuf>,
}

impl SidePanelState {
    /// Opens the "New File" input targeting `dir`. Used by both the side
    /// panel's own "New File…" button/context-menu entries and the menu
    /// bar's File > New File item.
    pub fn begin_new_file(&mut self, dir: PathBuf) {
        self.new_file_draft = Some((dir, String::new()));
    }
}

/// What the side panel wants the caller to do this frame.
#[derive(Default)]
pub struct SidePanelOutcome {
    /// A file (newly created or clicked in the tree) that should be opened.
    pub open: Option<PathBuf>,
    /// A file was renamed on disk; any tab pointing at `old` should be
    /// repointed to `new`.
    pub renamed: Option<(PathBuf, PathBuf)>,
    /// A file was deleted from disk; any tab pointing at it should close.
    pub deleted: Option<PathBuf>,
}

#[derive(Default)]
struct TreeActions {
    open: Option<PathBuf>,
    start_new_file: Option<PathBuf>,
    start_rename: Option<PathBuf>,
    cancel_rename: bool,
    confirm_rename: Option<String>,
    delete_request: Option<PathBuf>,
}

pub fn show(ui: &mut egui::Ui, state: &mut EditorState, panel: &mut SidePanelState) -> SidePanelOutcome {
    let mut outcome = SidePanelOutcome::default();

    ui.horizontal(|ui| {
        if ui.button("📁").on_hover_text("Open Folder").clicked() {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                if let Err(err) = state.open_project(folder) {
                    eprintln!("failed to open project: {err}");
                }
            }
        }
        if let Some(root) = state.project.as_ref().map(|p| p.root.clone()) {
            if ui.button("📄").on_hover_text("New File").clicked() {
                panel.begin_new_file(root);
            }
        }
    });

    let created = show_new_file_row(ui, state, panel, &mut outcome);

    ui.separator();

    let mut actions = TreeActions::default();
    if let Some(project) = &state.project {
        egui::ScrollArea::vertical().show(ui, |ui| {
            render_node(ui, &project.tree, &mut panel.rename_draft, &mut actions);
        });
    } else {
        ui.weak("No folder open");
    }

    apply_tree_actions(panel, actions, &mut outcome);
    show_delete_confirm(ui, panel, &mut outcome);

    // Only genuinely tree-changing actions need a refresh — opening an
    // *existing* file (also carried on `outcome.open`, via a tree click)
    // doesn't touch the filesystem, so re-walking the whole project for it
    // would be a pointless full directory read on every single file click.
    if created || outcome.renamed.is_some() || outcome.deleted.is_some() {
        if let Some(root) = state.project.as_ref().map(|p| p.root.clone()) {
            if let Err(err) = state.open_project(root) {
                eprintln!("failed to refresh project tree: {err}");
            }
        }
    }

    outcome
}

/// Returns `true` if a file was actually created this frame — distinct from
/// `outcome.open` (which this also sets), because the caller needs to know
/// specifically whether the on-disk tree changed and needs refreshing.
/// Opening an *existing* file (the tree-click path) also goes through
/// `outcome.open` but doesn't change the tree, so it must not trigger that
/// refresh.
fn show_new_file_row(
    ui: &mut egui::Ui,
    state: &EditorState,
    panel: &mut SidePanelState,
    outcome: &mut SidePanelOutcome,
) -> bool {
    let Some((dir, name)) = panel.new_file_draft.as_mut() else {
        return false;
    };
    let dir = dir.clone();
    let project_root = state.project.as_ref().map(|p| p.root.clone());

    let mut close_draft = false;
    let mut created = false;
    ui.horizontal(|ui| {
        let dir_label = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.display().to_string());
        ui.label(format!("New file in {dir_label}:"));

        let response = ui.text_edit_singleline(name);
        let confirmed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        if ui.button("Create").clicked() || confirmed {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                let new_path = dir.join(trimmed);
                if new_path.exists() {
                    eprintln!("file already exists: {}", new_path.display());
                } else {
                    let content = new_path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .and_then(fg_core::Language::from_extension)
                        .zip(project_root.as_ref())
                        .map(|(language, root)| fg_core::generate_boilerplate(language, root, &new_path))
                        .unwrap_or_default();

                    if let Err(err) = std::fs::write(&new_path, content) {
                        eprintln!("failed to create file: {err}");
                    } else {
                        outcome.open = Some(new_path);
                        close_draft = true;
                        created = true;
                    }
                }
            }
        }
        if ui.button("Cancel").clicked() {
            close_draft = true;
        }
    });

    if close_draft {
        panel.new_file_draft = None;
    }
    created
}

fn apply_tree_actions(panel: &mut SidePanelState, actions: TreeActions, outcome: &mut SidePanelOutcome) {
    if let Some(path) = actions.open {
        outcome.open = Some(path);
    }

    if let Some(dir) = actions.start_new_file {
        panel.begin_new_file(dir);
    }

    if let Some(path) = actions.start_rename {
        let default_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        panel.rename_draft = Some((path, default_name));
    }
    if actions.cancel_rename {
        panel.rename_draft = None;
    }
    if let Some(new_name) = actions.confirm_rename {
        if let Some((old_path, _)) = panel.rename_draft.take() {
            let new_name = new_name.trim();
            let new_path = old_path.parent().map(|p| p.join(new_name));
            match new_path {
                _ if new_name.is_empty() => eprintln!("rename failed: empty name"),
                Some(new_path) if new_path.exists() => {
                    eprintln!("rename failed: {} already exists", new_path.display());
                }
                Some(new_path) => match std::fs::rename(&old_path, &new_path) {
                    Ok(()) => outcome.renamed = Some((old_path, new_path)),
                    Err(err) => eprintln!("failed to rename: {err}"),
                },
                None => eprintln!("rename failed: no parent directory"),
            }
        }
    }

    if let Some(path) = actions.delete_request {
        panel.pending_delete = Some(path);
    }
}

fn show_delete_confirm(ui: &mut egui::Ui, panel: &mut SidePanelState, outcome: &mut SidePanelOutcome) {
    let Some(path) = panel.pending_delete.clone() else {
        return;
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let ctx = ui.ctx().clone();
    egui::Modal::new(egui::Id::new("delete_confirm")).show(&ctx, |ui| {
        ui.label(format!("Delete {name}? This cannot be undone."));
        ui.horizontal(|ui| {
            if ui.button("Delete").clicked() {
                match std::fs::remove_file(&path) {
                    Ok(()) => outcome.deleted = Some(path.clone()),
                    Err(err) => eprintln!("failed to delete: {err}"),
                }
                panel.pending_delete = None;
            }
            if ui.button("Cancel").clicked() {
                panel.pending_delete = None;
            }
        });
    });
}

fn render_node(
    ui: &mut egui::Ui,
    node: &FileNode,
    rename_draft: &mut Option<(PathBuf, String)>,
    actions: &mut TreeActions,
) {
    match node.kind {
        FileKind::Dir => {
            let header = egui::CollapsingHeader::new(format!("📁 {}", node.name))
                .id_salt(&node.path)
                .default_open(false)
                .show(ui, |ui| {
                    for child in &node.children {
                        render_node(ui, child, rename_draft, actions);
                    }
                });
            header.header_response.context_menu(|ui| {
                if ui.button("New File").clicked() {
                    actions.start_new_file = Some(node.path.clone());
                    ui.close();
                }
            });
        }
        FileKind::File => {
            let is_being_renamed = rename_draft.as_ref().is_some_and(|(p, _)| p == &node.path);

            if is_being_renamed {
                let (_, name) = rename_draft.as_mut().expect("checked above");
                ui.horizontal(|ui| {
                    let response = ui.text_edit_singleline(name);
                    let confirmed =
                        response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if confirmed || ui.small_button("✓").clicked() {
                        actions.confirm_rename = Some(name.clone());
                    }
                    if ui.small_button("✗").clicked() {
                        actions.cancel_rename = true;
                    }
                });
                return;
            }

            let extension = node.path.extension().and_then(|ext| ext.to_str());
            let icon = match extension {
                Some("java") => "☕ ",
                Some("kt") => "🔷 ",
                _ => "📄 ",
            };
            let label_text = format!("{icon}{}", node.name);

            // Any file can be opened — `Document::open` only fails on files
            // that aren't valid UTF-8 text (binaries, etc), and that failure
            // is reported when the open is actually attempted, not guessed
            // at here from the extension alone.
            let response = ui.selectable_label(false, label_text);
            if response.clicked() {
                actions.open = Some(node.path.clone());
            }

            response.context_menu(|ui| {
                if ui.button("New File").clicked() {
                    let dir = node.path.parent().map_or_else(|| node.path.clone(), PathBuf::from);
                    actions.start_new_file = Some(dir);
                    ui.close();
                }
                if ui.button("Rename").clicked() {
                    actions.start_rename = Some(node.path.clone());
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    actions.delete_request = Some(node.path.clone());
                    ui.close();
                }
            });
        }
    }
}
