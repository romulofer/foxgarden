use std::collections::HashSet;
use std::path::{Path, PathBuf};

use fg_core::{EditorState, Language};
use notify::Watcher;
use ropey::Rope;
use syntax::IncrementalParser;

use crate::file_watch::{self, ReconcileOutcome};
use crate::panels::go_to_file::{self, GoToFileState};
use crate::panels::menu_bar::{self, MenuBarState};
use crate::panels::quick_switcher::{self, QuickSwitcherState};
use crate::panels::run_configs::{self, RunConfigsDialogState};
use crate::panels::side_panel::{self, SidePanelState};
use crate::panels::spring_endpoints::{self, SpringEndpointsState};
use crate::panels::tabs;
use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::style::theme;
use crate::style::view::ViewSettings;
use crate::widgets::editor::{
    CompletionState, GenerateAccessorsDialog, GenerateMethodDialog, OverrideMethodDialog, UserTemplates, jump_to,
};
use crate::widgets::modal::show_modal;

const LAST_PROJECT_KEY: &str = "last_project";
/// Newline-joined absolute paths of the tabs that were open at last exit, in
/// tab order. Newline-joined rather than a structured format since neither
/// `eframe::Storage` nor this crate currently pulls in `serde` — a file path
/// can't itself contain a `\n`, so this is an unambiguous, dependency-free
/// encoding.
const OPEN_TABS_KEY: &str = "open_tabs";
/// The path of whichever tab was focused at last exit, so restoring session
/// doesn't just reopen the same tabs but land on the first one arbitrarily.
const ACTIVE_TAB_KEY: &str = "active_tab";

const EDITOR_FONT_KEY: &str = "editor_font";
const FONT_SIZE_KEY: &str = "font_size";
const DARK_MODE_KEY: &str = "dark_mode";
const INDENT_USE_TABS_KEY: &str = "indent_use_tabs";
const INDENT_WIDTH_KEY: &str = "indent_width";
const WORD_WRAP_KEY: &str = "word_wrap";
const SHOW_WHITESPACE_KEY: &str = "show_whitespace";
const SHOW_INDENT_GUIDES_KEY: &str = "show_indent_guides";
const SHOW_STICKY_SCROLL_KEY: &str = "show_sticky_scroll";
const CURSOR_BLINK_KEY: &str = "cursor_blink";
const SHOW_EDITOR_OUTLINE_KEY: &str = "show_editor_outline";
const SIDE_PANEL_WIDTH_KEY: &str = "side_panel_width";
const SIDE_PANEL_VISIBLE_KEY: &str = "side_panel_visible";
const CUSTOM_JAVA_TEMPLATES_KEY: &str = "custom_java_templates";
const CUSTOM_KOTLIN_TEMPLATES_KEY: &str = "custom_kotlin_templates";
const CUSTOM_GLOBAL_TEMPLATES_KEY: &str = "custom_global_templates";

/// The editor's default code-font point size, before any Settings > Font
/// Size adjustment.
const DEFAULT_FONT_SIZE: f32 = 14.0;
/// Matches `egui::Visuals::default()`'s own `dark_mode: true` — the
/// out-of-the-box theme before any Settings > Theme choice or persisted
/// setting overrides it.
const DEFAULT_DARK_MODE: bool = true;
/// Matches `egui::Panel::left`'s own built-in default outer width, so a
/// fresh install (no persisted `SIDE_PANEL_WIDTH_KEY` yet) looks exactly as
/// if this app had never overridden it.
const DEFAULT_SIDE_PANEL_WIDTH: f32 = 200.0;

