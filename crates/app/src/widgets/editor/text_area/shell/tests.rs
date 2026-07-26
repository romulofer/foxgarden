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
        egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
            result = Some(show(
                ui,
                id,
                buffer,
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
