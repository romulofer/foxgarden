use std::collections::HashSet;
use std::path::{Path, PathBuf};

use fg_core::{EditorState, Language};
use fg_i18n::{msg, t};
use notify::Watcher;
use ropey::Rope;
use syntax::IncrementalParser;

use crate::auto_save::{AutoSaveMode, AutoSaveSettings, AutoSaveState};
use crate::debug_state;
use crate::file_watch::{self, ReconcileOutcome};
use crate::goto_definition::{GotoDefinitionState, Target as GotoDefinitionTarget};
use crate::rename::RenameState;
use crate::jdk_registry::JdkRegistry;
use crate::lsp_settings::LspSettings;
use crate::lsp_state::LspState;
use crate::panels::build_panel;
use crate::panels::debug_panel;
use crate::panels::debug_toolbar;
use crate::panels::git_diff::DiffState;
use crate::panels::git_stage::{self, GitStageState};
use crate::panels::go_to_file::{self, GoToFileState};
use crate::panels::jdk_registry::{self as jdk_registry_ui, JdkRegistryState};
use crate::panels::lsp_servers::{self, LspServersState};
use crate::panels::menu_bar::{self, MenuBarState};
use crate::panels::new_project::{self, NewProjectWizardState};
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
use crate::widgets::modal::show_modal;
use crate::widgets::editor::{
    CodeActionGutter, CompletionState, FindReferencesState, GenerateAccessorsDialog, GenerateMethodDialog, HoverState,
    OverrideMethodDialog, PeekState, RenameBox, UserTemplates, jump_to,
};

const LAST_PROJECT_KEY: &str = "last_project";
/// Newline-joined roots of previously opened projects, most recent first —
/// what the welcome screen offers. Same newline encoding (and same reason)
/// as `OPEN_TABS_KEY`.
const RECENT_PROJECTS_KEY: &str = "recent_projects";
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
const BUILD_PANEL_VISIBLE_KEY: &str = "build_panel_visible";
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
/// Settings > Trim Trailing Whitespace on Save. Absent (a fresh install)
/// means on, matching what saving always did before this became a choice.
const TRIM_TRAILING_WHITESPACE_KEY: &str = "trim_trailing_whitespace_on_save";
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

/// How long the project tree waits for filesystem events to stop arriving
/// before rebuilding itself. One external action (`git checkout`, an
/// unzip, a build that writes into a watched source folder) produces many
/// individual events; rebuilding per event would walk the whole project
/// repeatedly for what the user experiences as a single change.
const TREE_REFRESH_DEBOUNCE_SECONDS: f64 = 0.3;

/// How often unsaved buffers are copied into `.foxgarden/drafts/`. Short
/// enough that a crash costs seconds of typing rather than an afternoon,
/// long enough that a burst of keystrokes doesn't turn into a burst of
/// writes — the drafts are a safety net, not a second save path.
const DRAFT_INTERVAL_SECONDS: f64 = 5.0;

