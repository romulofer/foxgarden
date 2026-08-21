use fg_i18n::{msg, t};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

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
    /// The path(s) awaiting delete confirmation — more than one whenever the
    /// triggering Delete was invoked over an active multi-selection
    /// (`PLAN.md` Track 1 Phase 2), so the confirm dialog can ask once
    /// ("Delete 4 items?") instead of once per file.
    pending_delete: Option<Vec<PathBuf>>,
    /// Set whenever `new_file_draft`/`rename_draft` is freshly opened,
    /// consumed (cleared) by the very next frame that draws the
    /// corresponding text field — so a freshly opened "New File"/rename
    /// input grabs keyboard focus immediately instead of requiring an
    /// extra click before the user can type.
    focus_new_file: bool,
    focus_rename: bool,
    /// The file(s)/directory(-ies) a Copy or Cut is waiting to be pasted
    /// somewhere — set by a tree node's "Copy"/"Cut" context-menu entry
    /// (more than one path whenever that node was part of an active multi-
    /// selection, `PLAN.md` Track 1 Phase 2), read (and, for `Cut`, cleared)
    /// by a directory's "Paste" entry. `Copy` stays here across a paste (so
    /// it can be pasted again elsewhere, same as an OS file manager); `Cut`
    /// is one-shot.
    clipboard: Option<(Vec<PathBuf>, ClipboardOp)>,
    /// Multi-selected tree nodes (`PLAN.md` Track 1) — Cmd/Ctrl+Click
    /// toggles a node in/out, Shift+Click selects the contiguous visible
    /// range from `last_selected`, a plain click collapses the set back to
    /// just that one node. Deliberately independent of any "currently open
    /// tab" concept: a file's tab being open and its tree node being
    /// selected are unrelated states.
    selected: HashSet<PathBuf>,
    /// The anchor a Shift+Click extends a range from — set by the last
    /// plain or Cmd/Ctrl+Click, left untouched by a Shift+Click itself so
    /// repeated Shift+Clicks from the same starting point keep recomputing
    /// a fresh range rather than drifting from wherever the previous one
    /// ended.
    last_selected: Option<PathBuf>,
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
    /// One entry per file/directory renamed or moved on disk this frame
    /// (more than one after a multi-selected Cut+Paste, `PLAN.md` Track 1
    /// Phase 2); any tab pointing at an `old` should be repointed to its
    /// matching `new`.
    pub renamed: Vec<(PathBuf, PathBuf)>,
    /// One entry per file/directory deleted from disk this frame (more than
    /// one after a multi-selected batch delete); any tab pointing at one
    /// should close.
    pub deleted: Vec<PathBuf>,
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
    /// A node's label was clicked (file or directory) — the path plus
    /// whatever modifiers were held, for `apply_selection_click` to resolve
    /// against this frame's own `visible_order` once the whole tree has been
    /// walked (a Shift+Click's range can extend either direction, so it
    /// can't be resolved mid-walk before every node's position is known).
    select_click: Option<(PathBuf, egui::Modifiers)>,
}

