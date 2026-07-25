use std::path::{Path, PathBuf};

use fg_core::{EditorState, FileKind, FileNode};

use crate::terminal;
use crate::widgets::modal::show_modal;

/// Which action a previous Copy/Cut left waiting for Paste — `SidePanelState
/// ::clipboard`'s tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOp {
    Copy,
    Cut,
}

/// Transient UI state for the side panel, owned by the caller across frames.
#[derive(Default)]
pub struct SidePanelState {
    /// (target directory, typed name so far).
    new_file_draft: Option<(PathBuf, String)>,
    rename_draft: Option<(PathBuf, String)>,
    pending_delete: Option<PathBuf>,
    /// Set whenever `new_file_draft`/`rename_draft` is freshly opened,
    /// consumed (cleared) by the very next frame that draws the
    /// corresponding text field — so a freshly opened "New File"/rename
    /// input grabs keyboard focus immediately instead of requiring an
    /// extra click before the user can type.
    focus_new_file: bool,
    focus_rename: bool,
    /// The file/directory a Copy or Cut is waiting to be pasted somewhere —
    /// set by a tree node's "Copy"/"Cut" context-menu entry, read (and, for
    /// `Cut`, cleared) by a directory's "Paste" entry. `Copy` stays here
    /// across a paste (so it can be pasted again elsewhere, same as an OS
    /// file manager); `Cut` is one-shot.
    clipboard: Option<(PathBuf, ClipboardOp)>,
}

