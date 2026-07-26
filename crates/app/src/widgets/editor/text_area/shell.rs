//! The virtualized editor's **interactive shell** (PLAN.md 2f + "shell"): the
//! `show()` that reads egui's input queue and clipboard each frame, drives
//! the pure caret/edit primitives in `input.rs` and the undo stack in
//! `history.rs`, and paints caret/selection/IME-preedit on top of the
//! virtualized render path from `render.rs`. This is the piece that turns
//! the read-only render (2b) into something actually editable.
//!
//! Drives the live editor as of the PLAN 2g/2h swap — `widget.rs` calls
//! `show` (re-exported as `show_interactive`) in place of `egui::TextEdit`.
//! Kept decoupled from `fg_core::Document` on purpose (a `Rope` in, an
//! `Option<String>` new-text out, same as `render.rs`) so it stays testable
//! without a real document/project.

use std::ops::Range;

use egui::{Color32, Event, FontId, Id, Key, Stroke};
use ropey::Rope;

use super::history::{EditKind, History, Snapshot};
use super::input::{
    Caret, LineIndex, backspace, clamp_out_of_hidden, column_of, delete_forward, move_down, move_end, move_home,
    move_left, move_right, move_up, replace_selection,
};
use super::render::{
    HighlightSpan, TextAreaOutput, layout_visible, layout_visible_wrapped, paint_rows, shape_line_range, shape_range,
};

/// Per-widget state persisted across frames (`egui::Context`'s temp storage,
/// the same mechanism `widget.rs` already uses for its own per-widget state
/// — see e.g. `LayoutCacheKey`/`SelectionExpandState` there), keyed by the
/// caller's stable `Id`.
#[derive(Clone)]
struct ShellState {
    caret: Caret,
    preferred_col: usize,
    history: History,
    /// The char range currently occupied by an in-progress IME composition
    /// (so the next `Preedit`/`Commit` knows what to replace), or `None`
    /// outside of composition.
    ime_range: Option<std::ops::Range<usize>>,
    /// `ui.input(|i| i.time)` as of the caret's last move/edit (or the
    /// widget's last gaining focus) — the same anchor `egui::text_edit::
    /// TextEditState::last_interaction_time` uses to drive `paint_caret`'s
    /// blink cycle, so the caret snaps solid-visible on every keystroke/
    /// click instead of blinking mid-cycle right when the user is looking
    /// at it.
    last_interaction: f64,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            caret: Caret::at(0),
            preferred_col: 0,
            history: History::default(),
            ime_range: None,
            last_interaction: 0.0,
        }
    }
}

fn load(ui: &egui::Ui, id: Id) -> ShellState {
    ui.ctx()
        .data_mut(|d| d.get_temp_mut_or_default::<ShellState>(id).clone())
}

fn store(ui: &egui::Ui, id: Id, state: ShellState) {
    ui.ctx().data_mut(|d| d.insert_temp(id, state));
}

/// Reads the persisted caret/selection for widget `id`, if `show` has ever
/// run for it — the text_area analogue of `egui::text_edit::TextEditState::
/// load(ctx, id).and_then(|s| s.cursor.char_range())`, for the `widget.rs`
/// interceptions (wrap-selection, Tab/indent, comment toggle, …) that need
/// to know the selection as it stood *before* this frame's `show` call runs,
/// the same timing `TextEditState::load` gives them today.
pub fn peek_caret(ctx: &egui::Context, id: Id) -> Option<Caret> {
    ctx.data(|d| d.get_temp::<ShellState>(id)).map(|s| s.caret)
}

/// Overrides the persisted caret/selection for widget `id` ahead of the next
/// `show` call — the analogue of stashing a `manual_cursor_range` for
/// `egui::text_edit::TextEditState::store` to apply. Also breaks the undo
/// run-coalescing (`History::break_run`): an externally-driven caret move —
/// an interception's own edit landing the cursor somewhere, a generated
/// getter/setter jumping to it, … — is exactly the "unrelated change in
/// between" the coalescing rule exists to not silently merge past.
pub fn set_caret(ctx: &egui::Context, id: Id, caret: Caret) {
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|d| {
        let state = d.get_temp_mut_or_insert_with(id, ShellState::default);
        state.caret = caret;
        state.history.break_run();
        state.last_interaction = now;
    });
}

