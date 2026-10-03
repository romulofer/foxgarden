//! The completion popup's state, filtering, rendering, and insertion —
//! shared by every trigger path (word-completion, dot-completion) so they
//! end at one popup instead of drifting into subtly different UIs.

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, TryRecvError};

use ropey::Rope;

use super::templates::CURSOR_MARKER;
use super::text_area::TextAreaOutput;
use super::text_offset::{byte_to_char, char_to_byte};
use crate::lsp_client::ResponseError;

/// What kind of thing a `CompletionItem` represents — drives both its icon
/// (once rendered) and how `insert_completion` (`SPEC.md` §5) applies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Word,
    Field,
    Method,
    Keyword,
    Template,
    /// A Spring config property key (`application.properties`/`.yml` —
    /// `PLAN.md` Track 12), sourced from a scanned dependency jar's own
    /// bundled `spring-configuration-metadata.json` rather than anything
    /// parsed from the open project's own source.
    Property,
    /// A Spring framework annotation (`@Component`, `@Autowired`, ...) —
    /// accepting one also inserts a matching `import`, unlike every other
    /// kind here (see `widget.rs`'s accept handler).
    Annotation,
}

/// One candidate in the popup's list.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    /// What's shown and what gets inserted.
    pub label: String,
    pub kind: CompletionKind,
    /// e.g. `"int"` for a field, `"(String) -> void"` for a method.
    pub detail: Option<String>,
    /// `Method` only: whether it takes at least one parameter —
    /// `insert_completion` places the cursor between `()` if so, right
    /// after otherwise. Ignored for every other kind.
    pub has_params: bool,
}

/// Per-tab completion-popup state — `None` (via the caller's
/// `Option<CompletionState>`) means closed, same convention
/// `OverrideMethodDialog`/`GenerateMethodDialog` already use.
pub struct CompletionState {
    /// Byte offset of the trigger point (right after the `.`, or right
    /// after the word-start for a bare word-completion) — candidates are
    /// filtered against the buffer slice from here to the current cursor.
    anchor_byte: usize,
    candidates: Vec<CompletionItem>,
    selected: usize,
    /// A `textDocument/completion` request in flight (`PLAN.md` Track 20
    /// Phase 5), attached by the dot-completion trigger right after
    /// `open`/`set_pending_lsp`. `None` once its response has been merged
    /// in (or it failed) — polled once a frame by `poll_lsp`, same
    /// non-blocking `try_recv` shape every other background op in this
    /// app already uses.
    pending_lsp: Option<Receiver<Result<serde_json::Value, ResponseError>>>,
}

/// Where the popup's `egui::Area` should anchor: one row below
/// `char_rect` (the trigger char's own rect, from `TextAreaOutput::
/// char_rect`), clamped so the popup never runs off `pane_rect`'s
/// right/bottom edge. `go_to_file.rs`/`quick_switcher.rs` never had to
/// solve this — both are centered `egui::Modal`s, not caret-anchored
/// popups — so this is genuinely new rather than a mirrored pattern.
pub(super) fn popup_position(char_rect: egui::Rect, popup_size: egui::Vec2, pane_rect: egui::Rect) -> egui::Pos2 {
    // `.max(pane_rect.left()/.top())` first: if the popup is wider/taller
    // than the pane itself, staying flush with the near edge wins over
    // fitting the far edge, same priority a clamp for an oversized element
    // always has to pick.
    let x_max = (pane_rect.right() - popup_size.x).max(pane_rect.left());
    let y_max = (pane_rect.bottom() - popup_size.y).max(pane_rect.top());
    let x = char_rect.left().max(pane_rect.left()).min(x_max);
    let y = char_rect.bottom().max(pane_rect.top()).min(y_max);
    egui::pos2(x, y)
}

impl CompletionState {
    /// Opens a popup anchored at `anchor_byte`, holding `candidates`
    /// unfiltered until the first `visible` call narrows them against
    /// whatever's typed after the anchor. `selected` starts on the first
    /// row.
    pub(super) fn open(anchor_byte: usize, candidates: Vec<CompletionItem>) -> Self {
        Self {
            anchor_byte,
            candidates,
            selected: 0,
            pending_lsp: None,
        }
    }

