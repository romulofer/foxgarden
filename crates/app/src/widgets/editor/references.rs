//! Find references (`PLAN.md` Track 20 Phase 6): Shift+F12 requests
//! `textDocument/references` at the caret and shows every result in a
//! results-list popup — one row per reference, each showing the
//! containing file's own line text so the list is scannable without
//! opening every hit first. Clicking a row jumps there via the same
//! cross-tab `pending_navigation` primitive Ctrl+Click
//! (`goto_definition::GotoDefinitionState`) already uses; this module
//! only tracks the request/decode/list half, handing the actual jump back
//! to `app.rs` through `take_navigation` the same "poll once a frame"
//! shape `goto_definition`'s own `poll` already has.
//!
//! `include_declaration: true` (`lsp_state::LspState::request_references`)
//! means the symbol's own declaration is itself one of the rows — real
//! editors list it too (VS Code, IntelliJ both do), rather than only the
//! call sites.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use fg_core::Document;

use crate::lsp_client::ResponseError;
use crate::lsp_state::{LspState, uri_to_path, utf16_range_to_bytes};

use super::text_area::TextAreaOutput;

/// One resolved reference: where it is, plus its own source line (best-
/// effort — `None` if the containing file couldn't be read, e.g. deleted
/// out from under the server between the reply and this being resolved)
/// so a row reads as more than a bare path.
pub struct ReferenceHit {
    pub path: PathBuf,
    pub byte_offset: usize,
    /// 1-based, for display only.
    pub line: usize,
    pub preview: Option<String>,
}

/// One slot for whichever tab is currently focused — same "one field, not
/// one per open tab" shape `hover::HoverState`/`goto_definition::
/// GotoDefinitionState`/`peek::PeekState` all already use, for the same
/// reason: a find-references request only ever targets the focused tab's
/// own caret.
#[derive(Default)]
pub struct FindReferencesState {
    rx: Option<Receiver<Result<serde_json::Value, ResponseError>>>,
    results: Option<Vec<ReferenceHit>>,
    /// The char offset the request was fired at — this popup's own
    /// anchor position, captured once at request time rather than
    /// re-read from the live caret at paint time (`peek::PeekState`'s
    /// own `anchor_char` doc comment explains why: the caret is free to
    /// move away while a reply is still pending).
    anchor_char: usize,
    /// Set by `paint` when a row is clicked; taken (and cleared) by
    /// `take_navigation` — `app.rs`'s own poll-once-a-frame handoff for
    /// an actual cross-tab jump, since this module has no access to
    /// `pending_navigation`/`open_path` itself. Already a byte offset
    /// (unlike `goto_definition::Target::File`'s UTF-16 `Range`): each
    /// `ReferenceHit` was resolved against its own file's real text back
    /// in `decode_references`, so there's no further conversion left to
    /// do once a row is clicked.
    navigate_to: Option<(PathBuf, usize)>,
}

impl FindReferencesState {
    /// Fires a `textDocument/references` request at `byte_offset`
    /// (`char_offset`'s byte equivalent) in `doc`, replacing whatever was
    /// already tracked — a second Shift+F12 before the first reply lands
    /// means the first one's answer is no longer what's being asked
    /// about.
    pub(super) fn request(&mut self, doc: &mut Document, char_offset: usize, byte_offset: usize, lsp: &mut LspState) {
        self.rx = lsp.request_references(doc, byte_offset);
        self.anchor_char = char_offset;
        self.results = None;
        self.navigate_to = None;
    }

    /// Same unprompted-background-message concern `LspState::
    /// wants_repaint`/`HoverState::wants_repaint` already document.
    pub fn wants_repaint(&self) -> bool {
        self.rx.is_some()
    }

    /// Drops whatever's tracked — used on a tab switch, same reasoning
    /// `HoverState::clear`/`PeekState::clear` already have: a different
    /// tab's buffer is now on screen, whatever this was anchored to is
    /// meaningless.
    pub fn clear(&mut self) {
        self.rx = None;
        self.results = None;
    }