/// What an interactive frame produces: the underlying render geometry (for
/// any overlay that still wants it) plus the editing outcome — `new_text`,
/// set exactly on a frame where an event actually mutated the buffer, is the
/// caller's cue to run it through the same `apply_edit` reparse pipeline
/// every other edit path in `widget.rs` already uses. `caret` is `None`
/// exactly when the widget isn't focused this frame — the same semantics
/// `egui::text_edit::TextEditOutput::cursor_range` has (only set inside
/// `handle_events`, gated on `ui.memory(|mem| mem.has_focus(id))`), so a
/// caller porting a `let Some(range) = output.cursor_range` guard (Ctrl+D,
/// Ctrl+J, generation cursor-placement, …) can swap the field name and keep
/// the exact same "only acts while focused" behavior.
pub struct ShellOutput {
    pub base: TextAreaOutput,
    pub new_text: Option<String>,
    pub caret: Option<Caret>,
}

/// Char length of logical line `logical`, excluding its trailing newline —
/// the valid range for a `CCursor` into that line's galley (which was shaped
/// with the newline already stripped, see `render::shape_line`).
fn line_char_len(buffer: &Rope, logical: usize) -> usize {
    let raw = buffer.line(logical.min(buffer.len_lines().saturating_sub(1)));
    raw.chars().filter(|&c| c != '\n' && c != '\r').count()
}

/// Resolves a screen position to a char offset, using rows already shaped in
/// `out` (so it reflects exactly what's painted on screen this frame — see
/// `render::layout_visible`'s doc comment on why pointer resolution always
/// happens against the pre-edit shape). Clamps to the nearest edge row for a
/// click above/below the shaped range, same as `text_area::row_at_y`.
///
/// Public (unlike most of this module) because `widget.rs`'s Alt+Click
/// secondary-cursor and sticky-scroll top-line hit-testing need the exact
/// same screen-position-to-char-offset math this frame's own click handling
/// already uses — the `egui::TextEdit`-era equivalent of both was
/// `output.galley.cursor_from_pos(pos - output.galley_pos)`; this is that,
/// generalized to the per-row-galley model, taking `ShellOutput::base` as
/// `out`.
pub fn char_offset_for_pos(out: &TextAreaOutput, buffer: &Rope, pos: egui::Pos2) -> usize {
    if out.row_galleys.is_empty() {
        return buffer.len_chars();
    }
    let raw_row = ((pos.y - out.content_origin.y) / out.row_height).floor().max(0.0) as usize;
    // The last entry whose block *starts* at-or-before `raw_row` — same
    // "which block contains this row" search `text_area::visible_lines`
    // does over a prefix sum, just over `row_offsets` directly since we
    // already have the per-entry values rather than a line-indexed table.
    // In the no-wrap case this always resolves to `raw_row - visible_rows.
    // start`, same as before word-wrap existed.
    let idx = out
        .row_offsets
        .partition_point(|&r| r <= raw_row)
        .saturating_sub(1)
        .min(out.row_galleys.len() - 1);
    let (logical, galley) = &out.row_galleys[idx];
    // How far into *this entry's own* (possibly multi-row, once wrapped)
    // galley the click landed — `galley.cursor_from_pos` needs a position
    // relative to the galley's own top, not the viewport's.
    let local_y = raw_row.saturating_sub(out.row_offsets[idx]) as f32 * out.row_height;
    let local = egui::vec2(pos.x - out.content_origin.x, local_y);
    let ccursor = galley.cursor_from_pos(local);
    buffer.line_to_char(*logical) + ccursor.index.0
}