pub struct FoxGardenApp {
    state: EditorState,
    /// Kept index-aligned with `state.open_tabs`: one incremental parser per
    /// open document.
    parsers: Vec<Option<IncrementalParser>>,
    pending_close: Option<usize>,
    /// Open while the active tab's file has more than one class with
    /// eligible fields and "Generate Getters/Setters" was just requested —
    /// lets the user pick which class, then which fields, before
    /// generating (see `widgets::editor::GenerateAccessorsDialog`). A
    /// single eligible class skips this and generates immediately, so this
    /// is only ever `Some` when there was real ambiguity to resolve.
    generate_dialog: Option<GenerateAccessorsDialog>,
    /// Same as `generate_dialog`, for "Generate Constructor"/"Generate
    /// toString()"/"Generate equals() and hashCode()"
    /// (`widgets::editor::GenerateMethodDialog`) instead.
    generate_method_dialog: Option<GenerateMethodDialog>,
    /// Open while "Override Method" found at least one overridable,
    /// not-already-overridden method on the enclosing class's superclass —
    /// see `widgets::editor::OverrideMethodDialog`. Unlike `generate_dialog`/
    /// `generate_method_dialog`, this always opens (never skips straight to
    /// inserting) once there's at least one candidate: the choice being
    /// made is *which* methods to override, not a class-ambiguity
    /// resolution step that's only sometimes needed.
    override_method_dialog: Option<OverrideMethodDialog>,
    /// The completion popup for whichever tab is currently focused — see
    /// `widgets::editor::completion::CompletionState`. Same "one field, not
    /// one per open tab" shape `override_method_dialog` already uses: only
    /// the active tab's editor ever renders, so there's never more than one
    /// popup open at a time regardless of how many tabs are open.
    completion: Option<CompletionState>,
    /// Synthetic key events (Undo/Redo/Select All) queued by the editor's
    /// right-click menu, drained back into real input at the top of the
    /// very next frame — see `widgets::editor::show`'s doc comment on why
    /// that's the only way to drive those three from outside egui's own
    /// `TextEdit`.
    pending_editor_input: Vec<egui::Event>,
    /// The editor's right-click menu's Paste item's last OS-clipboard read,
    /// refreshed only on the frame the menu opens rather than on every
    /// frame it stays open (see `context_menu::show_context_menu`) — kept
    /// here, not on a per-tab basis, since only the active tab's editor (and
    /// so only one context menu) is ever shown at a time.
    cached_clipboard_text: Option<String>,
    /// The Spring endpoint map's jump-to-handler (`PLAN.md` Phase 4): a
    /// picked popup row's `(path, byte offset)`, set the frame the popup
    /// closes and resolved (converted to a char offset, then cleared) the
    /// next time `resolve_pending_navigation` runs, once `open_path` has
    /// made that path's document the active tab.
    pending_navigation: Option<(PathBuf, usize)>,
    side_panel: SidePanelState,
    /// The project tree panel's current width, in points — read back every
    /// frame from `egui::Panel::left`'s own response rect (so it tracks a
    /// live drag), fed back in as that same panel's `default_size` next
    /// frame, and persisted/restored across launches by `persist_settings`/
    /// `restore_settings` so a resize sticks around the way every other
    /// Settings choice already does, rather than resetting to `egui::Panel`'s
    /// built-in default on every relaunch.
    side_panel_width: f32,
    /// Whether the project tree panel is shown at all — toggled by the View
    /// menu's "Side Panel" checkbox or `Ctrl+B`, both of which just flip this
    /// one flag. Independent of `zen_mode`, which hides this *and* the menu
    /// bar together; this hides only the project panel, leaving the menu bar
    /// (and so a way back via the View menu) in place.
    side_panel_visible: bool,
    menu_bar: MenuBarState,
    /// `Ctrl+E`'s recent-files popup.
    quick_switcher: QuickSwitcherState,
    /// `Ctrl+P`'s fuzzy-file-open popup.
    go_to_file: GoToFileState,
    /// `Ctrl+Shift+E`'s Spring endpoint map popup.
    spring_endpoints: SpringEndpointsState,
    /// Run > Edit Configurations… — see `panels::run_configs`.
    run_configs_dialog: RunConfigsDialogState,
    editor_font: EditorFont,
    /// The editor's code-font point size, adjustable via Settings > Font
    /// Size. Independent of `editor_font` (the family) — both feed into
    /// `widgets::editor::show`'s `FontId`.
    font_size: f32,
    /// Mirrors whichever theme Settings > Theme last applied to
    /// `egui::Context`'s visuals — `eframe::App::save` has no `Context`
    /// access to read that back at persist time, so this field is the
    /// source of truth `persist_settings` reads instead.
    dark_mode: bool,
    /// Tabs-vs-spaces and indent width, adjustable via Settings >
    /// Indentation. Feeds `widgets::editor::show`'s auto-indent, Tab/
    /// Shift+Tab block indent/dedent, and plain-Tab-with-no-selection paths.
    indent_settings: IndentSettings,
    /// Word wrap, whitespace rendering, and indentation guides, adjustable
    /// via Settings/View. Feeds `widgets::editor::show`'s layouter and
    /// overlay painting — display-only, unlike `indent_settings`, which
    /// governs editing behavior.
    view_settings: ViewSettings,
    /// User-added live templates (Help > Live Templates…), alongside the
    /// built-in `widgets::editor::JAVA_TEMPLATES`/`KOTLIN_TEMPLATES` — a
    /// personal editor preference like `editor_font`/`indent_settings`, not
    /// project-specific data like `RunConfig` (see that type's own doc
    /// comment on the distinction), so it's persisted the same way as every
    /// other Settings value here rather than under a project's `.foxgarden/`.
    custom_templates: UserTemplates,
    /// Hides the menu bar and side panel, leaving just the tab bar and
    /// editor. Toggled by `F11` (checked every frame, independent of
    /// whether the menu bar is currently shown — otherwise there'd be no
    /// way back out once the menu holding the toggle is itself hidden) or
    /// via View > Zen Mode while the menu is visible.
    zen_mode: bool,
    /// The most recent user-facing failure from any panel (open/save/rename/
    /// delete/create a file, open or refresh a project, restore last
    /// session, ...), shown as a dismissable modal (see `show_error_modal`).
    /// A single shared slot rather than one flag per failure kind: every
    /// site that used to just `eprintln!` — invisible outside a terminal
    /// the GUI user probably isn't watching — sets this instead, so a new
    /// failure kind never needs a new field, just another `*last_error =
    /// Some(...)` at the point it's detected. Last write wins if two
    /// failures somehow land the same frame, which in practice never
    /// happens since these all come from distinct, mutually exclusive user
    /// actions.
    last_error: Option<String>,
    /// `None` if `notify::recommended_watcher` itself failed (rare — e.g.
    /// an inotify instance limit) — file watching just silently does
    /// nothing rather than crashing the app over a background convenience
    /// feature.
    file_watcher: Option<notify::RecommendedWatcher>,
    /// The other end of `file_watcher`'s `notify::EventHandler` sender —
    /// events arrive on `notify`'s own background thread and are drained
    /// here once per frame (`process_file_events`), never blocking.
    file_event_rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
    /// Which directories `file_watcher` currently has a `watch()` on —
    /// re-derived from `state.open_tabs` every frame
    /// (`file_watch::watched_dirs_for`) so closing the last tab in a
    /// directory actually stops watching it, not just closing tabs in
    /// general.
    watched_dirs: HashSet<PathBuf>,
    /// Paths of open tabs currently showing the "changed on disk, and you
    /// have unsaved edits here too" conflict banner — cleared by Reload or
    /// Keep Mine.
    external_conflicts: HashSet<PathBuf>,
    /// Paths of open tabs whose file was deleted out from under them.
    externally_deleted: HashSet<PathBuf>,
}