impl SidePanelState {
    /// Opens the "New File" input targeting `dir`. Used by both the side
    /// panel's own "New File…" button/context-menu entries and the menu
    /// bar's File > New File item.
    pub fn begin_new_file(&mut self, dir: PathBuf) {
        self.new_file_draft = Some((dir, String::new()));
        self.focus_new_file = true;
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
    /// A user-facing message for a failure this frame (open project,
    /// create/rename/delete file, refresh tree, ...), for the caller to
    /// surface through the app's shared error modal. `Some` overwrites
    /// whatever the caller was already holding — last error wins, same as
    /// every other single-slot outcome field here.
    pub error: Option<String>,
}

#[derive(Default)]
struct TreeActions {
    open: Option<PathBuf>,
    start_new_file: Option<PathBuf>,
    start_rename: Option<PathBuf>,
    cancel_rename: bool,
    confirm_rename: Option<String>,
    delete_request: Option<PathBuf>,
    copy_request: Option<PathBuf>,
    cut_request: Option<PathBuf>,
    /// The directory a "Paste" click targets.
    paste_request: Option<PathBuf>,
}

pub fn show(ui: &mut egui::Ui, state: &mut EditorState, panel: &mut SidePanelState) -> SidePanelOutcome {
    let mut outcome = SidePanelOutcome::default();

    ui.horizontal(|ui| {
        if ui.button("📁").on_hover_text("Open Folder").clicked() {
            // Default to the already-open project's folder, if there is
            // one, so re-opening (a sibling folder, or the same project
            // after it was closed) doesn't mean re-navigating away from
            // wherever the OS's own default (home, Desktop, ...) happens
            // to be every single time.
            let mut dialog = rfd::FileDialog::new();
            if let Some(root) = state.project.as_ref().map(|p| p.root.clone()) {
                dialog = dialog.set_directory(root);
            }
            if let Some(folder) = dialog.pick_folder()
                && let Err(err) = state.open_project(folder) {
                    outcome.error = Some(format!("failed to open project: {err}"));
                }
        }
        if let Some(root) = state.project.as_ref().map(|p| p.root.clone()) {
            if ui.button("📄").on_hover_text("New File").clicked() {
                panel.begin_new_file(root.clone());
            }
            if ui.button("💻").on_hover_text("Open Terminal").clicked()
                && let Err(err) = terminal::open(&root)
            {
                outcome.error = Some(format!("failed to open terminal: {err}"));
            }
        }
    });

    let created = show_new_file_row(ui, state, panel, &mut outcome);

    ui.separator();

    let mut actions = TreeActions::default();
    if let Some(project) = &state.project {
        // Taken before the tree walk (which only ever visits *one* node
        // matching `rename_draft`, so a plain `bool` threaded through the
        // recursion is enough — no need for `render_node` to reach back
        // into `panel` itself for it).
        let should_focus_rename = std::mem::take(&mut panel.focus_rename);
        egui::ScrollArea::vertical().show(ui, |ui| {
            render_node(ui, &project.tree, &mut panel.rename_draft, should_focus_rename, &panel.clipboard, &mut actions);
        });
    } else {
        ui.weak("No folder open");
    }

    let pasted = apply_tree_actions(panel, actions, &mut outcome);
    show_delete_confirm(ui, panel, &mut outcome);

    // Only genuinely tree-changing actions need a refresh — opening an
    // *existing* file (also carried on `outcome.open`, via a tree click)
    // doesn't touch the filesystem, so re-walking the whole project for it
    // would be a pointless full directory read on every single file click.
    if (created || pasted || outcome.renamed.is_some() || outcome.deleted.is_some())
        && let Some(root) = state.project.as_ref().map(|p| p.root.clone())
            && let Err(err) = state.open_project(root) {
                outcome.error = Some(format!("failed to refresh project tree: {err}"));
            }

    outcome
}

/// Draws a single-line text field, focusing it (once, per `should_focus`)
/// and reading back whether the user just confirmed or cancelled it.
/// Shared by the "New File" row and the tree's inline rename field, which
/// otherwise each hand-rolled the identical three steps.
///
/// Both Enter and Escape make egui's focused-widget tracking drop the
/// field's focus on the same frame they're pressed (Escape unconditionally
/// clears it; Enter does for any single-line `TextEdit` since it's treated
/// as a submit) — so `lost_focus()` paired with the specific key pressed
/// distinguishes "confirmed" from "cancelled" from an unrelated, ordinary
/// focus change (e.g. clicking elsewhere), which should count as neither.
fn text_field_outcome(ui: &mut egui::Ui, text: &mut String, should_focus: bool) -> (bool, bool) {
    let response = ui.text_edit_singleline(text);
    if should_focus {
        response.request_focus();
    }
    let confirmed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let escaped = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
    (confirmed, escaped)
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
    // Taken (not just read) before borrowing `new_file_draft` below, both to
    // sidestep any borrow-checker friction between the two fields and so it
    // naturally only fires once: the very first frame this draft is drawn.
    let should_focus = std::mem::take(&mut panel.focus_new_file);
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

        let (confirmed, escaped) = text_field_outcome(ui, name, should_focus);

        if ui.button("Create").clicked() || confirmed {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                // `trimmed` may itself contain `/` (e.g. "controllers/
                // UserController.java") to create the file inside a new,
                // not-yet-existing subdirectory in one step — `dir.join`
                // already resolves that into the right nested path, it
                // just needs its parent directories to actually exist
                // before `fs::write` can create the file in them.
                let new_path = dir.join(trimmed);
                if new_path.exists() {
                    outcome.error = Some(format!("file already exists: {}", new_path.display()));
                } else {
                    let content = new_path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .and_then(fg_core::Language::from_extension)
                        .zip(project_root.as_ref())
                        .map(|(language, root)| fg_core::generate_boilerplate(language, root, &new_path))
                        .unwrap_or_default();

                    match create_file_with_parents(&new_path, &content) {
                        Ok(()) => {
                            outcome.open = Some(new_path);
                            close_draft = true;
                            created = true;
                        }
                        Err(err) => outcome.error = Some(format!("failed to create file: {err}")),
                    }
                }
            }
        }
        if ui.button("Cancel").clicked() || escaped {
            close_draft = true;
        }
    });

    if close_draft {
        panel.new_file_draft = None;
    }
    created
}

/// Creates `path` with `content`, creating any missing parent directories
/// first. Lets "New File" accept a nested relative name like
/// "controllers/UserController.java" and have it just work, rather than
/// failing because "controllers/" doesn't exist yet — `fs::write` alone
/// only ever creates the final file, never its parent directories.
fn create_file_with_parents(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
}

