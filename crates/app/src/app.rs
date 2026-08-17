use std::collections::HashSet;
use std::path::{Path, PathBuf};

use fg_core::{EditorState, Language};
use fg_i18n::{msg, t};
use notify::Watcher;
use ropey::Rope;
use syntax::IncrementalParser;

use crate::auto_save::{AutoSaveMode, AutoSaveSettings, AutoSaveState};
use crate::file_watch::{self, ReconcileOutcome};
use crate::jdk_registry::JdkRegistry;
use crate::lsp_settings::LspSettings;
use crate::lsp_state::LspState;
use crate::panels::git_diff::DiffState;
use crate::panels::git_stage::{self, GitStageState};
use crate::panels::go_to_file::{self, GoToFileState};
use crate::panels::jdk_registry::{self as jdk_registry_ui, JdkRegistryState};
use crate::panels::lsp_servers::{self, LspServersState};
use crate::panels::menu_bar::{self, MenuBarState};
use crate::panels::quick_switcher::{self, QuickSwitcherState};
use crate::panels::run_configs::{self, RunConfigsDialogState};
use crate::panels::side_panel::{self, SidePanelState};
use crate::panels::spring_config::SpringConfigState;
use crate::panels::spring_endpoints::{self, SpringEndpointsState};
use crate::panels::static_analysis::{self, ExternalToolPaths, StaticAnalysisState};
use crate::panels::status_bar;
use crate::panels::tabs;
use crate::panels::terminal_panel;
use crate::pty_session::PtySession;
use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::style::theme;
use crate::style::view::ViewSettings;
use crate::widgets::editor::{
    CompletionState, GenerateAccessorsDialog, GenerateMethodDialog, HoverState, OverrideMethodDialog, UserTemplates,
    jump_to,
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
/// The UI language, as a `fg_i18n::Lang::tag` (`"pt-BR"`, `"en-US"`).
///
/// Absent until the user picks one in Settings > Language: an unset key
/// means "follow the system locale", which is what a fresh install does.
const LANGUAGE_KEY: &str = "language";
const INDENT_USE_TABS_KEY: &str = "indent_use_tabs";
const INDENT_WIDTH_KEY: &str = "indent_width";
const WORD_WRAP_KEY: &str = "word_wrap";
const SHOW_WHITESPACE_KEY: &str = "show_whitespace";
const SHOW_INDENT_GUIDES_KEY: &str = "show_indent_guides";
const SHOW_STICKY_SCROLL_KEY: &str = "show_sticky_scroll";
const CURSOR_BLINK_KEY: &str = "cursor_blink";
const SHOW_EDITOR_OUTLINE_KEY: &str = "show_editor_outline";
const SHOW_INLINE_BLAME_KEY: &str = "show_inline_blame";
const TERMINAL_PANEL_VISIBLE_KEY: &str = "terminal_panel_visible";
const SOURCE_CONTROL_PANEL_VISIBLE_KEY: &str = "source_control_panel_visible";
const SIDE_PANEL_WIDTH_KEY: &str = "side_panel_width";
const SIDE_PANEL_VISIBLE_KEY: &str = "side_panel_visible";
const CUSTOM_JAVA_TEMPLATES_KEY: &str = "custom_java_templates";
const CUSTOM_KOTLIN_TEMPLATES_KEY: &str = "custom_kotlin_templates";
const CUSTOM_GLOBAL_TEMPLATES_KEY: &str = "custom_global_templates";
const CHECKSTYLE_BINARY_KEY: &str = "checkstyle_binary";
const CHECKSTYLE_CONFIG_KEY: &str = "checkstyle_config";
const CHECKSTYLE_INSTALLED_VERSION_KEY: &str = "checkstyle_installed_version";
const PMD_BINARY_KEY: &str = "pmd_binary";
const PMD_RULESET_KEY: &str = "pmd_ruleset";
const PMD_INSTALLED_VERSION_KEY: &str = "pmd_installed_version";
const SPOTBUGS_BINARY_KEY: &str = "spotbugs_binary";
const SPOTBUGS_INSTALLED_VERSION_KEY: &str = "spotbugs_installed_version";
const AUTO_SAVE_ENABLED_KEY: &str = "auto_save_enabled";
const AUTO_SAVE_MODE_KEY: &str = "auto_save_mode";
const AUTO_SAVE_IDLE_SECONDS_KEY: &str = "auto_save_idle_seconds";
/// `AUTO_SAVE_MODE_KEY`'s persisted value for `AutoSaveMode::OnFocusLoss` —
/// an explicit string rather than `{:?}`, so a future `Debug` reformat
/// (e.g. renaming the variant) can't silently change what's on disk and
/// break restoring an existing install's choice.
const AUTO_SAVE_MODE_ON_FOCUS_LOSS: &str = "on_focus_loss";
const AUTO_SAVE_MODE_AFTER_IDLE: &str = "after_idle";
const LSP_ENABLED_KEY: &str = "lsp_enabled";
const LSP_JDTLS_BINARY_KEY: &str = "lsp_jdtls_binary";
const LSP_JDTLS_INSTALLED_VERSION_KEY: &str = "lsp_jdtls_installed_version";
const LSP_JDTLS_JAVA_HOME_KEY: &str = "lsp_jdtls_java_home";
const LSP_KOTLIN_LANGUAGE_SERVER_BINARY_KEY: &str = "lsp_kotlin_language_server_binary";
const LSP_KOTLIN_LANGUAGE_SERVER_INSTALLED_VERSION_KEY: &str = "lsp_kotlin_language_server_installed_version";
const JDK_REGISTRY_KEY: &str = "jdk_registry";

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
    /// The hover-docs popup for whichever tab is currently focused —
    /// `widgets::editor::hover::HoverState` (`PLAN.md` Track 20 Phase 3).
    /// Same "one field, not one per open tab" shape `completion` above
    /// already uses, for the same reason.
    hover: HoverState,
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
    /// Whether the terminal panel is docked open at the bottom — toggled by
    /// the View menu's "Terminal Panel" checkbox or `Ctrl+\``, same
    /// "one flag, two triggers" shape as `side_panel_visible`. Persisted
    /// across restarts just like `side_panel_visible` — a dead shell
    /// process has no scrollback/state worth resuming (`SPEC.md` §8's own
    /// non-goal), but the *panel being open* is itself a preference worth
    /// remembering; `FoxGardenApp::new` spawns a fresh session for it right
    /// after restoring this flag, the same "opening the terminal starts a
    /// shell" convention `Ctrl+\``'s own handler already established, so a
    /// relaunch resumes to a working terminal rather than an empty panel.
    terminal_panel_visible: bool,
    /// Kept index-aligned with `state.terminal_tabs`, same shape as
    /// `parsers`/`open_tabs`: each session's real pty child process/writer/
    /// output can't live on `EditorState` (`crates/core` stays headless,
    /// `AGENTS.md`), so it lives here instead, one per `TerminalTab`.
    terminal_sessions: Vec<crate::pty_session::PtySession>,
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
    /// Settings > External Tools… dialog state and any in-flight Checkstyle
    /// scan — see `panels::static_analysis`.
    static_analysis: StaticAnalysisState,
    /// The current project's own scanned Spring config properties (Track
    /// 12), lazily scanned the first time a `.properties`/`.yml` file's
    /// completion trigger actually needs them — see `panels::spring_config`.
    spring_config: SpringConfigState,
    /// Settings > External Tools — binary/config paths for Checkstyle/PMD/
    /// SpotBugs, a personal per-machine preference like `editor_font`, so
    /// persisted the same way (see `restore_settings`/`persist_settings`),
    /// not under a project's `.foxgarden/`.
    external_tool_paths: ExternalToolPaths,
    /// Settings > Auto-save — see `auto_save::AutoSaveSettings`.
    auto_save_settings: AutoSaveSettings,
    /// Settings > Language Server — see `lsp_settings::LspSettings`. Off by
    /// default; `lsp_state::LspState` is its Phase 1 consumer and launches
    /// an external `jdtls`/`kotlin-language-server` process only after the
    /// user enables it and opens a matching project document.
    lsp_settings: LspSettings,
    /// Runtime-only owner of the opt-in Java/Kotlin server processes. Kept
    /// beside the persisted settings, but never persisted itself: a fresh
    /// launch performs a fresh handshake against the current project.
    lsp: LspState,
    /// Settings > Language Servers… — the dialog that edits `lsp_settings`
    /// above, plus the background installs/update checks behind it
    /// (`lsp_manager`). Runtime-only: what an install produced is persisted
    /// through `lsp_settings`, and a job in flight at shutdown is simply
    /// gone, not resumed.
    lsp_servers: LspServersState,
    /// Machine-wide inventory of registered JDKs (`PLAN.md` Track 29,
    /// Phase 1) — persisted (a JDK install is a fact about the machine,
    /// same as `lsp_settings` above), edited through Settings > JDKs… via
    /// `jdk_registry_ui`.
    jdk_registry: JdkRegistry,
    /// Settings > JDKs… — the dialog that edits `jdk_registry` above.
    /// Runtime-only, same reasoning as `lsp_servers` above.
    jdk_registry_ui: JdkRegistryState,
    /// Focus-edge/idle-clock tracking `auto_save_settings`'s triggers need —
    /// runtime-only, never persisted (there's nothing meaningful to resume
    /// across a restart: `was_focused` starts however the OS hands focus to
    /// a freshly launched window, and the idle clock starts over anyway).
    auto_save_state: AutoSaveState,
    /// In-flight/completed `git diff` scans backing the diff gutter
    /// (`PLAN.md` Track 9 Phase 1) — see `panels::git_diff::DiffState`.
    /// Runtime-only, same as `auto_save_state`: a fresh launch just runs a
    /// fresh diff for every reopened tab rather than trying to resume
    /// anything.
    diff: DiffState,
    /// Whether the Source Control (stage/commit) panel is docked open on
    /// the right — toggled by the View menu's "Source Control" checkbox,
    /// same "one flag, one menu toggle" shape as `side_panel_visible`/
    /// `terminal_panel_visible`. Persisted across restarts the same way —
    /// unlike the terminal, there's no session to spawn on resume, just an
    /// empty panel that refreshes itself once a project's open.
    source_control_visible: bool,
    /// The Source Control panel's own status list plus any in-flight
    /// `git status`/add/reset/commit — see `panels::git_stage::GitStageState`.
    /// Runtime-only, same as `diff`: a fresh launch just runs a fresh
    /// `git status` once the panel's shown rather than resuming anything.
    git_stage: GitStageState,
}

