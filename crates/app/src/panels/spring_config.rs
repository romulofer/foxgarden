//! Background classpath scan feeding Spring config property autocomplete
//! (`PLAN.md` Track 12 Phase 1) — the app-side counterpart to the project's
//! own build tool resolving a classpath (`BuildToolHandle::
//! analysis_classpath`) and `fg_core::scan_classpath_for_metadata` reading
//! Spring's metadata out of it. Lazily triggered (`ensure_scanning`, called from the
//! completion trigger the first time a `.properties`/`.yml` file actually
//! needs candidates) rather than eagerly on every project open — real
//! classpath resolution shells out to `mvn`/`gradle` and can be genuinely
//! slow, so a project that never opens a Spring config file never pays for
//! it, mirroring `static_analysis`'s own "only runs on an explicit ask, not
//! automatically" cost discipline, just triggered by a file-open condition
//! instead of a menu click.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use fg_core::{BuildToolHandle, ConfigProperty, ExtensionHandle};
use fg_extension::Registry;

/// One project's own scanned properties, plus whichever project root
/// they're for — a fresh `open_project` (a different root) must invalidate
/// a previous project's stale results rather than silently keep serving
/// them under the new project's own files.
#[derive(Default)]
pub struct SpringConfigState {
    scanned_root: Option<PathBuf>,
    scan_rx: Option<Receiver<Vec<ConfigProperty>>>,
    properties: Vec<ConfigProperty>,
    /// The extensions a scan asks for properties, held as handles rather
    /// than looked up per scan: the completion trigger that starts one runs
    /// deep inside the editor widget, which has no registry to ask, and the
    /// scan itself runs on a thread that could not borrow one anyway.
    /// `None` until `observe` has run once.
    extensions: Option<Vec<ExtensionHandle>>,
    /// The build tool the registry detected for a project root, kept for the
    /// same reason as `extensions`. `None` inside means no tool claims that
    /// root, so there is no classpath to scan.
    build_tool: Option<(PathBuf, Option<BuildToolHandle>)>,
}

impl SpringConfigState {
    /// Test-only fixture: a state that already has `properties` cached, as
    /// if a real scan had already completed, without spawning one — the
    /// widget-side completion trigger tests need known candidates without
    /// depending on a real `mvn`/`gradle` process.
    #[cfg(test)]
    pub(crate) fn with_properties(properties: Vec<ConfigProperty>) -> Self {
        Self {
            properties,
            ..Self::default()
        }
    }

    /// The currently cached candidates — empty until a scan for
    /// `project_root` has actually completed (`poll`), including while one
    /// is still running.
    pub fn properties(&self) -> &[ConfigProperty] {
        &self.properties
    }

    /// Whether a classpath scan is running right now — what the status bar
    /// reports it with. A scan shells out to `mvn`/`gradle` and can take a
    /// while, and it's triggered by opening a config file rather than by
    /// anything the user asked for directly, so it's exactly the kind of
    /// job the bar exists to make visible.
    pub fn scanning(&self) -> bool {
        self.scan_rx.is_some()
    }

    /// Picks up what a scan needs from the registry — called every frame
    /// from `FoxGardenApp::ui`, where the registry is in reach. The handles
    /// are taken once (nothing is registered after startup), and the build
    /// tool is detected again only when `project_root` changes.
    pub fn observe(&mut self, languages: &Registry, project_root: Option<&Path>) {
        if self.extensions.is_none() {
            self.extensions = Some(languages.handles());
        }
        match project_root {
            Some(root) if self.build_tool.as_ref().is_some_and(|(seen, _)| seen == root) => {}
            Some(root) => self.build_tool = Some((root.to_path_buf(), languages.detect_build_tool(root))),
            None => self.build_tool = None,
        }
    }

    /// Kicks off a background scan for `project_root` if none has run (or
    /// is running) for it yet — a no-op otherwise, so a completion trigger
    /// can call this unconditionally on every keystroke without piling up
    /// redundant scans. A different `project_root` than the one last
    /// scanned (a new project opened since) clears the stale cache
    /// immediately, before the fresh scan even finishes, rather than
    /// leaving the previous project's properties visible in the meantime.
    ///
    /// A project no build tool claims has no classpath, so it is marked
    /// scanned without starting a thread. One `observe` has not seen yet is
    /// left unmarked, to be scanned on a later call once it has.
    pub fn ensure_scanning(&mut self, project_root: &Path) {
        if self.scanned_root.as_deref() == Some(project_root) {
            return;
        }
        if self.scan_rx.is_some() {
            return;
        }
        self.properties.clear();
        let Some((_, build_tool)) = self.build_tool.as_ref().filter(|(seen, _)| seen == project_root) else {
            return;
        };
        let build_tool = build_tool.clone();
        self.scanned_root = Some(project_root.to_path_buf());
        let Some(build_tool) = build_tool else { return };

        let extensions = self.extensions.clone().unwrap_or_default();
        let root = project_root.to_path_buf();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(scan_project(&extensions, &root, &build_tool));
        });
        self.scan_rx = Some(rx);
    }

    /// Drains a completed scan's result, if any finished since the last
    /// poll — called once per frame from `FoxGardenApp::ui`, the same
    /// "poll every frame regardless of whether anything's actually
    /// running" shape `static_analysis::poll_checkstyle`/`poll_pmd` use.
    pub fn poll(&mut self) {
        let Some(rx) = self.scan_rx.as_ref() else { return };
        match rx.try_recv() {
            Ok(properties) => {
                self.properties = properties;
                self.scan_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.scan_rx = None,
        }
    }
}

/// Asks every extension what configuration keys this project has. An
/// extension that recognizes nothing about the project (or whose own
/// classpath resolution fails — no `mvn`/`gradle` on `PATH`, a real build
/// error) contributes nothing rather than an error: the same "silently show
/// nothing" degrade `fg_core::diff`/`blame` uses for "not a git repository",
/// since a failed background scan is exactly as unsurprising to a user who
/// never asked for it directly.
fn scan_project(
    extensions: &[ExtensionHandle],
    project_root: &Path,
    build_tool: &BuildToolHandle,
) -> Vec<ConfigProperty> {
    extensions
        .iter()
        .flat_map(|extension| extension.config_properties(project_root, build_tool))
        .collect()
}

#[cfg(test)]
#[path = "spring_config_test.rs"]
mod spring_config_test;
