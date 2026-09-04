//! The status bar across the bottom of the window: one line reporting
//! whatever the app is doing *on its own initiative* — a language server
//! coming up, a background install, a Checkstyle/PMD run, a classpath scan,
//! a `git` refresh.
//!
//! Every job it reports already runs on its own thread and is polled once
//! per frame from `FoxGardenApp::ui`, so that none of them can ever join
//! the keystroke-to-pixels path (`AGENTS.md`). That discipline is also what
//! made them invisible: opening a `.java` file spawns jdt.ls and then waits
//! out a handshake that takes tens of seconds on a real project, during
//! which the editor looks exactly like one whose language support is simply
//! broken. The bar is the one place that says otherwise.
//!
//! Gathering ([`BackgroundWork::gather`]) is kept apart from labelling
//! ([`activities`]) and from drawing ([`show`]): each subsystem owns its
//! own "am I busy" answer, the mapping from those answers to what the bar
//! actually says is then a pure function over a plain struct, and no test
//! of it has to spawn a real child process to reach a given line.

use fg_core::Language;
use fg_i18n::{msg, t};

use crate::style::icons;
use crate::style::indent::IndentSettings;

use crate::debug_state::{DebugState, DebugStatus};
use crate::lsp_manager::{ALL_SERVERS, LspManagerState};
use crate::lsp_state::LspState;
use crate::panels::git_diff::DiffState;
use crate::panels::git_stage::GitStageState;
use crate::panels::lsp_servers::LspServersState;
use crate::panels::spring_config::SpringConfigState;
use crate::panels::static_analysis::StaticAnalysisState;
use crate::tool_manager::{ALL_TOOLS, ToolManagerState};

/// The spinner's diameter, in points. Sized to sit inside a body-text line
/// rather than stretch the bar taller than the text it precedes.
const SPINNER_SIZE: f32 = 12.0;