pub struct FoxGardenApp {
    state: EditorState,
    /// Kept index-aligned with `state.open_tabs`: one incremental parser per
    /// open document.
    parsers: Vec<Option<IncrementalParser>>,
    /// Unsaved tabs waiting on a save/discard answer before they close, in
    /// the order they'll be asked about — see `tabs::request_close_tab`.
    pending_close: Vec<PathBuf>,
    /// Non-blocking failure notices, drained from `last_error` every frame.
    toasts: crate::toasts::Toasts,
    /// `Ctrl+Shift+P` — every common action by name, with its shortcut.
    command_palette: crate::panels::command_palette::CommandPaletteState,
    /// When unsaved buffers were last copied to `.foxgarden/drafts/` — see
    /// `DRAFT_INTERVAL_SECONDS`.
    drafts_written_at: f64,
    /// Drafts found at startup, waiting for the user to say whether to
    /// restore them (`show_draft_restore_prompt`).
    pending_drafts: Vec<fg_core::Draft>,
    /// Project roots opened before, most recent first — offered on the
    /// welcome screen and persisted across restarts.
    recent_projects: Vec<PathBuf>,
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
    /// The go-to-definition request/reply for whichever tab is currently
    /// focused — `goto_definition::GotoDefinitionState` (`PLAN.md` Track 20
    /// Phase 4). Same "one field, not one per open tab" shape `hover` above
    /// already uses, for the same reason; unlike `hover` it owns no popup
    /// state, just the async request this frame's `resolve_pending_
    /// navigation` block polls.
    goto_definition: GotoDefinitionState,
    /// The peek-definition popup for whichever tab is currently focused —
    /// `widgets::editor::peek::PeekState` (`PLAN.md` Track 17 Phase 1). Same
    /// "one field, not one per open tab" shape `hover`/`goto_definition`
    /// above already use, for the same reason.
    peek: PeekState,
    /// The find-references results list for whichever tab is currently
    /// focused — `widgets::editor::references::FindReferencesState`
    /// (`PLAN.md` Track 20 Phase 6). Same "one field, not one per open
    /// tab" shape `hover`/`goto_definition`/`peek` above already use, for
    /// the same reason; a row click there is handed back through `take_
    /// navigation`, polled the same frame as `goto_definition`'s own
    /// `poll` right below.
    find_references: FindReferencesState,
    /// The "new name" input box for whichever tab is currently focused —
    /// `widgets::editor::rename::RenameBox` (`PLAN.md` Track 20 Phase 7).
    /// Same "one field, not one per open tab" shape `hover`/`goto_
    /// definition`/`peek`/`find_references` above already use, for the
    /// same reason.
    rename_box: RenameBox,
    /// The async `textDocument/rename` request/reply and `WorkspaceEdit`
    /// application for whichever tab is currently focused —
    /// `rename::RenameState` (`PLAN.md` Track 20 Phase 7). Unlike `rename_
    /// box` this owns no popup of its own, just the request `resolve_
    /// pending_rename` (this file's own update loop) polls.
    rename: RenameState,
    /// The gutter lightbulb + quick-fix picker for whichever tab is
    /// currently focused — `widgets::editor::code_action::
    /// CodeActionGutter` (`PLAN.md` Track 15 Phase 1). Owns its own
    /// `textDocument/codeAction` request/reply, unlike `rename_box`/
    /// `rename` above: picking an offer needs no further network
    /// round-trip (the edit is already resolved), so there's no separate
    /// app.rs-level polling state — `take_confirmed` (below) hands the
    /// picked `WorkspaceEdit` straight to `workspace_edit::apply`.
    code_action_gutter: CodeActionGutter,
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
    /// session, ...), moved into `toasts` at the end of each frame (see
    /// `drain_errors_into_toasts`).
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
    /// When the project tree should be rebuilt after an external
    /// filesystem change, as an `egui` input-time timestamp — `None` when
    /// no rebuild is pending. See `TREE_REFRESH_DEBOUNCE_SECONDS`.
    tree_refresh_due: Option<f64>,
    /// A background project-tree walk in flight, if any — see
    /// `tree_refresh_due`.
    tree_refresh_rx: Option<std::sync::mpsc::Receiver<std::io::Result<fg_core::Project>>>,
    /// Whether `Document::save` strips trailing whitespace from every line
    /// (Settings > Trim Trailing Whitespace on Save). On by default; off
    /// exists so opening and saving a file in a codebase that never had
    /// this doesn't rewrite lines the user never touched.
    trim_trailing_whitespace_on_save: bool,
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
    /// File > New Project… — `PLAN.md` Track 29 Phase 3. Runtime-only, same
    /// reasoning as `lsp_servers`/`jdk_registry_ui` above: nothing here is
    /// meaningful to resume across a restart.
    new_project_wizard: NewProjectWizardState,
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
    /// The tab context menu's "File History…" window (`PLAN.md` Track 4
    /// Phase 2) — see `panels::file_history::FileHistoryState`. Runtime-
    /// only, same as `git_stage`: nothing here is worth resuming across a
    /// relaunch, a snapshot list is cheap to re-scan the next time it's
    /// opened.
    file_history: crate::panels::file_history::FileHistoryState,
    /// Whether the Build Output panel is docked open at the bottom —
    /// toggled by the View menu's "Build Output" checkbox, or automatically
    /// whenever Run > Build starts a new build (`PLAN.md` Track 22 Phase
    /// 1), same "one flag, several triggers" shape `terminal_panel_visible`
    /// already established. Persisted across restarts the same way — like
    /// the Source Control panel, there's no running process worth resuming
    /// on relaunch, just the panel being open at all.
    build_panel_visible: bool,
    /// The Build Output panel's own accumulated log lines plus any
    /// in-flight `mvn`/`gradle` build — see `panels::build_panel::
    /// BuildState`. Runtime-only: a fresh launch has no build to resume.
    build_state: build_panel::BuildState,
    /// One in-flight or attached Java debug session (`PLAN.md` Track 23
    /// Phase 1) — see `debug_state::DebugState`. Runtime-only, same as
    /// `build_state`: no real process is ever worth trying to resume across
    /// a relaunch.
    debug_state: debug_state::DebugState,
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
/// Opens `path` in a tab and hands back its index — the piece of
/// `open_path` a draft restore needs (it has to write into the buffer
/// afterwards) without the diff/error plumbing that isn't relevant when the
/// file is about to be overwritten from a draft anyway.
fn open_path_for_draft(
    state: &mut EditorState,
    parsers: &mut Vec<Option<IncrementalParser>>,
    path: PathBuf,
) -> Option<usize> {
    let index = state.open_tab(path).ok()?;
    if index == parsers.len() {
        let parser = tabs::open_parser_for(&mut state.open_tabs[index]);
        parsers.push(parser);
    }
    Some(index)
}

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
            crate::errors::report(last_error, message);
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
        Err(err) => crate::errors::report(last_error, msg::failed_to_start_terminal(&err.to_string())),
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
    // `byte` was resolved against the target buffer's text at the moment
    // the navigation was queued; an edit (this document's own, or an
    // external reload) landing before this frame gets to resolve it can
    // shrink the buffer out from under that offset. `Rope::byte_to_char`
    // panics on an out-of-bounds offset, so clamp to the buffer's current
    // length first — same defensive clamp `build_click`'s own line/column
    // resolution already applies for the identical "stale target" shape.
    let byte = byte.min(doc.buffer.len_bytes());
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
    doc.buffer.replace(Rope::from_str(new_content));
    doc.saved_buffer = doc.buffer.rope().clone();
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
    let mut needed = file_watch::watched_dirs_for(state.open_tabs.iter().map(|doc| doc.path()));
    // Every directory the project tree shows, so a file created or deleted
    // outside FoxGarden (a `git checkout`, a `mvn archetype:generate`, an
    // editor in another window) updates the side panel instead of leaving
    // it quietly wrong until the user happens to do something that
    // refreshes it. `Project::directories` already excludes `target/`,
    // `.git/` and friends, so this doesn't register watches for build
    // churn.
    if let Some(project) = &state.project {
        needed.extend(project.directories());
    }
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
) -> bool {
    let mut tree_changed = false;
    while let Ok(event_result) = rx.try_recv() {
        let Ok(event) = event_result else { continue };
        if !matches!(
            event.kind,
            notify::EventKind::Modify(_) | notify::EventKind::Create(_) | notify::EventKind::Remove(_)
        ) {
            continue;
        }
        // A create or delete changes the *shape* of the tree, so the side
        // panel needs rebuilding — including for paths with no open tab,
        // which is the common case (a file appearing in a folder nobody
        // has opened yet). A plain content modification never does.
        if matches!(event.kind, notify::EventKind::Create(_) | notify::EventKind::Remove(_)) {
            tree_changed = true;
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
    tree_changed
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
            crate::errors::report(last_error, msg::failed_to_reopen_last_project(&err.to_string()));
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
                Err(err) => crate::errors::report(last_error, msg::failed_to_reopen_tab(&err.to_string())),
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
fn persist_session(storage: &mut dyn eframe::Storage, state: &EditorState, recent_projects: &[PathBuf]) {
    storage.set_string(
        RECENT_PROJECTS_KEY,
        recent_projects.iter().map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n"),
    );
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
    build_panel_visible: &mut bool,
    custom_templates: &mut UserTemplates,
    external_tool_paths: &mut ExternalToolPaths,
    auto_save_settings: &mut AutoSaveSettings,
    trim_trailing_whitespace_on_save: &mut bool,
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
    if let Some(visible) = storage.get_string(BUILD_PANEL_VISIBLE_KEY) {
        *build_panel_visible = visible == "true";
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
    if let Some(trim) = storage.get_string(TRIM_TRAILING_WHITESPACE_KEY) {
        *trim_trailing_whitespace_on_save = trim == "true";
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
    build_panel_visible: bool,
    custom_templates: &UserTemplates,
    external_tool_paths: &ExternalToolPaths,
    auto_save_settings: AutoSaveSettings,
    trim_trailing_whitespace_on_save: bool,
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
    storage.set_string(BUILD_PANEL_VISIBLE_KEY, build_panel_visible.to_string());
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
    storage.set_string(TRIM_TRAILING_WHITESPACE_KEY, trim_trailing_whitespace_on_save.to_string());
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
        let mut build_panel_visible = false;
        let mut custom_templates = UserTemplates::default();
        let mut external_tool_paths = ExternalToolPaths::default();
        let mut auto_save_settings = AutoSaveSettings::default();
        let mut trim_trailing_whitespace_on_save = true;
        let mut recent_projects: Vec<PathBuf> = Vec::new();
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
            if let Some(stored) = storage.get_string(RECENT_PROJECTS_KEY) {
                recent_projects = stored.lines().map(PathBuf::from).filter(|path| path.is_dir()).collect();
            }
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
                &mut build_panel_visible,
                &mut custom_templates,
                &mut external_tool_paths,
                &mut auto_save_settings,
                &mut trim_trailing_whitespace_on_save,
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
            pending_close: Vec::new(),
            toasts: crate::toasts::Toasts::default(),
            command_palette: crate::panels::command_palette::CommandPaletteState::default(),
            drafts_written_at: 0.0,
            pending_drafts: Vec::new(),
            recent_projects,
            generate_dialog: None,
            generate_method_dialog: None,
            override_method_dialog: None,
            completion: None,
            hover: HoverState::default(),
            goto_definition: GotoDefinitionState::default(),
            peek: PeekState::default(),
            find_references: FindReferencesState::default(),
            rename_box: RenameBox::default(),
            rename: RenameState::default(),
            code_action_gutter: CodeActionGutter::default(),
            pending_editor_input: Vec::new(),
            cached_clipboard_text: None,
            pending_navigation: None,
            side_panel: SidePanelState::default(),
            side_panel_width,
            side_panel_visible,
            terminal_panel_visible,
            source_control_visible,
            git_stage: GitStageState::default(),
            file_history: crate::panels::file_history::FileHistoryState::default(),
            build_panel_visible,
            build_state: build_panel::BuildState::default(),
            debug_state: debug_state::DebugState::default(),
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
            tree_refresh_due: None,
            tree_refresh_rx: None,
            trim_trailing_whitespace_on_save,
            lsp_settings,
            lsp: LspState::default(),
            lsp_servers: LspServersState::default(),
            jdk_registry,
            jdk_registry_ui: JdkRegistryState::default(),
            new_project_wizard: NewProjectWizardState::default(),
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

impl FoxGardenApp {
    /// What the status bar reports about the active tab: where the caret
    /// is, what the file is, how it indents, and how many problems it has.
    ///
    /// The caret lives in the editor widget's own persisted state (it is
    /// the widget, not the document, that owns a cursor), so this reads it
    /// back through `text_area::peek_caret` rather than duplicating it on
    /// `Document`. `None` when no tab is open — the bar then shows only
    /// background activity, as it always did.
    /// Performs a command picked from the palette.
    ///
    /// Anything the menus already raise as a request is routed through
    /// `menu_outcome` rather than reimplemented here, so a palette command
    /// and its menu item can't drift apart — they are literally the same
    /// code path from this point on. Only the actions that are plain state
    /// flips (panel toggles, theme) or direct calls are handled inline.
    /// Copies every unsaved buffer into `.foxgarden/drafts/`, and clears
    /// the draft of anything that now matches disk.
    ///
    /// Best-effort throughout: a read-only `.foxgarden/`, a full disk, a
    /// file outside the project — none of that is worth interrupting the
    /// user over, since the buffer they're typing into is unaffected. The
    /// worst case is the safety net not being there, which is where this
    /// app already was.
    fn write_drafts(&self) {
        for doc in &self.state.open_tabs {
            let Some(root) = &doc.project_root else {
                continue;
            };
            if doc.is_dirty() {
                let _ = fg_core::write_draft(root, doc.path(), &doc.buffer.to_string());
            } else {
                let _ = fg_core::discard_draft(root, doc.path());
            }
        }
    }

    /// Offers to restore whatever unsaved work the last session left
    /// behind. One prompt for all of them, not one per file: they were all
    /// lost by the same event, and answering the same question six times
    /// is its own annoyance.
    ///
    /// Restoring opens each file and puts the draft's text into its buffer
    /// *without saving* — the result is exactly the dirty tab the user had,
    /// not a decision made on their behalf about what belongs on disk.
    fn show_draft_restore_prompt(&mut self, ui: &egui::Ui) {
        if self.pending_drafts.is_empty() {
            return;
        }
        let names: Vec<String> = self
            .pending_drafts
            .iter()
            .filter_map(|draft| draft.file_path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();

        let outcome = show_modal(ui, "restore_drafts", Some(names.len()), |ui, _| {
            ui.label(msg::unsaved_work_found(names.len()));
            ui.weak(names.join("\n"));
            ui.horizontal(|ui| (ui.button(t().common.restore).clicked(), ui.button(t().tabs.discard).clicked()))
                .inner
        });

        let Some(((restore, discard), escaped)) = outcome else {
            return;
        };
        if !restore && !discard && !escaped {
            return;
        }
        let drafts = std::mem::take(&mut self.pending_drafts);
        for draft in drafts {
            let root = self.state.project.as_ref().map(|project| project.root.clone());
            if restore {
                match open_path_for_draft(&mut self.state, &mut self.parsers, draft.file_path.clone()) {
                    Some(index) => {
                        let doc = &mut self.state.open_tabs[index];
                        doc.buffer.replace(Rope::from_str(&draft.content));
                        doc.lsp_version += 1;
                        doc.lsp_sync_pending = true;
                        self.parsers[index] = tabs::open_parser_for(doc);
                    }
                    None => continue,
                }
            } else if let Some(root) = root {
                // Declined (or dismissed): the draft has served its
                // purpose and must not be offered again next launch.
                let _ = fg_core::discard_draft(&root, &draft.file_path);
            }
        }
    }

    fn run_command(
        &mut self,
        ctx: &egui::Context,
        command: crate::panels::command_palette::Command,
        menu_outcome: &mut menu_bar::MenuBarOutcome,
    ) {
        use crate::panels::command_palette::Command;

        match command {
            Command::Save => tabs::save_active_tab(
                &mut self.state,
                &mut self.parsers,
                &mut self.last_error,
                self.trim_trailing_whitespace_on_save,
            ),
            Command::SaveAll => tabs::save_all_dirty_tabs(
                &mut self.state,
                &mut self.parsers,
                &mut self.last_error,
                &HashSet::new(),
                self.trim_trailing_whitespace_on_save,
            ),
            Command::CloseTab => {
                if let Some(active) = self.state.active_tab {
                    tabs::request_close_tab(&mut self.state, &mut self.parsers, &mut self.pending_close, active);
                }
            }
            Command::ReopenClosedTab => tabs::reopen_last_closed_tab(&mut self.state, &mut self.parsers),
            Command::OpenFolder => self
                .side_panel
                .open_folder_picker(self.state.project.as_ref().map(|project| project.root.clone())),
            Command::NewFile => {
                if let Some(root) = self.state.project.as_ref().map(|project| project.root.clone()) {
                    self.side_panel.begin_new_file(root);
                }
            }
            Command::NewProject => menu_outcome.open_new_project_wizard_request = true,
            Command::GoToFile => self.go_to_file.toggle(),
            Command::RecentFiles => self.quick_switcher.toggle(),
            // The editor owns diagnostic navigation (it needs the caret),
            // so this arrives as the same key event pressing F8 does.
            Command::NextDiagnostic => self
                .pending_editor_input
                .push(egui::Event::Key {
                    key: egui::Key::F8,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }),
            Command::ToggleSidePanel => self.side_panel_visible = !self.side_panel_visible,
            Command::ToggleTerminal => self.terminal_panel_visible = !self.terminal_panel_visible,
            Command::ToggleSourceControl => self.source_control_visible = !self.source_control_visible,
            Command::ToggleBuildPanel => self.build_panel_visible = !self.build_panel_visible,
            Command::ToggleTheme => {
                self.dark_mode = !self.dark_mode;
                theme::apply(ctx, self.dark_mode);
            }
            Command::ZenMode => self.zen_mode = !self.zen_mode,
            Command::Build => menu_outcome.build_request = true,
            Command::RunProject => menu_outcome.run_project_request = true,
            Command::RunTests => menu_outcome.run_tests_request = true,
            Command::RunCheckstyle => menu_outcome.run_checkstyle_request = true,
            Command::RunPmd => menu_outcome.run_pmd_request = true,
            Command::RunSpotBugs => menu_outcome.run_spotbugs_request = true,
            Command::FoldAll => menu_outcome.fold_all_request = true,
            Command::ExpandAll => menu_outcome.expand_all_request = true,
            Command::SortLines => menu_outcome.sort_lines_request = true,
            Command::UniqueLines => menu_outcome.unique_lines_request = true,
        }
    }

    fn document_status(&self, ctx: &egui::Context) -> Option<status_bar::DocumentStatus> {
        let doc = self.state.open_tabs.get(self.state.active_tab?)?;
        let caret = crate::widgets::editor::peek_caret(ctx, egui::Id::new(doc.path.to_string_lossy().into_owned()));
        let offset = caret.map_or(0, |caret| caret.primary).min(doc.buffer.len_chars());
        let line = doc.buffer.char_to_line(offset);
        let column = offset - doc.buffer.line_to_char(line);

        let diagnostics = doc
            .diagnostics
            .iter()
            .chain(doc.checkstyle_diagnostics.iter())
            .chain(doc.pmd_diagnostics.iter())
            .chain(doc.spotbugs_diagnostics.iter())
            .chain(doc.lsp_diagnostics.iter());
        let (mut errors, mut warnings) = (0, 0);
        for diagnostic in diagnostics {
            match diagnostic.severity {
                fg_core::Severity::Error => errors += 1,
                fg_core::Severity::Warning => warnings += 1,
            }
        }

        Some(status_bar::DocumentStatus {
            line: line + 1,
            column: column + 1,
            language: doc.language,
            indent: self.indent_settings,
            errors,
            warnings,
        })
    }
}

/// Moves whatever failed this frame into the toast stack.
///
/// These used to be a modal, which meant a language server dying in the
/// background, or a `git` refresh failing, stole focus mid-keystroke and
/// had to be dismissed before typing could continue. None of them are
/// questions — the modals that remain (reload-or-keep, confirm delete,
/// save-before-closing) are the ones that actually need an answer.
///
/// One toast per line, since `errors::report` accumulates several failures
/// into one string.
fn drain_errors_into_toasts(last_error: &mut Option<String>, toasts: &mut crate::toasts::Toasts) {
    let Some(message) = last_error.take() else {
        return;
    };
    for line in message.lines() {
        toasts.push(line.to_string());
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
        if !terminal_focused
            && ui.input(|i| i.key_pressed(egui::Key::P) && i.modifiers.command && !i.modifiers.shift)
        {
            self.go_to_file.toggle();
        }
        if !terminal_focused && ui.input(|i| i.key_pressed(egui::Key::P) && i.modifiers.command && i.modifiers.shift) {
            self.command_palette.toggle();
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
        if now - self.drafts_written_at >= DRAFT_INTERVAL_SECONDS {
            self.drafts_written_at = now;
            self.write_drafts();
        }

        // Computed once and reused at every `git diff` trigger point below
        // (open/save/reload) — `None` when no project is open, in which
        // case each of those points is simply a no-op (nothing to diff
        // against).
        let diff_root = self.state.project.as_ref().map(|p| p.root.clone());
        // Recorded here rather than at each `open_project` call site (the
        // side panel's own button, the menu, the welcome screen, a restored
        // session): whatever route was taken, this frame sees the result.
        if let Some(root) = &diff_root
            && self.recent_projects.first() != Some(root)
        {
            crate::panels::welcome::remember_project(&mut self.recent_projects, root);
            // Whatever the last session left unsaved in this project: read
            // once, here, on the frame it's opened (including the restored
            // session's own project at startup), rather than polled.
            self.pending_drafts = fg_core::pending_drafts(root);
        }

        sync_watched_dirs(&mut self.file_watcher, &mut self.watched_dirs, &self.state);
        let tree_changed_on_disk = process_file_events(
            &self.file_event_rx,
            &mut self.state,
            &mut self.parsers,
            &mut self.external_conflicts,
            &mut self.externally_deleted,
            &mut self.diff,
            diff_root.as_deref(),
        );
        // Rebuilding the tree is a full directory walk, and a single
        // external action (a `git checkout`, an unzip) arrives as a burst
        // of individual create/delete events — so the rebuild is deferred
        // until the burst goes quiet rather than run once per event.
        if tree_changed_on_disk {
            self.tree_refresh_due = Some(now + TREE_REFRESH_DEBOUNCE_SECONDS);
        }
        if let Some(due) = self.tree_refresh_due
            && now >= due
            && self.tree_refresh_rx.is_none()
            && let Some(root) = self.state.project.as_ref().map(|project| project.root.clone())
        {
            self.tree_refresh_due = None;
            // Walked on a background thread: on a real multi-module project
            // this reads thousands of directory entries, and doing that on
            // the UI thread turns "a file appeared on disk" into a visible
            // stutter.
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(fg_core::Project::open(root));
                ctx.request_repaint();
            });
            self.tree_refresh_rx = Some(rx);
        }
        if let Some(rx) = &self.tree_refresh_rx {
            match rx.try_recv() {
                Ok(Ok(project)) => {
                    self.state.refresh_project_tree(project);
                    self.tree_refresh_rx = None;
                }
                Ok(Err(err)) => {
                    crate::errors::report(&mut self.last_error, msg::failed_to_refresh_tree(&err.to_string()));
                    self.tree_refresh_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.tree_refresh_rx = None,
            }
        }
        if self.tree_refresh_due.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(TREE_REFRESH_DEBOUNCE_SECONDS));
        }

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
        if self.lsp.wants_repaint()
            || self.hover.wants_repaint()
            || self.goto_definition.wants_repaint()
            || self.peek.wants_repaint()
            || self.find_references.wants_repaint()
            || self.rename.wants_repaint()
            || self.code_action_gutter.wants_repaint()
        {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }

        if auto_save_fired {
            tabs::save_all_dirty_tabs(
                &mut self.state,
                &mut self.parsers,
                &mut self.last_error,
                &self.external_conflicts,
                self.trim_trailing_whitespace_on_save,
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
        let mut debug_toolbar_outcome = debug_toolbar::DebugToolbarOutcome::default();
        // Set by `build_panel::show` the frame a clickable compiler-error
        // row is clicked — resolved into `pending_navigation` further down,
        // once `open_path` has made the target file's real buffer available
        // to convert its line/column into a byte offset (see that call
        // site's own comment).
        let mut build_click: Option<(PathBuf, usize, usize)> = None;
        // Set by `debug_panel::show` the frame a clickable call-stack frame
        // row is clicked (`PLAN.md` Track 23 Phase 3) — resolved the same
        // way `build_click` is below, just with an already-0-indexed line
        // (`debug_state::StackFrameSummary::line`) instead of a compiler's
        // 1-based one, so no `line_col_to_byte` column argument is needed.
        let mut debug_click: Option<(PathBuf, usize)> = None;
        // Compared against `self.source_control_visible` after `menu_bar::
        // show` runs (which mutates it directly, same "one flag, two
        // triggers" shape every other View checkbox here already uses) to
        // detect a false -> true transition — the panel has no saved
        // status to resume (unlike the terminal panel's session), so
        // opening it needs an explicit first `git status` to have anything
        // to show at all.
        let source_control_was_visible = self.source_control_visible;

        // Drawn before the panels so a command picked this frame is acted
        // on by the very same code paths the menus feed, rather than a
        // frame later.
        if let Some(command) = crate::panels::command_palette::show(ui, &mut self.command_palette) {
            self.run_command(ui.ctx(), command, &mut menu_outcome);
        }

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
                        &mut self.trim_trailing_whitespace_on_save,
                        &mut self.zen_mode,
                        &mut self.side_panel_visible,
                        &mut self.terminal_panel_visible,
                        &mut self.source_control_visible,
                        &mut self.build_panel_visible,
                        &mut self.last_error,
                        &mut self.custom_templates,
                        self.static_analysis.checkstyle_running(),
                        self.static_analysis.pmd_running(),
                        self.static_analysis.spotbugs_running(),
                        self.build_state.is_build_running(),
                        self.build_state.is_run_running(),
                        self.build_state.is_test_running(),
                        self.build_state.is_coverage_running(),
                        self.build_state.is_docker_build_run_running(),
                        self.build_state.is_docker_compose_running(),
                        self.debug_state.is_running(),
                    )
                })
                .inner;

            // Shown exactly while a debug session is live (`PLAN.md` Track
            // 23 Phase 2) — not a `View`-menu-toggled dock like the build/
            // terminal panels below, since its whole purpose is 1:1 tied to
            // `self.debug_state` actually running.
            if self.debug_state.is_running() {
                egui::Panel::top("debug_toolbar")
                    .show(ui, |ui| {
                        debug_toolbar_outcome = debug_toolbar::show(ui, self.debug_state.is_paused());
                    });
            }

            // Same "tied 1:1 to a live session" visibility rule as the
            // toolbar above (`PLAN.md` Track 23 Phase 3) — see `debug_panel`'s
            // own header for why this isn't a `View`-menu-toggled dock.
            if self.debug_state.is_running() {
                egui::Panel::right("debug_panel")
                    .resizable(true)
                    .default_size(280.0)
                    .show(ui, |ui| {
                        debug_click = debug_panel::show(ui, &self.debug_state);
                    });
            }

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
                &self.debug_state,
            );
            let activities = status_bar::activities(&work);
            let document_status = self.document_status(ui.ctx());
            egui::Panel::bottom("status_bar")
                .show(ui, |ui| status_bar::show(ui, &activities, document_status.as_ref()));

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

            if self.build_panel_visible {
                egui::Panel::bottom("build_output_panel")
                    .resizable(true)
                    .default_size(220.0)
                    .show(ui, |ui| {
                        build_click = build_panel::show(ui, &mut self.build_state);
                    });
                if let Some(result) = self.build_state.take_coverage_result() {
                    match result {
                        Ok(files) => build_panel::apply_coverage_results(&mut self.state, &files),
                        Err(err) => crate::errors::report(&mut self.last_error, msg::coverage_report_failed(&err.to_string())),
                    }
                }
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
        // `build_panel::show` only hands back the compiler's own 1-based
        // line/column, not a byte offset (`pending_navigation`'s own unit) —
        // unlike the Spring endpoint map, which already knows a raw byte
        // offset at scan time, a build-output row has no buffer to convert
        // against until its file is actually open. `open_path` first, then
        // read the byte offset straight off that now-live `Rope` — no
        // separate on-disk read, and correct even if the buffer's content
        // differs slightly from what was just compiled (an unsaved edit
        // since the build started).
        if let Some((path, line, column)) = build_click {
            open_path(
                &mut self.state,
                &mut self.parsers,
                &mut self.last_error,
                &mut self.diff,
                diff_root.clone(),
                path.clone(),
            );
            if let Some(doc) = self.state.open_tabs.iter().find(|d| d.path() == path.as_path()) {
                let byte = fg_core::line_col_to_byte(&doc.buffer, line, Some(column));
                self.pending_navigation = Some((path, byte));
            }
        }

        // A call-stack frame row's own `line` (`debug_state::
        // StackFrameSummary::line`) is already 0-indexed, unlike `build_
        // click`'s compiler-reported one above — `line_col_to_byte` still
        // wants 1-based input, so `+ 1` undoes that before conversion.
        if let Some((path, line)) = debug_click {
            open_path(
                &mut self.state,
                &mut self.parsers,
                &mut self.last_error,
                &mut self.diff,
                diff_root.clone(),
                path.clone(),
            );
            if let Some(doc) = self.state.open_tabs.iter().find(|d| d.path() == path.as_path()) {
                let byte = fg_core::line_col_to_byte(&doc.buffer, line + 1, None);
                self.pending_navigation = Some((path, byte));
            }
        }

        for (old, new) in &outcome.renamed {
            handle_rename(&mut self.state, &mut self.parsers, old, new);
        }
        for path in &outcome.deleted {
            close_tabs_under(&mut self.state, &mut self.parsers, path);
        }
        if outcome.tree_changed {
            // Scheduled rather than walked here, so an in-app file
            // operation goes through the same debounced background refresh
            // an external one does — see `TREE_REFRESH_DEBOUNCE_SECONDS`.
            self.tree_refresh_due = Some(now);
        }
        if let Some(err) = outcome.error {
            crate::errors::report(&mut self.last_error, err);
        }

        if menu_outcome.open_run_configs_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            self.run_configs_dialog.open(&root);
        }

        if menu_outcome.build_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            match fg_core::detect_build_tool(&root) {
                Some(tool) => match self.build_state.start_build(&root, tool) {
                    Ok(()) => self.build_panel_visible = true,
                    Err(err) => crate::errors::report(&mut self.last_error, msg::failed_to_start_build(&err.to_string())),
                },
                None => crate::errors::report(&mut self.last_error, t().errors.no_build_tool_detected.to_string()),
            }
        }

        // The first saved `RunConfig`, not a user-picked one — `PLAN.md`
        // Track 22 Phase 2's own deliberate scope limit; a real config
        // *picker* (a dropdown next to Run, the way every mainstream IDE
        // has one) is a natural follow-up but isn't what this checkpoint
        // asks for, and `RunConfigsDialogState`'s own `selected` field only
        // exists while that dialog is open, not as a durable "active"
        // choice to read here instead.
        if menu_outcome.run_project_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            match fg_core::detect_build_tool(&root) {
                Some(tool) => match fg_core::load_run_configs(&root).into_iter().next() {
                    Some(config) => match self.build_state.start_run(&root, tool, config) {
                        Ok(()) => self.build_panel_visible = true,
                        Err(err) => crate::errors::report(&mut self.last_error, msg::failed_to_start_build(&err.to_string())),
                    },
                    None => crate::errors::report(&mut self.last_error, t().errors.no_run_config.to_string()),
                },
                None => crate::errors::report(&mut self.last_error, t().errors.no_build_tool_detected.to_string()),
            }
        }

        if menu_outcome.run_tests_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            match fg_core::detect_build_tool(&root) {
                Some(tool) => match self.build_state.start_test(&root, tool) {
                    Ok(()) => self.build_panel_visible = true,
                    Err(err) => crate::errors::report(&mut self.last_error, msg::failed_to_start_build(&err.to_string())),
                },
                None => crate::errors::report(&mut self.last_error, t().errors.no_build_tool_detected.to_string()),
            }
        }

        if menu_outcome.run_with_coverage_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            match fg_core::detect_build_tool(&root) {
                Some(fg_core::BuildTool::Maven) => match self.build_state.start_coverage(&root) {
                    Ok(()) => self.build_panel_visible = true,
                    Err(err) => crate::errors::report(&mut self.last_error, msg::failed_to_start_build(&err.to_string())),
                },
                Some(fg_core::BuildTool::Gradle) => crate::errors::report(&mut self.last_error, t().errors.coverage_requires_maven.to_string()),
                None => crate::errors::report(&mut self.last_error, t().errors.no_build_tool_detected.to_string()),
            }
        }

        if menu_outcome.docker_build_run_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            if fg_core::has_dockerfile(&root) {
                match self.build_state.start_docker_build_and_run(&root) {
                    Ok(()) => self.build_panel_visible = true,
                    Err(err) => crate::errors::report(&mut self.last_error, msg::failed_to_start_docker(&err.to_string())),
                }
            } else {
                crate::errors::report(&mut self.last_error, t().errors.no_dockerfile_detected.to_string());
            }
        }

        if menu_outcome.docker_compose_up_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            match fg_core::compose_file(&root) {
                Some(compose_file) => match self.build_state.start_docker_compose_up(&compose_file) {
                    Ok(()) => self.build_panel_visible = true,
                    Err(err) => crate::errors::report(&mut self.last_error, msg::failed_to_start_docker(&err.to_string())),
                },
                None => crate::errors::report(&mut self.last_error, t().errors.no_compose_file_detected.to_string()),
            }
        }

        // Same "one button, `is_running()` decides which action it means"
        // shape `menu_bar::MenuBarOutcome::debug_request`'s own doc comment
        // describes — `start`'s own precondition failures (no `Ready` Java
        // session, no saved `RunConfig`, classpath resolution failing) all
        // surface through the same `last_error` modal every other Run
        // menu action already uses.
        if menu_outcome.debug_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            if self.debug_state.is_running() {
                self.debug_state.stop();
            } else {
                match fg_core::detect_build_tool(&root) {
                    Some(tool) => match fg_core::load_run_configs(&root).into_iter().next() {
                        Some(config) => {
                            let initial_breakpoints = self
                                .state
                                .open_tabs
                                .iter()
                                .filter(|doc| !doc.breakpoints.is_empty())
                                .map(|doc| (doc.path.clone(), doc.breakpoints.clone()))
                                .collect();
                            if let Err(err) =
                                self.debug_state.start(&mut self.lsp, &root, tool, &config, initial_breakpoints)
                            {
                                crate::errors::report(&mut self.last_error, err);
                            }
                        }
                        None => crate::errors::report(&mut self.last_error, t().errors.no_run_config.to_string()),
                    },
                    None => crate::errors::report(&mut self.last_error, t().errors.no_build_tool_detected.to_string()),
                }
            }
        }

        // Debug toolbar dispatch (`PLAN.md` Track 23 Phase 2) — each button
        // maps straight to its matching `DebugState` method, all of which
        // are harmless no-ops outside a paused session (Continue/Step) or
        // an idle one (Stop), so no extra guard is needed here.
        if debug_toolbar_outcome.continue_request {
            self.debug_state.continue_();
        }
        if debug_toolbar_outcome.step_over_request {
            self.debug_state.step_over();
        }
        if debug_toolbar_outcome.step_into_request {
            self.debug_state.step_into();
        }
        if debug_toolbar_outcome.step_out_request {
            self.debug_state.step_out();
        }
        if debug_toolbar_outcome.stop_request {
            self.debug_state.stop();
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
        if menu_outcome.open_new_project_wizard_request {
            self.new_project_wizard.open();
        }
        if menu_outcome.run_checkstyle_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            let binary = self.external_tool_paths.checkstyle_binary.trim();
            let config = self.external_tool_paths.checkstyle_config.trim();
            if binary.is_empty() || config.is_empty() {
                crate::errors::report(&mut self.last_error, t().errors.checkstyle_not_configured.to_string());
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
                crate::errors::report(&mut self.last_error, t().errors.pmd_not_configured.to_string());
            } else {
                self.static_analysis
                    .run_pmd(PathBuf::from(binary), ruleset.to_string(), root);
            }
        }
        if menu_outcome.run_spotbugs_request
            && let Some(root) = self.state.project.as_ref().map(|p| p.root.clone())
        {
            let binary = self.external_tool_paths.spotbugs_binary.trim();
            if binary.is_empty() {
                crate::errors::report(&mut self.last_error, t().errors.spotbugs_not_configured.to_string());
            } else {
                match fg_core::detect_build_tool(&root) {
                    Some(tool) => {
                        let classes_dir = fg_core::default_classes_dir(&root, tool);
                        if classes_dir.is_dir() {
                            self.static_analysis.run_spotbugs(PathBuf::from(binary), classes_dir, root);
                        } else {
                            crate::errors::report(&mut self.last_error, t().errors.spotbugs_no_compiled_classes.to_string());
                        }
                    }
                    None => crate::errors::report(&mut self.last_error, t().errors.no_build_tool_detected.to_string()),
                }
            }
        }
        if let Some(result) = self.static_analysis.poll_checkstyle() {
            match result {
                Ok(diagnostics) => static_analysis::apply_checkstyle_results(&mut self.state, &diagnostics),
                Err(err) => crate::errors::report(&mut self.last_error, msg::checkstyle_failed(&err.to_string())),
            }
        }
        if let Some(result) = self.static_analysis.poll_pmd() {
            match result {
                Ok(diagnostics) => static_analysis::apply_pmd_results(&mut self.state, &diagnostics),
                Err(err) => crate::errors::report(&mut self.last_error, msg::pmd_failed(&err.to_string())),
            }
        }
        if let Some(result) = self.static_analysis.poll_spotbugs() {
            match result {
                Ok(diagnostics) => static_analysis::apply_spotbugs_results(&mut self.state, &diagnostics),
                Err(err) => crate::errors::report(&mut self.last_error, msg::spotbugs_failed(&err.to_string())),
            }
        }
        self.spring_config.poll();
        self.debug_state.poll();
        self.debug_state.sync_breakpoints(self.state.open_tabs.iter());
        if let Some(err) = self.debug_state.take_failure() {
            crate::errors::report(&mut self.last_error, err);
        }
        for (tool, result) in self.static_analysis.tool_manager.poll_installs() {
            match result {
                Ok(installed) => self.external_tool_paths.apply_installed(&installed),
                Err(err) => crate::errors::report(
                    &mut self.last_error,
                    msg::install_failed(tool.display_name(), &err.to_string()),
                ),
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
        for (server, result) in self.lsp_servers.manager.poll_installs() {
            match result {
                Ok(installed) => self.lsp_settings.apply_installed(&installed),
                Err(err) => crate::errors::report(
                    &mut self.last_error,
                    msg::language_server_install_failed(server.display_name(), &err.to_string()),
                ),
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
            crate::errors::report(&mut self.last_error, msg::git_status_failed(&err.to_string()));
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
                Err(err) => crate::errors::report(&mut self.last_error, msg::git_operation_failed(&err.to_string())),
            }
        }
        if let Some(result) = self.git_stage.poll_expanded()
            && let Err(err) = result
        {
            crate::errors::report(&mut self.last_error, msg::git_diff_failed(&err.to_string()));
        }
        if let Some(result) = self.git_stage.poll_full_diff()
            && let Err(err) = result
        {
            crate::errors::report(&mut self.last_error, msg::git_diff_failed(&err.to_string()));
        }

        // A tab's "Reveal in Tree", raised inside `tabs::show` below and
        // applied to the side panel on the next frame (the panel is drawn
        // before the tab bar, so this frame's own tree is already laid out
        // by the time the request exists).
        let mut reveal_in_tree: Option<PathBuf> = None;
        let mut welcome = crate::panels::welcome::WelcomeOutcome::default();
        // Everything but the project already open — reopening that one is
        // not a thing anyone needs offered.
        let recent_to_offer: Vec<PathBuf> = self
            .recent_projects
            .iter()
            .filter(|path| Some(*path) != self.state.project.as_ref().map(|project| &project.root))
            .cloned()
            .collect();
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
                &mut self.goto_definition,
                &mut self.peek,
                crate::widgets::editor::EditorRequests {
                    case_conversion: menu_outcome.case_conversion_request,
                    sort_lines: menu_outcome.sort_lines_request,
                    unique_lines: menu_outcome.unique_lines_request,
                    fold_all: menu_outcome.fold_all_request,
                    expand_all: menu_outcome.expand_all_request,
                },
                &mut self.last_error,
                &mut self.pending_editor_input,
                &mut self.cached_clipboard_text,
                jump_target.as_ref().map(|(_, char_offset)| *char_offset),
                &self.custom_templates,
                &mut self.spring_config,
                &mut self.lsp,
                &mut self.find_references,
                &mut self.rename_box,
                &mut self.code_action_gutter,
                &self.debug_state,
                &mut self.file_history,
                self.dark_mode,
                self.trim_trailing_whitespace_on_save,
                &mut reveal_in_tree,
                &mut welcome,
                &recent_to_offer,
            );
            if welcome.open_folder {
                self.side_panel.open_folder_picker(self.state.project.as_ref().map(|p| p.root.clone()));
            }
            if welcome.new_project {
                self.new_project_wizard.open();
            }
            if let Some(path) = welcome.open_recent.take()
                && let Err(err) = self.state.open_project(path)
            {
                crate::errors::report(&mut self.last_error, msg::failed_to_open_project(&err.to_string()));
            }
            if let Some(path) = reveal_in_tree.take() {
                // Handled on the *next* frame by the panel itself: the side
                // panel is drawn before the tab bar, so its tree for this
                // frame is already laid out by the time the request exists.
                self.side_panel.request_reveal(path);
                self.side_panel_visible = true;
            }
        });

        // Find references (`PLAN.md` Track 20 Phase 6): a row clicked in
        // `find_references`'s own results popup inside `tabs::show` just
        // above hands its target straight back here, rather than owning
        // `pending_navigation`/`open_path` itself — same cross-tab jump
        // primitive Ctrl+Click uses just below, and already a real byte
        // offset (each `ReferenceHit` resolved its own target file's text
        // once, back when the reply first landed), so no further UTF-16
        // conversion is needed the way `goto_definition`'s own `Target::
        // File` still requires below.
        if let Some((path, byte)) = self.find_references.take_navigation() {
            open_path(&mut self.state, &mut self.parsers, &mut self.last_error, &mut self.diff, diff_root.clone(), path.clone());
            self.pending_navigation = Some((path, byte));
        }

        // Rename symbol (`PLAN.md` Track 20 Phase 7): a confirmed Enter
        // inside `rename_box`'s own popup (`tabs::show` above) fires the
        // actual request here, against the focused tab's own `Document`
        // — the widget itself has no way to reach that other than
        // through this exact `(char_offset, new_name)` handoff.
        if let Some((char_offset, new_name)) = self.rename_box.take_confirmed()
            && let Some(index) = self.state.active_tab
        {
            self.rename.request(&mut self.state.open_tabs[index], char_offset, &new_name, &mut self.lsp);
        }
        if let Some(Err(err)) = self.rename.poll(&mut self.state, &mut self.parsers) {
            crate::errors::report(&mut self.last_error, msg::failed_to_rename(&err));
        }

        // Quick-fix intention actions (`PLAN.md` Track 15 Phase 1): a
        // title picked in `code_action_gutter`'s own popup (`tabs::show`
        // above) hands back the offer's already-resolved `WorkspaceEdit`
        // directly — unlike rename, no further request is needed here,
        // just the same shared `workspace_edit::apply` primitive
        // `rename.rs`'s own `apply_reply` uses.
        if let Some(edit) = self.code_action_gutter.take_confirmed()
            && let Err(err) = crate::workspace_edit::apply(edit, &mut self.state, &mut self.parsers)
        {
            crate::errors::report(&mut self.last_error, msg::failed_to_apply_code_action(&err));
        }

        // Go to definition (`PLAN.md` Track 20 Phase 4): a Ctrl+Click inside
        // `tabs::show` just above (this frame, or an earlier one still
        // awaiting a reply) may have just resolved. `open_path` first, same
        // "no buffer to convert a server's line/column against until the
        // target file is actually open" shape `build_click` below already
        // has; a `Target::Ready` decompiled-source file needs no such
        // conversion (its own byte offset was already computed against the
        // exact text just written to it) but still gets marked `read_only`
        // — editing a jdt.ls decompilation and expecting Save to do
        // anything meaningful would be actively misleading.
        if let Some(target) = self.goto_definition.poll(&mut self.lsp) {
            let is_decompiled = matches!(target, GotoDefinitionTarget::Ready { .. });
            let (path, byte) = match target {
                GotoDefinitionTarget::Ready { path, byte_offset } => (path, Some(byte_offset)),
                GotoDefinitionTarget::File { path, range } => {
                    let byte = self
                        .state
                        .open_tabs
                        .iter()
                        .find(|doc| doc.path() == path.as_path())
                        .and_then(|doc| crate::lsp_state::utf16_range_to_bytes(&doc.buffer.to_string(), range))
                        .map(|range| range.start);
                    (path, byte)
                }
            };
            open_path(&mut self.state, &mut self.parsers, &mut self.last_error, &mut self.diff, diff_root.clone(), path.clone());
            if let Some(byte) = byte {
                self.pending_navigation = Some((path.clone(), byte));
            }
            if is_decompiled
                && let Some(doc) = self.state.open_tabs.iter_mut().find(|doc| doc.path() == path.as_path())
            {
                doc.read_only = true;
            }
        }

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
        new_project::show(ui, &mut self.new_project_wizard, &mut self.state);

        self.show_draft_restore_prompt(ui);
        drain_errors_into_toasts(&mut self.last_error, &mut self.toasts);
        self.toasts.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        persist_session(storage, &self.state, &self.recent_projects);
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
            self.build_panel_visible,
            &self.custom_templates,
            &self.external_tool_paths,
            self.auto_save_settings,
            self.trim_trailing_whitespace_on_save,
            &self.lsp_settings,
            &self.jdk_registry,
        );
    }
}

#[cfg(test)]
mod e2e_test;
#[cfg(test)]
mod app_test;