    /// Attaches an in-flight `textDocument/completion` request — its
    /// result gets merged into `candidates` once `poll_lsp` sees it land.
    pub(super) fn set_pending_lsp(&mut self, rx: Receiver<Result<serde_json::Value, ResponseError>>) {
        self.pending_lsp = Some(rx);
    }

    /// Non-blocking poll of `pending_lsp`, called once a frame. A real
    /// response gets decoded and merged into `candidates` (deduped by
    /// exact label against what's already there — `SPEC.md` §20's own "no
    /// visible duplication" ask). A request error or a disconnected
    /// channel (the server exited mid-request) just clears `pending_lsp`
    /// silently — this is a best-effort second source; whatever local
    /// candidates already opened the popup with (possibly none) are a
    /// perfectly fine result on their own, so nothing here is surfaced as
    /// a user-facing error.
    pub(super) fn poll_lsp(&mut self) {
        let Some(rx) = &self.pending_lsp else { return };
        match rx.try_recv() {
            Ok(Ok(value)) => {
                self.merge_candidates(from_lsp_response(value));
                self.pending_lsp = None;
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => self.pending_lsp = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    /// Whether an LSP response could still land and populate `candidates`
    /// — the caller's own "close the popup if there's nothing to show"
    /// check must not treat *currently* empty the same as *permanently*
    /// empty while this is true.
    pub(super) fn has_pending_lsp(&self) -> bool {
        self.pending_lsp.is_some()
    }

    fn merge_candidates(&mut self, new_items: Vec<CompletionItem>) {
        // Owned, not borrowed: `self.candidates.extend(...)` below can
        // reallocate the very `Vec` a borrowed `&str` set would still be
        // pointing into mid-iteration.
        let existing: HashSet<String> = self.candidates.iter().map(|c| c.label.clone()).collect();
        self.candidates
            .extend(new_items.into_iter().filter(|item| !existing.contains(&item.label)));
    }

    pub(super) fn anchor_byte(&self) -> usize {
        self.anchor_byte
    }

    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    /// This frame's candidate list: `self.candidates` narrowed and ordered
    /// by `filter_and_rank` (`SPEC.md` §2) against the buffer slice from
    /// `anchor_byte` to `cursor_byte`. Recomputed fresh on every call
    /// rather than cached — a single already-open buffer's pass over its
    /// own candidate list is cheap, the same "only runs while the popup is
    /// open" reasoning `SPEC.md` §0 gives for not caching this across
    /// keystrokes.
    pub(super) fn visible<'a>(&'a self, text: &str, cursor_byte: usize) -> Vec<&'a CompletionItem> {
        let prefix = text.get(self.anchor_byte..cursor_byte).unwrap_or_default();
        filter_and_rank(&self.candidates, prefix)
    }

    /// Moves `selected` by `delta` (positive = down), clamped to
    /// `[0, visible_len - 1]` — same `saturating_sub`/`min(len - 1)` clamp
    /// shape `go_to_file.rs:134-139` already uses for its own list
    /// navigation. `visible_len` is the caller's current `visible(...)
    /// .len()` — passed in rather than recomputed here so a single frame's
    /// arrow-key handling and its subsequent `paint` agree on the same
    /// filtered length without filtering twice.
    pub(super) fn move_selection(&mut self, delta: isize, visible_len: usize) {
        if visible_len == 0 {
            self.selected = 0;
            return;
        }
        let current = self.selected.min(visible_len - 1) as isize;
        self.selected = (current + delta).clamp(0, visible_len as isize - 1) as usize;
    }

    /// Renders the popup's current `visible(text, cursor_byte)` list,
    /// positioned by `popup_position`, with `selected` shown as a
    /// highlighted row. The first frame a popup opens, `id`'s Area has no
    /// prior measured size to clamp against yet and renders at the
    /// unclamped position, self-correcting one frame later once
    /// `ctx.memory().area_rect` has something to read — the same
    /// one-frame-lag cost class `AGENTS.md` already accepts for
    /// highlighting/reparse.
    #[expect(
        clippy::too_many_arguments,
        reason = "each parameter is independently threaded per-frame render state (the id, the render target, the buffer in both its rope/str forms, the cursor, the pane bound), not a bundle waiting to be a struct — same shape and reasoning as widget::show's own allowance"
    )]
    pub(super) fn paint(
        &self,
        ui: &egui::Ui,
        id: egui::Id,
        out: &TextAreaOutput,
        buffer: &Rope,
        text: &str,
        cursor_byte: usize,
        pane_rect: egui::Rect,
    ) {
        let anchor_char = byte_to_char(text, self.anchor_byte);
        let Some(char_rect) = out.char_rect(buffer, anchor_char) else {
            return;
        };
        let popup_size = ui
            .ctx()
            .memory(|mem| mem.area_rect(id))
            .map_or(egui::Vec2::ZERO, |r| r.size());
        let pos = popup_position(char_rect, popup_size, pane_rect);
        let visible = self.visible(text, cursor_byte);
        let selected = self.selected.min(visible.len().saturating_sub(1));

        egui::Area::new(id)
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    // Without this, a row's label wraps against whatever width
                    // happens to be left between `pos` and the *screen's* edge
                    // (an `Area`'s default layout width, not `pane_rect` —
                    // `popup_position`'s own clamp only ever adjusts `pos`, it
                    // can't widen the room left after it), which shrinks to
                    // near zero the closer the caret sits to the pane's right
                    // edge and wraps a label like "protected" one character per
                    // line instead of showing it on one row. `Extend` makes a
                    // row grow the `Area`/`Frame` to fit instead of wrapping —
                    // same effect `go_to_file.rs`/`quick_switcher.rs` get via
                    // an explicit `set_min_width`, just content-sized here
                    // rather than a fixed modal width.
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    for (i, item) in visible.iter().enumerate() {
                        // Click-to-accept lands with the rest of §0's mouse
                        // handling; for now the row just renders highlighted.
                        let _ = ui.selectable_label(i == selected, row_text(item, ui));
                    }
                });
            });
    }
}