/// Opens `path` in a new tab (or focuses its existing tab, via
/// `EditorState::open_tab`'s own dedup), surfacing any failure through
/// `last_error`. Shared by the side panel's "open a file from the tree"
/// outcome and the recent-files quick switcher (`Ctrl+E`) — both just want
/// "open this path, tell the user if it didn't work," identically.
fn open_path(
    state: &mut EditorState,
    parsers: &mut Vec<Option<IncrementalParser>>,
    last_error: &mut Option<String>,
    path: PathBuf,
) {
    match state.open_tab(path) {
        Ok(index) => {
            if index == parsers.len() {
                let parser = tabs::open_parser_for(&mut state.open_tabs[index]);
                parsers.push(parser);
            }
        }
        Err(err) => {
            // Both variants carry their own `PathBuf`, so the display string
            // is built here in the failure arm only, instead of
            // unconditionally before the match on every open attempt.
            let message = match &err {
                fg_core::OpenDocumentError::Binary(path) => {
                    format!("Couldn't open {}: not a text file.", path.display())
                }
                fg_core::OpenDocumentError::Io(path, e) => {
                    format!("Couldn't open {}:\n{e}", path.display())
                }
            };
            *last_error = Some(message);
        }
    }
}

/// Resolves `pending_navigation` (the Spring endpoint map's jump-to-handler,
/// `PLAN.md` Phase 4) once its target document is open: converts the byte
/// offset to a char offset via that document's buffer, clears the field, and
/// returns `(path, char_offset)` for the caller to act on. `None` if there's
/// nothing pending, or the target document isn't open yet — left pending for
/// a later frame to retry, though in practice `open_path` always opens it
/// synchronously before this ever runs, so that path doesn't currently
/// happen.
fn resolve_pending_navigation(
    state: &EditorState,
    pending_navigation: &mut Option<(PathBuf, usize)>,
) -> Option<(PathBuf, usize)> {
    let (path, byte) = pending_navigation.clone()?;
    let doc = state.open_tabs.iter().find(|d| d.path == path)?;
    let char_offset = doc.buffer.byte_to_char(byte);
    *pending_navigation = None;
    Some((path, char_offset))
}

/// Closes every open tab whose path is `path` itself or starts inside it,
/// keeping `parsers` in lockstep — used when `path` was deleted out from
/// under the tree. A directory delete removes every file beneath it (via
/// `remove_dir_all`), so every tab pointing anywhere under it needs to
/// close, not just one pointing at `path` exactly; a plain file delete is
/// just the case where the only tab that can match is `path` itself.
/// Iterates back-to-front so removing an index never shifts the position
/// of one still to be checked.
fn close_tabs_under(state: &mut EditorState, parsers: &mut Vec<Option<IncrementalParser>>, path: &Path) {
    for index in (0..state.open_tabs.len()).rev() {
        if state.open_tabs[index].path().starts_with(path) {
            state.close_tab(index);
            parsers.remove(index);
        }
    }
}

