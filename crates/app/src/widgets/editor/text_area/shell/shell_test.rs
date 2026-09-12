//! Smoke tests for the interactive shell ([`super`]) — PLAN.md 2f/"shell" is
//! explicitly frame-driven, "only smoke-testable" code, so these drive real
//! `egui::Context::run_ui` frames (following `widget/tests.rs`'s own
//! `focused_frame` pattern: request focus, then call `show` with a queue of
//! synthetic events) rather than asserting on pure functions in isolation —
//! that half already has exhaustive coverage in `input/tests.rs` and
//! `history/tests.rs`.

use egui::{Event, Key, Modifiers};

use super::*;
use crate::widgets::editor::text_area::render::layout_visible;

fn key_event(key: Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

fn command_key_event(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn sized_raw_input(events: Vec<Event>) -> egui::RawInput {
    // Mirrors `focused_frame`'s derivation in `widget/tests.rs`: the
    // top-level `modifiers` field (what `ui.input(|i| i.modifiers)` and
    // `Event::Key`'s own per-event `modifiers` both need to agree with) has
    // to be set from whichever event actually carries the modifier, since
    // `RawInput::modifiers` defaults to `NONE` independently of the events.
    let modifiers = events
        .iter()
        .find_map(|e| match e {
            Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        )),
        events,
        modifiers,
        ..Default::default()
    }
}

/// Runs one frame of `shell::show` against `buffer`, focused, with `events`
/// queued — on `ctx`, so the caller controls whether persisted `ShellState`
/// (caret/history/IME) carries over from a prior call (same `ctx`, same
/// `id`) or starts fresh (a new `ctx` each time).
fn frame(ctx: &egui::Context, id: egui::Id, buffer: &Rope, events: Vec<Event>, read_only: bool) -> ShellOutput {
    let mut result = None;
    let text = buffer.to_string();
    let _ = ctx.run_ui(sized_raw_input(events), |ui| {
        ui.memory_mut(|m| m.request_focus(id));
        egui::ScrollArea::vertical()
            .max_height(400.0)
            .id_salt("shell_tests_scroll_area")
            .show(ui, |ui| {
                result = Some(show(
                    ui,
                    id,
                    buffer,
                    0,
                    &text,
                    egui::FontId::monospace(14.0),
                    egui::Color32::WHITE,
                    read_only,
                    &[],
                    &[],
                    false,
                    true,
                ));
            });
    });
    result.expect("show ran inside the scroll area closure")
}

/// Regression test for scroll-follows-cursor: repeated `ArrowDown` past the
/// bottom of a 400px-tall viewport must eventually scroll the `ScrollArea`
/// so the caret's own line comes back into view — before `show`'s own
/// `ui.scroll_to_rect_animation` call, the caret would move but the viewport
/// wouldn't follow it, leaving it to scroll off-screen.
///
/// `ui.scroll_to_rect` only ever schedules a target offset; the enclosing
/// `ScrollArea` reads and starts moving toward it on its own *next* `show`
/// call, then keeps easing every call after using real elapsed wall-clock
/// time (`egui`'s own `ScrollArea` animation, `ScrollAnimation::none()` here
/// just collapses its duration to zero rather than skipping the mechanism
/// entirely) — so proving it actually arrived needs a couple of realistic,
/// real-time-spaced frames after the one that moved the caret, exactly like
/// a real 60fps run already provides well before a user could ever notice.
#[test]
fn arrow_down_past_the_visible_window_scrolls_the_caret_into_view() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("scroll_follow");
    let lines: Vec<String> = (0..100).map(|i| format!("line{i}")).collect();
    let buffer = Rope::from_str(&lines.join("\n"));

    let arrow_downs = std::iter::repeat_with(|| key_event(Key::ArrowDown)).take(60).collect();
    let out = frame(&ctx, id, &buffer, arrow_downs, false);
    let caret_line = buffer.char_to_line(out.caret.expect("focused").primary);
    assert_eq!(caret_line, 60);
    assert!(
        !out.base.row_galleys.iter().any(|(line, _)| *line == caret_line),
        "line 60 shouldn't already be visible the same frame it was reached"
    );

    for _ in 0..2 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = frame(&ctx, id, &buffer, vec![], false);
    }
    let out = frame(&ctx, id, &buffer, vec![], false);
    assert!(
        out.base.row_galleys.iter().any(|(line, _)| *line == caret_line),
        "line 60 should now be visible after the scroll took effect"
    );
}

