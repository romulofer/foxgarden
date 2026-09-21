use std::path::{Path, PathBuf};

use crate::document::{Document, OpenDocumentError};
use crate::project::Project;

/// Cap on `closed_tabs`: it exists only so `Ctrl+Shift+T` can walk
/// backwards through recently closed tabs, not as a full undo history, so
/// unbounded growth buys nothing — each entry holds a full `Document`
/// (its entire `Rope` buffer) that would otherwise sit in memory for the
/// life of the process every time a tab is closed.
const MAX_CLOSED_TABS: usize = 20;

/// One terminal session in the terminal panel (`PLAN.md`'s terminal-panel
/// track). Deliberately **not** part of `open_tabs`/`active_tab` at all:
/// `SPEC.md` §8.2 puts the terminal in its own dockable bottom panel (the
/// VSCode shape), not the file tab strip, so a session never shares a
/// namespace or an index with a file tab.
///
/// Just a title plus a blink-timing anchor — the real child process,
/// writer, and `vt100::Parser` all live on the app-side `PtySession`
/// (`crates/app/src/pty_session.rs`, index-aligned with `terminal_tabs`, the
/// same "index-aligned side vec, not on `EditorState`" shape `parsers`
/// already uses for `open_tabs`), since a live process and its background
/// reader thread can't be headless-testable the way `fg-core` needs to stay.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalTab {
    pub title: String,
    /// Mirrors `text_area::shell::ShellState::last_interaction` — the
    /// timestamp `terminal_widget::show`'s own cursor-blink cycle is
    /// anchored to, reset whenever the session receives keyboard input, so
    /// the terminal's cursor blinks on the same cadence the editor's own
    /// caret does rather than a second, independently-phased timer.
    pub last_interaction: f64,
}

/// A two-pane editor split (`PLAN.md` Track 11 / `SPEC.md` §11, which
/// recommends split-in-two before any multi-window work). Both panes draw
/// from the shared `EditorState::open_tabs` pool, so the app-side `parsers`
/// stay index-aligned for whichever pane renders a given tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Split {
    /// Each pane's own active tab, as an index into `open_tabs`: `[left,
    /// right]`. Either can be `None` (that pane showing no tab). Private so
    /// the "`active_tab` mirrors `active[focused]`" invariant can only be
    /// changed through `EditorState`'s own methods.
    active: [Option<usize>; 2],
    /// Which pane (`0` left, `1` right) is focused. `EditorState::active_tab`
    /// is always kept identical to `active[focused]`, so every existing
    /// `active_tab` reader keeps acting on whatever pane the user is in.
    focused: usize,
}

#[derive(Default)]
pub struct EditorState {
    pub project: Option<Project>,
    pub open_tabs: Vec<Document>,
    /// The focused pane's active tab, as an index into `open_tabs`. Kept as
    /// a plain field (not a per-pane vec) so every existing reader is
    /// unchanged: when the editor is split, this always mirrors the focused
    /// pane's own active tab (`Split::active[focused]`); when it isn't, it's
    /// simply the single pane's active tab, exactly as before.
    pub active_tab: Option<usize>,
    /// The editor split (`Track 11`). `None` is single-pane — every
    /// `active_tab` read then behaves exactly as it always has. `Some`
    /// splits the editor area into two side-by-side panes over the same
    /// `open_tabs` pool; the side panel and terminal stay shared chrome
    /// (`SPEC.md` §11 non-goal), so only the editor's own focus is per-pane.
    /// Public only so the pub `EditorState` stays constructible by struct
    /// literal across crates (its tests do `EditorState { .., ..default() }`);
    /// `Split`'s own fields are private, so the mirror invariant can still
    /// only be touched through this type's methods — prefer `is_split`/
    /// `split_editor`/`focus_pane`/`unsplit` over reading it directly.
    pub split: Option<Split>,
    /// Recently closed tabs, most-recently-closed last. `close_tab` pushes
    /// onto this (capped at `MAX_CLOSED_TABS`, dropping the oldest);
    /// `reopen_last_closed_tab` pops off it.
    pub closed_tabs: Vec<Document>,
    /// The terminal panel's own sessions — entirely independent of
    /// `open_tabs`/`active_tab` (see `TerminalTab`'s own doc comment).
    pub terminal_tabs: Vec<TerminalTab>,
    /// Which `terminal_tabs` entry the panel's own small tab strip has
    /// focused, if any.
    pub active_terminal: Option<usize>,
}

