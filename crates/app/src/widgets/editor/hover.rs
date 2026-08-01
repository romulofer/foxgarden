//! Hover-docs popup (`PLAN.md` Track 20 Phase 3): a `textDocument/hover`
//! request fired once the pointer rests on the same identifier for a short
//! delay, rendered as a floating tooltip if (and once) a real server
//! replies with something to show. Structurally mirrors `completion`'s own
//! request-in-flight/poll-once-a-frame shape, just triggered by pointer
//! dwell time instead of a keystroke, and rendered read-only (no selection/
//! keyboard navigation — there's nothing to pick from a single hover).

use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use ropey::Rope;

use super::completion::popup_position;
use super::text_area::TextAreaOutput;
use super::text_offset::char_to_byte;
use crate::lsp_client::ResponseError;
use crate::lsp_state::LspState;
use fg_core::Document;

/// How long the pointer must rest on the same identifier before a
/// `textDocument/hover` request actually fires. Matches egui's own default
/// tooltip delay (`style().interaction.tooltip_delay`) so this doesn't read
/// as slower or faster than every other hover already in the app.
const HOVER_DELAY: Duration = Duration::from_millis(500);

struct Tracked {
    doc_path: PathBuf,
    /// The identifier's full char extent (`identifier_span`), not just the
    /// exact char the pointer landed on — anchoring to a single char would
    /// re-arm this on every pixel of sub-character pointer jitter while the
    /// user is very much still looking at the same symbol, which in
    /// practice means the request never gets a full `HOVER_DELAY` of
    /// stillness to actually fire.
    span: Range<usize>,
    since: Instant,
    fired: bool,
    pending: Option<Receiver<Result<serde_json::Value, ResponseError>>>,
    content: Option<String>,
}

/// One slot for whichever tab is currently focused — same "one field, not
/// one per open tab" shape `completion`'s own single active-popup slot
/// already uses, for the same reason: only the focused tab's editor is ever
/// shown, so there's never more than one hover to track.
#[derive(Default)]
pub struct HoverState {
    tracked: Option<Tracked>,
}

impl HoverState {
    /// Called once a frame with the pointer's current char offset over
    /// `doc`'s text (`None` when the pointer isn't hovering the text area
    /// at all this frame, e.g. it's elsewhere in the UI or a drag is in
    /// progress) — advances the dwell timer, fires a request once
    /// `HOVER_DELAY` elapses, and polls whatever request is already in
    /// flight.
    pub(super) fn update(&mut self, doc: &mut Document, pointer_char_offset: Option<usize>, lsp: &mut LspState) {
        let Some(pointer_char_offset) = pointer_char_offset else {
            self.tracked = None;
            return;
        };
        let span = identifier_span(&doc.buffer, pointer_char_offset);
        let stays = self.tracked.as_ref().is_some_and(|t| t.doc_path == doc.path && t.span == span);
        if !stays {
            self.tracked = Some(Tracked {
                doc_path: doc.path.clone(),
                span: span.clone(),
                since: Instant::now(),
                fired: false,
                pending: None,
                content: None,
            });
        }
        let tracked = self.tracked.as_mut().expect("just set above if absent");
        if !tracked.fired && tracked.since.elapsed() >= HOVER_DELAY {
            tracked.fired = true;
            let text = doc.buffer.to_string();
            let byte_offset = char_to_byte(&text, span.start);
            tracked.pending = lsp.request_hover(doc, byte_offset);
            eprintln!(
                "[hover-debug] fired at span {:?} byte {} -> request sent: {}",
                span,
                byte_offset,
                tracked.pending.is_some()
            );
        }
        if let Some(rx) = &tracked.pending {
            match rx.try_recv() {
                Ok(Ok(value)) => {
                    eprintln!("[hover-debug] raw response: {value}");
                    tracked.content = hover_text_from_response(value);
                    eprintln!("[hover-debug] decoded content: {:?}", tracked.content);
                    tracked.pending = None;
                }
                Ok(Err(error)) => {
                    eprintln!("[hover-debug] server returned an error: {error:?}");
                    tracked.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    eprintln!("[hover-debug] request channel disconnected with no response");
                    tracked.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    /// Drops whatever's tracked without waiting for a natural "pointer moved
    /// away" — used when something else (the completion popup opening, or
    /// `panels::tabs::show` switching the focused tab) has first claim on
    /// the screen space a hover tooltip would otherwise use, or has made
    /// its tracked position meaningless. `pub`, not `pub(super)`, since
    /// both of those call sites sit outside `widgets::editor`.
    pub fn clear(&mut self) {
        self.tracked = None;
    }

    /// Whether a `textDocument/hover` reply could still land — same
    /// unprompted-background-message concern `LspState::wants_repaint`
    /// already documents for `publishDiagnostics`: a reply arriving between
    /// user input events needs its own repaint request, or it sits unread
    /// until an unrelated one comes along. `pub`: read from `app.rs`'s own
    /// update loop, alongside `LspState::wants_repaint` itself.
    pub fn wants_repaint(&self) -> bool {
        self.tracked.as_ref().is_some_and(|t| t.pending.is_some())
    }

    /// Renders the resolved hover content (if any) as a floating tooltip
    /// anchored just below the hovered identifier, reusing `completion`'s
    /// own caret-relative popup-anchoring math — the same shape of problem
    /// (a popup clamped to stay inside `pane_rect`), just anchored to a
    /// hovered span instead of a completion trigger point.
    pub(super) fn paint(&self, ui: &egui::Ui, id: egui::Id, out: &TextAreaOutput, buffer: &Rope, pane_rect: egui::Rect) {
        let Some(tracked) = &self.tracked else { return };
        let Some(content) = &tracked.content else { return };
        let Some(char_rect) = out.char_rect(buffer, tracked.span.start) else { return };
        let popup_size = ui.ctx().memory(|mem| mem.area_rect(id)).map_or(egui::Vec2::ZERO, |r| r.size());
        let pos = popup_position(char_rect, popup_size, pane_rect);
        egui::Area::new(id)
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(480.0);
                    ui.label(content);
                });
            });
    }
}