/// Regression test for scroll-follows-cursor on an *externally-driven* jump
/// (`set_caret` — how `widget.rs` applies a Ctrl+D occurrence hop, Ctrl+J
/// join, go-to-line, a getter/setter jump, …, all *after* `show` already ran
/// that frame). Unlike an arrow key, the caret change here never happens
/// between two of `show`'s own runs, so `caret_moved` can't see it — the
/// `scroll_to_caret` flag `set_caret` raises is what forces the follow. Before
/// that flag, a Ctrl+D landing far off-screen moved the caret but left the
/// viewport behind.
#[test]
fn an_external_set_caret_jump_scrolls_the_caret_into_view() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("scroll_follow_external");
    let lines: Vec<String> = (0..100).map(|i| format!("line{i}")).collect();
    let buffer = Rope::from_str(&lines.join("\n"));

    // Frame 1: caret at 0, line 0 visible.
    let _ = frame(&ctx, id, &buffer, vec![], false);

    // A jump to line 60, applied the way `widget.rs` applies Ctrl+D's own.
    let target = buffer.line_to_char(60);
    set_caret(&ctx, id, Caret::at(target));

    // The frame that consumes the jump schedules the scroll but can't have
    // shaped the far row yet — same one-frame lag the arrow-down test asserts.
    let out = frame(&ctx, id, &buffer, vec![], false);
    assert_eq!(buffer.char_to_line(out.caret.expect("focused").primary), 60);
    assert!(
        !out.base.row_galleys.iter().any(|(line, _)| *line == 60),
        "line 60 shouldn't already be visible the same frame the jump is consumed"
    );

    for _ in 0..2 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = frame(&ctx, id, &buffer, vec![], false);
    }
    let out = frame(&ctx, id, &buffer, vec![], false);
    assert!(
        out.base.row_galleys.iter().any(|(line, _)| *line == 60),
        "line 60 should be visible after the scroll took effect"
    );
}

#[test]
fn typing_inserts_text_and_advances_the_caret() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("typing");
    let buffer = Rope::from_str("");
    let out = frame(
        &ctx,
        id,
        &buffer,
        vec![Event::Text("a".into()), Event::Text("b".into())],
        false,
    );
    assert_eq!(out.new_text.as_deref(), Some("ab"));
    assert_eq!(out.caret, Some(Caret::at(2)));
}

#[test]
fn end_then_backspace_deletes_the_last_char() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("backspace");
    let buffer = Rope::from_str("ab");
    let out = frame(
        &ctx,
        id,
        &buffer,
        vec![key_event(Key::End), key_event(Key::Backspace)],
        false,
    );
    assert_eq!(out.new_text.as_deref(), Some("a"));
    assert_eq!(out.caret, Some(Caret::at(1)));
}

#[test]
fn enter_splits_the_line_at_the_caret() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("enter");
    let buffer = Rope::from_str("ab");
    let out = frame(
        &ctx,
        id,
        &buffer,
        vec![key_event(Key::Home), key_event(Key::ArrowRight), key_event(Key::Enter)],
        false,
    );
    assert_eq!(out.new_text.as_deref(), Some("a\nb"));
    assert_eq!(out.caret, Some(Caret::at(2)));
}

#[test]
fn read_only_blocks_typing_but_not_navigation() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("read_only");
    let buffer = Rope::from_str("ab");
    let out = frame(
        &ctx,
        id,
        &buffer,
        vec![Event::Text("x".into()), key_event(Key::End)],
        true,
    );
    assert_eq!(out.new_text, None, "read-only must not mutate the buffer");
    assert_eq!(out.caret, Some(Caret::at(2)), "navigation stays live in read-only mode");
}

#[test]
fn select_all_then_typing_replaces_the_whole_buffer() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("select_all");
    let buffer = Rope::from_str("hello");
    let select_all = command_key_event(Key::A, Modifiers::COMMAND);
    let out = frame(&ctx, id, &buffer, vec![select_all, Event::Text("x".into())], false);
    assert_eq!(out.new_text.as_deref(), Some("x"));
}