/// One popup row's own text: `item.label`, plus `item.detail` (if any) in
/// the theme's dimmed/weak color right after it — e.g. a Spring config
/// property's own `"java.lang.Integer = 8080"`. `weak_text_color` rather
/// than threading a `dark_mode` bool into `paint` just for this: egui's own
/// theme-aware "de-emphasized but still legible" color already exists for
/// exactly this, no new parameter needed.
fn row_text(item: &CompletionItem, ui: &egui::Ui) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &item.label,
        0.0,
        egui::TextFormat::simple(egui::FontId::default(), ui.visuals().text_color()),
    );
    if let Some(detail) = &item.detail {
        job.append(
            &format!("  {detail}"),
            0.0,
            egui::TextFormat::simple(egui::FontId::default(), ui.visuals().weak_text_color()),
        );
    }
    job
}

/// Case-insensitive prefix filter over `candidates` (`SPEC.md` §2), ordered
/// by: exact-case prefix match before a case-insensitive-only match, then
/// shorter labels before longer ones, then alphabetical — a stable,
/// predictable order with no frequency/recency scoring.
pub(super) fn filter_and_rank<'a>(candidates: &'a [CompletionItem], prefix: &str) -> Vec<&'a CompletionItem> {
    let prefix_lower = prefix.to_lowercase();
    let mut matched: Vec<&CompletionItem> = candidates
        .iter()
        .filter(|c| c.label.to_lowercase().starts_with(&prefix_lower))
        .collect();
    matched.sort_by(|a, b| {
        let a_exact = a.label.starts_with(prefix);
        let b_exact = b.label.starts_with(prefix);
        b_exact
            .cmp(&a_exact)
            .then_with(|| a.label.len().cmp(&b.label.len()))
            .then_with(|| a.label.cmp(&b.label))
    });
    matched
}