/// Interactive `show`: renders `buffer` and, when focused, edits it in
/// response to this frame's input. `text` is `buffer.to_string()` — the
/// caller (`widget.rs`) already has this from its own pre-frame
/// stringification, so it's threaded through here rather than re-derived
/// (SPEC.md §2: avoids a second full-buffer allocation every frame). `read_
/// only` mirrors `doc.read_only` (strips every mutating event, same policy
/// `widget.rs::strip_mutating_events` enforces for the `TextEdit` path today
/// — navigation/selection/copy stay live). `spans` are this frame's syntax-
/// highlighting spans (computed by the caller against `buffer`'s *pre*-edit
/// contents — see `render::HighlightSpan`'s doc comment); on an edit frame
/// they're reused as best-effort for the immediate post-edit re-shape below
/// rather than left stale for a whole extra frame, matching the same
/// "highlighted against the not-yet-reparsed tree for one frame" behavior
/// `widget.rs`'s `egui::TextEdit` path already has today (its layouter runs
/// inside the same `show()` call that produces the post-edit text, using
/// whatever `parser` state existed before that edit's own `apply_edit`
/// call).
#[expect(
    clippy::too_many_arguments,
    reason = "each parameter is independently threaded per-frame state, not a bundle waiting to be a struct — see widget::show's own too-many-arguments allowance for the same shape"
)]
pub fn show(
    ui: &mut egui::Ui,
    id: Id,
    buffer: &Rope,
    text: &str,
    font_id: FontId,
    text_color: Color32,
    read_only: bool,
    spans: &[HighlightSpan],
    hidden: &[Range<usize>],
    word_wrap: bool,
    cursor_blink: bool,
) -> ShellOutput {
    let mut state = load(ui, id);
    let caret_at_frame_start = state.caret;

    // Read (and lock) focus *before* shaping below — `layout_visible`'s own
    // `ui.interact` call is what makes this widget "focusable," and if that
    // runs first, its `Sense::click_and_drag()` participates in egui's
    // default Tab-cycles-focus handling and can steal focus away *within
    // this same frame*, before `process_events` ever sees the Tab event
    // that triggered it — matching `egui::TextEdit`'s own timing (its
    // `handle_events` reads/locks focus from inside the atom closure that
    // runs *before* the widget's own outer `interact` call within the same
    // `allocate()`). `set_focus_lock_filter` itself only ever takes effect
    // starting *next* frame (egui decides a frame's Tab-focus-cycling
    // outcome once, at frame start, from last frame's filter) — so this
    // read/lock order doesn't save focus for a frame with an genuinely
    // fresh, first-ever `request_focus` and a Tab in the very same frame,
    // only for every frame after that, same as `TextEdit`.
    let had_focus_at_frame_start = ui.memory(|m| m.has_focus(id));
    if had_focus_at_frame_start {
        // Without this, egui's default focus-cycling would steal Tab/arrow
        // keys away from a focused widget (moving focus to "the next
        // widget") on subsequent frames — the same reason `egui::TextEdit::
        // code_editor()` sets `lock_focus(true)` (which is exactly this
        // filter). Escape is deliberately left `false` so it still
        // surrenders focus / closes an enclosing modal rather than being
        // swallowed here.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            );
        });
    }

    // Captured *before* `pre`'s own `ui.allocate_space` runs (inside
    // `layout_visible_wrapped`) and reused below for the post-edit reshape,
    // rather than each querying `ui.available_width()` independently: once
    // `pre` allocates its row space, the horizontal layout's cursor has
    // already advanced by that width, so a second query on the same `ui`
    // moments later would see almost nothing left — collapsing every
    // character onto its own wrapped row for that one frame.
    let wrap_width = ui.available_width();

    // Shape (never paint yet) against the pre-edit buffer: this is what's
    // actually on screen right now, so pointer clicks below resolve against
    // it rather than against text that doesn't exist on screen until this
    // function returns.
    let pre = if word_wrap {
        layout_visible_wrapped(ui, id, buffer, font_id.clone(), hidden, spans)
    } else {
        layout_visible(ui, id, buffer, font_id.clone(), hidden, spans)
    };

    if pre.response.clicked() || pre.response.drag_started() {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let has_focus = had_focus_at_frame_start || pre.response.clicked() || pre.response.drag_started();

    // PLAN.md Phase 5 / SPEC.md §7: built once here (from `text`, this
    // frame's starting content) and kept in sync through `process_events`
    // below — rebuilt only when an event actually edits `current`, not on
    // every motion — so every `line_col`/`line_col_to_char`/`char_to_byte`
    // lookup this frame (click resolution here, every `move_*` call in
    // `process_events`, the `clamp_out_of_hidden` call after it returns)
    // binary-searches this instead of each independently rescanning the
    // whole buffer from char/byte 0.
    let mut index = LineIndex::build(text);

    if let Some(pos) = pre.response.interact_pointer_pos() {
        let extend = ui.input(|i| i.modifiers.shift);
        let offset = char_offset_for_pos(&pre, buffer, pos);
        if pre.response.drag_started() {
            state.caret = Caret {
                primary: offset,
                anchor: if extend { state.caret.anchor } else { offset },
            };
            state.history.break_run();
        } else if pre.response.dragged() || pre.response.clicked() {
            let keep_anchor = extend || pre.response.dragged();
            state.caret = Caret {
                primary: offset,
                anchor: if keep_anchor { state.caret.anchor } else { offset },
            };
        }
        state.preferred_col = column_of(&index, state.caret.primary);
    }

    let mut new_text = None;
    if has_focus {
        new_text = process_events(ui, text, &mut index, &mut state, read_only);
    }

    // Whichever buffer is actually going on screen this frame — the edited
    // one if an event just produced one, else the original — is what both
    // painting and the IME anchor rect below need to agree on, so it's
    // resolved once rather than re-derived at each call site.
    let post_buffer = new_text.as_ref().map(|t| Rope::from_str(t));
    let (final_out, final_buffer): (TextAreaOutput, &Rope) = match &post_buffer {
        Some(post_buffer) => {
            // The same *logical lines* `pre` already shaped, re-derived from
            // its own `row_galleys` (contiguous and sorted by construction)
            // rather than `pre.visible_rows` — with word-wrap on that field
            // is a *visual*-row range, not directly a line range, and one
            // entry can now cover more than one of those rows.
            let lines =
                pre.row_galleys.first().map_or(0, |(l, _)| *l)..pre.row_galleys.last().map_or(0, |(l, _)| *l + 1);
            let row_galleys = if word_wrap {
                shape_line_range(ui, post_buffer, lines, hidden, &font_id, spans, wrap_width)
            } else {
                shape_range(ui, post_buffer, pre.visible_rows.clone(), hidden, &font_id, spans)
            };
            // Re-derived from the *post*-edit galleys' own row counts,
            // anchored at `pre`'s starting offset — an edit can change how
            // many rows a re-wrapped line takes, so reusing `pre.row_
            // offsets` verbatim here would silently go stale the moment a
            // keystroke changes a wrapped line's row count.
            let mut offset = pre.row_offsets.first().copied().unwrap_or(0);
            let row_offsets: Vec<usize> = row_galleys
                .iter()
                .map(|(_, g)| {
                    let this = offset;
                    offset += g.rows.len().max(1);
                    this
                })
                .collect();
            (
                TextAreaOutput {
                    row_galleys,
                    row_offsets,
                    ..pre.clone()
                },
                post_buffer,
            )
        }
        None => (pre.clone(), buffer),
    };

    // PLAN.md 3d: keyboard motion (`process_events`'s arrow/Home/End arms)
    // operates on the plain char/line model, with no idea a fold hid the
    // line it just landed on — click resolution never has this problem
    // (`char_offset_for_pos` only ever resolves against rows `layout_
    // visible` actually shaped, which `hidden` already excluded), so this
    // only needs to run after keyboard-driven motion, i.e. whenever focused.
    if has_focus && !hidden.is_empty() {
        // `index` already reflects `final_buffer`'s exact content: `process_
        // events` (above) rebuilds it in lockstep with `current` on every
        // edit, and `final_buffer` is that same post-edit `current` (or,
        // absent an edit, still the original `text` `index` started from)
        // — so no separate `final_buffer.to_string()` is needed just to
        // re-derive what `index` already has.
        state.caret.primary = clamp_out_of_hidden(&index, state.caret.primary, hidden);
    }

    // Resets the blink cycle to solid-visible on anything that should make
    // the caret "jump to attention": just gaining focus, a click/drag moving
    // it, an arrow/Home/End move, or an edit — the same set of triggers
    // `egui::text_edit::TextEditState::last_interaction_time` resets on
    // (`response.changed() || selection_changed`, plus the gained-focus
    // case). Without this a click would leave the caret starting mid-blink
    // (possibly invisible) instead of visible right where the user just
    // looked.
    if (!had_focus_at_frame_start && has_focus) || state.caret != caret_at_frame_start || new_text.is_some() {
        state.last_interaction = ui.input(|i| i.time);
    }

    paint_rows(ui, &final_out, text_color);
    if has_focus {
        paint_caret(ui, &final_out, final_buffer, &state, text_color, cursor_blink);

        // Tell the platform integration where to anchor its IME candidate
        // window — without this, an IME popup (e.g. composing Japanese/
        // Chinese input) would appear wherever it last was instead of
        // tracking the caret, since nothing else in this frame reports it.
        if !read_only && let Some(cursor_rect) = final_out.char_rect(final_buffer, state.caret.primary) {
            ui.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    rect: pre.response.rect,
                    cursor_rect,
                    should_interrupt_composition: false,
                });
            });
        }
    }

    let caret = has_focus.then_some(state.caret);
    store(ui, id, state);

    ShellOutput {
        base: pre,
        new_text,
        caret,
    }
}