#[test]
fn ctrl_home_goes_to_the_very_start_of_the_document_not_just_the_current_line() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("ctrl_home");
    let buffer = Rope::from_str("aaa\nbbb\nccc");
    let ctrl_home = command_key_event(Key::Home, Modifiers::COMMAND);
    // End first, on the last line, so a plain Home (line-start) and Ctrl+Home
    // (document-start) would land at genuinely different offsets — proving
    // this reaches `move_document_start`, not `move_home`.
    let out = frame(&ctx, id, &buffer, vec![key_event(Key::End), ctrl_home], false);
    assert_eq!(out.caret, Some(Caret::at(0)));
}

#[test]
fn ctrl_end_goes_to_the_very_end_of_the_document_not_just_the_current_line() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("ctrl_end");
    let buffer = Rope::from_str("aaa\nbbb\nccc");
    let ctrl_end = command_key_event(Key::End, Modifiers::COMMAND);
    let out = frame(&ctx, id, &buffer, vec![ctrl_end], false);
    assert_eq!(out.caret, Some(Caret::at(buffer.len_chars())));
}

#[test]
fn undo_restores_the_snapshot_from_before_two_frames_of_coalesced_typing() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("undo");

    let mut buffer = Rope::from_str("");
    let out = frame(&ctx, id, &buffer, vec![Event::Text("a".into())], false);
    buffer = Rope::from_str(out.new_text.as_deref().expect("first keystroke edits"));

    let out = frame(&ctx, id, &buffer, vec![Event::Text("b".into())], false);
    buffer = Rope::from_str(out.new_text.as_deref().expect("second keystroke edits"));
    assert_eq!(buffer.to_string(), "ab", "sanity: both keystrokes landed");

    // Same-kind (`Typing`) runs across frames coalesce into one undo step —
    // see `history::tests` for the pure-function proof of the coalescing
    // rule itself; this is the frame-level "keystrokes across real frames
    // actually reach it" half.
    let undo = command_key_event(Key::Z, Modifiers::COMMAND);
    let out = frame(&ctx, id, &buffer, vec![undo], false);
    assert_eq!(
        out.new_text.as_deref(),
        Some(""),
        "one undo should erase both coalesced keystrokes"
    );
}

#[test]
fn redo_reapplies_what_undo_just_undid() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("redo");

    let mut buffer = Rope::from_str("");
    let out = frame(&ctx, id, &buffer, vec![Event::Text("a".into())], false);
    buffer = Rope::from_str(out.new_text.as_deref().expect("typed"));

    let undo = command_key_event(Key::Z, Modifiers::COMMAND);
    let out = frame(&ctx, id, &buffer, vec![undo], false);
    buffer = Rope::from_str(out.new_text.as_deref().expect("undo produced a snapshot"));
    assert_eq!(buffer.to_string(), "");

    let redo = command_key_event(Key::Y, Modifiers::COMMAND);
    let out = frame(&ctx, id, &buffer, vec![redo], false);
    assert_eq!(out.new_text.as_deref(), Some("a"));
}

#[test]
fn undo_is_a_no_op_once_the_widget_turns_read_only() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("read_only_undo");

    let mut buffer = Rope::from_str("");
    let out = frame(&ctx, id, &buffer, vec![Event::Text("a".into())], false);
    buffer = Rope::from_str(out.new_text.as_deref().expect("typed while still editable"));
    assert_eq!(buffer.to_string(), "a");

    // Same `ShellState` (same `ctx`/`id`) as above, so its `History` from
    // the still-editable frame carries over — the exact "editable earlier
    // in the session, read-only now" shape a large-file guard or an
    // external-change lock produces.
    let undo = command_key_event(Key::Z, Modifiers::COMMAND);
    let out = frame(&ctx, id, &buffer, vec![undo], true);
    assert_eq!(out.new_text, None, "Ctrl+Z must not mutate a read-only buffer");
}