/// Repoints every open tab under `old` to the equivalent path under `new`,
/// recreating each one's parser if the rename changed its language —
/// including to or from no language at all (e.g. `.java` -> `.kt`, or
/// `.java` -> `.txt`). A directory rename moves every file beneath it, so
/// every tab pointing anywhere under `old` needs repointing; `strip_prefix`
/// against a tab whose path *is* `old` (the plain file-rename case) yields
/// an empty suffix. `new.join(suffix)` on an empty suffix is *not* simply
/// `new`: it appends a trailing separator (e.g. `new/`), which `PathBuf`'s
/// `==` treats as equal to `new` but which the OS does not — a later
/// `std::fs::write` to that path fails with "Is a directory", since a
/// trailing separator tells the OS the path must resolve to one. So the
/// empty-suffix case is handled separately, without going through `join` at
/// all.
fn handle_rename(state: &mut EditorState, parsers: &mut [Option<IncrementalParser>], old: &Path, new: &Path) {
    for (index, doc) in state.open_tabs.iter_mut().enumerate() {
        let Ok(suffix) = doc.path().strip_prefix(old) else {
            continue;
        };
        let new_path = if suffix.as_os_str().is_empty() {
            new.to_path_buf()
        } else {
            new.join(suffix)
        };
        doc.path = new_path.clone();

        let new_language = new_path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(Language::from_extension);
        if new_language != doc.language {
            doc.language = new_language;
            parsers[index] = tabs::open_parser_for(doc);
        }
    }
}

/// Replaces tab `index`'s buffer with `new_content` (an external change
/// `file_watch::ReconcileOutcome::ReloadTransparently` cleared to reload)
/// and gives it a fresh parser — the same "content changed out from under
/// the existing tree, start over" move `handle_rename` already makes for a
/// rename that changes a file's language, and every brand-new tab open
/// already makes for its very first parse.
fn reload_tab_from_disk(
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
    index: usize,
    new_content: &str,
) {
    let doc = &mut state.open_tabs[index];
    doc.buffer = Rope::from_str(new_content);
    doc.saved_buffer = doc.buffer.clone();
    parsers[index] = tabs::open_parser_for(doc);
}

/// Adds/removes `watcher`'s directory watches to match whatever `state`'s
/// open tabs currently need (`file_watch::watched_dirs_for`), called once
/// a frame — cheap even every frame (a HashSet diff over, realistically, a
/// handful of paths), and simpler than trying to hook this into every
/// individual place a tab can open or close instead. A `watch`/`unwatch`
/// failure (a directory that vanished, permissions) is silently dropped
/// rather than surfaced through `last_error`: this runs unconditionally
/// every frame regardless of whether anything actually changed, so a
/// persistently-failing directory would otherwise spam the same error
/// every frame forever.
fn sync_watched_dirs(
    watcher: &mut Option<notify::RecommendedWatcher>,
    watched_dirs: &mut HashSet<PathBuf>,
    state: &EditorState,
) {
    let Some(watcher) = watcher else { return };
    let needed = file_watch::watched_dirs_for(state.open_tabs.iter().map(|doc| doc.path()));
    for dir in needed.difference(watched_dirs) {
        let _ = watcher.watch(dir, notify::RecursiveMode::NonRecursive);
    }
    for dir in watched_dirs.difference(&needed) {
        let _ = watcher.unwatch(dir);
    }
    *watched_dirs = needed;
}

/// Drains every filesystem event `file_watcher` has queued since the last
/// frame, reconciling each one that touches an open tab
/// (`file_watch::reconcile`) — `try_recv` never blocks, so a frame with no
/// pending events costs one empty channel check. Paths outside any open
/// tab (most events, for a directory holding more than one file) are
/// silently ignored; `find_tab` is the same "does this path have an open
/// tab" lookup every other path-driven outcome in this app already uses.
fn process_file_events(
    rx: &std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
    external_conflicts: &mut HashSet<PathBuf>,
    externally_deleted: &mut HashSet<PathBuf>,
) {
    while let Ok(event_result) = rx.try_recv() {
        let Ok(event) = event_result else { continue };
        if !matches!(
            event.kind,
            notify::EventKind::Modify(_) | notify::EventKind::Create(_) | notify::EventKind::Remove(_)
        ) {
            continue;
        }
        for path in &event.paths {
            let Some(index) = state.find_tab(path) else {
                continue;
            };
            let disk_content = std::fs::read_to_string(path).ok();
            let doc = &state.open_tabs[index];
            let outcome = file_watch::reconcile(doc.is_dirty(), &doc.buffer.to_string(), disk_content.as_deref());

            match outcome {
                ReconcileOutcome::Unchanged => {
                    external_conflicts.remove(path);
                    externally_deleted.remove(path);
                }
                ReconcileOutcome::ReloadTransparently => {
                    reload_tab_from_disk(
                        state,
                        parsers,
                        index,
                        &disk_content.expect("Some per ReloadTransparently"),
                    );
                    external_conflicts.remove(path);
                    externally_deleted.remove(path);
                }
                ReconcileOutcome::Conflict => {
                    external_conflicts.insert(path.clone());
                }
                ReconcileOutcome::Deleted => {
                    externally_deleted.insert(path.clone());
                    external_conflicts.remove(path);
                }
            }
        }
    }
}