/// Paints the primary caret — blinking, on the same on/off cycle
/// `egui::text_selection::visuals::paint_text_cursor` drives real
/// `egui::TextEdit`s with, anchored at `state.last_interaction` rather than
/// unconditionally solid — and, when there's a selection, a filled rect per
/// visible row it touches.
fn paint_caret(
    ui: &egui::Ui,
    out: &TextAreaOutput,
    buffer: &Rope,
    state: &ShellState,
    text_color: Color32,
    cursor_blink: bool,
) {
    let painter = ui.painter();
    let caret_color = ui.visuals().text_cursor.stroke.color;
    let selection_color = ui.visuals().selection.bg_fill;
    let _ = text_color;

    let range = state.caret.range();
    if !range.is_empty() {
        for (i, (logical, _)) in out.row_galleys.iter().enumerate() {
            let line_start = buffer.line_to_char(*logical);
            let line_len = line_char_len(buffer, *logical);
            let line_end = line_start + line_len;
            if range.end <= line_start || range.start > line_end {
                continue;
            }
            let start_col = range.start.max(line_start) - line_start;
            let end_col = range.end.min(line_end) - line_start;
            let Some(row_galley) = out.row_galleys.get(i).map(|(_, g)| g) else {
                continue;
            };
            let x0 = out.content_origin.x + row_galley.pos_from_cursor(egui::text::CCursor::new(start_col)).left();
            let x1 = out.content_origin.x + row_galley.pos_from_cursor(egui::text::CCursor::new(end_col)).left();
            let y = out.content_origin.y + out.row_offsets[i] as f32 * out.row_height;
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1.max(x0), y + out.row_height)),
                0.0,
                selection_color,
            );
        }
    }

    if let Some(rect) = out.char_rect(buffer, state.caret.primary) {
        if caret_visible(ui, state.last_interaction, cursor_blink) {
            painter.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.5, caret_color));
        }
        if let Some(ime) = &state.ime_range
            && let (Some(start), Some(end)) = (out.char_rect(buffer, ime.start), out.char_rect(buffer, ime.end))
        {
            let y = start.bottom();
            painter.line_segment(
                [egui::pos2(start.left(), y), egui::pos2(end.left(), y)],
                Stroke::new(1.0, caret_color),
            );
        }
    }
}

