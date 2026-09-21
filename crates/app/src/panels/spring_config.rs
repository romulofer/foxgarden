//! Background classpath scan feeding Spring config property autocomplete
//! (`PLAN.md` Track 12 Phase 1) — the app-side counterpart to `fg_core`'s
//! `maven_classpath`/`gradle_classpaths`/`scan_classpath_for_metadata`
//! (Track 21). Lazily triggered (`ensure_scanning`, called from the
//! completion trigger the first time a `.properties`/`.yml` file actually
//! needs candidates) rather than eagerly on every project open — real
//! classpath resolution shells out to `mvn`/`gradle` and can be genuinely
//! slow, so a project that never opens a Spring config file never pays for
//! it, mirroring `static_analysis`'s own "only runs on an explicit ask, not
//! automatically" cost discipline, just triggered by a file-open condition
//! instead of a menu click.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use fg_core::SpringConfigProperty;

/// One project's own scanned properties, plus whichever project root
/// they're for — a fresh `open_project` (a different root) must invalidate
/// a previous project's stale results rather than silently keep serving
/// them under the new project's own files.
#[derive(Default)]
pub struct SpringConfigState {
    scanned_root: Option<PathBuf>,
    scan_rx: Option<Receiver<Vec<SpringConfigProperty>>>,
    properties: Vec<SpringConfigProperty>,
}

impl SpringConfigState {
    /// Test-only fixture: a state that already has `properties` cached, as
    /// if a real scan had already completed, without spawning one — the
    /// widget-side completion trigger tests need known candidates without
    /// depending on a real `mvn`/`gradle` process.
    #[cfg(test)]
    pub(crate) fn with_properties(properties: Vec<SpringConfigProperty>) -> Self {
        Self {
            properties,
            ..Self::default()
        }
    }

    /// The currently cached candidates — empty until a scan for
    /// `project_root` has actually completed (`poll`), including while one
    /// is still running.
    pub fn properties(&self) -> &[SpringConfigProperty] {
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

    /// Kicks off a background scan for `project_root` if none has run (or
    /// is running) for it yet — a no-op otherwise, so a completion trigger
    /// can call this unconditionally on every keystroke without piling up
    /// redundant scans. A different `project_root` than the one last
    /// scanned (a new project opened since) clears the stale cache
    /// immediately, before the fresh scan even finishes, rather than
    /// leaving the previous project's properties visible in the meantime.
    pub fn ensure_scanning(&mut self, project_root: &Path) {
        if self.scanned_root.as_deref() == Some(project_root) {
            return;
        }
        if self.scan_rx.is_some() {
            return;
        }
        self.properties.clear();
        self.scanned_root = Some(project_root.to_path_buf());

        let root = project_root.to_path_buf();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(scan_project(&root));
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

/// Detects the build tool at `project_root` (a plain file-presence check —
/// this app has no other project-type signal yet) and scans whatever
/// classpath it resolves to. Neither tool detected, or resolution failing
/// outright (no `mvn`/`gradle` on `PATH`, a real build error, ...), yields
/// an empty result rather than an error — the same "silently show nothing"
/// degrade `fg_core::diff`/`blame` already established for "not a git
/// repository," since a failed background classpath scan is exactly as
/// unsurprising to a user who never asked for it directly.
fn scan_project(project_root: &Path) -> Vec<SpringConfigProperty> {
    let classpath = if project_root.join("pom.xml").exists() {
        maven_module_tree_classpath(project_root)
    } else if project_root.join("build.gradle.kts").exists() || project_root.join("build.gradle").exists() {
        gradle_project_classpath(project_root)
    } else {
        Vec::new()
    };
    fg_core::scan_classpath_for_metadata(&classpath)
}

/// A Maven project's own classpath, module-aware: a plain single-module
/// `pom.xml` resolves directly, but a multi-module *aggregator* `pom.xml`
/// (`<packaging>pom</packaging>`, real `<modules>`) has no dependencies of
/// its own to resolve at all — `mvn dependency:build-classpath` there would
/// resolve nothing useful, so this reads the aggregator's own `<modules>`
/// (`fg_core::parse_pom`, Track 21 Phase 1) and resolves each real module
/// directory's own classpath instead, unioning the results. A module whose
/// own classpath fails to resolve is skipped rather than failing the whole
/// scan, same as `scan_classpath_for_metadata`'s own per-jar tolerance.
fn maven_module_tree_classpath(project_root: &Path) -> Vec<PathBuf> {
    let module_dirs = match std::fs::read_to_string(project_root.join("pom.xml"))
        .ok()
        .and_then(|xml| fg_core::parse_pom(&xml).ok())
    {
        Some(project) if !project.modules.is_empty() => project.modules.iter().map(|m| project_root.join(m)).collect(),
        _ => vec![project_root.to_path_buf()],
    };

    module_dirs
        .iter()
        .filter_map(|dir| fg_core::maven_classpath(dir).ok())
        .flatten()
        .collect()
}

/// A Gradle project's own classpath, every subproject's `compile`/`runtime`
/// jars unioned in one pass — unlike Maven, `gradle_classpaths` already
/// walks the whole multi-module tree from a single invocation at the root
/// (Track 21 Phase 3), so there's no separate aggregator-vs-module case to
/// handle here.
fn gradle_project_classpath(project_root: &Path) -> Vec<PathBuf> {
    fg_core::gradle_classpaths(project_root)
        .map(|classpaths| {
            classpaths
                .into_iter()
                .flat_map(|c| c.compile.into_iter().chain(c.runtime))
                .collect()
        })
        .unwrap_or_default()
}