/// Opens `path` in a new tab (or focuses its existing tab, via
/// `EditorState::open_tab`'s own dedup), surfacing any failure through
/// `last_error`. Shared by the side panel's "open a file from the tree"
/// outcome and the recent-files quick switcher (`Ctrl+E`) — both just want
/// "open this path, tell the user if it didn't work," identically. Also
/// kicks off a `git diff` for the newly-opened tab (`PLAN.md` Track 9 Phase
/// 1's "on open" trigger) — a no-op when `diff_root` is `None` (no project
/// open). Deliberately unconditional, even when `open_tab` just focused an
/// *already*-open tab rather than truly opening a new one: cheap enough to
/// re-run, and simpler than threading a "was this actually new" flag out of
/// `EditorState::open_tab`'s own dedup just to skip it.
fn open_path(
    state: &mut EditorState,
    parsers: &mut Vec<Option<IncrementalParser>>,
    last_error: &mut Option<String>,
    diff: &mut DiffState,
    diff_root: Option<PathBuf>,
    path: PathBuf,
) {
    match state.open_tab(path) {
        Ok(index) => {
            if index == parsers.len() {
                let parser = tabs::open_parser_for(&mut state.open_tabs[index]);
                parsers.push(parser);
            }
            if let Some(root) = diff_root {
                diff.run(state.open_tabs[index].path.clone(), root);
            }
        }
        Err(err) => {
            // Both variants carry their own `PathBuf`, so the display string
            // is built here in the failure arm only, instead of
            // unconditionally before the match on every open attempt.
            let message = match &err {
                fg_core::OpenDocumentError::Binary(path) => msg::couldnt_open_not_text(&path.display().to_string()),
                fg_core::OpenDocumentError::Io(path, e) => {
                    msg::couldnt_open(&path.display().to_string(), &e.to_string())
                }
            };
            *last_error = Some(message);
        }
    }
}