#[test]
fn redo_is_a_no_op_once_the_widget_turns_read_only() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("read_only_redo");

    let mut buffer = Rope::from_str("");
    let out = frame(&ctx, id, &buffer, vec![Event::Text("a".into())], false);
    buffer = Rope::from_str(out.new_text.as_deref().expect("typed"));

    let undo = command_key_event(Key::Z, Modifiers::COMMAND);
    let out = frame(&ctx, id, &buffer, vec![undo], false);
    buffer = Rope::from_str(out.new_text.as_deref().expect("undo produced a snapshot"));
    assert_eq!(buffer.to_string(), "");

    let redo = command_key_event(Key::Y, Modifiers::COMMAND);
    let out = frame(&ctx, id, &buffer, vec![redo], true);
    assert_eq!(out.new_text, None, "Ctrl+Y must not mutate a read-only buffer");
}

#[test]
fn paste_replaces_a_selection() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("paste");
    let buffer = Rope::from_str("hello");
    let select_all = command_key_event(Key::A, Modifiers::COMMAND);
    let out = frame(&ctx, id, &buffer, vec![select_all, Event::Paste("bye".into())], false);
    assert_eq!(out.new_text.as_deref(), Some("bye"));
}

#[test]
fn ime_preedit_previews_text_then_commit_finalizes_it() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("ime");

    let buffer = Rope::from_str("");
    let preedit = Event::Ime(egui::ImeEvent::Preedit {
        text: "n".into(),
        active_range_chars: Some(0..1),
    });
    let out = frame(&ctx, id, &buffer, vec![preedit], false);
    let after_preedit = out.new_text.expect("preedit previews text into the buffer");
    assert_eq!(after_preedit, "n");

    let buffer = Rope::from_str(&after_preedit);
    let commit = Event::Ime(egui::ImeEvent::Commit("ñ".into()));
    let out = frame(&ctx, id, &buffer, vec![commit], false);
    assert_eq!(
        out.new_text.as_deref(),
        Some("ñ"),
        "commit replaces the preedit preview with the final composed text"
    );
    assert_eq!(out.caret, Some(Caret::at(1)));
}

#[test]
fn peek_caret_reads_what_a_prior_frame_left_persisted() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("peek");
    assert_eq!(peek_caret(&ctx, id), None, "nothing persisted before the first frame");

    let buffer = Rope::from_str("");
    let out = frame(&ctx, id, &buffer, vec![Event::Text("a".into())], false);
    assert_eq!(peek_caret(&ctx, id), out.caret);
}

#[test]
fn set_caret_overrides_what_the_next_frame_sees_and_breaks_the_undo_run() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("set");

    let mut buffer = Rope::from_str("ab");
    // Establish some history so there's a run to break.
    let out = frame(
        &ctx,
        id,
        &buffer,
        vec![key_event(Key::End), Event::Text("c".into())],
        false,
    );
    buffer = Rope::from_str(out.new_text.as_deref().unwrap());

    set_caret(&ctx, id, Caret::at(0));
    assert_eq!(peek_caret(&ctx, id), Some(Caret::at(0)));

    // Typing right after an external caret set should start a fresh undo
    // step rather than coalescing into the run from before the jump.
    let out = frame(&ctx, id, &buffer, vec![Event::Text("X".into())], false);
    assert_eq!(out.new_text.as_deref(), Some("Xabc"));
}

#[test]
fn char_offset_for_pos_resolves_a_point_on_the_first_row_to_its_column() {
    let ctx = egui::Context::default();
    let buffer = Rope::from_str("hello\nworld");
    let mut out = None;
    let _ = ctx.run_ui(sized_raw_input(vec![]), |ui| {
        egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
            out = Some(layout_visible(
                ui,
                egui::Id::new("test"),
                &buffer,
                egui::FontId::monospace(14.0),
                &[],
                &[],
            ));
        });
    });
    let out = out.expect("layout_visible ran inside the scroll area closure");
    // Clicking at the content origin should resolve to char 0.
    let offset = char_offset_for_pos(&out, &buffer, out.content_origin);
    assert_eq!(offset, 0);
}

/// Same shape as `sized_raw_input`, but for a pointer-drag test: modifiers
/// (`Alt`, here) have to be set explicitly since, unlike `sized_raw_input`,
/// there's no `Event::Key` in these frames for it to derive them from.
fn pointer_raw_input(events: Vec<Event>, modifiers: Modifiers) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        )),
        events,
        modifiers,
        ..Default::default()
    }
}

