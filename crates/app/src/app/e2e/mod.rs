//! End-to-end tests: the whole app, driven the way a user drives it.
//!
//! Unlike [`super::tests`] (which calls individual free functions with
//! hand-built state), everything here goes through a real `FoxGardenApp`
//! rendering real frames via `egui_kittest`, and asserts on what the
//! accessibility tree actually says is on screen — a tab labelled
//! `*Main.java`, a project-tree row labelled `☕ Main.java` — plus what
//! ended up on disk. A regression that leaves the state correct but the
//! UI wrong (or vice versa) fails here even when every unit test passes.
//!
//! Split by user-facing area rather than by which function is under test
//! (there isn't one — every test drives the whole app), following the same
//! "same module, separate file" convention as `widgets::editor::widget::
//! tests` (`AGENTS.md`): `common` holds the harness every topic module
//! below drives the app through.
//!
//! Two things a real user does are deliberately *not* driven through the
//! UI here, because neither can be:
//!
//! - **Opening a project.** "Open Folder…" is an `rfd` native dialog, a
//!   separate OS window outside egui's event loop entirely (`AGENTS.md`),
//!   so tests call `EditorState::open_project` with a temp directory
//!   instead — exactly what the dialog's own callback does with whatever
//!   folder was picked.
//! - **Placing the caret by clicking in the editor.** The editor is a
//!   custom-painted widget, not an `egui::TextEdit`, so it has no
//!   accessibility node to click by label; `type_into_active_tab` uses
//!   `widgets::editor::jump_to` (the same public entry point the Spring
//!   endpoint map's jump-to-handler uses) to focus it and put the caret
//!   somewhere specific. Every keystroke after that is a real event
//!   through the app's real input pipeline.
//!
//! And four areas are deliberately left to unit tests rather than covered
//! here, each because driving it from a frame loop would assert on a race
//! rather than on behavior:
//!
//! - **Anything an external process answers** — LSP (`jdtls`/
//!   `kotlin-language-server`), Checkstyle/PMD, `git status`/`git diff`,
//!   the terminal's shell. `panels` covers the terminal and Source Control
//!   panels only as far as opening and closing them; what fills them
//!   arrives on a background thread whenever it arrives.
//! - **The file watcher's external-change paths** (reload, conflict banner,
//!   externally-deleted) — `notify` delivers on its own thread with no
//!   bound on when, so `super::tests` drives `process_file_events` with
//!   hand-built events instead.
//! - **Auto-save's timers**, for the same reason, plus needing wall-clock
//!   time to pass.
//! - **Session persistence**, which needs an `eframe::Storage` across two
//!   launches; `egui_kittest`'s `CreationContext` has none, so
//!   `super::tests` uses its own `FakeStorage` instead.

mod common;

mod completion;
mod diagnostics;
mod editing;
mod editor_input;
mod file_operations;
mod menus;
mod navigation;
mod panels;
mod project_tree;
mod tabs;