/// Spawns a new pty session (the open project's root as its cwd, if any)
/// and, only once that succeeds, appends a matching `TerminalTab` — keeping
/// `sessions` and `state.terminal_tabs` index-aligned the same way
/// `open_path` keeps `parsers` aligned with `open_tabs`. A spawn failure
/// (no shell found, pty allocation failed, ...) surfaces through
/// `last_error` instead of leaving a `TerminalTab` with no real session
/// behind it.
fn new_terminal_session(
    state: &mut EditorState,
    sessions: &mut Vec<PtySession>,
    ctx: &egui::Context,
    last_error: &mut Option<String>,
) {
    let cwd = state.project.as_ref().map(|project| project.root.clone());
    match PtySession::spawn(cwd.as_deref(), ctx.clone()) {
        Ok(session) => {
            sessions.push(session);
            state.new_terminal_tab();
        }
        Err(err) => *last_error = Some(msg::failed_to_start_terminal(&err.to_string())),
    }
}

/// Removes both the session at `index` (dropping it kills its child
/// process, see `PtySession`'s own `Drop`) and its `TerminalTab`, together —
/// same index-aligned-removal shape `new_terminal_session` uses on the way
/// in.
fn close_terminal_session(state: &mut EditorState, sessions: &mut Vec<PtySession>, index: usize) {
    if index < sessions.len() {
        sessions.remove(index);
    }
    state.close_terminal_tab(index);
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
/// `file_watch::ReconcileOutcome::ReloadTransparently` cleared to reload, or
/// the external-change banner's manual "Reload" button) and gives it a
/// fresh parser — the same "content changed out from under the existing
/// tree, start over" move `handle_rename` already makes for a rename that
/// changes a file's language, and every brand-new tab open already makes
/// for its very first parse. Also kicks off a fresh `git diff`
/// unconditionally (`PLAN.md` Track 9 Phase 1's "on reload" trigger, a
/// no-op when `diff_root` is `None`) — `panels::git_diff::DiffState::
/// check_for_saves`'s own dirty-transition heuristic already happens to
/// catch the manual-Reload case (discarding local edits is itself a dirty
/// -> clean transition) but *not* this transparent-auto-reload case (never
/// dirty before or after, since it only fires when there were no local
/// edits to begin with), so this explicit call is what actually covers it;
/// re-running for the manual-Reload case too is harmless, just redundant.
fn reload_tab_from_disk(
    state: &mut EditorState,
    parsers: &mut [Option<IncrementalParser>],
    index: usize,
    new_content: &str,
    diff: &mut DiffState,
    diff_root: Option<PathBuf>,
) {
    let doc = &mut state.open_tabs[index];
    doc.buffer = Rope::from_str(new_content);
    doc.saved_buffer = doc.buffer.clone();
    doc.lsp_version += 1;
    doc.lsp_sync_pending = true;
    parsers[index] = tabs::open_parser_for(doc);
    if let Some(root) = diff_root {
        diff.run(doc.path.clone(), root);
    }
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
    diff: &mut DiffState,
    diff_root: Option<&Path>,
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
                        diff,
                        diff_root.map(Path::to_path_buf),
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
    diff: &mut DiffState,
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
            ui.label(msg::file_deleted_on_disk(&name));
            if ui.button(t().common.dismiss).clicked() {
                externally_deleted.remove(&path);
            }
        });
        ui.separator();
    } else if external_conflicts.contains(&path) {
        ui.horizontal(|ui| {
            ui.label(msg::file_changed_on_disk(&name));
            if ui.button(t().common.reload).clicked() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    // No explicit `git diff` trigger here — discarding
                    // local edits to match disk is itself a dirty -> clean
                    // transition, which `DiffState::check_for_saves`
                    // (called once per frame from `FoxGardenApp::ui`)
                    // already catches generically. See `reload_tab_from_
                    // disk`'s own doc comment.
                    reload_tab_from_disk(state, parsers, active, &content, diff, None);
                }
                external_conflicts.remove(&path);
            }
            if ui.button(t().common.keep_mine).clicked() {
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
            *last_error = Some(msg::failed_to_reopen_last_project(&err.to_string()));
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
                Err(err) => *last_error = Some(msg::failed_to_reopen_tab(&err.to_string())),
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
/// The language the user explicitly chose in Settings > Language, if any.
///
/// `None` means "nobody has chosen" — no storage at all (a first run), no
/// `LANGUAGE_KEY` in it, or a tag this build doesn't recognise (a settings
/// file written by a newer FoxGarden, or hand-edited). All three deserve
/// the same answer: fall back to the host system's locale rather than to an
/// arbitrary language.
///
/// Split out of `FoxGardenApp::new` so it can be tested without touching
/// `fg_i18n`'s process-global active language, which the rest of the test
/// binary is simultaneously reading.
fn stored_language(storage: Option<&dyn eframe::Storage>) -> Option<fg_i18n::Lang> {
    storage
        .and_then(|storage| storage.get_string(LANGUAGE_KEY))
        .and_then(|tag| fg_i18n::Lang::from_tag(&tag))
}

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
    terminal_panel_visible: &mut bool,
    source_control_visible: &mut bool,
    custom_templates: &mut UserTemplates,
    external_tool_paths: &mut ExternalToolPaths,
    auto_save_settings: &mut AutoSaveSettings,
    lsp_settings: &mut LspSettings,
    jdk_registry: &mut JdkRegistry,
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
    if let Some(show_inline_blame) = storage.get_string(SHOW_INLINE_BLAME_KEY) {
        view_settings.show_inline_blame = show_inline_blame == "true";
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
    if let Some(visible) = storage.get_string(TERMINAL_PANEL_VISIBLE_KEY) {
        *terminal_panel_visible = visible == "true";
    }
    if let Some(visible) = storage.get_string(SOURCE_CONTROL_PANEL_VISIBLE_KEY) {
        *source_control_visible = visible == "true";
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
    if let Some(path) = storage.get_string(CHECKSTYLE_BINARY_KEY) {
        external_tool_paths.checkstyle_binary = path;
    }
    if let Some(path) = storage.get_string(CHECKSTYLE_CONFIG_KEY) {
        external_tool_paths.checkstyle_config = path;
    }
    if let Some(version) = storage.get_string(CHECKSTYLE_INSTALLED_VERSION_KEY) {
        external_tool_paths.checkstyle_installed_version = version;
    }
    if let Some(path) = storage.get_string(PMD_BINARY_KEY) {
        external_tool_paths.pmd_binary = path;
    }
    if let Some(path) = storage.get_string(PMD_RULESET_KEY) {
        external_tool_paths.pmd_ruleset = path;
    }
    if let Some(version) = storage.get_string(PMD_INSTALLED_VERSION_KEY) {
        external_tool_paths.pmd_installed_version = version;
    }
    if let Some(path) = storage.get_string(SPOTBUGS_BINARY_KEY) {
        external_tool_paths.spotbugs_binary = path;
    }
    if let Some(version) = storage.get_string(SPOTBUGS_INSTALLED_VERSION_KEY) {
        external_tool_paths.spotbugs_installed_version = version;
    }
    if let Some(enabled) = storage.get_string(AUTO_SAVE_ENABLED_KEY) {
        auto_save_settings.enabled = enabled == "true";
    }
    if let Some(mode) = storage.get_string(AUTO_SAVE_MODE_KEY) {
        auto_save_settings.mode = match mode.as_str() {
            AUTO_SAVE_MODE_AFTER_IDLE => AutoSaveMode::AfterIdle,
            _ => AutoSaveMode::OnFocusLoss,
        };
    }
    if let Some(idle_seconds) = storage
        .get_string(AUTO_SAVE_IDLE_SECONDS_KEY)
        .and_then(|s| s.parse::<u32>().ok())
    {
        auto_save_settings.idle_seconds = idle_seconds;
    }
    if let Some(enabled) = storage.get_string(LSP_ENABLED_KEY) {
        lsp_settings.enabled = enabled == "true";
    }
    if let Some(path) = storage.get_string(LSP_JDTLS_BINARY_KEY) {
        lsp_settings.jdtls_binary = path;
    }
    if let Some(version) = storage.get_string(LSP_JDTLS_INSTALLED_VERSION_KEY) {
        lsp_settings.jdtls_installed_version = version;
    }
    if let Some(home) = storage.get_string(LSP_JDTLS_JAVA_HOME_KEY) {
        lsp_settings.jdtls_java_home = home;
    }
    if let Some(path) = storage.get_string(LSP_KOTLIN_LANGUAGE_SERVER_BINARY_KEY) {
        lsp_settings.kotlin_language_server_binary = path;
    }
    if let Some(version) = storage.get_string(LSP_KOTLIN_LANGUAGE_SERVER_INSTALLED_VERSION_KEY) {
        lsp_settings.kotlin_language_server_installed_version = version;
    }
    if let Some(saved) = storage.get_string(JDK_REGISTRY_KEY) {
        *jdk_registry = JdkRegistry::from_json(&saved);
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
    terminal_panel_visible: bool,
    source_control_visible: bool,
    custom_templates: &UserTemplates,
    external_tool_paths: &ExternalToolPaths,
    auto_save_settings: AutoSaveSettings,
    lsp_settings: &LspSettings,
    jdk_registry: &JdkRegistry,
) {
    storage.set_string(EDITOR_FONT_KEY, editor_font.storage_key().to_string());
    storage.set_string(FONT_SIZE_KEY, font_size.to_string());
    storage.set_string(DARK_MODE_KEY, dark_mode.to_string());
    // Read straight off `fg_i18n`'s global rather than taken as a
    // parameter: the active language *is* that global (see
    // `menu_bar`'s Settings > Language), so there's no copy on
    // `FoxGardenApp` that could disagree with it.
    storage.set_string(LANGUAGE_KEY, fg_i18n::lang().tag().to_string());
    storage.set_string(INDENT_USE_TABS_KEY, indent_settings.use_tabs.to_string());
    storage.set_string(INDENT_WIDTH_KEY, indent_settings.width.to_string());
    storage.set_string(WORD_WRAP_KEY, view_settings.word_wrap.to_string());
    storage.set_string(SHOW_WHITESPACE_KEY, view_settings.show_whitespace.to_string());
    storage.set_string(SHOW_INDENT_GUIDES_KEY, view_settings.show_indent_guides.to_string());
    storage.set_string(SHOW_STICKY_SCROLL_KEY, view_settings.show_sticky_scroll.to_string());
    storage.set_string(CURSOR_BLINK_KEY, view_settings.cursor_blink.to_string());
    storage.set_string(SHOW_EDITOR_OUTLINE_KEY, view_settings.show_editor_outline.to_string());
    storage.set_string(SHOW_INLINE_BLAME_KEY, view_settings.show_inline_blame.to_string());
    storage.set_string(SIDE_PANEL_WIDTH_KEY, side_panel_width.to_string());
    storage.set_string(SIDE_PANEL_VISIBLE_KEY, side_panel_visible.to_string());
    storage.set_string(TERMINAL_PANEL_VISIBLE_KEY, terminal_panel_visible.to_string());
    storage.set_string(SOURCE_CONTROL_PANEL_VISIBLE_KEY, source_control_visible.to_string());
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
    storage.set_string(CHECKSTYLE_BINARY_KEY, external_tool_paths.checkstyle_binary.clone());
    storage.set_string(CHECKSTYLE_CONFIG_KEY, external_tool_paths.checkstyle_config.clone());
    storage.set_string(
        CHECKSTYLE_INSTALLED_VERSION_KEY,
        external_tool_paths.checkstyle_installed_version.clone(),
    );
    storage.set_string(PMD_BINARY_KEY, external_tool_paths.pmd_binary.clone());
    storage.set_string(PMD_RULESET_KEY, external_tool_paths.pmd_ruleset.clone());
    storage.set_string(
        PMD_INSTALLED_VERSION_KEY,
        external_tool_paths.pmd_installed_version.clone(),
    );
    storage.set_string(SPOTBUGS_BINARY_KEY, external_tool_paths.spotbugs_binary.clone());
    storage.set_string(
        SPOTBUGS_INSTALLED_VERSION_KEY,
        external_tool_paths.spotbugs_installed_version.clone(),
    );
    storage.set_string(AUTO_SAVE_ENABLED_KEY, auto_save_settings.enabled.to_string());
    storage.set_string(
        AUTO_SAVE_MODE_KEY,
        match auto_save_settings.mode {
            AutoSaveMode::OnFocusLoss => AUTO_SAVE_MODE_ON_FOCUS_LOSS,
            AutoSaveMode::AfterIdle => AUTO_SAVE_MODE_AFTER_IDLE,
        }
        .to_string(),
    );
    storage.set_string(AUTO_SAVE_IDLE_SECONDS_KEY, auto_save_settings.idle_seconds.to_string());
    storage.set_string(LSP_ENABLED_KEY, lsp_settings.enabled.to_string());
    storage.set_string(LSP_JDTLS_BINARY_KEY, lsp_settings.jdtls_binary.clone());
    storage.set_string(
        LSP_JDTLS_INSTALLED_VERSION_KEY,
        lsp_settings.jdtls_installed_version.clone(),
    );
    storage.set_string(LSP_JDTLS_JAVA_HOME_KEY, lsp_settings.jdtls_java_home.clone());
    storage.set_string(
        LSP_KOTLIN_LANGUAGE_SERVER_BINARY_KEY,
        lsp_settings.kotlin_language_server_binary.clone(),
    );
    storage.set_string(
        LSP_KOTLIN_LANGUAGE_SERVER_INSTALLED_VERSION_KEY,
        lsp_settings.kotlin_language_server_installed_version.clone(),
    );
    storage.set_string(JDK_REGISTRY_KEY, jdk_registry.to_json());
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
        let mut terminal_panel_visible = false;
        let mut source_control_visible = false;
        let mut custom_templates = UserTemplates::default();
        let mut external_tool_paths = ExternalToolPaths::default();
        let mut auto_save_settings = AutoSaveSettings::default();
        let mut lsp_settings = LspSettings::default();
        let mut jdk_registry = JdkRegistry::default();

        // Resolved before anything else can render, and deliberately outside
        // the `if let Some(storage)` below — a first run has no storage at
        // all, and that's exactly the case where detection matters most.
        match stored_language(cc.storage) {
            Some(chosen) => fg_i18n::set_lang(chosen),
            // Under `cargo test` the host locale is whatever the developer's
            // shell happens to export, which would make every label
            // assertion in the e2e suite pass or fail depending on who ran
            // it — and the language is process-global, so one test's app
            // launch would flip it under every other test running in
            // parallel. Test builds therefore stay on `Lang`'s default
            // (pt-BR, the primary language) unless something sets one.
            None if !cfg!(test) => fg_i18n::set_lang(fg_i18n::detect_from_env()),
            None => {}
        }

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
                &mut terminal_panel_visible,
                &mut source_control_visible,
                &mut custom_templates,
                &mut external_tool_paths,
                &mut auto_save_settings,
                &mut lsp_settings,
                &mut jdk_registry,
            );
        }
        theme::apply(&cc.egui_ctx, dark_mode);

        let (file_event_tx, file_event_rx) = std::sync::mpsc::channel();
        let file_watcher = notify::recommended_watcher(file_event_tx).ok();

        let mut app = Self {
            state,
            parsers,
            pending_close: None,
            generate_dialog: None,
            generate_method_dialog: None,
            override_method_dialog: None,
            completion: None,
            hover: HoverState::default(),
            pending_editor_input: Vec::new(),
            cached_clipboard_text: None,
            pending_navigation: None,
            side_panel: SidePanelState::default(),
            side_panel_width,
            side_panel_visible,
            terminal_panel_visible,
            source_control_visible,
            git_stage: GitStageState::default(),
            terminal_sessions: Vec::new(),
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
            static_analysis: StaticAnalysisState::default(),
            spring_config: SpringConfigState::default(),
            external_tool_paths,
            auto_save_settings,
            lsp_settings,
            lsp: LspState::default(),
            lsp_servers: LspServersState::default(),
            jdk_registry,
            jdk_registry_ui: JdkRegistryState::default(),
            auto_save_state: AutoSaveState::default(),
            diff: DiffState::default(),
        };

        // A resumed-open terminal panel has no session of its own yet (a
        // real child process can't be persisted/restored) — spawn one right
        // away, the same "opening the terminal starts a shell" convention
        // `Ctrl+\``'s own handler already uses, so the panel resumes to a
        // working terminal instead of the empty "No terminal session" state.
        if app.terminal_panel_visible {
            new_terminal_session(
                &mut app.state,
                &mut app.terminal_sessions,
                &cc.egui_ctx,
                &mut app.last_error,
            );
        }
        // Same "resumed already open" gap the terminal panel above just
        // fixed, for the Source Control panel: `FoxGardenApp::ui`'s own
        // `!source_control_was_visible` transition check only fires when
        // the panel goes from closed to open *during* a run, so a session
        // that starts with it already open (restored from `persist_
        // settings` below) would otherwise sit there showing a stale empty
        // list until the user manually hit Refresh — a real bug reported
        // against the very first version of this panel.
        if app.source_control_visible
            && let Some(root) = app.state.project.as_ref().map(|p| p.root.clone())
        {
            app.git_stage.refresh(root.clone());
            app.git_stage.load_committer_first_name(&root);
        }
        app
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
        ui.button(t().common.ok).clicked()
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
        // Gates every Ctrl+letter shortcut below (but not `Ctrl+\``/`F11`,
        // neither of which collides with a real terminal control byte):
        // `PLAN.md`'s terminal-panel Phase 8 wires these same letters up as
        // real control bytes (`Ctrl+P` recalls shell history, `Ctrl+E`
        // moves to end-of-line, ...), so without this guard, typing one of
        // them into a focused terminal session would *also* fire the app's
        // own global popup/panel toggle.
        let terminal_focused = terminal_panel::is_terminal_focused(ui.ctx());
        if !terminal_focused && ui.input(|i| i.key_pressed(egui::Key::E) && i.modifiers.command && !i.modifiers.shift) {
            self.quick_switcher.toggle();
        }
        if !terminal_focused && ui.input(|i| i.key_pressed(egui::Key::P) && i.modifiers.command) {
            self.go_to_file.toggle();
        }
        if !terminal_focused && ui.input(|i| i.key_pressed(egui::Key::E) && i.modifiers.command && i.modifiers.shift) {
            self.spring_endpoints
                .toggle(self.state.project.as_ref().map(|p| &p.tree));
        }
        if !terminal_focused
            && ui.input(|i| i.key_pressed(egui::Key::N) && i.modifiers.command)
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            self.side_panel.begin_new_file(root);
        }
        if !terminal_focused && ui.input(|i| i.key_pressed(egui::Key::B) && i.modifiers.command) {
            self.side_panel_visible = !self.side_panel_visible;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Backtick) && i.modifiers.command) {
            self.terminal_panel_visible = !self.terminal_panel_visible;
            // Matches VSCode's own "opening the terminal for the first
            // time starts a shell" behavior, rather than toggling open to
            // an empty panel with nothing in it and a second click needed
            // just to get a session going.
            if self.terminal_panel_visible && self.state.terminal_tabs.is_empty() {
                new_terminal_session(
                    &mut self.state,
                    &mut self.terminal_sessions,
                    ui.ctx(),
                    &mut self.last_error,
                );
            }
        }

        // `i.time`/`i.focused`/`i.events` are read up front (frame-stable),
        // but the actual auto-save trigger check happens *after*
        // `process_file_events` below, so it sees this frame's freshest
        // `external_conflicts` — a conflict banner that just appeared this
        // very frame must already suppress it, not wait a frame.
        let (now, focused, had_activity) = ui.input(|i| (i.time, i.focused, !i.events.is_empty()));
        if had_activity {
            self.auto_save_state.record_activity(now);
        }
        let auto_save_fired = self.auto_save_state.tick(self.auto_save_settings, focused, now);

        // Computed once and reused at every `git diff` trigger point below
        // (open/save/reload) — `None` when no project is open, in which
        // case each of those points is simply a no-op (nothing to diff
        // against).
        let diff_root = self.state.project.as_ref().map(|p| p.root.clone());

        sync_watched_dirs(&mut self.file_watcher, &mut self.watched_dirs, &self.state);
        process_file_events(
            &self.file_event_rx,
            &mut self.state,
            &mut self.parsers,
            &mut self.external_conflicts,
            &mut self.externally_deleted,
            &mut self.diff,
            diff_root.as_deref(),
        );

        // The process owner only polls channels/child state here; it never
        // waits. This keeps an unavailable or slow external language server
        // completely off the editor's keystroke-to-pixels path.
        let lsp_errors = self.lsp.sync(
            &self.lsp_settings,
            self.state.project.as_ref().map(|project| project.root.as_path()),
            &mut self.state.open_tabs,
        );
        if self.last_error.is_none() {
            self.last_error = lsp_errors.into_iter().next();
        }
        // A handshake response or an unprompted `publishDiagnostics` can
        // land on the background reader thread at any time, not just in
        // reply to a keystroke — without this, a session sitting between
        // user input events would have its own replies sit unread in the
        // channel until some unrelated repaint happened to come along.
        if self.lsp.wants_repaint() || self.hover.wants_repaint() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }

        if auto_save_fired {
            tabs::save_all_dirty_tabs(
                &mut self.state,
                &mut self.parsers,
                &mut self.last_error,
                &self.external_conflicts,
            );
        }

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
        let mut terminal_outcome = terminal_panel::TerminalPanelOutcome::default();
        // Compared against `self.source_control_visible` after `menu_bar::
        // show` runs (which mutates it directly, same "one flag, two
        // triggers" shape every other View checkbox here already uses) to
        // detect a false -> true transition — the panel has no saved
        // status to resume (unlike the terminal panel's session), so
        // opening it needs an explicit first `git status` to have anything
        // to show at all.
        let source_control_was_visible = self.source_control_visible;

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
                        &mut self.auto_save_settings,
                        &mut self.zen_mode,
                        &mut self.side_panel_visible,
                        &mut self.terminal_panel_visible,
                        &mut self.source_control_visible,
                        &mut self.last_error,
                        &mut self.custom_templates,
                        self.static_analysis.checkstyle_running(),
                        self.static_analysis.pmd_running(),
                    )
                })
                .inner;

            // Added before every other bottom/side panel so it spans the
            // full window width along the very bottom edge, underneath the
            // project tree and the terminal both — egui gives each panel
            // the outermost strip of whatever space is left when it's
            // added, so anything registered earlier would push the bar
            // inward. Hidden in zen mode along with the rest of the chrome.
            //
            // What it reports is gathered here rather than after this
            // frame's own polls further down: a job that finished this
            // frame therefore stays on the bar for one more frame (~16ms),
            // exactly as the Tools menu's own "Running Checkstyle…" label
            // already does, and one frame of staleness is invisible next to
            // jobs that run for seconds or minutes.
            let work = status_bar::BackgroundWork::gather(
                &self.lsp,
                &self.lsp_servers,
                &self.static_analysis,
                &self.spring_config,
                &self.git_stage,
                &self.diff,
            );
            let activities = status_bar::activities(&work);
            egui::Panel::bottom("status_bar").show(ui, |ui| status_bar::show(ui, &activities));

            if self.side_panel_visible {
                let panel_response = egui::Panel::left("project_panel")
                    .default_size(self.side_panel_width)
                    .show(ui, |ui| side_panel::show(ui, &mut self.state, &mut self.side_panel));
                // Tracks a live drag, not just the size at the frame the
                // resize handle is released — so `self.side_panel_width`
                // (what `save()` persists) always reflects exactly what's on
                // screen.
                self.side_panel_width = panel_response.response.rect.width();
                outcome = panel_response.inner;
            }

            if self.terminal_panel_visible {
                egui::Panel::bottom("terminal_panel")
                    .resizable(true)
                    .default_size(220.0)
                    .show(ui, |ui| {
                        terminal_outcome = terminal_panel::show(
                            ui,
                            &mut self.state,
                            &mut self.terminal_sessions,
                            self.editor_font,
                            self.font_size,
                            self.dark_mode,
                            self.view_settings.cursor_blink,
                        );
                    });
            }

            if self.source_control_visible
                && let Some(root) = diff_root.clone()
            {
                if !source_control_was_visible {
                    self.git_stage.refresh(root.clone());
                    self.git_stage.load_committer_first_name(&root);
                }
                egui::Panel::right("source_control_panel")
                    .resizable(true)
                    .default_size(280.0)
                    .show(ui, |ui| {
                        git_stage::show(
                            ui,
                            &mut self.git_stage,
                            &root,
                            self.editor_font,
                            self.font_size,
                            self.dark_mode,
                        );
                    });
            }
        }

        if terminal_outcome.new_session_requested {
            new_terminal_session(
                &mut self.state,
                &mut self.terminal_sessions,
                ui.ctx(),
                &mut self.last_error,
            );
        }
        if let Some(index) = terminal_outcome.close_request {
            close_terminal_session(&mut self.state, &mut self.terminal_sessions, index);
        }

        if let Some(path) = outcome.open {
            open_path(
                &mut self.state,
                &mut self.parsers,
                &mut self.last_error,
                &mut self.diff,
                diff_root.clone(),
                path,
            );
        }
        for (old, new) in &outcome.renamed {
            handle_rename(&mut self.state, &mut self.parsers, old, new);
        }
        for path in &outcome.deleted {
            close_tabs_under(&mut self.state, &mut self.parsers, path);
        }
        if let Some(err) = outcome.error {
            self.last_error = Some(err);
        }

        if menu_outcome.open_run_configs_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            self.run_configs_dialog.open(&root);
        }

        if menu_outcome.open_external_tools_settings_request {
            self.static_analysis.open_settings();
        }
        if menu_outcome.open_lsp_servers_settings_request {
            self.lsp_servers.open_settings(
                &self.lsp_settings,
                self.state.project.as_ref().map(|p| p.root.as_path()),
            );
        }
        if menu_outcome.open_jdk_registry_settings_request {
            self.jdk_registry_ui.open_settings();
        }
        if menu_outcome.run_checkstyle_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            let binary = self.external_tool_paths.checkstyle_binary.trim();
            let config = self.external_tool_paths.checkstyle_config.trim();
            if binary.is_empty() || config.is_empty() {
                self.last_error = Some(t().errors.checkstyle_not_configured.to_string());
            } else {
                self.static_analysis
                    .run_checkstyle(PathBuf::from(binary), PathBuf::from(config), root);
            }
        }
        if menu_outcome.run_pmd_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            let binary = self.external_tool_paths.pmd_binary.trim();
            let ruleset = self.external_tool_paths.pmd_ruleset.trim();
            if binary.is_empty() || ruleset.is_empty() {
                self.last_error = Some(t().errors.pmd_not_configured.to_string());
            } else {
                self.static_analysis
                    .run_pmd(PathBuf::from(binary), ruleset.to_string(), root);
            }
        }
        if let Some(result) = self.static_analysis.poll_checkstyle() {
            match result {
                Ok(diagnostics) => static_analysis::apply_checkstyle_results(&mut self.state, &diagnostics),
                Err(err) => self.last_error = Some(msg::checkstyle_failed(&err.to_string())),
            }
        }
        if let Some(result) = self.static_analysis.poll_pmd() {
            match result {
                Ok(diagnostics) => static_analysis::apply_pmd_results(&mut self.state, &diagnostics),
                Err(err) => self.last_error = Some(msg::pmd_failed(&err.to_string())),
            }
        }
        self.spring_config.poll();
        for result in self.static_analysis.tool_manager.poll_installs() {
            match result {
                Ok(installed) => self.external_tool_paths.apply_installed(&installed),
                Err(err) => self.last_error = Some(msg::install_failed(&err.to_string())),
            }
        }
        for (tool, result) in self.static_analysis.tool_manager.poll_checks() {
            self.static_analysis.record_latest_version(tool, result);
        }
        // A language-server install runs for minutes (jdt.ls is built from
        // source — see `lsp_manager`'s own header), reporting progress from
        // a background thread with no input event to ride in on, so its
        // dialog needs repaints requested for it the same way `lsp_state`'s
        // own background replies do.
        for result in self.lsp_servers.manager.poll_installs() {
            match result {
                Ok(installed) => self.lsp_settings.apply_installed(&installed),
                Err(err) => self.last_error = Some(msg::language_server_install_failed(&err.to_string())),
            }
        }
        for (server, result) in self.lsp_servers.manager.poll_checks() {
            self.lsp_servers.record_latest_version(server, result);
        }
        // A finished JDK scan always wins over what's in the field: it only
        // ever runs because the field was empty when the dialog opened, or
        // because Detect was clicked — which is a direct request to replace
        // whatever is there. A machine with no JDK 21+ leaves it empty, so
        // jdt.ls still falls back to JAVA_HOME/PATH.
        if let Some(found) = self.lsp_servers.manager.poll_java_home_detection() {
            if let Some(home) = &found {
                self.lsp_settings.jdtls_java_home = home.display().to_string();
            }
            // A machine with no JDK 21+ is reported inside the dialog next
            // to the field itself, not as an app-wide error: the scan runs
            // unprompted whenever the dialog opens empty, and a modal error
            // over the dialog the user just opened would be noise.
            self.lsp_servers.record_java_home_detection(found.is_some());
        }
        if self.lsp_servers.manager.busy() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        self.diff.poll(&mut self.state);

        if let Some(result) = self.git_stage.poll_status()
            && let Err(err) = result
        {
            self.last_error = Some(msg::git_status_failed(&err.to_string()));
        }
        if let Some(result) = self.git_stage.poll_op() {
            match result {
                // A stage/unstage/commit/push/hunk-(un)stage only ever
                // changes what `git status` would report, never `diff_
                // hunks`/`blame` directly, so a fresh `git_stage.refresh`
                // (not `self.diff.run`) is the only follow-up needed here —
                // plus `refresh_expanded` for whichever row (if any) is
                // currently showing its own hunk breakdown, since a hunk
                // stage/unstage changes exactly that view's own data and
                // shifts every later hunk's index.
                Ok(()) => {
                    if let Some(root) = diff_root.clone() {
                        self.git_stage.refresh(root.clone());
                        self.git_stage.refresh_expanded(root);
                    }
                }
                Err(err) => self.last_error = Some(msg::git_operation_failed(&err.to_string())),
            }
        }
        if let Some(result) = self.git_stage.poll_expanded()
            && let Err(err) = result
        {
            self.last_error = Some(msg::git_diff_failed(&err.to_string()));
        }
        if let Some(result) = self.git_stage.poll_full_diff()
            && let Err(err) = result
        {
            self.last_error = Some(msg::git_diff_failed(&err.to_string()));
        }

        egui::CentralPanel::default().show(ui, |ui| {
            show_external_change_banner(
                ui,
                &mut self.state,
                &mut self.parsers,
                &mut self.external_conflicts,
                &mut self.externally_deleted,
                &mut self.diff,
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
                &mut self.hover,
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
                &mut self.spring_config,
                &mut self.lsp,
            );
        });

        // The frame's own `Ctrl+S`/File > Save/close-confirmation-modal
        // Save/right-click Save/auto-save outcomes have all already landed
        // by now (every one of them happens inside `tabs::show` above, or —
        // for auto-save — earlier this same frame) — this is what lets
        // `check_for_saves`'s dirty-transition check see the *post*-save
        // state and fire for whichever of those actually happened, without
        // any of those save call sites needing to know this feature exists.
        self.diff.check_for_saves(&self.state, diff_root.as_deref());

        if let Some(path) = quick_switcher::show(ui, &self.state, &mut self.quick_switcher) {
            open_path(
                &mut self.state,
                &mut self.parsers,
                &mut self.last_error,
                &mut self.diff,
                diff_root.clone(),
                path,
            );
        }
        if let Some(path) = go_to_file::show(ui, &self.state, &mut self.go_to_file) {
            open_path(
                &mut self.state,
                &mut self.parsers,
                &mut self.last_error,
                &mut self.diff,
                diff_root.clone(),
                path,
            );
        }
        if let Some((path, handler_byte)) = spring_endpoints::show(ui, &self.state, &mut self.spring_endpoints) {
            open_path(
                &mut self.state,
                &mut self.parsers,
                &mut self.last_error,
                &mut self.diff,
                diff_root.clone(),
                path.clone(),
            );
            self.pending_navigation = Some((path, handler_byte));
        }
        if let Some(root) = self.state.project.as_ref().map(|p| p.root.clone()) {
            run_configs::show(ui, &root, &mut self.run_configs_dialog, &mut self.last_error);
        }
        static_analysis::show_settings(ui, &mut self.static_analysis, &mut self.external_tool_paths);
        lsp_servers::show_settings(ui, &mut self.lsp_servers, &mut self.lsp_settings);
        jdk_registry_ui::show_settings(ui, &mut self.jdk_registry_ui, &mut self.jdk_registry);

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
            self.terminal_panel_visible,
            self.source_control_visible,
            &self.custom_templates,
            &self.external_tool_paths,
            self.auto_save_settings,
            &self.lsp_settings,
            &self.jdk_registry,
        );
    }
}

#[cfg(test)]
mod e2e_test;
#[cfg(test)]
mod app_test;