pub fn show(ui: &mut egui::Ui, state: &mut EditorState, panel: &mut SidePanelState) -> SidePanelOutcome {
    let mut outcome = SidePanelOutcome::default();

    ui.horizontal(|ui| {
        if ui.button("📁").on_hover_text(t().side_panel.open_folder_hint).clicked() {
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
                && let Err(err) = state.open_project(folder)
            {
                outcome.error = Some(msg::failed_to_open_project(&err.to_string()));
            }
        }
        if let Some(root) = state.project.as_ref().map(|p| p.root.clone()) {
            if ui.button("📄").on_hover_text(t().side_panel.new_file_hint).clicked() {
                panel.begin_new_file(root.clone());
            }
            if ui.button("💻").on_hover_text(t().side_panel.open_terminal_hint).clicked()
                && let Err(err) = terminal::open(&root)
            {
                outcome.error = Some(msg::failed_to_open_terminal(&err.to_string()));
            }
        }
    });

    let created = show_new_file_row(ui, state, panel, &mut outcome);

    ui.separator();

    let mut actions = TreeActions::default();
    // This frame's flattened, currently-visible node order (depth-first,
    // skipping a collapsed directory's children) — built alongside the walk
    // itself so a Shift+Click's range can be resolved against it afterward,
    // regardless of whether the anchor is above or below the clicked node.
    let mut visible_order = Vec::new();
    if let Some(project) = &state.project {
        // Taken before the tree walk (which only ever visits *one* node
        // matching `rename_draft`, so a plain `bool` threaded through the
        // recursion is enough — no need for `render_node` to reach back
        // into `panel` itself for it).
        let should_focus_rename = std::mem::take(&mut panel.focus_rename);
        egui::ScrollArea::vertical().show(ui, |ui| {
            render_node(
                ui,
                &project.tree,
                "/",
                &mut panel.rename_draft,
                should_focus_rename,
                &panel.clipboard,
                &panel.selected,
                &mut visible_order,
                &mut actions,
            );
        });
    } else {
        ui.weak(t().side_panel.no_folder_open);
    }

    if let Some((path, modifiers)) = actions.select_click.take() {
        apply_selection_click(panel, &path, modifiers, &visible_order);
    }

    let pasted = apply_tree_actions(panel, actions, &mut outcome);
    show_delete_confirm(ui, panel, &mut outcome);

    // Only genuinely tree-changing actions need a refresh — opening an
    // *existing* file (also carried on `outcome.open`, via a tree click)
    // doesn't touch the filesystem, so re-walking the whole project for it
    // would be a pointless full directory read on every single file click.
    if (created || pasted || !outcome.renamed.is_empty() || !outcome.deleted.is_empty())
        && let Some(root) = state.project.as_ref().map(|p| p.root.clone())
        && let Err(err) = state.open_project(root)
    {
        outcome.error = Some(msg::failed_to_refresh_tree(&err.to_string()));
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
        ui.label(msg::new_file_in(&dir_label));

        let (confirmed, escaped) = text_field_outcome(ui, name, should_focus);

        if ui.button(t().side_panel.create).clicked() || confirmed {
            let trimmed = name.trim();
            if has_unsafe_path_component(trimmed) {
                outcome.error = Some(msg::invalid_path_name(trimmed));
            } else if !trimmed.is_empty() {
                // `trimmed` may itself contain `/` (e.g. "controllers/
                // UserController.java") to create the file inside a new,
                // not-yet-existing subdirectory in one step — `dir.join`
                // already resolves that into the right nested path, it
                // just needs its parent directories to actually exist
                // before `fs::write` can create the file in them.
                // `has_unsafe_path_component` above already rejected `..`,
                // an absolute path, and a Windows drive prefix, so the
                // joined result can't land outside `dir`.
                let new_path = dir.join(trimmed);
                if new_path.exists() {
                    outcome.error = Some(msg::file_already_exists(&new_path.display().to_string()));
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
                        Err(err) => outcome.error = Some(msg::failed_to_create_file(&err.to_string())),
                    }
                }
            }
        }
        if ui.button(t().common.cancel).clicked() || escaped {
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
/// True if `input`, parsed as a path, contains a component that could walk
/// the joined result outside its intended base directory — `..`, a root
/// (`/foo`), or (Windows) a drive prefix (`C:\foo`). A bare `Component::
/// Normal` segment (including several joined by `/`, e.g. "controllers/
/// UserController.java") is always safe; `.` is left alone too since it
/// resolves to the same directory it's already in.
fn has_unsafe_path_component(input: &str) -> bool {
    Path::new(input).components().any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
}

fn create_file_with_parents(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
}

/// Applies one tree-node click's selection effect — the "anchor + shift-
/// extends" model most tree/list widgets use (`SPEC.md` §1, `PLAN.md`
/// Track 1). `visible_order` is this frame's own flattened, currently-
/// visible node order, needed to resolve what falls "between" `panel.
/// last_selected` and `clicked` for a Shift+Click. `Modifiers::command`
/// (this codebase's existing cross-platform "primary modifier" — Ctrl on
/// Windows/Linux, Cmd on Mac, the same flag `app.rs`'s own shortcuts key
/// off) toggles `clicked` in/out of the set; Shift selects the contiguous
/// range from `last_selected` (left untouched here, so repeated Shift+
/// Clicks keep recomputing from the same anchor); anything else (a plain
/// click) collapses the selection down to just `clicked`.
fn apply_selection_click(panel: &mut SidePanelState, clicked: &Path, modifiers: egui::Modifiers, visible_order: &[PathBuf]) {
    if modifiers.shift
        && let Some(anchor) = panel.last_selected.clone()
        && let Some(start) = visible_order.iter().position(|p| p == &anchor)
        && let Some(end) = visible_order.iter().position(|p| p == clicked)
    {
        let (lo, hi) = (start.min(end), start.max(end));
        panel.selected = visible_order[lo..=hi].iter().cloned().collect();
    } else if modifiers.command {
        if !panel.selected.remove(clicked) {
            panel.selected.insert(clicked.to_path_buf());
        }
        panel.last_selected = Some(clicked.to_path_buf());
    } else {
        panel.selected.clear();
        panel.selected.insert(clicked.to_path_buf());
        panel.last_selected = Some(clicked.to_path_buf());
    }
}

/// The real target set for a Delete/Copy/Cut invoked from `path`'s own
/// context menu (`PLAN.md` Track 1 Phase 2): the *whole* current
/// multi-selection whenever one is active (more than one node selected),
/// regardless of which specific node's menu was actually opened — matching
/// `SPEC.md` §1's plain wording ("Delete... over a set"), not a narrower
/// "only if `path` itself is one of the selected nodes" rule. Falls back to
/// just `path` alone the rest of the time (nothing selected, or only `path`
/// itself), so every single-node context-menu action keeps working exactly
/// as it did before multi-select existed.
fn action_targets(panel: &SidePanelState, path: &Path) -> Vec<PathBuf> {
    if panel.selected.len() > 1 {
        panel.selected.iter().cloned().collect()
    } else {
        vec![path.to_path_buf()]
    }
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
        && let Some((old_path, _)) = panel.rename_draft.take()
    {
        let new_name = new_name.trim();
        let new_path = old_path.parent().map(|p| p.join(new_name));
        match new_path {
            _ if new_name.is_empty() => outcome.error = Some(t().errors.rename_empty_name.to_string()),
            // A rename box is a single filename, not a path — unlike New
            // File's own nested-subdirectory allowance, *any* separator
            // here (not just `..`/absolute) means "move", which rename
            // doesn't support and shouldn't silently attempt.
            _ if Path::new(new_name).components().count() != 1 || has_unsafe_path_component(new_name) => {
                outcome.error = Some(msg::invalid_path_name(new_name));
            }
            Some(new_path) if new_path.exists() => {
                outcome.error = Some(msg::rename_target_exists(&new_path.display().to_string()));
            }
            Some(new_path) => match std::fs::rename(&old_path, &new_path) {
                Ok(()) => outcome.renamed.push((old_path, new_path)),
                Err(err) => outcome.error = Some(msg::failed_to_rename(&err.to_string())),
            },
            None => outcome.error = Some(t().errors.rename_no_parent.to_string()),
        }
    }

    if let Some(path) = actions.delete_request {
        panel.pending_delete = Some(action_targets(panel, &path));
    }

    if let Some(path) = actions.copy_request {
        panel.clipboard = Some((action_targets(panel, &path), ClipboardOp::Copy));
    }
    if let Some(path) = actions.cut_request {
        panel.clipboard = Some((action_targets(panel, &path), ClipboardOp::Cut));
    }

    let mut tree_changed = false;
    if let Some(target_dir) = actions.paste_request
        && let Some((sources, op)) = panel.clipboard.clone()
    {
        // One bad source (a paste-into-itself, a name collision) doesn't
        // abort the rest of the batch — same "don't fail the whole
        // operation over one bad entry" reasoning `TECHNICAL_DEBT.md` #11
        // already established — but every failure is collected so the user
        // still hears about it, not just whichever succeeded silently.
        let mut failures = Vec::new();
        for source in &sources {
            if is_invalid_paste_target(source, &target_dir) {
                failures.push(format!(
                    "{}: can't paste into itself or one of its own subdirectories",
                    source.display()
                ));
                continue;
            }
            match paste_into(source, &target_dir, op) {
                Ok(dest) => {
                    tree_changed = true;
                    if op == ClipboardOp::Cut {
                        outcome.renamed.push((source.clone(), dest));
                    }
                }
                Err(err) => failures.push(format!("{}: {err}", source.display())),
            }
        }
        if op == ClipboardOp::Cut {
            // One-shot: every cut-and-pasted file is gone from where it
            // was, so pasting the same clipboard entries again would just
            // fail with "source not found" for each — clearing it here is
            // what makes a second Paste with nothing newly copied/cut a
            // silent no-op instead of that confusing error. A source that
            // itself failed to paste (still in `failures`) is dropped from
            // the clipboard too, same as a successful one — retrying a
            // paste that failed once (e.g. onto itself) isn't a "one-shot"
            // exception worth special-casing here.
            panel.clipboard = None;
        }
        if !failures.is_empty() {
            outcome.error = Some(msg::failed_to_paste(&failures.join("\n")));
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
    let Some(paths) = panel.pending_delete.clone() else {
        return;
    };
    // Confirms once for the whole batch (`PLAN.md` Track 1 Phase 2:
    // "Delete confirms once for the whole set"), not once per file — a
    // single-item delete (the common, pre-multi-select case) is just the
    // `paths.len() == 1` case of the same message.
    let message = match paths.as_slice() {
        [path] => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_dir() {
                msg::confirm_delete_directory(&name)
            } else {
                msg::confirm_delete_file(&name)
            }
        }
        _ => msg::confirm_delete_many(paths.len()),
    };

    let modal_outcome = show_modal(ui, "delete_confirm", Some(paths), |ui, paths| {
        ui.label(message);
        ui.horizontal(|ui| {
            if ui.button(t().common.delete).clicked() {
                let mut failures = Vec::new();
                for path in paths {
                    match delete_path(path) {
                        Ok(()) => outcome.deleted.push(path.clone()),
                        Err(err) => failures.push(format!("{}: {err}", path.display())),
                    }
                }
                if !failures.is_empty() {
                    outcome.error = Some(msg::failed_to_delete(&failures.join("\n")));
                }
                panel.pending_delete = None;
            }
            if ui.button(t().common.cancel).clicked() {
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

/// Directory names that mark a Java/Kotlin source root (`src/main/java`,
/// `src/test/kotlin`, a multi-module project's own nested equivalents, …).
/// Everything single-child *beneath* one of these is package structure, so
/// `render_node`/`collapse_chain` join it with `.` instead of the generic
/// `/` every other single-child directory chain uses — matching IntelliJ's
/// own "Compact Middle Packages" convention. The source-root directory
/// itself is never folded into a chain (see `collapse_chain`) — it's
/// always its own row, exactly like the real screenshot this feature was
/// built from: a "java" row, then "br.ufsc.bridge.pec.backend" as its own
/// single child row below it.
const SOURCE_ROOT_DIR_NAMES: &[&str] = &["java", "kotlin"];

/// One or more consecutive single-child directories collapsed into a
/// single visual tree row — most real Java/Kotlin package hierarchies are
/// otherwise a long, mostly-empty scroll of one folder per row before
/// reaching anything with real siblings. `terminal` (the chain's last
/// node) is what the row's actions — select/rename/delete/new-file/paste/
/// expand — actually operate on/reveal; a rename, in particular,
/// deliberately only ever renames `terminal` itself, not the whole
/// collapsed chain (editing "br.ufsc.bridge.pec.backend" as one string and
/// restructuring several real directories from it is real IntelliJ
/// behavior this doesn't attempt to replicate). `label` is purely the
/// display text — just `start.name` if nothing collapsed.
struct CollapsedDir<'a> {
    label: String,
    terminal: &'a FileNode,
}

/// Walks forward from `start` through single-child directory descendants,
/// joining each segment's name with `sep`, stopping at the first node with
/// zero or 2+ children, whose only child is a file, or whose only child is
/// itself a source root (`SOURCE_ROOT_DIR_NAMES`) — a source root always
/// starts its own row, never gets folded into a generic parent chain.
/// `start` itself is also never folded into a chain if it's a source root:
/// it's a boundary all on its own, so `render_node` can restart compaction
/// with `.` for everything beneath it (see that function's `child_sep`).
fn collapse_chain<'a>(start: &'a FileNode, sep: &str) -> CollapsedDir<'a> {
    if SOURCE_ROOT_DIR_NAMES.contains(&start.name.as_str()) {
        return CollapsedDir { label: start.name.clone(), terminal: start };
    }
    let mut label = start.name.clone();
    let mut terminal = start;
    while let [only] = terminal.children.as_slice()
        && only.kind == FileKind::Dir
        && !SOURCE_ROOT_DIR_NAMES.contains(&only.name.as_str())
    {
        label.push_str(sep);
        label.push_str(&only.name);
        terminal = only;
    }
    CollapsedDir { label, terminal }
}

#[allow(clippy::too_many_arguments)]
fn render_node(
    ui: &mut egui::Ui,
    node: &FileNode,
    sep: &'static str,
    rename_draft: &mut Option<(PathBuf, String)>,
    should_focus_rename: bool,
    clipboard: &Option<(Vec<PathBuf>, ClipboardOp)>,
    selected: &HashSet<PathBuf>,
    visible_order: &mut Vec<PathBuf>,
    actions: &mut TreeActions,
) {
    // A chain of single-child directories collapses into one visual row
    // (see `collapse_chain`) — `effective_path` is whichever real node this
    // row's own actions apply to: the chain's last node for a directory, or
    // `node` itself for a file (files never collapse).
    let collapsed = (node.kind == FileKind::Dir).then(|| collapse_chain(node, sep));
    let effective_path = collapsed.as_ref().map_or(&node.path, |c| &c.terminal.path);
    visible_order.push(effective_path.clone());

    let is_being_renamed = rename_draft.as_ref().is_some_and(|(p, _)| p == effective_path);
    if is_being_renamed {
        let (_, name) = rename_draft.as_mut().expect("checked above");
        show_rename_field(ui, name, should_focus_rename, actions);
        return;
    }

    let is_selected = selected.contains(effective_path);

    match collapsed {
        Some(collapsed) => {
            let terminal = collapsed.terminal;
            // Once a chain has passed through a Java/Kotlin source root,
            // everything beneath it is package structure for the rest of
            // that subtree — `.` from here on, never reverting to `/`
            // (package structure never "un-nests" back into arbitrary
            // folders beneath a source root).
            let child_sep = if SOURCE_ROOT_DIR_NAMES.contains(&terminal.name.as_str()) { "." } else { sep };

            // Split into the disclosure-triangle icon (toggles expand/
            // collapse, entirely on its own) and a custom label rendered as
            // a `selectable_label` (this node's own Cmd/Ctrl/Shift-click
            // handling, below) — `CollapsingHeader`'s own higher-level
            // `.show()` bundles both into one whole-row click, which would
            // mean Ctrl/Shift-clicking a folder to multi-select it also
            // toggled it open/closed every time. A plain click on the
            // label *also* toggles expand (preserving today's "click
            // anywhere on the row" ergonomics for the common case) — only
            // a Cmd/Ctrl/Shift-click is select-only.
            let collapsing_id = ui.make_persistent_id(&terminal.path);
            let collapsing = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), collapsing_id, false);
            let header = collapsing.show_header(ui, |ui| ui.selectable_label(is_selected, format!("📁 {}", collapsed.label)));
            let (_, header_response, _) = header.body(|ui| {
                for child in &terminal.children {
                    render_node(ui, child, child_sep, rename_draft, should_focus_rename, clipboard, selected, visible_order, actions);
                }
            });
            let label_response = header_response.inner;

            if label_response.clicked() {
                let modifiers = ui.input(|i| i.modifiers);
                actions.select_click = Some((terminal.path.clone(), modifiers));
                if !modifiers.command && !modifiers.shift {
                    let mut collapsing = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), collapsing_id, false);
                    collapsing.toggle(ui);
                    collapsing.store(ui.ctx());
                }
            }

            // New File/Rename are inherently single-target operations
            // (`SPEC.md` §1) — disabled, not hidden, whenever a multi-
            // selection is active, so they read as "not applicable right
            // now" rather than silently disappearing.
            let single_target = selected.len() <= 1;
            label_response.context_menu(|ui| {
                if ui
                    .add_enabled(single_target, egui::Button::new(t().common.new_file))
                    .clicked()
                {
                    actions.start_new_file = Some(terminal.path.clone());
                    ui.close();
                }
                if ui
                    .add_enabled(single_target, egui::Button::new(t().common.rename))
                    .clicked()
                {
                    actions.start_rename = Some(terminal.path.clone());
                    ui.close();
                }
                if ui.button(t().common.delete).clicked() {
                    actions.delete_request = Some(terminal.path.clone());
                    ui.close();
                }
                ui.separator();
                if ui.button(t().common.copy).clicked() {
                    actions.copy_request = Some(terminal.path.clone());
                    ui.close();
                }
                if ui.button(t().common.cut).clicked() {
                    actions.cut_request = Some(terminal.path.clone());
                    ui.close();
                }
                // Only a directory is a meaningful paste *target* — pasting
                // "onto" a file doesn't have an obvious destination the way
                // pasting into a folder does, so `FileKind::File` below
                // gets no Paste entry at all.
                if ui
                    .add_enabled(clipboard.is_some(), egui::Button::new(t().common.paste))
                    .clicked()
                {
                    actions.paste_request = Some(terminal.path.clone());
                    ui.close();
                }
            });
        }
        None => {
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
            let response = ui.selectable_label(is_selected, label_text);
            if response.clicked() {
                let modifiers = ui.input(|i| i.modifiers);
                actions.select_click = Some((node.path.clone(), modifiers));
                // A plain click still opens the file, unchanged from before
                // this feature existed — Cmd/Ctrl/Shift-click are the new,
                // explicit multi-select gestures layered on top (`SPEC.md`
                // §1: "multi-select is an explicit gesture, never the
                // default"), so they select without also opening.
                if !modifiers.command && !modifiers.shift {
                    actions.open = Some(node.path.clone());
                }
            }

            let single_target = selected.len() <= 1;
            response.context_menu(|ui| {
                if ui
                    .add_enabled(single_target, egui::Button::new(t().common.new_file))
                    .clicked()
                {
                    let dir = node.path.parent().map_or_else(|| node.path.clone(), PathBuf::from);
                    actions.start_new_file = Some(dir);
                    ui.close();
                }
                if ui
                    .add_enabled(single_target, egui::Button::new(t().common.rename))
                    .clicked()
                {
                    actions.start_rename = Some(node.path.clone());
                    ui.close();
                }
                if ui.button(t().common.delete).clicked() {
                    actions.delete_request = Some(node.path.clone());
                    ui.close();
                }
                ui.separator();
                if ui.button(t().common.copy).clicked() {
                    actions.copy_request = Some(node.path.clone());
                    ui.close();
                }
                if ui.button(t().common.cut).clicked() {
                    actions.cut_request = Some(node.path.clone());
                    ui.close();
                }
            });
        }
    }
}

#[cfg(test)]
mod side_panel_test;