/// Extends `char_offset` to the full run of identifier characters
/// (alphanumeric, `_`, `$` — valid in both Java and Kotlin identifiers) it
/// sits inside or touches, so the popup's own anchor and `HoverState`'s
/// dwell tracking both key off "which symbol" rather than "which exact
/// char". A `char_offset` that isn't inside any identifier (whitespace,
/// punctuation) yields an empty span at that exact point — hovering there
/// just never accumulates enough dwell time to fire, since the span (and so
/// `stays`) changes on almost every pointer move across non-identifier text.
fn identifier_span(buffer: &Rope, char_offset: usize) -> Range<usize> {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let len = buffer.len_chars();
    let mut start = char_offset.min(len);
    while start > 0 && is_ident(buffer.char(start - 1)) {
        start -= 1;
    }
    let mut end = char_offset.min(len);
    while end < len && is_ident(buffer.char(end)) {
        end += 1;
    }
    start..end.max(start)
}

/// Decodes a raw `textDocument/hover` response into plain display text.
/// `None` covers both a `null` result (the protocol's own "nothing to show
/// here" — most positions on most files) and a malformed response — either
/// way, no tooltip is the correct degrade, the same "one missing/broken
/// source is never a user-facing error" spirit `completion::from_lsp_
/// response` already established for its own decode step.
fn hover_text_from_response(value: serde_json::Value) -> Option<String> {
    let hover = serde_json::from_value::<Option<lsp_types::Hover>>(value).ok().flatten()?;
    let text = match hover.contents {
        lsp_types::HoverContents::Scalar(marked) => marked_string_text(marked),
        lsp_types::HoverContents::Array(items) => items.into_iter().map(marked_string_text).collect::<Vec<_>>().join("\n\n"),
        lsp_types::HoverContents::Markup(markup) => markup.value,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn marked_string_text(marked: lsp_types::MarkedString) -> String {
    match marked {
        lsp_types::MarkedString::String(text) => text,
        lsp_types::MarkedString::LanguageString(language) => language.value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope(text: &str) -> Rope {
        Rope::from_str(text)
    }

    #[test]
    fn identifier_span_extends_to_both_ends_of_a_word() {
        let r = rope("foo.barBaz(qux)");
        // char 6 is the 'r' in "barBaz" (f=0,o=1,o=2,.=3,b=4,a=5,r=6).
        assert_eq!(identifier_span(&r, 6), 4..10);
    }

    #[test]
    fn identifier_span_at_the_very_start_of_a_word_still_extends_right() {
        let r = rope("foo");
        assert_eq!(identifier_span(&r, 0), 0..3);
    }

    #[test]
    fn identifier_span_touching_the_end_of_a_word_includes_it() {
        let r = rope("foo bar");
        // char 3 is the space right after "foo" — "touches" foo from the
        // right, same convention word_range_at-style helpers elsewhere in
        // this app already use for a boundary position.
        assert_eq!(identifier_span(&r, 3), 0..3);
    }

    #[test]
    fn identifier_span_surrounded_by_punctuation_on_both_sides_is_empty_at_that_point() {
        let r = rope("foo . bar");
        // Char 4 sits right between the space after "foo" and the standalone
        // "." token — unlike a position right against an identifier's own
        // edge (see the "touching" test above, which correctly extends into
        // it), neither side here is an identifier char at all, so this is a
        // genuine empty span.
        let r_text = r.to_string();
        assert_eq!(&r_text[3..5], " .");
        assert_eq!(identifier_span(&r, 4), 4..4);
    }

    #[test]
    fn identifier_span_dollar_sign_counts_as_an_identifier_char() {
        let r = rope("$foo");
        assert_eq!(identifier_span(&r, 2), 0..4);
    }

    #[test]
    fn update_with_no_pointer_position_clears_any_tracked_hover() {
        let (_dir, mut doc) = test_support::temp_document("Foo.java", "class Foo {}");
        let mut state = HoverState::default();
        let mut lsp = LspState::default();
        state.update(&mut doc, Some(0), &mut lsp);
        assert!(state.tracked.is_some());
        state.update(&mut doc, None, &mut lsp);
        assert!(state.tracked.is_none());
    }

    #[test]
    fn update_does_not_fire_a_request_before_the_hover_delay_elapses() {
        let (_dir, mut doc) = test_support::temp_document("Foo.java", "class Foo {}");
        let mut state = HoverState::default();
        let mut lsp = LspState::default();
        // Character 6 sits inside "Foo" — no language server is running (a
        // fresh `LspState::default()`), so `request_hover` itself would
        // return `None` regardless; this only asserts the *timer* gate:
        // `fired` stays false immediately after the first sighting.
        state.update(&mut doc, Some(6), &mut lsp);
        let tracked = state.tracked.as_ref().unwrap();
        assert!(!tracked.fired);
        assert!(tracked.pending.is_none());
    }

    #[test]
    fn update_moving_to_a_different_identifier_resets_the_dwell_timer() {
        let (_dir, mut doc) = test_support::temp_document("Foo.java", "aaa bbb");
        let mut state = HoverState::default();
        let mut lsp = LspState::default();
        state.update(&mut doc, Some(1), &mut lsp);
        let first_since = state.tracked.as_ref().unwrap().since;
        state.update(&mut doc, Some(5), &mut lsp);
        let tracked = state.tracked.as_ref().unwrap();
        assert_eq!(tracked.span, 4..7);
        assert!(!tracked.fired);
        assert!(tracked.since >= first_since);
    }

    #[test]
    fn hover_text_from_response_reads_a_scalar_marked_string() {
        let value = serde_json::json!({ "contents": "plain text doc" });
        assert_eq!(hover_text_from_response(value).as_deref(), Some("plain text doc"));
    }

    #[test]
    fn hover_text_from_response_reads_markup_content() {
        let value = serde_json::json!({ "contents": { "kind": "markdown", "value": "**bold**" } });
        assert_eq!(hover_text_from_response(value).as_deref(), Some("**bold**"));
    }

    #[test]
    fn hover_text_from_response_joins_an_array_of_marked_strings() {
        let value = serde_json::json!({ "contents": ["first", { "language": "java", "value": "int x" }] });
        assert_eq!(hover_text_from_response(value).as_deref(), Some("first\n\nint x"));
    }

    #[test]
    fn hover_text_from_response_a_null_result_is_none() {
        assert!(hover_text_from_response(serde_json::Value::Null).is_none());
    }

    #[test]
    fn hover_text_from_response_blank_content_is_none() {
        let value = serde_json::json!({ "contents": "   " });
        assert!(hover_text_from_response(value).is_none());
    }
}