/// Same shape as `frame`, but for `pointer_raw_input` — kept separate
/// rather than adding a `modifiers` parameter to `frame` itself, since only
/// a pointer-drag gesture needs a frame-level modifier with no accompanying
/// `Event::Key`.
fn pointer_frame(
    ctx: &egui::Context,
    id: egui::Id,
    buffer: &Rope,
    events: Vec<Event>,
    modifiers: Modifiers,
) -> ShellOutput {
    let mut result = None;
    let text = buffer.to_string();
    let _ = ctx.run_ui(pointer_raw_input(events, modifiers), |ui| {
        ui.memory_mut(|m| m.request_focus(id));
        egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
            result = Some(show(
                ui,
                id,
                buffer,
                0,
                &text,
                egui::FontId::monospace(14.0),
                egui::Color32::WHITE,
                false,
                &[],
                &[],
                false,
                true,
            ));
        });
    });
    result.expect("show ran inside the scroll area closure")
}

/// The Track 7 Phase 1 checkpoint test: a real Alt+drag gesture (press,
/// then a small move so egui itself classifies it as a drag rather than a
/// click — same distinction a real mouse gesture has — then a further move
/// two rows down and well to the right) must produce a `BlockSelection`
/// spanning every row the drag crossed, with real column width, and must
/// leave the ordinary linear `Caret` completely untouched (`SPEC.md` §7:
/// block selection is its own mode, not a reinterpretation of the existing
/// one).
#[test]
fn alt_drag_produces_a_rectangular_block_selection_spanning_multiple_rows() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("alt_drag_block");
    let buffer = Rope::from_str("aaaaaaaaaa\nbbbbbbbbbb\ncccccccccc\n");
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };

    // Frame 0: an event-free pass purely to learn this widget's real
    // on-screen geometry (content origin, row height) — the same "derive
    // real pixel positions from an actual layout pass rather than guessing
    // them" approach the `char_offset_for_pos` test above already uses.
    let geometry = frame(&ctx, id, &buffer, vec![], false).base;
    let press_pos = geometry.content_origin + egui::vec2(0.0, geometry.row_height * 0.5);
    // Just past the press point, past whatever pixel threshold egui uses to
    // tell a click from a drag apart — small enough to still land on row 0.
    let drag_start_pos = press_pos + egui::vec2(12.0, 0.0);
    let drag_pos = geometry.content_origin + egui::vec2(60.0, geometry.row_height * 2.5);

    // Frame 1: just the press — egui needs to see this land on its own
    // frame before any later movement can be classified as a drag rather
    // than folded into the same click.
    pointer_frame(
        &ctx,
        id,
        &buffer,
        vec![Event::PointerButton {
            pos: press_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: alt,
        }],
        alt,
    );

    // Frame 2: a small move while still down — the frame where egui itself
    // first classifies this gesture as a drag (`drag_started()`), so
    // *this* frame's position is what becomes the block's anchor corner.
    pointer_frame(&ctx, id, &buffer, vec![Event::PointerMoved(drag_start_pos)], alt);

    // Frame 3: the button's still down from frame 1 (egui tracks that
    // across frames on the same `Context`, the same continuity
    // `undo_restores_the_snapshot_from_before_two_frames_of_coalesced_
    // typing` above already relies on for its own multi-frame flow) — this
    // further move is a plain `dragged()` frame, extending the block's far
    // corner while its anchor (set in frame 2) stays fixed.
    let out = pointer_frame(&ctx, id, &buffer, vec![Event::PointerMoved(drag_pos)], alt);

    let block = ctx
        .data(|d| d.get_temp::<ShellState>(id))
        .and_then(|s| s.block_selection)
        .expect("Alt+drag across two rows must produce a block selection");
    assert_eq!(*block.lines().start(), 0, "the drag started on row 0");
    assert_eq!(*block.lines().end(), 2, "the drag ended on row 2");
    assert!(
        !block.cols().is_empty(),
        "the drag moved well to the right, so the block must have real width"
    );
    assert_eq!(
        out.caret,
        Some(Caret::at(0)),
        "block-select must never move the ordinary linear caret"
    );
}