/// Whether the caret should be drawn solid this frame, and the driver of the
/// blink animation itself — mirrors `egui::text_selection::visuals::
/// paint_text_cursor`'s own cycle math (`time_in_cycle` against `on_duration`/
/// `off_duration` from `Visuals::text_cursor`) so this widget's blink looks
/// and feels identical to a real `egui::TextEdit`'s, and schedules the next
/// repaint itself (`request_repaint_after`) so the "off" half of the cycle
/// actually arrives rather than only ever repainting in response to input.
/// Respects `Visuals::text_cursor.blink` (always solid if the theme disables
/// it), `cursor_blink` (View > Blinking Cursor — this app's own on/off
/// switch, same idea as the theme's but user-facing and independent of it),
/// and the viewport's own focus (no point animating — or repainting — a
/// caret nobody can see because the OS window itself isn't focused).
fn caret_visible(ui: &egui::Ui, last_interaction: f64, cursor_blink: bool) -> bool {
    if !cursor_blink || !ui.visuals().text_cursor.blink || !ui.input(|i| i.focused) {
        return true;
    }

    let on_duration = ui.visuals().text_cursor.on_duration;
    let off_duration = ui.visuals().text_cursor.off_duration;
    let total_duration = on_duration + off_duration;
    if total_duration <= 0.0 {
        return true;
    }

    let now = ui.input(|i| i.time);
    let time_since_interaction = (now - last_interaction).max(0.0);
    let time_in_cycle = (time_since_interaction % total_duration as f64) as f32;

    let (visible, wake_in) = if time_in_cycle < on_duration {
        (true, on_duration - time_in_cycle)
    } else {
        (false, total_duration - time_in_cycle)
    };
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs_f32(wake_in.max(0.0)));
    visible
}