/// Returns `true` if a paste actually changed the filesystem tree — needed
/// alongside `outcome.renamed`/`outcome.deleted` (which a `Cut` paste also
/// sets, and which `show`'s refresh condition already watches) because a
/// `Copy` paste sets neither of those: the pasted-from file is untouched,
/// only a new one appeared, the same "something new exists that a tab
/// click didn't put there" case `show_new_file_row`'s `created` return
/// covers for New File.
fn apply_tree_actions(panel: &mut SidePanelState, actions: TreeActions, outcome: &mut SidePanelOutcome) -> bool {
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
        panel.focus_rename = true;
    }
    if actions.cancel_rename {
        panel.rename_draft = None;
    }
    if let Some(new_name) = actions.confirm_rename
        && let Some((old_path, _)) = panel.rename_draft.take() {
            let new_name = new_name.trim();
            let new_path = old_path.parent().map(|p| p.join(new_name));
            match new_path {
                _ if new_name.is_empty() => outcome.error = Some("rename failed: empty name".to_string()),
                Some(new_path) if new_path.exists() => {
                    outcome.error = Some(format!("rename failed: {} already exists", new_path.display()));
                }
                Some(new_path) => match std::fs::rename(&old_path, &new_path) {
                    Ok(()) => outcome.renamed = Some((old_path, new_path)),
                    Err(err) => outcome.error = Some(format!("failed to rename: {err}")),
                },
                None => outcome.error = Some("rename failed: no parent directory".to_string()),
            }
        }

    if let Some(path) = actions.delete_request {
        panel.pending_delete = Some(path);
    }

    if let Some(path) = actions.copy_request {
        panel.clipboard = Some((path, ClipboardOp::Copy));
    }
    if let Some(path) = actions.cut_request {
        panel.clipboard = Some((path, ClipboardOp::Cut));
    }

    let mut tree_changed = false;
    if let Some(target_dir) = actions.paste_request
        && let Some((source, op)) = panel.clipboard.clone()
    {
        if is_invalid_paste_target(&source, &target_dir) {
            outcome.error = Some(format!(
                "can't paste {} into itself or one of its own subdirectories",
                source.display()
            ));
        } else {
            match paste_into(&source, &target_dir, op) {
                Ok(dest) => {
                    tree_changed = true;
                    if op == ClipboardOp::Cut {
                        outcome.renamed = Some((source, dest));
                        // One-shot: a cut-and-pasted file is gone from
                        // where it was, so pasting the same clipboard
                        // entry again would just fail with "source not
                        // found" — clearing it here is what makes a
                        // second Paste with nothing newly copied/cut a
                        // silent no-op instead of that confusing error.
                        panel.clipboard = None;
                    }
                }
                Err(err) => outcome.error = Some(format!("failed to paste: {err}")),
            }
        }
    }

    tree_changed
}

/// Deletes `path` — a whole subtree via `remove_dir_all` if it's a
/// directory, otherwise a single `remove_file` — so the delete
/// confirmation dialog can treat file and directory nodes the same way.
fn delete_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Whether pasting `source` into `target_dir` would paste it into itself or
/// one of its own descendants — copying/moving a directory into its own
/// subtree either can't mean anything sensible (pasting onto itself) or
/// would have `copy_recursive` walk into the very directory it's still
/// writing to (a descendant target), so both are rejected up front rather
/// than attempted.
fn is_invalid_paste_target(source: &Path, target_dir: &Path) -> bool {
    target_dir == source || target_dir.starts_with(source)
}

/// Copies `source` to `dest`, recursing into every entry if `source` is a
/// directory — `std::fs::copy` alone only ever copies a single file.
fn copy_recursive(source: &Path, dest: &Path) -> std::io::Result<()> {
    if source.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(source, dest).map(|_| ())
    }
}

/// Pastes `source` into `target_dir`, returning the resulting path.
/// `ClipboardOp::Copy` always copies (recursively, for a directory);
/// `ClipboardOp::Cut` prefers a same-filesystem `std::fs::rename` (fast,
/// atomic) and falls back to copy-then-delete-original on any failure —
/// cross-device is the expected reason `rename` alone can't do it, but
/// `std::io::ErrorKind` doesn't reliably distinguish that across platforms,
/// so this just attempts `rename` first rather than pre-detecting it.
fn paste_into(source: &Path, target_dir: &Path, op: ClipboardOp) -> std::io::Result<PathBuf> {
    let name = source
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "source has no file name"))?;
    let dest = target_dir.join(name);

    if dest.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} already exists", dest.display()),
        ));
    }

    match op {
        ClipboardOp::Copy => copy_recursive(source, &dest)?,
        ClipboardOp::Cut => {
            if std::fs::rename(source, &dest).is_err() {
                copy_recursive(source, &dest)?;
                delete_path(source)?;
            }
        }
    }
    Ok(dest)
}