    /// Polls whatever request `request` fired, resolving it into
    /// `results` once it lands. `current_doc` is read directly (no disk
    /// I/O) for a hit in the same file already open in this editor —
    /// same "don't show a stale on-disk copy of the very buffer being
    /// edited" reasoning `peek::PeekState::update` already has; every
    /// other hit reads from disk, an accepted best-effort limitation
    /// matching how every other cross-file LSP path in this app already
    /// degrades.
    pub(super) fn update(&mut self, lsp: &mut LspState, current_doc: &Document) {
        let Some(rx) = &self.rx else { return };
        match rx.try_recv() {
            Ok(Ok(value)) => {
                self.rx = None;
                self.results = Some(decode_references(value, current_doc));
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => self.rx = None,
            Err(TryRecvError::Empty) => {}
        }
        let _ = lsp;
    }

    /// Takes whatever row-click navigation is pending — `app.rs` polls
    /// this once a frame alongside `goto_definition::GotoDefinitionState
    /// ::poll`, and reacts the identical way (`open_path` then
    /// `pending_navigation`).
    pub fn take_navigation(&mut self) -> Option<(PathBuf, usize)> {
        self.navigate_to.take()
    }

    /// Renders the resolved results list (if any) as a floating popup
    /// anchored just below `anchor_char`'s own position, reusing
    /// `completion`'s caret-relative popup-anchoring math the same way
    /// `hover`/`peek` already do. A no-op while nothing is tracked, or
    /// while a request is still in flight (no "loading" placeholder,
    /// matching `hover`/`peek`'s own silent-until-real-content
    /// restraint). Escape or a click outside the popup's own rect closes
    /// it; clicking a row instead sets `navigate_to` and also closes it,
    /// since a find-references popup that stayed open after jumping
    /// would just be in the way of whatever the user meant to look at.
    pub(super) fn paint(&mut self, ui: &egui::Ui, id: egui::Id, out: &TextAreaOutput, buffer: &ropey::Rope, pane_rect: egui::Rect) {
        let Some(results) = &self.results else { return };
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.results = None;
            return;
        }
        let Some(char_rect) = out.char_rect(buffer, self.anchor_char) else {
            self.results = None;
            return;
        };
        let popup_size = ui.ctx().memory(|mem| mem.area_rect(id)).map_or(egui::vec2(1.0, 1.0), |r| r.size());
        let pos = super::completion::popup_position(char_rect, popup_size, pane_rect);

        let mut close_requested = false;
        let mut clicked_target: Option<(PathBuf, usize)> = None;
        let area_response = egui::Area::new(id)
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(640.0);
                    ui.set_max_height(320.0);
                    ui.label(msg_reference_count(results.len()));
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for hit in results {
                            let label = match &hit.preview {
                                Some(preview) => format!("{}:{}  {}", hit.path.display(), hit.line, preview.trim()),
                                None => format!("{}:{}", hit.path.display(), hit.line),
                            };
                            if ui.selectable_label(false, label).clicked() {
                                clicked_target = Some((hit.path.clone(), hit.byte_offset));
                            }
                        }
                    });
                });
            });

        let clicked_outside = ui
            .ctx()
            .input(|i| i.pointer.any_click())
            && ui.ctx().pointer_interact_pos().is_some_and(|pos| !area_response.response.rect.contains(pos));

        if let Some(target) = clicked_target {
            self.navigate_to = Some(target);
            close_requested = true;
        }
        if close_requested || clicked_outside {
            self.results = None;
        }
    }
}

fn msg_reference_count(count: usize) -> String {
    if count == 1 { "1 reference".to_string() } else { format!("{count} references") }
}

/// Decodes a raw `textDocument/references` reply (`Location[] | null`)
/// into displayable `ReferenceHit`s, resolving each one's own source line
/// for the preview text. `current_doc` lets a same-file hit read the live
/// buffer instead of disk (see `update`'s own doc comment); every other
/// hit's file is read once here rather than per-frame. A location this
/// app can't resolve a path for (`uri_to_path` returning `None` — a
/// `jdt://` reference would be the one real case, vanishingly rare for
/// "who calls this", since a JDK type's own internals are never edited or
/// referenced back from) is dropped, same best-effort degrade every
/// other LSP decode path in this app already has.
fn decode_references(value: serde_json::Value, current_doc: &Document) -> Vec<ReferenceHit> {
    let Some(locations) = serde_json::from_value::<Option<Vec<lsp_types::Location>>>(value).ok().flatten() else {
        return Vec::new();
    };
    locations
        .into_iter()
        .filter_map(|location| resolve_hit(location, current_doc))
        .collect()
}

fn resolve_hit(location: lsp_types::Location, current_doc: &Document) -> Option<ReferenceHit> {
    let path = uri_to_path(&location.uri)?;
    let text = if path == current_doc.path { current_doc.buffer.to_string() } else { std::fs::read_to_string(&path).ok()? };
    let byte_offset = utf16_range_to_bytes(&text, location.range)?.start;
    let line = text[..byte_offset].matches('\n').count() + 1;
    let preview = text.lines().nth(line - 1).map(str::to_string);
    Some(ReferenceHit { path, byte_offset, line, preview })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, text: &str) -> Document {
        let (_dir, doc) = test_support::temp_document(path, text);
        doc
    }

    #[test]
    fn decode_references_a_null_result_is_empty() {
        let d = doc("Foo.java", "class Foo {}");
        assert!(decode_references(serde_json::Value::Null, &d).is_empty());
    }

    #[test]
    fn decode_references_reads_the_current_docs_own_live_buffer_for_a_same_file_hit() {
        let d = doc("Foo.java", "class Foo {\n    int x;\n}\n");
        let uri = format!("file://{}", d.path.display());
        let value = serde_json::json!([{
            "uri": uri,
            "range": { "start": { "line": 1, "character": 4 }, "end": { "line": 1, "character": 5 } }
        }]);
        let hits = decode_references(value, &d);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].preview.as_deref(), Some("    int x;"));
    }

    #[test]
    fn decode_references_drops_a_location_whose_file_cant_be_read() {
        let d = doc("Foo.java", "class Foo {}");
        let value = serde_json::json!([{
            "uri": "file:///does/not/exist/Nowhere.java",
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }
        }]);
        assert!(decode_references(value, &d).is_empty());
    }

    #[test]
    fn decode_references_reads_multiple_hits_in_source_order() {
        let d = doc("Foo.java", "class Foo {\n    void a() {}\n    void b() {}\n}\n");
        let uri = format!("file://{}", d.path.display());
        let value = serde_json::json!([
            { "uri": uri, "range": { "start": { "line": 1, "character": 9 }, "end": { "line": 1, "character": 10 } } },
            { "uri": uri, "range": { "start": { "line": 2, "character": 9 }, "end": { "line": 2, "character": 10 } } },
        ]);
        let hits = decode_references(value, &d);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[1].line, 3);
    }

    #[test]
    fn msg_reference_count_singular_vs_plural() {
        assert_eq!(msg_reference_count(1), "1 reference");
        assert_eq!(msg_reference_count(2), "2 references");
        assert_eq!(msg_reference_count(0), "0 references");
    }
}