/// Applies `item` at the popup's position: replaces
/// `text[anchor_char..cursor_char]` with `item.label`, plain prefix-replace
/// for `Word`/`Keyword`/`Field`. For `Method`, appends `()` and places the
/// cursor between the parens (`has_params`) or right after — built as
/// `"{label}(${cursor})"` run through `templates::expand`'s own
/// marker-strip-and-locate step, rather than a second cursor mechanism.
/// `Template` never reaches this function — its caller calls
/// `templates::expand` directly instead.
pub(super) fn insert_completion(
    text: &str,
    anchor_char: usize,
    cursor_char: usize,
    item: &CompletionItem,
) -> (String, usize) {
    let anchor_byte = char_to_byte(text, anchor_char);
    let cursor_byte = char_to_byte(text, cursor_char);

    let body = if item.kind == CompletionKind::Method {
        if item.has_params {
            format!("{}({CURSOR_MARKER})", item.label)
        } else {
            format!("{}()", item.label)
        }
    } else {
        item.label.clone()
    };
    let cursor_offset = body
        .find(CURSOR_MARKER)
        .map_or(body.chars().count(), |byte_pos| body[..byte_pos].chars().count());
    let expansion = body.replace(CURSOR_MARKER, "");

    let new_text = format!("{}{expansion}{}", &text[..anchor_byte], &text[cursor_byte..]);
    let new_cursor = anchor_char + cursor_offset;
    (new_text, new_cursor)
}

/// Decodes a raw `textDocument/completion` response (`PLAN.md` Track 20
/// Phase 5) into this popup's own `CompletionItem`s. `None`/a parse
/// failure (a malformed or absent response) is just an empty `Vec` — one
/// missing/broken source is never a reason to fail the whole popup, same
/// spirit as every other LSP source in this app degrading silently.
fn from_lsp_response(value: serde_json::Value) -> Vec<CompletionItem> {
    let items = match serde_json::from_value::<Option<lsp_types::CompletionResponse>>(value) {
        Ok(Some(lsp_types::CompletionResponse::Array(items))) => items,
        Ok(Some(lsp_types::CompletionResponse::List(list))) => list.items,
        Ok(None) | Err(_) => Vec::new(),
    };
    items.into_iter().map(completion_item_from_lsp).collect()
}

fn completion_item_from_lsp(item: lsp_types::CompletionItem) -> CompletionItem {
    let kind = match item.kind {
        Some(
            lsp_types::CompletionItemKind::METHOD
            | lsp_types::CompletionItemKind::FUNCTION
            | lsp_types::CompletionItemKind::CONSTRUCTOR,
        ) => CompletionKind::Method,
        Some(
            lsp_types::CompletionItemKind::FIELD
            | lsp_types::CompletionItemKind::PROPERTY
            | lsp_types::CompletionItemKind::VARIABLE
            | lsp_types::CompletionItemKind::CONSTANT
            | lsp_types::CompletionItemKind::ENUM_MEMBER,
        ) => CompletionKind::Field,
        Some(lsp_types::CompletionItemKind::KEYWORD) => CompletionKind::Keyword,
        _ => CompletionKind::Word,
    };
    let (label, has_params) = bare_label_and_has_params(&item.label);
    CompletionItem {
        label,
        kind,
        detail: item.detail,
        has_params,
    }
}

/// Extracts a bare, insertable identifier from a raw LSP `label` — real
/// servers routinely decorate it with a signature/return type (e.g.
/// `"add(E e) : boolean"`), which `insert_completion` (this file, above)
/// must never see as-is: it treats `Method`'s own `label` as bare, adding
/// `()`/a cursor marker itself, so an undecorated label would come out as
/// `"add(E e) : boolean()"` in the buffer. Rather than trusting `insert_
/// text`/`text_edit` instead (either of which may be `Snippet`-formatted
/// with `$1`/`${1:name}` tab-stop syntax this popup has no way to
/// navigate — inserting *that* verbatim would be worse, not better),
/// this reads only `label`, which the protocol never allows to itself be
/// a snippet. Also recovers a real `has_params` signal from the same
/// text (`"add(E e)"` → `true`, `"length"` → `false`) instead of a blind
/// kind-based guess.
fn bare_label_and_has_params(raw: &str) -> (String, bool) {
    let name_end = raw
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(raw.len());
    let name = raw[..name_end].to_string();
    let rest = raw[name_end..].trim_start();
    let has_params = rest
        .strip_prefix('(')
        .and_then(|after_open| after_open.find(')').map(|close| !after_open[..close].trim().is_empty()))
        .unwrap_or(false);
    (name, has_params)
}

#[cfg(test)]
#[path = "completion_test.rs"]
mod completion_test;