/// Draws the "changed on disk" / "deleted on disk" banner for the active
/// tab, if it's flagged in either set — directly above the tab bar/editor,
/// inside the same `CentralPanel` (see `FoxGardenApp::ui`), rather than as
/// its own dialog: this is meant to read as "a heads-up about the file
/// you're already looking at," not an interruption that has to be
/// dismissed before continuing to work.
fn show_external_change_banner(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
    external_conflicts: &mut HashSet<PathBuf>,
    externally_deleted: &mut HashSet<PathBuf>,
) {
    let Some(active) = state.active_tab else {
        return;
    };
    let path = state.open_tabs[active].path().to_path_buf();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    if externally_deleted.contains(&path) {
        ui.horizontal(|ui| {
            ui.label(format!("⚠ {name} was deleted on disk."));
            if ui.button("Dismiss").clicked() {
                externally_deleted.remove(&path);
            }
        });
        ui.separator();
    } else if external_conflicts.contains(&path) {
        ui.horizontal(|ui| {
            ui.label(format!("⚠ {name} changed on disk since you opened it."));
            if ui.button("Reload").clicked() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    reload_tab_from_disk(state, parsers, active, &content);
                }
                external_conflicts.remove(&path);
            }
            if ui.button("Keep Mine").clicked() {
                external_conflicts.remove(&path);
            }
        });
        ui.separator();
    }
}

/// Reopens whatever `persist_session` saved: the last project folder, its
/// open tabs (each given a parser, same as any other tab open), and which
/// one was focused. Free function (rather than inlined into `new`) so it's
/// testable against a fake `Storage` without needing a real
/// `eframe::CreationContext`, which isn't practically constructible in a
/// unit test.
fn restore_session(
    storage: &dyn eframe::Storage,
    state: &mut EditorState,
    parsers: &mut Vec<Option<IncrementalParser>>,
    last_error: &mut Option<String>,
) {
    if let Some(last_project) = storage.get_string(LAST_PROJECT_KEY) {
        let path = PathBuf::from(last_project);
        if path.is_dir()
            && let Err(err) = state.open_project(path)
        {
            *last_error = Some(format!("failed to reopen last project: {err}"));
        }
    }

    if let Some(open_tabs) = storage.get_string(OPEN_TABS_KEY) {
        for path_str in open_tabs.lines() {
            let path = PathBuf::from(path_str);
            if !path.is_file() {
                // Deleted, moved, or on since-unmounted storage since last
                // exit — just skip it rather than surfacing an error for a
                // tab the user can't act on.
                continue;
            }
            match state.open_tab(path) {
                Ok(index) => {
                    if index == parsers.len() {
                        let parser = tabs::open_parser_for(&mut state.open_tabs[index]);
                        parsers.push(parser);
                    }
                }
                Err(err) => *last_error = Some(format!("failed to reopen tab: {err}")),
            }
        }
    }

    if let Some(active_path) = storage.get_string(ACTIVE_TAB_KEY)
        && let Some(index) = state.find_tab(Path::new(&active_path))
    {
        state.focus_tab(index);
    }
}

