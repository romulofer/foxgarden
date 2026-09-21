
use super::*;

fn escape_event() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// Runs one frame with `events` queued and `show_modal` called exactly
/// as every real call site does, returning what it returned. `warm_up`
/// runs one prior, event-less frame first — egui's `Modal` only knows
/// it's the topmost modal (`ModalResponse::is_top_modal`, which
/// `escape_pressed` requires) from the *previous* frame's layer
/// bookkeeping, promoted at that frame's end; on a modal's first-ever
/// frame there is no previous frame yet, so it reads as not-topmost and
/// Escape wouldn't register. That's a non-issue for a real dialog
/// (it's already rendered at least once by the time a user reacts to
/// it), but a single-frame test needs the same warm-up to match.
fn run_modal(guard: Option<()>, warm_up: bool, events: Vec<egui::Event>) -> Option<((), bool)> {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let show = |ui: &mut egui::Ui| {
        show_modal(ui, "test_modal", guard, |ui, ()| {
            ui.label("hello");
        })
    };

    if warm_up {
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui);
        });
    }

    let raw_input = egui::RawInput {
        events,
        ..Default::default()
    };
    let mut outcome = None;
    let _ = ctx.run_ui(raw_input, |ui| {
        outcome = show(ui);
    });
    outcome
}

#[test]
fn escape_closes_an_already_open_modal() {
    let (_, escape_pressed) = run_modal(Some(()), true, vec![escape_event()]).expect("modal was open");
    assert!(escape_pressed);
}

#[test]
fn no_escape_event_leaves_the_modal_open() {
    let (_, escape_pressed) = run_modal(Some(()), true, vec![]).expect("modal was open");
    assert!(!escape_pressed);
}

#[test]
fn a_closed_modal_guard_short_circuits_to_none_even_with_escape_queued() {
    assert_eq!(run_modal(None, false, vec![escape_event()]), None);
}