/// Every autonomous job in flight this frame, as reported by whichever
/// subsystem owns it. A plain snapshot with no borrows of its own, so
/// [`activities`] can be exercised against a hand-built one.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct BackgroundWork {
    /// Display names of language servers still inside their `initialize`
    /// handshake (`"JDTLS"`).
    pub starting_servers: Vec<&'static str>,
    /// Display name plus latest `language/status` message of every `Ready`
    /// server still importing its project in the background
    /// (`LspState::indexing_servers`) — the handshake above finishes in
    /// seconds regardless of project size; this is the (often much longer)
    /// import that follows it, with jdt.ls' own progress text.
    pub indexing_servers: Vec<(&'static str, String)>,
    /// Running installs: display name plus the job's own latest progress
    /// line, when it reports one. Language-server installs do (jdt.ls is
    /// built from source and takes minutes — see `lsp_manager`); external
    /// tools are a single download, so they report none.
    pub installing: Vec<(&'static str, Option<String>)>,
    /// Display names of the servers/tools whose "Check for Updates" is
    /// still waiting on the GitHub API.
    pub checking_versions: Vec<&'static str>,
    /// A `detect_java_home` scan of this machine's JDK installs.
    pub detecting_java_home: bool,
    pub running_checkstyle: bool,
    pub running_pmd: bool,
    pub running_spotbugs: bool,
    /// A `mvn`/`gradle` classpath resolution feeding Spring config
    /// completion.
    pub scanning_classpath: bool,
    /// Any `git` subprocess at all: a `status` refresh, a `diff`/`blame`
    /// scan, or a stage/commit/push. Reported as one line rather than
    /// itemised — they're all "the app is talking to git", and several
    /// routinely overlap after a single save.
    pub running_git: bool,
    /// A debug session's own launch handshake in flight (`PLAN.md` Track 23
    /// Phase 1) — `Some` only while `DebugStatus::Starting`; once `Attached`
    /// the Run menu's own button already reads `Stop`, a persistent enough
    /// signal that this ambient line would just be redundant noise once the
    /// handshake itself is done.
    pub debug_starting: bool,
}

impl BackgroundWork {
    /// Asks every subsystem that owns a background job what it's doing
    /// right now. Cheap enough to call every frame: each answer is an
    /// `Option`/`HashMap` emptiness check, no I/O and no allocation beyond
    /// the handful of names actually collected.
    pub fn gather(
        lsp: &LspState,
        lsp_servers: &LspServersState,
        static_analysis: &StaticAnalysisState,
        spring_config: &SpringConfigState,
        git_stage: &GitStageState,
        diff: &DiffState,
        debug: &DebugState,
    ) -> Self {
        let servers = &lsp_servers.manager;
        let tools = &static_analysis.tool_manager;
        Self {
            starting_servers: lsp.starting_servers(),
            indexing_servers: lsp.indexing_servers().into_iter().map(|(name, message)| (name, message.to_string())).collect(),
            installing: installing_servers(servers).chain(installing_tools(tools)).collect(),
            checking_versions: checking_servers(servers).chain(checking_tools(tools)).collect(),
            detecting_java_home: servers.detecting_java_home(),
            running_checkstyle: static_analysis.checkstyle_running(),
            running_pmd: static_analysis.pmd_running(),
            running_spotbugs: static_analysis.spotbugs_running(),
            scanning_classpath: spring_config.scanning(),
            running_git: diff.running()
                || git_stage.status_running()
                || git_stage.op_running()
                || git_stage.expanded_running()
                || git_stage.full_diff_running(),
            debug_starting: matches!(debug.status(), DebugStatus::Starting(_)),
        }
    }
}

fn installing_servers(servers: &LspManagerState) -> impl Iterator<Item = (&'static str, Option<String>)> {
    ALL_SERVERS
        .into_iter()
        .filter(|&server| servers.installing(server))
        .map(|server| (server.display_name(), servers.status(server).map(str::to_string)))
}

fn installing_tools(tools: &ToolManagerState) -> impl Iterator<Item = (&'static str, Option<String>)> {
    ALL_TOOLS
        .into_iter()
        .filter(|&tool| tools.installing(tool))
        .map(|tool| (tool.display_name(), None))
}

fn checking_servers(servers: &LspManagerState) -> impl Iterator<Item = &'static str> {
    ALL_SERVERS
        .into_iter()
        .filter(|&server| servers.checking(server))
        .map(|server| server.display_name())
}

fn checking_tools(tools: &ToolManagerState) -> impl Iterator<Item = &'static str> {
    ALL_TOOLS.into_iter().filter(|&tool| tools.checking(tool)).map(|tool| tool.display_name())
}

/// One job, as the bar words it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    /// What's running, already localised — `"Iniciando JDTLS…"`.
    pub label: String,
    /// The job's own latest progress line, when it reports one. Comes from
    /// the job itself (`lsp_manager`'s install reports `"Cloning…"`,
    /// `"Building…"`), so unlike `label` it isn't translated.
    pub detail: Option<String>,
}

impl Activity {
    fn new(label: String) -> Self {
        Self { label, detail: None }
    }

    /// The single string the bar paints for this job — label plus whatever
    /// progress detail the job last reported.
    pub fn text(&self) -> String {
        match &self.detail {
            Some(detail) => format!("{} {detail}", self.label),
            None => self.label.clone(),
        }
    }
}