/// Inverse of `restore_session`: writes the project folder, open tab paths
/// (in tab order), and the focused tab's path, so the next launch can
/// reconstruct the same session.
fn persist_session(storage: &mut dyn eframe::Storage, state: &EditorState) {
    if let Some(project) = &state.project {
        storage.set_string(LAST_PROJECT_KEY, project.root.to_string_lossy().into_owned());
    }

    let open_tabs = state
        .open_tabs
        .iter()
        .map(|doc| doc.path().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    storage.set_string(OPEN_TABS_KEY, open_tabs);

    let active_tab = state
        .active_tab
        .and_then(|index| state.open_tabs.get(index))
        .map(|doc| doc.path().to_string_lossy().into_owned())
        .unwrap_or_default();
    storage.set_string(ACTIVE_TAB_KEY, active_tab);
}

/// Restores the font family/size, theme choice, and indentation style saved
/// by `persist_settings`, leaving each parameter at its current (default)
/// value if its key is missing or unparseable — same "best-effort, one
/// missing key doesn't block the rest" shape as `restore_session`. Separate
/// from that function because this is "how the editor looks/behaves," not
/// "what was open"; the two happen to both live in `eframe::Storage` but
/// are independent concerns.
#[expect(
    clippy::too_many_arguments,
    reason = "each parameter is an independently-owned Settings value restored from its own storage key, not a bundle waiting to be a struct — same shape and reasoning as menu_bar::show's own allowance"
)]
fn restore_settings(
    storage: &dyn eframe::Storage,
    editor_font: &mut EditorFont,
    font_size: &mut f32,
    dark_mode: &mut bool,
    indent_settings: &mut IndentSettings,
    view_settings: &mut ViewSettings,
    side_panel_width: &mut f32,
    side_panel_visible: &mut bool,
    custom_templates: &mut UserTemplates,
) {
    if let Some(key) = storage.get_string(EDITOR_FONT_KEY)
        && let Some(font) = EditorFont::from_storage_key(&key)
    {
        *editor_font = font;
    }
    if let Some(size) = storage.get_string(FONT_SIZE_KEY).and_then(|s| s.parse::<f32>().ok()) {
        *font_size = size;
    }
    if let Some(dark) = storage.get_string(DARK_MODE_KEY) {
        *dark_mode = dark == "true";
    }
    if let Some(use_tabs) = storage.get_string(INDENT_USE_TABS_KEY) {
        indent_settings.use_tabs = use_tabs == "true";
    }
    if let Some(width) = storage
        .get_string(INDENT_WIDTH_KEY)
        .and_then(|s| s.parse::<usize>().ok())
    {
        indent_settings.width = width;
    }
    if let Some(word_wrap) = storage.get_string(WORD_WRAP_KEY) {
        view_settings.word_wrap = word_wrap == "true";
    }
    if let Some(show_whitespace) = storage.get_string(SHOW_WHITESPACE_KEY) {
        view_settings.show_whitespace = show_whitespace == "true";
    }
    if let Some(show_indent_guides) = storage.get_string(SHOW_INDENT_GUIDES_KEY) {
        view_settings.show_indent_guides = show_indent_guides == "true";
    }
    if let Some(show_sticky_scroll) = storage.get_string(SHOW_STICKY_SCROLL_KEY) {
        view_settings.show_sticky_scroll = show_sticky_scroll == "true";
    }
    if let Some(cursor_blink) = storage.get_string(CURSOR_BLINK_KEY) {
        view_settings.cursor_blink = cursor_blink == "true";
    }
    if let Some(show_editor_outline) = storage.get_string(SHOW_EDITOR_OUTLINE_KEY) {
        view_settings.show_editor_outline = show_editor_outline == "true";
    }
    if let Some(width) = storage
        .get_string(SIDE_PANEL_WIDTH_KEY)
        .and_then(|s| s.parse::<f32>().ok())
    {
        *side_panel_width = width;
    }
    if let Some(visible) = storage.get_string(SIDE_PANEL_VISIBLE_KEY) {
        *side_panel_visible = visible == "true";
    }
    if let Some(saved) = storage.get_string(CUSTOM_JAVA_TEMPLATES_KEY) {
        custom_templates.java = crate::widgets::editor::parse_user_templates(&saved);
    }
    if let Some(saved) = storage.get_string(CUSTOM_KOTLIN_TEMPLATES_KEY) {
        custom_templates.kotlin = crate::widgets::editor::parse_user_templates(&saved);
    }
    if let Some(saved) = storage.get_string(CUSTOM_GLOBAL_TEMPLATES_KEY) {
        custom_templates.global = crate::widgets::editor::parse_user_templates(&saved);
    }
}

/// Inverse of `restore_settings`.
#[expect(
    clippy::too_many_arguments,
    reason = "each parameter is an independently-owned Settings value written to its own storage key, not a bundle waiting to be a struct — same shape and reasoning as restore_settings' own allowance"
)]
fn persist_settings(
    storage: &mut dyn eframe::Storage,
    editor_font: EditorFont,
    font_size: f32,
    dark_mode: bool,
    indent_settings: IndentSettings,
    view_settings: ViewSettings,
    side_panel_width: f32,
    side_panel_visible: bool,
    custom_templates: &UserTemplates,
) {
    storage.set_string(EDITOR_FONT_KEY, editor_font.storage_key().to_string());
    storage.set_string(FONT_SIZE_KEY, font_size.to_string());
    storage.set_string(DARK_MODE_KEY, dark_mode.to_string());
    storage.set_string(INDENT_USE_TABS_KEY, indent_settings.use_tabs.to_string());
    storage.set_string(INDENT_WIDTH_KEY, indent_settings.width.to_string());
    storage.set_string(WORD_WRAP_KEY, view_settings.word_wrap.to_string());
    storage.set_string(SHOW_WHITESPACE_KEY, view_settings.show_whitespace.to_string());
    storage.set_string(SHOW_INDENT_GUIDES_KEY, view_settings.show_indent_guides.to_string());
    storage.set_string(SHOW_STICKY_SCROLL_KEY, view_settings.show_sticky_scroll.to_string());
    storage.set_string(CURSOR_BLINK_KEY, view_settings.cursor_blink.to_string());
    storage.set_string(SHOW_EDITOR_OUTLINE_KEY, view_settings.show_editor_outline.to_string());
    storage.set_string(SIDE_PANEL_WIDTH_KEY, side_panel_width.to_string());
    storage.set_string(SIDE_PANEL_VISIBLE_KEY, side_panel_visible.to_string());
    storage.set_string(
        CUSTOM_JAVA_TEMPLATES_KEY,
        crate::widgets::editor::serialize_user_templates(&custom_templates.java),
    );
    storage.set_string(
        CUSTOM_KOTLIN_TEMPLATES_KEY,
        crate::widgets::editor::serialize_user_templates(&custom_templates.kotlin),
    );
    storage.set_string(
        CUSTOM_GLOBAL_TEMPLATES_KEY,
        crate::widgets::editor::serialize_user_templates(&custom_templates.global),
    );
}

