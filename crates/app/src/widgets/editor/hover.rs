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
    /// `doc.lsp_version` as of the request this content came from. Part of
    /// what identifies "the same hover" alongside `doc_path`/`span`: a
    /// stationary pointer over a buffer that's being *typed* into keeps the
    /// same path and (until the edit reaches it) the same span, so without
    /// this the tooltip would keep painting a resolved answer computed
    /// against text that no longer exists.
    version: i32,
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
    /// Called once a frame with whichever identifier the pointer is
    /// currently resting on (`hovered_span`; `None` when it isn't on one at
    /// all this frame — elsewhere in the UI, over whitespace, past the end
    /// of a line, or mid-drag) — advances the dwell timer, fires a request
    /// once `HOVER_DELAY` elapses, and polls whatever request is already in
    /// flight.
    pub(super) fn update(&mut self, doc: &mut Document, hovered: Option<Range<usize>>, lsp: &mut LspState) {
        let Some(span) = hovered else {
            self.tracked = None;
            return;
        };
        let stays = self
            .tracked
            .as_ref()
            .is_some_and(|t| t.doc_path == doc.path && t.span == span && t.version == doc.lsp_version);
        if !stays {
            self.tracked = Some(Tracked {
                doc_path: doc.path.clone(),
                span: span.clone(),
                version: doc.lsp_version,
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
            tracked.pending = lsp.request_hover(doc, char_to_byte(&text, span.start));
        }
        if let Some(rx) = &tracked.pending {
            match rx.try_recv() {
                Ok(Ok(value)) => {
                    tracked.content = hover_text_from_response(value);
                    tracked.pending = None;
                }
                // A rejected request or a server that exited mid-request
                // both degrade the same way every other best-effort LSP
                // path in this app does: no tooltip, nothing surfaced.
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => tracked.pending = None,
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

    /// Whether there's resolved content being painted right now — read by
    /// `widget.rs` to decide whether the tooltip's own `egui::Area` rect is
    /// meaningful this frame (see its "pointer over the tooltip" guard).
    /// `pub(super)`: that call site is inside `widgets::editor`.
    pub(super) fn has_content(&self) -> bool {
        self.tracked.as_ref().is_some_and(|t| t.content.is_some())
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
    pub(super) fn paint(
        &self,
        ui: &egui::Ui,
        id: egui::Id,
        out: &TextAreaOutput,
        buffer: &Rope,
        pane_rect: egui::Rect,
    ) {
        let Some(tracked) = &self.tracked else { return };
        let Some(content) = &tracked.content else { return };
        let Some(char_rect) = out.char_rect(buffer, tracked.span.start) else {
            return;
        };
        let popup_size = ui
            .ctx()
            .memory(|mem| mem.area_rect(id))
            .map_or(egui::Vec2::ZERO, |r| r.size());
        let pos = popup_position(char_rect, popup_size, pane_rect);
        egui::Area::new(id)
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(480.0);
                    // A real server's hover doc (java.util.Deque's whole
                    // class/method-summary Javadoc, say) can run many
                    // screens tall — cap the tooltip's height and let it
                    // scroll rather than paint off the bottom of the pane
                    // with no way to reach the rest
                    // (bugs/tooltip_formatting_and_lack_of_scrollbar.png).
                    // Half the pane, floored so a short pane still gets a
                    // usable box.
                    let max_height = (pane_rect.height() * 0.5).max(160.0);
                    egui::ScrollArea::vertical().max_height(max_height).show(ui, |ui| {
                        ui.label(content);
                    });
                });
            });
    }
}

/// Which identifier — if any — the pointer at `pointer` is actually
/// resting on. Two gates, both needed, because `char_offset_for_pos`
/// answers "which text position is *nearest* this point", never "is this
/// point on any text at all":
///
/// * an empty `identifier_span` (whitespace, punctuation, a blank line, the
///   indentation to the left of a line's first character) is nothing to
///   look up, so it never arms a request in the first place;
/// * a point past the end of a line resolves to that line's last position,
///   which `identifier_span` then happily extends *leftward* into the final
///   token — so pointing at the blank space to the right of `foo();`, or
///   anywhere in the empty area below the last line, used to pop up docs
///   for a symbol the pointer plainly wasn't on. `pointer_is_on_span`
///   rejects those by measuring the identifier's real painted extent.
pub(super) fn hovered_span(out: &TextAreaOutput, buffer: &Rope, pointer: egui::Pos2) -> Option<Range<usize>> {
    let span = identifier_span(buffer, super::text_area::char_offset_for_pos(out, buffer, pointer));
    if span.is_empty() {
        return None;
    }
    let start = out.char_rect(buffer, span.start)?;
    let end = out.char_rect(buffer, span.end)?;
    pointer_is_on_span(start, end, pointer).then_some(span)
}

/// Whether `pointer` lies on the glyphs between `start` and `end` — the
/// caret rects (`TextAreaOutput::char_rect`, one pixel wide by one row
/// tall) at an identifier's first char and at the position just past its
/// last, so `end.left()` is the identifier's own right edge.
fn pointer_is_on_span(start: egui::Rect, end: egui::Rect, pointer: egui::Pos2) -> bool {
    if pointer.y < start.top() || pointer.y >= start.bottom() || pointer.x < start.left() {
        return false;
    }
    // `end` on a lower row means word wrap split this identifier and the
    // part on *this* row runs to the row's own end — there's no right
    // bound left to test against here.
    end.top() > start.top() || pointer.x < end.left()
}

/// Extends `char_offset` to the full run of identifier characters
/// (alphanumeric, `_`, `$` — valid in both Java and Kotlin identifiers) it
/// sits inside or touches, so the popup's own anchor and `HoverState`'s
/// dwell tracking both key off "which symbol" rather than "which exact
/// char". A `char_offset` that isn't inside any identifier (whitespace,
/// punctuation) yields an empty span at that exact point, which
/// `hovered_span` above treats as "nothing hovered".
pub(super) fn identifier_span(buffer: &Rope, char_offset: usize) -> Range<usize> {
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
    let hover = serde_json::from_value::<Option<lsp_types::Hover>>(value)
        .ok()
        .flatten()?;
    let text = match hover.contents {
        lsp_types::HoverContents::Scalar(marked) => marked_string_text(marked),
        lsp_types::HoverContents::Array(items) => items
            .into_iter()
            .map(marked_string_text)
            .collect::<Vec<_>>()
            .join("\n\n"),
        lsp_types::HoverContents::Markup(markup) => markup.value,
    };
    let text = strip_markdown(&text);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn marked_string_text(marked: lsp_types::MarkedString) -> String {
    match marked {
        lsp_types::MarkedString::String(text) => text,
        lsp_types::MarkedString::LanguageString(language) => language.value,
    }
}

/// Strips the Markdown a real jdtls/`kotlin-language-server` sends
/// regardless of this client's declared `PlainText` preference
/// (TECHNICAL_DEBT.md #22) down to plain text `HoverState::paint`'s
/// `ui.label` can render correctly — a client-side hand-rolled subset
/// rather than a full renderer (`egui_commonmark`, weighed and deferred in
/// that entry against this project's own Zed-class startup/frame-cost bar),
/// scoped tightly to the forms real captured server output actually uses:
/// fenced code blocks, inline code spans, `**bold**`, `>`-quoted blocks, and
/// `[text](url)` links.
fn strip_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            out.push_str(line);
        } else if is_table_row(line) {
            // A separator row (`|---|:--:|`) carries no content — drop it
            // whole, `\n` included, rather than leave a blank line.
            let Some(row) = format_table_row(line) else { continue };
            out.push_str(&row);
        } else {
            out.push_str(&strip_inline_markdown(strip_blockquote_marker(line)));
        }
        out.push('\n');
    }
    out
}

