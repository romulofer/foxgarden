//! The bottom dock: one docked strip along the bottom edge with a tab per
//! bottom panel (Terminal, Build Output, Profiler), replacing the three
//! independent `*_panel_visible` booleans those panels each used to carry
//! (`FEATURES.md`'s own "Tab-controlled bottom panels" request).
//!
//! The old shape let all three stack on top of each other — three separate
//! `egui::Panel::bottom`s, three separate View-menu checkboxes, three
//! separate shortcuts — so a running build plus an open terminal ate two
//! thirds of the window between them and neither could be reached without
//! hunting for its own toggle. One dock with a tab strip is the same
//! interaction the file tab bar (`panels::tabs`) already uses: clicking
//! `Terminal` shows the terminal *instead of* the build output, not
//! alongside it.
//!
//! This module owns only the strip and the "which tab, open or closed"
//! state; each tab's content stays in its own module (`terminal_panel`,
//! `build_panel`, `profiler_panel`), which is why the dock hands back a
//! plain `BottomTab` rather than trying to render any of them itself — the
//! three take wildly different borrows off `FoxGardenApp` (a `&mut
//! EditorState` plus live ptys, a `&mut BuildState`, a `&ProfilerState`
//! plus `&mut FlameGraphState`), and threading all of that through one
//! closure here would couple the strip to every panel it hosts.

use fg_i18n::t;

/// Which panel the dock is showing. `Terminal` is the default because it's
/// the one with a keyboard shortcut of its own (`Ctrl+\``) and so the one a
/// first-ever open is most likely to have meant.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BottomTab {
    #[default]
    Terminal,
    Build,
    Profiler,
}

impl BottomTab {
    /// Left-to-right strip order. Terminal first for the same reason it's
    /// the default; Profiler last, since it's the only one that can't be
    /// reached from the keyboard at all.
    pub const ALL: [Self; 3] = [Self::Terminal, Self::Build, Self::Profiler];

    /// The tab's own strip label (localized — see `fg_i18n::catalog::Dock`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => t().dock.terminal,
            Self::Build => t().dock.build,
            Self::Profiler => t().dock.profiler,
        }
    }

    /// The token `persist_settings` writes and `restore_settings` reads
    /// back. Spelled out rather than persisted as an index, so reordering
    /// `ALL` (or adding a fourth tab in the middle of it) can't silently
    /// reinterpret an existing user's saved choice as a different panel.
    pub fn key(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Build => "build",
            Self::Profiler => "profiler",
        }
    }

    /// Inverse of [`BottomTab::key`]; `None` for anything unrecognized (a
    /// hand-edited or future-version settings file), which the caller then
    /// treats as "no saved choice" rather than as an error.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.key() == key)
    }
}

/// Whether the dock is open at all, and which tab it's showing. The two are
/// kept separate — rather than a single `Option<BottomTab>` — so closing
/// and reopening the dock lands back on the tab it was last showing instead
/// of resetting to the default every time.
#[derive(Clone, Copy, Debug, Default)]
pub struct BottomDock {
    open: bool,
    active: BottomTab,
}

impl BottomDock {
    /// Rebuilds the dock from persisted settings (`app::restore_settings`).
    pub fn restored(open: bool, active: BottomTab) -> Self {
        Self { open, active }
    }

    pub fn is_open(self) -> bool {
        self.open
    }

    pub fn active(self) -> BottomTab {
        self.active
    }

    /// Whether `tab`'s content is on screen right now — i.e. the dock is
    /// open *and* this is the tab it's showing. Every caller that used to
    /// read a `*_panel_visible` flag wants this one, not `is_open`.
    pub fn shows(self, tab: BottomTab) -> bool {
        self.open && self.active == tab
    }

    /// Shows `tab`, opening the dock if it was closed. Used by everything
    /// that has a *reason* to surface a specific panel (Run > Build
    /// starting a build, a profile capture landing), where the panel
    /// appearing is the point and toggling it off would be wrong.
    pub fn open_tab(&mut self, tab: BottomTab) {
        self.open = true;
        self.active = tab;
    }

    /// The menu-checkbox / shortcut gesture: showing `tab` already means
    /// close the dock; anything else (closed, or open on a different tab)
    /// means show `tab`. The middle case is the one the old three-booleans
    /// shape couldn't express — with a build panel open, `Ctrl+\`` now
    /// switches to the terminal rather than stacking a second panel under
    /// it.
    pub fn toggle(&mut self, tab: BottomTab) {
        if self.shows(tab) {
            self.open = false;
        } else {
            self.open_tab(tab);
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }
}

/// Draws the strip itself at the top of the dock's panel and applies any
/// click to `dock`. Returns the tab to render underneath — which is just
/// `dock.active()` *after* this frame's own click, so clicking a tab swaps
/// the content in the same frame rather than a frame later.
pub fn show_tabs(ui: &mut egui::Ui, dock: &mut BottomDock) -> BottomTab {
    ui.horizontal(|ui| {
        for tab in BottomTab::ALL {
            if ui.selectable_label(dock.active() == tab, tab.label()).clicked() {
                dock.open_tab(tab);
            }
        }
        // Right-aligned, mirroring the file tab strip's own close affordance:
        // the dock's tabs switch between panels, so the only way to get the
        // whole strip off screen from here is a separate control.
        //
        // `allocate_ui_with_layout` with an explicit one-row height, not a
        // bare `with_layout`: a nested layout inside `horizontal` inherits
        // the *panel's* full remaining height, which silently stretches this
        // strip over the whole dock and leaves the panel below it a single
        // row. That cost a real crash when it first shipped — the terminal
        // was resized to a 1-row grid and `vt100` panicked parsing a
        // double-width glyph into it — so the height is pinned here rather
        // than inferred.
        let row_height = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), row_height),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let close = egui::RichText::new(crate::style::icons::CLOSE.to_string())
                    .small()
                    .weak();
                if ui.button(close).on_hover_text(t().dock.hide).clicked() {
                    dock.close();
                }
            },
        );
    });
    ui.separator();
    dock.active()
}

#[cfg(test)]
#[path = "bottom_dock_test.rs"]
mod bottom_dock_test;