impl FoxGardenApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state = EditorState::new();
        let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
        let mut last_error = None;
        let mut editor_font = EditorFont::default();
        let mut font_size = DEFAULT_FONT_SIZE;
        let mut dark_mode = DEFAULT_DARK_MODE;
        let mut indent_settings = IndentSettings::default();
        let mut view_settings = ViewSettings::default();
        let mut side_panel_width = DEFAULT_SIDE_PANEL_WIDTH;
        let mut side_panel_visible = true;
        let mut custom_templates = UserTemplates::default();

        if let Some(storage) = cc.storage {
            restore_session(storage, &mut state, &mut parsers, &mut last_error);
            restore_settings(
                storage,
                &mut editor_font,
                &mut font_size,
                &mut dark_mode,
                &mut indent_settings,
                &mut view_settings,
                &mut side_panel_width,
                &mut side_panel_visible,
                &mut custom_templates,
            );
        }
        theme::apply(&cc.egui_ctx, dark_mode);

        let (file_event_tx, file_event_rx) = std::sync::mpsc::channel();
        let file_watcher = notify::recommended_watcher(file_event_tx).ok();

        Self {
            state,
            parsers,
            pending_close: None,
            generate_dialog: None,
            generate_method_dialog: None,
            override_method_dialog: None,
            completion: None,
            pending_editor_input: Vec::new(),
            cached_clipboard_text: None,
            pending_navigation: None,
            side_panel: SidePanelState::default(),
            side_panel_width,
            side_panel_visible,
            menu_bar: MenuBarState::default(),
            quick_switcher: QuickSwitcherState::default(),
            go_to_file: GoToFileState::default(),
            spring_endpoints: SpringEndpointsState::default(),
            run_configs_dialog: RunConfigsDialogState::default(),
            editor_font,
            font_size,
            dark_mode,
            indent_settings,
            view_settings,
            custom_templates,
            zen_mode: false,
            last_error,
            file_watcher,
            file_event_rx,
            watched_dirs: HashSet::new(),
            external_conflicts: HashSet::new(),
            externally_deleted: HashSet::new(),
        }
    }
}

/// Shows `last_error` (if any) as a dismissable modal, and clears it once
/// acknowledged. Free function rather than a method so its borrow of
/// `last_error` doesn't overlap `&mut self` for the rest of `ui()`.
///
/// Borrows the message for the label instead of cloning it — `show_modal`
/// returning the closure's result (whether "OK" was clicked) is what lets
/// the actual `*last_error = None` write happen after `show_modal` returns,
/// once the borrow of `last_error` used for `message` has ended, without
/// needing a separate `dismissed` flag mutated from inside the closure.
fn show_error_modal(ui: &egui::Ui, last_error: &mut Option<String>) {
    let message = last_error.as_deref();
    let outcome = show_modal(ui, "error_modal", message, |ui, message| {
        ui.label(*message);
        ui.button("OK").clicked()
    });
    if let Some((ok_clicked, escape_pressed)) = outcome
        && (ok_clicked || escape_pressed)
    {
        *last_error = None;
    }
}