/// Drops a line's leading `>`/`> `/`>>` quote markers (Markdown's own
/// nesting shape) — the surrounding indentation they carried is dropped
/// with them rather than preserved, per TECHNICAL_DEBT.md #22's own
/// "convert to plain indentation" proposal; a quoted block reads fine as
/// left-aligned plain text.
fn strip_blockquote_marker(line: &str) -> &str {
    let mut rest = line.trim_start();
    while let Some(after) = rest.strip_prefix('>') {
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    rest
}

/// Whether `line` is a Markdown table row — its trimmed form begins with a
/// `|` cell delimiter. A real jdtls hover for `java.util.Deque` (and Queue,
/// List, …) renders its method-summary as a Markdown table; left unhandled,
/// the raw `| ... | ... |` pipe scaffolding lands in the tooltip verbatim
/// (bugs/tooltip_formatting_and_lack_of_scrollbar.png).
fn is_table_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// A table row reduced to plain text: cells split on `|`, each stripped of
/// its own inline Markdown, empty (padding) cells dropped, the rest rejoined
/// with a plain gap. `None` for a separator row (`|---|:--:|`) — pure table
/// scaffolding with nothing to show, dropped by the caller.
fn format_table_row(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let cells: Vec<String> = inner.split('|').map(|c| strip_inline_markdown(c.trim())).collect();
    let is_separator = cells
        .iter()
        .all(|c| !c.trim().is_empty() && c.trim().chars().all(|ch| ch == '-' || ch == ':'));
    if is_separator {
        return None;
    }
    Some(
        cells
            .into_iter()
            .filter(|c| !c.trim().is_empty())
            .collect::<Vec<_>>()
            .join("   "),
    )
}

/// One line's worth of inline Markdown noise removed: `[text](url)` links
/// collapse to just `text` (the target, often several-hundred-char `jdt://`
/// URLs, is never useful in a hover tooltip), `**bold**`/`*italic*` markers
/// and inline `` `code` `` backticks are dropped outright rather than
/// represented some other way — this is a plain-text tooltip, not a themed
/// rich-text one. `**` is stripped before the lone-`*` pass below so a bold
/// marker doesn't survive as two leftover single asterisks; a leading `* `
/// bullet marker falls to the same lone-`*` pass, degrading to plain
/// (slightly indented) text rather than a rendered bullet.
fn strip_inline_markdown(line: &str) -> String {
    strip_links(line).replace("**", "").replace(['*', '`'], "")
}

fn strip_links(mut s: &str) -> String {
    let mut out = String::new();
    loop {
        let Some(open) = s.find('[') else {
            out.push_str(s);
            return out;
        };
        let Some(close_rel) = s[open + 1..].find(']') else {
            out.push_str(s);
            return out;
        };
        let close = open + 1 + close_rel;
        let after_close = &s[close + 1..];
        if !after_close.starts_with('(') {
            out.push_str(&s[..=close]);
            s = after_close;
            continue;
        }
        let Some(paren_close_rel) = after_close[1..].find(')') else {
            out.push_str(&s[..=close]);
            s = after_close;
            continue;
        };
        out.push_str(&s[..open]);
        out.push_str(&s[open + 1..close]);
        s = &after_close[1 + paren_close_rel + 1..];
    }
}

#[cfg(test)]
#[path = "hover_test.rs"]
mod hover_test;