fn show_delete_confirm(ui: &mut egui::Ui, panel: &mut SidePanelState, outcome: &mut SidePanelOutcome) {
    let Some(path) = panel.pending_delete.clone() else {
        return;
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let message = if path.is_dir() {
        format!("Delete directory {name} and everything inside it? This cannot be undone.")
    } else {
        format!("Delete {name}? This cannot be undone.")
    };

    let modal_outcome = show_modal(ui, "delete_confirm", Some(path), |ui, path| {
        ui.label(message);
        ui.horizontal(|ui| {
            if ui.button("Delete").clicked() {
                match delete_path(path) {
                    Ok(()) => outcome.deleted = Some(path.clone()),
                    Err(err) => outcome.error = Some(format!("failed to delete: {err}")),
                }
                panel.pending_delete = None;
            }
            if ui.button("Cancel").clicked() {
                panel.pending_delete = None;
            }
        });
    });
    // Escape cancels, same as the "Cancel" button — nothing gets deleted.
    if let Some((_, true)) = modal_outcome {
        panel.pending_delete = None;
    }
}

/// Draws the inline rename text field + confirm/cancel buttons shown in
/// place of a node's normal label while it's the one `rename_draft` points
/// at — shared by both file and directory nodes, which otherwise differ
/// (directories render a `CollapsingHeader` with children, files a plain
/// selectable label) but hand off to identical rename UI once renaming.
fn show_rename_field(ui: &mut egui::Ui, name: &mut String, should_focus: bool, actions: &mut TreeActions) {
    ui.horizontal(|ui| {
        let (confirmed, escaped) = text_field_outcome(ui, name, should_focus);
        if confirmed || ui.small_button("✓").clicked() {
            actions.confirm_rename = Some(name.clone());
        }
        if escaped || ui.small_button("✗").clicked() {
            actions.cancel_rename = true;
        }
    });
}

fn render_node(
    ui: &mut egui::Ui,
    node: &FileNode,
    rename_draft: &mut Option<(PathBuf, String)>,
    should_focus_rename: bool,
    clipboard: &Option<(PathBuf, ClipboardOp)>,
    actions: &mut TreeActions,
) {
    let is_being_renamed = rename_draft.as_ref().is_some_and(|(p, _)| p == &node.path);
    if is_being_renamed {
        let (_, name) = rename_draft.as_mut().expect("checked above");
        show_rename_field(ui, name, should_focus_rename, actions);
        return;
    }

    match node.kind {
        FileKind::Dir => {
            let header = egui::CollapsingHeader::new(format!("📁 {}", node.name))
                .id_salt(&node.path)
                .default_open(false)
                .show(ui, |ui| {
                    for child in &node.children {
                        render_node(ui, child, rename_draft, should_focus_rename, clipboard, actions);
                    }
                });
            header.header_response.context_menu(|ui| {
                if ui.button("New File").clicked() {
                    actions.start_new_file = Some(node.path.clone());
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
                ui.separator();
                if ui.button("Copy").clicked() {
                    actions.copy_request = Some(node.path.clone());
                    ui.close();
                }
                if ui.button("Cut").clicked() {
                    actions.cut_request = Some(node.path.clone());
                    ui.close();
                }
                // Only a directory is a meaningful paste *target* — pasting
                // "onto" a file doesn't have an obvious destination the way
                // pasting into a folder does, so `FileKind::File` below
                // gets no Paste entry at all.
                if ui.add_enabled(clipboard.is_some(), egui::Button::new("Paste")).clicked() {
                    actions.paste_request = Some(node.path.clone());
                    ui.close();
                }
            });
        }
        FileKind::File => {
            let extension = node.path.extension().and_then(|ext| ext.to_str());
            // A bare `Dockerfile` has no extension at all for the `match`
            // below to key off, and a suffixed variant (`Dockerfile.dev`)
            // has one that means nothing here (`"dev"` isn't a real file
            // type) — so this is checked as its own name-based fallback,
            // the same `from_filename` check `Document::open` already uses
            // to recognize one, rather than folded into the `extension`
            // match itself.
            let is_dockerfile = node
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| fg_core::Language::from_filename(n).is_some());
            let icon = match extension {
                Some("java") => "☕ ",
                Some("kt") => "🔷 ",
                Some("properties") => "⚙️ ",
                Some("yml" | "yaml") => "📜 ",
                Some("xml") => "🏷️ ",
                _ if is_dockerfile => "🐳 ",
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
                ui.separator();
                if ui.button("Copy").clicked() {
                    actions.copy_request = Some(node.path.clone());
                    ui.close();
                }
                if ui.button("Cut").clicked() {
                    actions.cut_request = Some(node.path.clone());
                    ui.close();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests;