/// Same as `pointer_frame`, but forces the enclosing `ScrollArea` to a
/// given vertical offset — the piece the viewport-crossing drag test below
/// needs and `pointer_frame` (always at offset 0) can't express. The
/// `id_salt` is fixed so `ScrollArea` state (and the widget's own
/// `ShellState`) persists across the multi-frame gesture on the same `ctx`.
fn scrolled_pointer_frame(
    ctx: &egui::Context,
    id: egui::Id,
    buffer: &Rope,
    events: Vec<Event>,
    offset_y: f32,
) -> ShellOutput {
    let mut result = None;
    let text = buffer.to_string();
    let _ = ctx.run_ui(pointer_raw_input(events, Modifiers::NONE), |ui| {
        ui.memory_mut(|m| m.request_focus(id));
        egui::ScrollArea::vertical()
            .max_height(400.0)
            .id_salt("scrolled_drag_scroll_area")
            .vertical_scroll_offset(offset_y)
            .show(ui, |ui| {
                result = Some(show(
                    ui,
                    id,
                    buffer,
                    0,
                    &text,
                    egui::FontId::monospace(14.0),
                    egui::Color32::WHITE,
                    false,
                    &[],
                    &[],
                    false,
                    true,
                ));
            });
    });
    result.expect("show ran inside the scroll area closure")
}