impl eframe::App for FoxGardenApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.input(|i| i.key_pressed(egui::Key::F11)) {
            self.zen_mode = !self.zen_mode;
        }
        if ui.input(|i| i.key_pressed(egui::Key::E) && i.modifiers.command && !i.modifiers.shift) {
            self.quick_switcher.toggle();
        }
        if ui.input(|i| i.key_pressed(egui::Key::P) && i.modifiers.command) {
            self.go_to_file.toggle();
        }
        if ui.input(|i| i.key_pressed(egui::Key::E) && i.modifiers.command && i.modifiers.shift) {
            self.spring_endpoints.toggle(self.state.project.as_ref().map(|p| &p.tree));
        }
        if ui.input(|i| i.key_pressed(egui::Key::N) && i.modifiers.command)
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            self.side_panel.begin_new_file(root);
        }
        if ui.input(|i| i.key_pressed(egui::Key::B) && i.modifiers.command) {
            self.side_panel_visible = !self.side_panel_visible;
        }

        sync_watched_dirs(&mut self.file_watcher, &mut self.watched_dirs, &self.state);
        process_file_events(
            &self.file_event_rx,
            &mut self.state,
            &mut self.parsers,
            &mut self.external_conflicts,
            &mut self.externally_deleted,
        );

        // Resolved here, once per frame, ahead of `tabs::show` below so its
        // own `jump_to_char` reflects whatever a popup pick set last frame
        // (`spring_endpoints::show`, further down this same function, is
        // what actually sets `pending_navigation` — see its own call site).
        let jump_target = resolve_pending_navigation(&self.state, &mut self.pending_navigation);
        if let Some((path, char_offset)) = &jump_target {
            jump_to(ui.ctx(), path, *char_offset);
        }

        let mut outcome = side_panel::SidePanelOutcome::default();
        let mut menu_outcome = menu_bar::MenuBarOutcome::default();

        if !self.zen_mode {
            menu_outcome = egui::Panel::top("menu_bar")
                .show(ui, |ui| {
                    menu_bar::show(
                        ui,
                        &mut self.state,
                        &mut self.side_panel,
                        &mut self.parsers,
                        &mut self.pending_close,
                        &mut self.menu_bar,
                        &mut self.editor_font,
                        &mut self.font_size,
                        &mut self.dark_mode,
                        &mut self.indent_settings,
                        &mut self.view_settings,
                        &mut self.zen_mode,
                        &mut self.side_panel_visible,
                        &mut self.last_error,
                        &mut self.custom_templates,
                    )
                })
                .inner;

            if self.side_panel_visible {
                let panel_response = egui::Panel::left("project_panel")
                    .default_size(self.side_panel_width)
                    .show(ui, |ui| {
                        side_panel::show(ui, &mut self.state, &mut self.side_panel)
                    });
                // Tracks a live drag, not just the size at the frame the
                // resize handle is released — so `self.side_panel_width`
                // (what `save()` persists) always reflects exactly what's on
                // screen.
                self.side_panel_width = panel_response.response.rect.width();
                outcome = panel_response.inner;
            }
        }

        if let Some(path) = outcome.open {
            open_path(&mut self.state, &mut self.parsers, &mut self.last_error, path);
        }
        if let Some((old, new)) = outcome.renamed {
            handle_rename(&mut self.state, &mut self.parsers, &old, &new);
        }
        if let Some(path) = outcome.deleted {
            close_tabs_under(&mut self.state, &mut self.parsers, &path);
        }
        if let Some(err) = outcome.error {
            self.last_error = Some(err);
        }

        if menu_outcome.open_run_configs_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            self.run_configs_dialog.open(&root);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            show_external_change_banner(
                ui,
                &mut self.state,
                &mut self.parsers,
                &mut self.external_conflicts,
                &mut self.externally_deleted,
            );
            tabs::show(
                ui,
                &mut self.state,
                &mut self.pending_close,
                &mut self.parsers,
                self.editor_font,
                self.font_size,
                self.indent_settings,
                self.view_settings,
                menu_outcome.generate_request,
                &mut self.generate_dialog,
                menu_outcome.generate_method_request,
                &mut self.generate_method_dialog,
                menu_outcome.override_method_request,
                &mut self.override_method_dialog,
                &mut self.completion,
                menu_outcome.case_conversion_request,
                menu_outcome.sort_lines_request,
                menu_outcome.unique_lines_request,
                menu_outcome.fold_all_request,
                menu_outcome.expand_all_request,
                &mut self.last_error,
                &mut self.pending_editor_input,
                &mut self.cached_clipboard_text,
                jump_target.as_ref().map(|(_, char_offset)| *char_offset),
                &self.custom_templates,
            );
        });

        if let Some(path) = quick_switcher::show(ui, &self.state, &mut self.quick_switcher) {
            open_path(&mut self.state, &mut self.parsers, &mut self.last_error, path);
        }
        if let Some(path) = go_to_file::show(ui, &self.state, &mut self.go_to_file) {
            open_path(&mut self.state, &mut self.parsers, &mut self.last_error, path);
        }
        if let Some((path, handler_byte)) = spring_endpoints::show(ui, &self.state, &mut self.spring_endpoints) {
            open_path(&mut self.state, &mut self.parsers, &mut self.last_error, path.clone());
            self.pending_navigation = Some((path, handler_byte));
        }
        if let Some(root) = self.state.project.as_ref().map(|p| p.root.clone()) {
            run_configs::show(ui, &root, &mut self.run_configs_dialog, &mut self.last_error);
        }

        show_error_modal(ui, &mut self.last_error);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        persist_session(storage, &self.state);
        persist_settings(
            storage,
            self.editor_font,
            self.font_size,
            self.dark_mode,
            self.indent_settings,
            self.view_settings,
            self.side_panel_width,
            self.side_panel_visible,
            &self.custom_templates,
        );
    }
}

#[cfg(test)]
mod tests;
