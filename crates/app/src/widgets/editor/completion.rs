//! The completion popup's state, filtering, rendering, and insertion —
//! shared by every trigger path (word-completion, dot-completion) so they
//! end at one popup instead of drifting into subtly different UIs.

use ropey::Rope;

use super::templates::CURSOR_MARKER;
use super::text_area::TextAreaOutput;
use super::text_offset::{byte_to_char, char_to_byte};

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
        }
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
    job.append(&item.label, 0.0, egui::TextFormat::simple(egui::FontId::default(), ui.visuals().text_color()));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    fn item(label: &str) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            kind: CompletionKind::Word,
            detail: None,
            has_params: false,
        }
    }

    fn method_item(label: &str, has_params: bool) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            kind: CompletionKind::Method,
            detail: None,
            has_params,
        }
    }

    fn labels<'a>(items: &[&'a CompletionItem]) -> Vec<&'a str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn filter_and_rank_narrows_by_prefix() {
        let candidates = [item("foo"), item("foobar"), item("bar")];
        let result = filter_and_rank(&candidates, "foo");
        assert_eq!(labels(&result), vec!["foo", "foobar"]);
    }

    #[test]
    fn filter_and_rank_is_case_insensitive() {
        let candidates = [item("Foo"), item("bar")];
        let result = filter_and_rank(&candidates, "fo");
        assert_eq!(labels(&result), vec!["Foo"]);
    }

    #[test]
    fn filter_and_rank_exact_case_sorts_before_case_insensitive_match() {
        let candidates = [item("Foo"), item("foo")];
        let result = filter_and_rank(&candidates, "foo");
        assert_eq!(labels(&result), vec!["foo", "Foo"]);
    }

    #[test]
    fn filter_and_rank_shorter_labels_sort_before_longer_ones() {
        let candidates = [item("foobar"), item("foo")];
        let result = filter_and_rank(&candidates, "foo");
        assert_eq!(labels(&result), vec!["foo", "foobar"]);
    }

    #[test]
    fn filter_and_rank_ties_break_alphabetically() {
        let candidates = [item("fooz"), item("fooa")];
        let result = filter_and_rank(&candidates, "foo");
        assert_eq!(labels(&result), vec!["fooa", "fooz"]);
    }

    #[test]
    fn filter_and_rank_empty_prefix_returns_everything_unfiltered() {
        let candidates = [item("foo"), item("bar"), item("baz")];
        let result = filter_and_rank(&candidates, "");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn visible_filters_against_the_anchor_to_cursor_slice() {
        let state = CompletionState::open(5, vec![item("foo"), item("foobar"), item("bar")]);
        // "text_"[5..8] == "foo", the run typed so far after the anchor.
        let result = state.visible("text_foo", 8);
        assert_eq!(labels(&result), vec!["foo", "foobar"]);
    }

    #[test]
    fn move_selection_down_advances_and_clamps_at_the_last_row() {
        let mut state = CompletionState::open(0, vec![item("a"), item("b")]);
        state.move_selection(1, 2);
        assert_eq!(state.selected(), 1);
        state.move_selection(1, 2);
        assert_eq!(state.selected(), 1);
    }

    #[test]
    fn move_selection_up_clamps_at_the_first_row() {
        let mut state = CompletionState::open(0, vec![item("a"), item("b")]);
        state.move_selection(-1, 2);
        assert_eq!(state.selected(), 0);
    }

    #[test]
    fn move_selection_with_no_visible_candidates_resets_to_zero() {
        let mut state = CompletionState::open(0, vec![item("a")]);
        state.move_selection(1, 1);
        state.move_selection(1, 0);
        assert_eq!(state.selected(), 0);
    }

    #[test]
    fn popup_position_at_start_of_a_line_sits_one_row_below_unclamped() {
        let char_rect = rect(0.0, 100.0, 1.0, 18.0);
        let pane = rect(0.0, 0.0, 800.0, 600.0);
        let pos = popup_position(char_rect, egui::vec2(200.0, 80.0), pane);
        assert_eq!(pos, egui::pos2(0.0, 118.0));
    }

    #[test]
    fn popup_position_mid_line_sits_directly_under_the_caret_unclamped() {
        let char_rect = rect(340.0, 100.0, 1.0, 18.0);
        let pane = rect(0.0, 0.0, 800.0, 600.0);
        let pos = popup_position(char_rect, egui::vec2(200.0, 80.0), pane);
        assert_eq!(pos, egui::pos2(340.0, 118.0));
    }

    #[test]
    fn popup_position_near_right_edge_clamps_x_to_stay_inside_the_pane() {
        let char_rect = rect(750.0, 100.0, 1.0, 18.0);
        let pane = rect(0.0, 0.0, 800.0, 600.0);
        let pos = popup_position(char_rect, egui::vec2(200.0, 80.0), pane);
        assert_eq!(pos.x, 600.0); // pane_rect.right() - popup width
        assert_eq!(pos.y, 118.0); // y unaffected by the x clamp
    }

    #[test]
    fn popup_position_near_bottom_edge_clamps_y_to_stay_inside_the_pane() {
        let char_rect = rect(0.0, 550.0, 1.0, 18.0);
        let pane = rect(0.0, 0.0, 800.0, 600.0);
        let pos = popup_position(char_rect, egui::vec2(200.0, 80.0), pane);
        assert_eq!(pos.x, 0.0); // x unaffected by the y clamp
        assert_eq!(pos.y, 520.0); // pane_rect.bottom() - popup height
    }

    #[test]
    fn popup_position_oversized_popup_stays_flush_with_the_near_edge_instead_of_going_negative() {
        // Popup (200x80) is bigger than the pane (100x50) in both
        // dimensions — "one row below the caret" (y=18) can't be honored
        // at all here, so the y clamp wins and pins it flush to the pane's
        // top edge instead of pushing it off-screen.
        let char_rect = rect(0.0, 0.0, 1.0, 18.0);
        let pane = rect(0.0, 0.0, 100.0, 50.0);
        let pos = popup_position(char_rect, egui::vec2(200.0, 80.0), pane);
        assert_eq!(pos, egui::pos2(0.0, 0.0));
    }

    #[test]
    fn insert_completion_replaces_the_typed_prefix_with_the_label() {
        let (text, cursor) = insert_completion("foo.ba", 4, 6, &item("bar"));
        assert_eq!(text, "foo.bar");
        assert_eq!(cursor, 7);
    }

    #[test]
    fn insert_completion_with_no_prefix_typed_yet_just_inserts_the_label() {
        let (text, cursor) = insert_completion("foo.", 4, 4, &item("bar"));
        assert_eq!(text, "foo.bar");
        assert_eq!(cursor, 7);
    }

    #[test]
    fn insert_completion_a_zero_arg_method_places_the_cursor_after_the_closing_paren() {
        let (text, cursor) = insert_completion("foo.ru", 4, 6, &method_item("run", false));
        assert_eq!(text, "foo.run()");
        assert_eq!(cursor, text.chars().count());
    }

    #[test]
    fn insert_completion_a_parameterized_method_places_the_cursor_between_the_parens() {
        let (text, cursor) = insert_completion("foo.co", 4, 6, &method_item("compute", true));
        assert_eq!(text, "foo.compute()");
        assert_eq!(&text[..cursor], "foo.compute(");
    }
}
