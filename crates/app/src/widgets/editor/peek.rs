//! Peek definition (`PLAN.md` Track 17 Phase 1): Alt+F12 requests
//! `textDocument/definition` at the current caret and renders the resolved
//! definition's surrounding lines in an inline, read-only popup — without
//! switching tabs or touching `pending_navigation`, unlike Ctrl+Click
//! (`widget.rs`'s own wiring around `crate::goto_definition::
//! GotoDefinitionState`, `PLAN.md` Track 20 Phase 4, this module's hard
//! dependency). The request/reply plumbing is identical to that jump — this
//! module reuses `GotoDefinitionState` outright — only what happens once a
//! `Target` resolves differs: a line window gets painted here instead of
//! `open_path` and a real cross-tab jump running.

use std::path::PathBuf;

use fg_core::Document;

use crate::goto_definition::{GotoDefinitionState, Target};
use crate::lsp_state::{LspState, utf16_range_to_bytes};

use super::text_area::TextAreaOutput;

/// How many lines of context sit on each side of the defining line —
/// separately for the collapsed (default) and expanded views, so toggling
/// never needs a second round-trip: `resolve_panel` always captures the
/// wider `EXPANDED` radius up front, and `visible_window` below just slices
/// a narrower `COLLAPSED` sub-window out of it when not expanded.
const COLLAPSED_CONTEXT_LINES: usize = 3;
const EXPANDED_CONTEXT_LINES: usize = 12;

/// A resolved peek target's displayable content: `EXPANDED_CONTEXT_LINES`
/// worth of context on each side of the definition, captured once so
/// `expanded` can toggle the *visible* window instantly with no new
/// request.
struct PanelContent {
    path: PathBuf,
    /// 1-based line number of `lines[0]`.
    first_line: usize,
    /// 1-based; always within `[first_line, first_line + lines.len())` —
    /// the line the symbol is actually defined on, painted with a
    /// highlighted background so the window doesn't just read as an
    /// arbitrary slice of source.
    target_line: usize,
    lines: Vec<String>,
}

/// One slot for whichever tab is currently focused — same "one field, not
/// one per open tab" shape `hover::HoverState`'s own single tracked slot
/// already uses, for the same reason: only the focused tab's editor is ever
/// shown, so there's never more than one peek panel open at once.
#[derive(Default)]
pub struct PeekState {
    goto: GotoDefinitionState,
    panel: Option<PanelContent>,
    /// The char offset the still-in-flight (or just-resolved) request was
    /// fired at — the popup's own anchor position (`char_rect` below),
    /// captured once at request time rather than re-read from the live
    /// caret at paint time, since the caret is free to move away (or the
    /// selection to change) while a reply is still pending without the
    /// popup's eventual anchor following it around.
    anchor_char: usize,
    expanded: bool,
}

impl PeekState {
    /// Fires a `textDocument/definition` request at `byte_offset`
    /// (`char_offset`'s byte equivalent) in `doc`, replacing whatever was
    /// already tracked — a second Alt+F12 before the first reply lands
    /// means the first one's answer is no longer what's being asked about.
    pub(super) fn request(&mut self, doc: &mut Document, char_offset: usize, byte_offset: usize, lsp: &mut LspState) {
        self.goto.request(doc, byte_offset, lsp);
        self.anchor_char = char_offset;
        self.panel = None;
        self.expanded = false;
    }

    /// Same unprompted-background-message concern `LspState::wants_repaint`/
    /// `HoverState::wants_repaint` already document. `pub`, not
    /// `pub(super)`: read from `app.rs`'s own update loop, alongside those
    /// two.
    pub fn wants_repaint(&self) -> bool {
        self.goto.wants_repaint()
    }

    /// Drops whatever's tracked — used on a tab switch, the same "a
    /// different tab's buffer is now on screen, whatever this was anchored
    /// to is meaningless" reasoning `HoverState::clear`'s own call site
    /// already has.
    pub fn clear(&mut self) {
        self.goto = GotoDefinitionState::default();
        self.panel = None;
    }

    /// Polls whatever request `request` fired, resolving it into `panel`'s
    /// displayable line window once it lands. `current_doc` is read
    /// directly (no disk I/O) when the target is the same file already open
    /// in this very editor — the common "peek a local symbol" case — so an
    /// unsaved edit shows up immediately instead of a stale on-disk copy.
    /// Any other target reads from disk: an unsaved edit in a *different*
    /// open tab can then show slightly stale content, an accepted best-
    /// effort limitation rather than threading every open document through
    /// just for this, matching how every other cross-file LSP path in this
    /// app already degrades on a source it doesn't have a live buffer for.
    pub(super) fn update(&mut self, lsp: &mut LspState, current_doc: &Document) {
        if let Some(target) = self.goto.poll(lsp) {
            self.panel = resolve_panel(target, current_doc);
        }
    }