/// Track 19 Phase 3's still-owed live-verify, closed headlessly instead: a
/// mouse drag-select whose *anchor* line scrolls entirely out of the shaped
/// viewport mid-gesture — the one scenario `xdotool` against a WM-less Xvfb
/// could never reproduce (see the track's own two follow-up-session notes).
/// It's exactly the case virtualization has to get right: only the visible
/// slice is ever shaped, yet the selection has to stay anchored to a char
/// offset the widget is no longer painting. Proven deep in a 2,000-line
/// buffer, with the anchor row forced above the viewport top by a real
/// `ScrollArea` offset change between the drag-start and the drag frames —
/// not a seeded `Caret`, a real press → move → move pointer gesture.
#[test]
fn drag_select_stays_anchored_when_its_anchor_scrolls_out_of_view() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("scrolled_drag");
    let lines: Vec<String> = (0..2000).map(|i| format!("line{i:04}")).collect();
    let buffer = Rope::from_str(&lines.join("\n"));

    // One event-free probe (at the top) purely to learn real pixel geometry
    // — row height and the content's left edge — rather than guessing it,
    // the same approach `alt_drag_...` uses. Vertical scroll never changes
    // either, so the top is as good a probe point as any.
    let probe = scrolled_pointer_frame(&ctx, id, &buffer, vec![], 0.0).base;
    let rh = probe.row_height;
    let ox = probe.content_origin.x;

    // The anchor lands two rows below each viewport's top, so it's
    // comfortably on-screen when pressed; the far end likewise. `content_
    // origin.y` is `-offset` for a `ScrollArea` at a forced offset (its
    // content top scrolled that far above the viewport top), so both
    // positions resolve to the same 2.5-row screen y under their own frame's
    // offset — well inside the 400px viewport.
    let anchor_line = 1002usize;
    let far_line = 1015usize;
    let offset1 = (anchor_line - 2) as f32 * rh;
    let offset2 = (far_line - 2) as f32 * rh;
    let screen_y = 2.5 * rh;
    let press_pos = egui::pos2(ox + 4.0, screen_y);
    // Just past egui's click-vs-drag threshold, still on the anchor row.
    let drag_start_pos = press_pos + egui::vec2(12.0, 0.0);
    let far_pos = egui::pos2(ox + 4.0, screen_y);

    // Frame 1: the press, on its own frame, so egui can classify the later
    // move as a drag rather than fold it into a click.
    scrolled_pointer_frame(
        &ctx,
        id,
        &buffer,
        vec![Event::PointerButton {
            pos: press_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
        offset1,
    );
    // Frame 2: a small move while still down — `drag_started()`, so this
    // frame's position (still the anchor row) fixes the anchor.
    scrolled_pointer_frame(&ctx, id, &buffer, vec![Event::PointerMoved(drag_start_pos)], offset1);
    // Frame 3: the viewport has scrolled down 13 rows (the anchor row is now
    // above its top, no longer shaped) and the still-held pointer moves to a
    // row deep in the *new* viewport — a plain `dragged()` frame extending
    // the far end while the off-screen anchor stays put.
    let out = scrolled_pointer_frame(&ctx, id, &buffer, vec![Event::PointerMoved(far_pos)], offset2);

    let caret = ctx
        .data(|d| d.get_temp::<ShellState>(id))
        .map(|s| s.caret)
        .expect("the drag gesture ran, so ShellState exists");

    assert_eq!(
        buffer.char_to_line(caret.anchor),
        anchor_line,
        "the anchor stays pinned to the line the drag started on"
    );
    assert_eq!(
        buffer.char_to_line(caret.primary),
        far_line,
        "the moving end followed the pointer to the far line"
    );
    assert!(!caret.is_collapsed(), "a real multi-line selection, not a collapsed caret");

    // The whole point: the anchor line is genuinely off-screen in the final
    // frame (only the visible slice was shaped), yet the selection above is
    // still correct. In the no-wrap case a visual row is its own line.
    assert!(
        !out.base.visible_rows.contains(&anchor_line),
        "the anchor line must have scrolled out of the shaped viewport"
    );
    assert!(
        out.base.visible_rows.contains(&far_line),
        "the far line must be inside the shaped viewport it was clicked in"
    );
}

/// The Track 7 Phase 2 checkpoint test: with a block selection already
/// active (seeded directly rather than re-driving the drag gesture — that
/// path is `alt_drag_produces_a_rectangular_block_selection_spanning_
/// multiple_rows`'s own job above), typing must edit every spanned row at
/// the same column and must still leave the ordinary linear `Caret`
/// completely alone, exactly like Phase 1's drag itself does.
#[test]
fn typing_over_an_active_block_selection_edits_every_spanned_row() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("block_type");
    let buffer = Rope::from_str("aaaa\nbbbb\ncccc");

    ctx.data_mut(|d| {
        d.insert_temp(
            id,
            ShellState {
                block_selection: Some(BlockSelection::at(0, 2).moved_to(2, 2)),
                ..ShellState::default()
            },
        );
    });

    let out = frame(&ctx, id, &buffer, vec![Event::Text("X".into())], false);

    assert_eq!(out.new_text.as_deref(), Some("aaXaa\nbbXbb\nccXcc"));
    assert_eq!(
        out.caret,
        Some(Caret::at(0)),
        "block-scoped typing must not move the ordinary linear caret"
    );
    let block = ctx
        .data(|d| d.get_temp::<ShellState>(id))
        .and_then(|s| s.block_selection)
        .expect("the block selection stays active across the edit, so further typing keeps working");
    assert_eq!(block.cols(), 3..3, "collapses right after the inserted char, same as every spanned row");
}

/// The Track 7 Phase 3 checkpoint test: pasting a multi-line clipboard while
/// a block selection is active replaces each spanned row's own column range
/// with the matching clipboard line, same seeded-state pattern as the Phase
/// 2 typing test above, and leaves the ordinary linear `Caret` untouched.
#[test]
fn pasting_over_an_active_block_selection_replaces_each_spanned_row_with_its_matching_clipboard_line() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("block_paste");
    let buffer = Rope::from_str("aXXa\nbXXb\ncXXc");

    ctx.data_mut(|d| {
        d.insert_temp(
            id,
            ShellState {
                block_selection: Some(BlockSelection::at(0, 1).moved_to(2, 3)),
                ..ShellState::default()
            },
        );
    });

    let out = frame(&ctx, id, &buffer, vec![Event::Paste("11\n22\n33".into())], false);

    assert_eq!(out.new_text.as_deref(), Some("a11a\nb22b\nc33c"));
    assert_eq!(
        out.caret,
        Some(Caret::at(0)),
        "block-scoped paste must not move the ordinary linear caret"
    );
    let block = ctx
        .data(|d| d.get_temp::<ShellState>(id))
        .and_then(|s| s.block_selection)
        .expect("the block selection stays active across the paste");
    assert_eq!(block.cols(), 3..3, "collapses right after the first row's own pasted text");
}
