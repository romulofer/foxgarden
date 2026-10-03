//! What the editor shows when no file is open.
//!
//! That state used to be the words "No file open" in the corner of an
//! otherwise empty pane, with the only way forward a 16-pixel icon in the
//! side panel's toolbar. A first run therefore looked like an app that
//! hadn't finished loading. This is the same empty state with the three
//! things a user actually wants from it: open something, create something,
//! or go back to something recent — plus the shortcuts that are otherwise
//! undiscoverable.

use std::path::{Path, PathBuf};

use fg_i18n::t;

use crate::style::icons;

/// What the user asked for on the welcome screen, for the caller (which
/// owns the folder picker, the new-project wizard and the project itself)
/// to act on.
#[derive(Default)]
pub struct WelcomeOutcome {
    pub open_folder: bool,
    pub new_project: bool,
    pub open_recent: Option<PathBuf>,
}

/// Draws the welcome screen. `recent` is most-recent-first, and should not
/// include the project that's already open — offering to reopen what's
/// already there is noise.
pub fn show(ui: &mut egui::Ui, recent: &[PathBuf], project_open: bool) -> WelcomeOutcome {
    let mut outcome = WelcomeOutcome::default();

    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.heading("FoxGarden");
        ui.add_space(4.0);
        ui.weak(t().welcome.tagline);
        ui.add_space(24.0);

        if ui
            .button(format!("{}  {}", icons::OPEN_FOLDER, t().menu.open_folder))
            .clicked()
        {
            outcome.open_folder = true;
        }
        if ui
            .button(format!("{}  {}", icons::NEW_FILE, t().menu.new_project))
            .clicked()
        {
            outcome.new_project = true;
        }

        if !recent.is_empty() {
            ui.add_space(20.0);
            ui.weak(t().welcome.recent_projects);
            ui.add_space(4.0);
            for path in recent.iter().take(MAX_RECENT) {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                if ui
                    .button(format!("{}  {name}", icons::FOLDER))
                    .on_hover_text(path.display().to_string())
                    .clicked()
                {
                    outcome.open_recent = Some(path.clone());
                }
            }
        }

        ui.add_space(24.0);
        ui.weak(shortcut_hints(project_open));
    });

    outcome
}

/// How many recent projects the screen offers. Enough to cover "the few
/// things I'm actually working on", short enough to stay scannable.
const MAX_RECENT: usize = 5;

/// The shortcuts worth learning first, as one line. With no project open,
/// the file-oriented ones would do nothing, so they're left out rather than
/// advertised and then ignored.
fn shortcut_hints(project_open: bool) -> String {
    let strings = t();
    if project_open {
        format!(
            "{}  ·  {}  ·  {}  ·  {}",
            strings.welcome.hint_go_to_file,
            strings.welcome.hint_recent_files,
            strings.welcome.hint_command_palette,
            strings.welcome.hint_terminal,
        )
    } else {
        strings.welcome.hint_command_palette.to_string()
    }
}

/// Adds `root` to the front of `recent`, without duplicates, capped — the
/// bookkeeping behind the recent-projects list.
pub fn remember_project(recent: &mut Vec<PathBuf>, root: &Path) {
    recent.retain(|existing| existing != root);
    recent.insert(0, root.to_path_buf());
    recent.truncate(MAX_RECENT * 2);
}

#[cfg(test)]
#[path = "welcome_test.rs"]
mod welcome_test;