/// Words `work` as one [`Activity`] per running job, most worth naming
/// first: the bar only has room to *name* one, so the order decides which
/// job that is when several overlap. Starting servers lead because they
/// block real editing features (no completions, no diagnostics until one is
/// up); version checks and the JDK scan trail because they finish in a
/// second and change nothing the user is waiting on.
pub fn activities(work: &BackgroundWork) -> Vec<Activity> {
    let mut activities = Vec::new();
    for name in &work.starting_servers {
        activities.push(Activity::new(msg::starting_language_server(name)));
    }
    for (name, message) in &work.indexing_servers {
        activities.push(Activity { label: msg::indexing_language_server(name), detail: Some(message.clone()) });
    }
    for (name, detail) in &work.installing {
        activities.push(Activity { label: msg::installing_named(name), detail: detail.clone() });
    }
    if work.running_checkstyle {
        activities.push(Activity::new(t().common.running_checkstyle.to_string()));
    }
    if work.running_pmd {
        activities.push(Activity::new(t().common.running_pmd.to_string()));
    }
    if work.running_spotbugs {
        activities.push(Activity::new(t().common.running_spotbugs.to_string()));
    }
    if work.scanning_classpath {
        activities.push(Activity::new(t().status_bar.scanning_classpath.to_string()));
    }
    if work.running_git {
        activities.push(Activity::new(t().status_bar.running_git.to_string()));
    }
    for name in &work.checking_versions {
        activities.push(Activity::new(msg::checking_for_updates_to(name)));
    }
    if work.detecting_java_home {
        activities.push(Activity::new(t().status_bar.detecting_java_home.to_string()));
    }
    if work.debug_starting {
        activities.push(Activity::new(t().common.running_debug.to_string()));
    }
    activities
}

/// Draws the bar: a spinner and the first activity's text, plus a `+N`
/// count for however many others are running, all of which the hover text
/// then lists in full. Idle shows a dimmed "Ready" rather than nothing at
/// all — an empty strip along the bottom of the window reads as a bug, and
/// "nothing is running" is itself worth being able to see.
///
/// The spinner requests its own repaints (`egui::Spinner`), which is also
/// what keeps `FoxGardenApp::ui`'s once-per-frame polling of these very
/// jobs running while one is in flight: no user input arrives during a
/// multi-minute install, and none is needed.
pub fn show(ui: &mut egui::Ui, activities: &[Activity], document: Option<&DocumentStatus>) {
    ui.horizontal(|ui| {
        // The document's own facts sit on the right, where every editor
        // puts them, and are laid out first so the (variable-length)
        // activity text on the left can't push them off screen.
        if let Some(document) = document {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                show_document_status(ui, document);
            });
        }

        let Some((first, rest)) = activities.split_first() else {
            ui.weak(t().status_bar.ready);
            return;
        };
        ui.add(egui::Spinner::new().size(SPINNER_SIZE));
        let response = ui.label(first.text());
        if rest.is_empty() {
            return;
        }
        let response = response | ui.weak(format!("+{}", rest.len()));
        let all: Vec<String> = activities.iter().map(Activity::text).collect();
        response.on_hover_text(all.join("\n"));
    });
}

/// What the bar reports about the file currently being edited. Everything
/// here was previously invisible: the caret position, what language the
/// editor thinks the file is, what a Tab key inserts, and whether the file
/// has problems in it — all of which a user has to be able to check
/// without hunting through menus.
pub struct DocumentStatus {
    /// 1-based, as every editor displays them (and as compiler output
    /// refers to them), not the 0-based indices the buffer uses.
    pub line: usize,
    pub column: usize,
    /// `None` for a file whose extension maps to no supported language —
    /// worth saying explicitly, since that's also why it has no
    /// highlighting or completion.
    pub language: Option<Language>,
    pub indent: IndentSettings,
    pub errors: usize,
    pub warnings: usize,
}

fn show_document_status(ui: &mut egui::Ui, document: &DocumentStatus) {
    // Right-to-left layout: added last renders leftmost.
    let indent = if document.indent.use_tabs {
        msg::status_indent_tabs(document.indent.width)
    } else {
        msg::status_indent_spaces(document.indent.width)
    };
    ui.weak(indent);
    ui.weak("·");
    ui.weak(match document.language {
        Some(language) => language.display_name().to_string(),
        None => t().status_bar.plain_text.to_string(),
    });
    ui.weak("·");
    ui.weak(msg::status_line_column(document.line, document.column));

    if document.errors > 0 || document.warnings > 0 {
        ui.weak("·");
        let counts = format!(
            "{} {}  {} {}",
            icons::ERROR,
            document.errors,
            icons::WARNING,
            document.warnings
        );
        ui.label(counts).on_hover_text(t().status_bar.diagnostics_hint);
    }
}

#[cfg(test)]
mod status_bar_test;