/// Runs every event in this frame's queue against `text`/`state.caret` in
/// order (a fast typist can deliver more than one `Event::Text` in a single
/// frame), returning the final text if anything changed. Mirrors
/// `egui::TextEdit`'s own `events()` loop in shape, re-expressed over the
/// `input.rs` pure functions instead of a private `CCursorRange`. `index`
/// must reflect `text` on entry (the caller's own `LineIndex::build(text)`);
/// every branch below that reassigns `current` also rebuilds `*index` in the
/// same statement, so it's left reflecting `current`'s final value on return
/// — the caller reuses it as-is afterward rather than rebuilding again.
fn process_events(
    ui: &egui::Ui,
    text: &str,
    index: &mut LineIndex,
    state: &mut ShellState,
    read_only: bool,
) -> Option<String> {
    let mut current = text.to_string();
    let mut changed = false;
    let events = ui.input(|i| i.events.clone());

    for event in &events {
        match event {
            Event::Key {
                key: Key::ArrowLeft,
                pressed: true,
                modifiers,
                ..
            } => {
                state.caret = move_left(state.caret, modifiers.shift);
                state.preferred_col = column_of(index, state.caret.primary);
                state.history.break_run();
            }
            Event::Key {
                key: Key::ArrowRight,
                pressed: true,
                modifiers,
                ..
            } => {
                state.caret = move_right(index, state.caret, modifiers.shift);
                state.preferred_col = column_of(index, state.caret.primary);
                state.history.break_run();
            }
            Event::Key {
                key: Key::ArrowUp,
                pressed: true,
                modifiers,
                ..
            } => {
                let (caret, col) = move_up(index, state.caret, modifiers.shift, state.preferred_col);
                state.caret = caret;
                state.preferred_col = col;
                state.history.break_run();
            }
            Event::Key {
                key: Key::ArrowDown,
                pressed: true,
                modifiers,
                ..
            } => {
                let (caret, col) = move_down(index, state.caret, modifiers.shift, state.preferred_col);
                state.caret = caret;
                state.preferred_col = col;
                state.history.break_run();
            }
            Event::Key {
                key: Key::Home,
                pressed: true,
                modifiers,
                ..
            } => {
                state.caret = move_home(index, state.caret, modifiers.shift);
                state.preferred_col = column_of(index, state.caret.primary);
                state.history.break_run();
            }
            Event::Key {
                key: Key::End,
                pressed: true,
                modifiers,
                ..
            } => {
                state.caret = move_end(index, state.caret, modifiers.shift);
                state.preferred_col = column_of(index, state.caret.primary);
                state.history.break_run();
            }
            Event::Key {
                key: Key::A,
                pressed: true,
                modifiers,
                ..
            } if modifiers.command => {
                state.caret = Caret {
                    primary: index.char_len(),
                    anchor: 0,
                };
                state.history.break_run();
            }

            Event::Key {
                key: Key::Z,
                pressed: true,
                modifiers,
                ..
            } if modifiers.command && !modifiers.shift => {
                if let Some(restored) = state.history.undo(Snapshot {
                    text: current.clone(),
                    caret: state.caret,
                }) {
                    current = restored.text;
                    *index = LineIndex::build(&current);
                    state.caret = restored.caret;
                    changed = true;
                }
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if modifiers.command && ((modifiers.shift && *key == Key::Z) || (!modifiers.shift && *key == Key::Y)) => {
                if let Some(restored) = state.history.redo(Snapshot {
                    text: current.clone(),
                    caret: state.caret,
                }) {
                    current = restored.text;
                    *index = LineIndex::build(&current);
                    state.caret = restored.caret;
                    changed = true;
                }
            }

            _ if read_only => {}

            Event::Text(insert) => {
                if !insert.is_empty() && insert != "\n" && insert != "\r" {
                    state.history.checkpoint(
                        Snapshot {
                            text: current.clone(),
                            caret: state.caret,
                        },
                        EditKind::Typing,
                    );
                    let (out, caret) = replace_selection(&current, index, state.caret, insert);
                    current = out;
                    *index = LineIndex::build(&current);
                    state.caret = caret;
                    state.preferred_col = column_of(index, caret.primary);
                    changed = true;
                }
            }
            Event::Key {
                key: Key::Backspace,
                pressed: true,
                ..
            } => {
                state.history.checkpoint(
                    Snapshot {
                        text: current.clone(),
                        caret: state.caret,
                    },
                    EditKind::Deleting,
                );
                if let Some((out, caret)) = backspace(&current, index, state.caret) {
                    current = out;
                    *index = LineIndex::build(&current);
                    state.caret = caret;
                    state.preferred_col = column_of(index, caret.primary);
                    changed = true;
                }
            }
            Event::Key {
                key: Key::Delete,
                pressed: true,
                ..
            } => {
                state.history.checkpoint(
                    Snapshot {
                        text: current.clone(),
                        caret: state.caret,
                    },
                    EditKind::Deleting,
                );
                if let Some((out, caret)) = delete_forward(&current, index, state.caret) {
                    current = out;
                    *index = LineIndex::build(&current);
                    state.caret = caret;
                    state.preferred_col = column_of(index, caret.primary);
                    changed = true;
                }
            }
            Event::Key {
                key: Key::Enter,
                pressed: true,
                ..
            } => {
                state.history.checkpoint(
                    Snapshot {
                        text: current.clone(),
                        caret: state.caret,
                    },
                    EditKind::Other,
                );
                let (out, caret) = replace_selection(&current, index, state.caret, "\n");
                current = out;
                *index = LineIndex::build(&current);
                state.caret = caret;
                state.preferred_col = 0;
                changed = true;
            }
            Event::Key {
                key: Key::Tab,
                pressed: true,
                modifiers,
                ..
            } if !modifiers.shift => {
                state.history.checkpoint(
                    Snapshot {
                        text: current.clone(),
                        caret: state.caret,
                    },
                    EditKind::Other,
                );
                let (out, caret) = replace_selection(&current, index, state.caret, "\t");
                current = out;
                *index = LineIndex::build(&current);
                state.caret = caret;
                state.preferred_col = column_of(index, caret.primary);
                changed = true;
            }

            Event::Copy => {
                if !state.caret.is_collapsed() {
                    let selected: String = current
                        .chars()
                        .skip(state.caret.range().start)
                        .take(state.caret.range().len())
                        .collect();
                    ui.ctx().copy_text(selected);
                }
            }
            Event::Cut => {
                if !state.caret.is_collapsed() {
                    let selected: String = current
                        .chars()
                        .skip(state.caret.range().start)
                        .take(state.caret.range().len())
                        .collect();
                    ui.ctx().copy_text(selected);
                    state.history.checkpoint(
                        Snapshot {
                            text: current.clone(),
                            caret: state.caret,
                        },
                        EditKind::Other,
                    );
                    let (out, caret) = replace_selection(&current, index, state.caret, "");
                    current = out;
                    *index = LineIndex::build(&current);
                    state.caret = caret;
                    state.preferred_col = column_of(index, caret.primary);
                    changed = true;
                }
            }
            Event::Paste(pasted) => {
                if !pasted.is_empty() {
                    state.history.checkpoint(
                        Snapshot {
                            text: current.clone(),
                            caret: state.caret,
                        },
                        EditKind::Other,
                    );
                    let (out, caret) = replace_selection(&current, index, state.caret, pasted);
                    current = out;
                    *index = LineIndex::build(&current);
                    state.caret = caret;
                    state.preferred_col = column_of(index, caret.primary);
                    changed = true;
                }
            }

            Event::Ime(ime_event) => {
                changed |= apply_ime_event(ime_event, &mut current, index, state);
            }

            _ => {}
        }
    }

    changed.then_some(current)
}

/// Applies one IME composition event (PLAN 2f): `Preedit` replaces whatever
/// the previous `Preedit` inserted with the new candidate text and leaves it
/// selected (so it visually reads as "still being composed"); `Commit`
/// replaces it with the final, real text and ends composition. Mirrors
/// `egui::TextEdit`'s `events()` handling of `ImeEvent`, minus the
/// deprecated `Enabled`/`Disabled` variants egui itself no longer emits.
fn apply_ime_event(
    event: &egui::ImeEvent,
    current: &mut String,
    index: &mut LineIndex,
    state: &mut ShellState,
) -> bool {
    use egui::ImeEvent;

    let clear_preedit = |current: &mut String, index: &mut LineIndex, state: &mut ShellState| -> usize {
        if let Some(range) = state.ime_range.take() {
            let caret = Caret {
                primary: range.end,
                anchor: range.start,
            };
            let (out, caret) = replace_selection(current, index, caret, "");
            *current = out;
            *index = LineIndex::build(current);
            caret.primary
        } else {
            state.caret.range().start
        }
    };

    match event {
        #[expect(deprecated)]
        ImeEvent::Enabled | ImeEvent::Disabled => false,
        ImeEvent::Preedit { text: preedit, .. } => {
            if preedit.is_empty() && state.ime_range.is_none() {
                return false;
            }
            let start = clear_preedit(current, index, state);
            let caret_at_start = Caret::at(start);
            let (out, caret) = replace_selection(current, index, caret_at_start, preedit);
            *current = out;
            *index = LineIndex::build(current);
            state.ime_range = Some(start..caret.primary);
            state.caret = Caret {
                primary: caret.primary,
                anchor: start,
            };
            true
        }
        ImeEvent::Commit(commit) => {
            let start = clear_preedit(current, index, state);
            if commit.is_empty() {
                state.caret = Caret::at(start);
                return true;
            }
            let (out, caret) = replace_selection(current, index, Caret::at(start), commit);
            *current = out;
            *index = LineIndex::build(current);
            state.caret = caret;
            state.preferred_col = column_of(index, caret.primary);
            true
        }
    }
}

#[cfg(test)]
mod tests;