/// Re-points one pane's active-tab index after `open_tabs[removed]` is
/// removed (`new_len` is the length *after* removal): the closed slot itself
/// falls to its clamped neighbor, anything past it shifts down one, anything
/// before it is untouched, and an emptied pool clears to `None`. Shared by
/// `close_tab` across every pane so the shift rule lives in exactly one place.
fn shift_active_after_close(active: Option<usize>, removed: usize, new_len: usize) -> Option<usize> {
    match active {
        None => None,
        Some(_) if new_len == 0 => None,
        Some(a) if a == removed => Some(removed.min(new_len - 1)),
        Some(a) if a > removed => Some(a - 1),
        Some(a) => Some(a),
    }
}

impl EditorState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_project(&mut self, root: PathBuf) -> std::io::Result<()> {
        self.project = Some(Project::open(root)?);
        // Reopening a tab closed in a project you've since navigated away
        // from would be a confusing "resurrection", not a useful undo.
        self.closed_tabs.clear();
        Ok(())
    }

    /// Swaps in a freshly-walked tree for the project that's already open,
    /// without the rest of `open_project`'s semantics (clearing reopenable
    /// closed tabs, in particular — a file appearing on disk is not the
    /// user navigating to a different project). A no-op if `refreshed`
    /// isn't the currently-open project's own root, which is what makes a
    /// slow background walk safe: the user may have opened a different
    /// project while it was running, and its result must not clobber that.
    pub fn refresh_project_tree(&mut self, refreshed: Project) {
        if self.project.as_ref().is_some_and(|open| open.root == refreshed.root) {
            self.project = Some(refreshed);
        }
    }

    pub fn find_tab(&self, path: &Path) -> Option<usize> {
        self.open_tabs.iter().position(|doc| doc.path() == path)
    }

    /// Sets the *focused* pane's active tab, keeping `active_tab` and (when
    /// split) `Split::active[focused]` in lockstep — the one place either is
    /// written, so the "`active_tab` mirrors the focused pane" invariant
    /// can't drift.
    fn set_focused_active(&mut self, tab: Option<usize>) {
        self.active_tab = tab;
        if let Some(split) = &mut self.split {
            split.active[split.focused] = tab;
        }
    }

    /// Whether the editor is currently split into two panes.
    pub fn is_split(&self) -> bool {
        self.split.is_some()
    }

    /// The focused pane's index (`0` left, `1` right); always `0` when not split.
    pub fn focused_pane(&self) -> usize {
        self.split.as_ref().map_or(0, |split| split.focused)
    }

    /// The active-tab index (into `open_tabs`) for pane `pane`. Pane `0` is
    /// the only pane when unsplit and returns `active_tab`; pane `1` exists
    /// only while split.
    pub fn pane_active(&self, pane: usize) -> Option<usize> {
        match &self.split {
            Some(split) => split.active.get(pane).copied().flatten(),
            None => (pane == 0).then_some(self.active_tab).flatten(),
        }
    }

    /// Splits the editor in two: the new right pane opens on the same tab the
    /// current pane shows, and takes focus (matching how every editor's own
    /// "Split Right" lands the cursor in the new pane). A no-op if already
    /// split.
    pub fn split_editor(&mut self) {
        if self.split.is_none() {
            self.split = Some(Split {
                active: [self.active_tab, self.active_tab],
                focused: 1,
            });
        }
    }

    /// Collapses back to a single pane, keeping the focused pane's active tab
    /// as the surviving one (`active_tab` already mirrors it). A no-op when
    /// not split.
    pub fn unsplit(&mut self) {
        self.split = None;
    }

    /// Focuses pane `pane` (`0` or `1`), making its active tab the app-wide
    /// `active_tab`. A no-op when not split or `pane` is out of range.
    pub fn focus_pane(&mut self, pane: usize) {
        if let Some(split) = &mut self.split
            && pane < split.active.len()
        {
            split.focused = pane;
            self.active_tab = split.active[pane];
        }
    }

    /// Opens `path` in a new tab, or focuses its existing tab if already open.
    /// Either way it becomes the *focused pane's* active tab.
    pub fn open_tab(&mut self, path: PathBuf) -> Result<usize, OpenDocumentError> {
        if let Some(index) = self.find_tab(&path) {
            self.set_focused_active(Some(index));
            return Ok(index);
        }

        let mut document = Document::open(path)?;
        document.project_root = self.project.as_ref().map(|project| project.root.clone());
        self.open_tabs.push(document);
        let index = self.open_tabs.len() - 1;
        self.set_focused_active(Some(index));
        Ok(index)
    }

    pub fn focus_tab(&mut self, index: usize) {
        if index < self.open_tabs.len() {
            self.set_focused_active(Some(index));
        }
    }

    /// Removes the tab at `index`, pushing it onto `closed_tabs` (so
    /// `reopen_last_closed_tab` can restore it later), and moves each pane's
    /// active tab to a sensible neighbor if the closed tab was its active
    /// one. Every pane's index (not just the focused one's) is adjusted for
    /// the removed slot, since `open_tabs` is a shared pool: a tab the
    /// *other* pane was showing must still point at the right document, or
    /// go to a neighbor if it was the one closed.
    pub fn close_tab(&mut self, index: usize) {
        let document = self.open_tabs.remove(index);
        let new_len = self.open_tabs.len();

        self.active_tab = shift_active_after_close(self.active_tab, index, new_len);
        if let Some(split) = &mut self.split {
            for active in &mut split.active {
                *active = shift_active_after_close(*active, index, new_len);
            }
            // Keep the mirror invariant intact after the shift.
            self.active_tab = split.active[split.focused];
        }

        self.closed_tabs.push(document);
        if self.closed_tabs.len() > MAX_CLOSED_TABS {
            self.closed_tabs.remove(0); // drop the oldest
        }
    }

    /// Pops the most recently closed tab back onto `open_tabs` and focuses
    /// it. If a tab for that path is already open (e.g. the user reopened it
    /// manually since closing it), just focuses that tab instead of creating
    /// a duplicate. Returns the newly active tab's index, or `None` if
    /// there's nothing left to reopen.
    pub fn reopen_last_closed_tab(&mut self) -> Option<usize> {
        let document = self.closed_tabs.pop()?;

        if let Some(index) = self.find_tab(document.path()) {
            self.set_focused_active(Some(index));
            return Some(index);
        }

        self.open_tabs.push(document);
        let index = self.open_tabs.len() - 1;
        self.set_focused_active(Some(index));
        Some(index)
    }

    /// Pushes a new terminal session and focuses it in the panel's own tab
    /// strip. Returns its `terminal_tabs` index. The caller (`app.rs`'s own
    /// `new_terminal_session`) is responsible for spawning the matching
    /// `PtySession` and keeping the two index-aligned.
    pub fn new_terminal_tab(&mut self) -> usize {
        let index = self.terminal_tabs.len();
        self.terminal_tabs.push(TerminalTab {
            title: format!("Terminal {}", index + 1),
            last_interaction: 0.0,
        });
        self.active_terminal = Some(index);
        index
    }

    /// Removes the terminal session at `index` and moves `active_terminal`
    /// to a sensible neighbor if it was active — same "next, or the
    /// previous one if it was last" shape `close_tab` already uses for file
    /// tabs. Still a no-op beyond that for now: no process to kill until
    /// Phase 6 wires one up. Never touches `closed_tabs`: a closed terminal
    /// session has nothing meaningful to reopen (`SPEC.md` §8's own
    /// non-goal).
    pub fn close_terminal_tab(&mut self, index: usize) {
        self.terminal_tabs.remove(index);

        self.active_terminal = match self.active_terminal {
            None => None,
            Some(_) if self.terminal_tabs.is_empty() => None,
            Some(active) if active == index => Some(index.min(self.terminal_tabs.len() - 1)),
            Some(active) if active > index => Some(active - 1),
            Some(active) => Some(active),
        };
    }
}

#[cfg(test)]
#[path = "editor_state_test.rs"]
mod editor_state_test;