    /// Renders the resolved panel (if any) as a floating, read-only popup
    /// anchored just below `anchor_char`'s own position — reusing
    /// `completion`'s caret-relative popup-anchoring math, the same shape
    /// `hover::HoverState::paint` already reuses it for. A no-op while
    /// nothing is tracked, or while a request is still in flight (nothing
    /// to show yet — no "loading" placeholder, same "silent until there's
    /// something real" restraint `hover` already has). Escape or a click
    /// outside the popup's own rect closes it.
    pub(super) fn paint(
        &mut self,
        ui: &egui::Ui,
        id: egui::Id,
        out: &TextAreaOutput,
        buffer: &ropey::Rope,
        pane_rect: egui::Rect,
    ) {
        let Some(panel) = &self.panel else { return };
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.panel = None;
            return;
        }
        let Some(char_rect) = out.char_rect(buffer, self.anchor_char) else {
            self.panel = None;
            return;
        };
        let (window_first_line, window_lines) = visible_window(panel, self.expanded);
        let popup_size = ui
            .ctx()
            .memory(|mem| mem.area_rect(id))
            .map_or(egui::vec2(1.0, 1.0), |r| r.size());
        let pos = super::completion::popup_position(char_rect, popup_size, pane_rect);

        let mut close_requested = false;
        let mut toggle_requested = false;
        let area_response = egui::Area::new(id)
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(640.0);
                    ui.horizontal(|ui| {
                        ui.label(format!("{}:{}", panel.path.display(), panel.target_line));
                        if ui.small_button(if self.expanded { "-" } else { "+" }).clicked() {
                            toggle_requested = true;
                        }
                        if ui.small_button("x").clicked() {
                            close_requested = true;
                        }
                    });
                    ui.separator();
                    for (offset, line) in window_lines.iter().enumerate() {
                        let line_number = window_first_line + offset;
                        let text = format!("{line_number:>5} | {line}");
                        if line_number == panel.target_line {
                            egui::Frame::new().fill(ui.visuals().selection.bg_fill).show(ui, |ui| {
                                ui.monospace(text);
                            });
                        } else {
                            ui.monospace(text);
                        }
                    }
                });
            });

        let clicked_outside = ui.ctx().input(|i| i.pointer.any_click())
            && ui
                .ctx()
                .pointer_interact_pos()
                .is_some_and(|pos| !area_response.response.rect.contains(pos));

        if toggle_requested {
            self.expanded = !self.expanded;
        }
        if close_requested || clicked_outside {
            self.panel = None;
        }
    }
}

/// Slices `panel`'s own stored `EXPANDED_CONTEXT_LINES`-wide window down to
/// whichever radius (`COLLAPSED_CONTEXT_LINES`/`EXPANDED_CONTEXT_LINES`)
/// `expanded` calls for, centered on `target_line`. Returns the slice's own
/// first line number alongside it, since callers need real line numbers to
/// paint the gutter-style prefix, not just an index into `panel.lines`.
fn visible_window(panel: &PanelContent, expanded: bool) -> (usize, &[String]) {
    let radius = if expanded {
        EXPANDED_CONTEXT_LINES
    } else {
        COLLAPSED_CONTEXT_LINES
    };
    let target_index = panel.target_line - panel.first_line;
    let start = target_index.saturating_sub(radius);
    let end = (target_index + radius + 1).min(panel.lines.len());
    (panel.first_line + start, &panel.lines[start..end])
}

/// Resolves a `goto_definition::Target` into `PanelContent` — reading
/// `current_doc`'s own live buffer for a same-file target, or the target
/// path straight off disk otherwise (a `Target::Ready` decompiled-source
/// cache file always exists by the time this runs: `goto_definition`
/// itself only ever hands one back after synchronously writing it).
/// `None` on a read failure, dropped the same silent best-effort way every
/// other LSP path here already handles one.
fn resolve_panel(target: Target, current_doc: &Document) -> Option<PanelContent> {
    match target {
        Target::File { path, range } => {
            let text = if path == current_doc.path {
                current_doc.buffer.to_string()
            } else {
                std::fs::read_to_string(&path).ok()?
            };
            let byte_offset = utf16_range_to_bytes(&text, range)?.start;
            Some(panel_from_text(path, &text, byte_offset))
        }
        Target::Ready { path, byte_offset } => {
            let text = std::fs::read_to_string(&path).ok()?;
            Some(panel_from_text(path, &text, byte_offset.min(text.len())))
        }
    }
}

/// Builds `PanelContent` around `byte_offset` in `text` — `byte_offset`
/// must be a real char boundary in `text` (both callers above only ever
/// pass one resolved against this exact string), so slicing `text[
/// ..byte_offset]` to count preceding newlines never panics.
fn panel_from_text(path: PathBuf, text: &str, byte_offset: usize) -> PanelContent {
    let target_index = text[..byte_offset].matches('\n').count();
    let all_lines: Vec<&str> = text.lines().collect();
    let start = target_index.saturating_sub(EXPANDED_CONTEXT_LINES);
    let end = (target_index + EXPANDED_CONTEXT_LINES + 1).min(all_lines.len());
    let lines = all_lines[start..end].iter().map(|line| (*line).to_string()).collect();
    PanelContent {
        path,
        first_line: start + 1,
        target_line: target_index + 1,
        lines,
    }
}

#[cfg(test)]
#[path = "peek_test.rs"]
mod peek_test;
