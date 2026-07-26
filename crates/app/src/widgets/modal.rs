/// Shows a modal with `id` if `guard` is `Some`, calling `body` with the
/// guard's contents to draw the label/buttons. Returns `Some((result,
/// escape_pressed))`, so callers can signal outcomes (a dismissal, a
/// confirmed action) back out without needing interior mutability inside
/// the closure — `body` is free to mutate whatever state it captures by
/// reference from the caller's scope, same as it could inside a bare
/// `egui::Modal::show` call. `escape_pressed` is `true` exactly when this
/// modal is the topmost one, no popup (e.g. a combo box) inside it is
/// open, and Escape was pressed this frame — every caller must treat that
/// identically to its own "Cancel"/dismiss action, since a modal that
/// doesn't close on Escape is a common, easy-to-miss papercut.
///
/// Factors out the "clone `ctx`, open a `Modal` with this id" skeleton
/// every confirm/about/error dialog in this app was independently
/// reimplementing (`app::show_error_modal`, `menu_bar::show_about`,
/// `tabs::show_close_confirm`, `side_panel::show_delete_confirm`) — see the
/// now-resolved entry in `TECHNICAL_DEBT.md` for why that duplication was
/// worth fixing rather than leaving as four near-identical copies.
pub fn show_modal<T, R>(
    ui: &egui::Ui,
    id: &str,
    guard: Option<T>,
    body: impl FnOnce(&mut egui::Ui, &T) -> R,
) -> Option<(R, bool)> {
    let data = guard?;
    let ctx = ui.ctx().clone();
    let response = egui::Modal::new(egui::Id::new(id)).show(&ctx, |ui| body(ui, &data));
    // Deliberately narrower than `ModalResponse::should_close` (which also
    // treats a backdrop click as a close) — only Escape was asked for, and
    // backdrop-click-to-close is a distinct UX decision this app hasn't
    // made yet.
    let escape_pressed = response.is_top_modal
        && !response.any_popup_open
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    Some((response.inner, escape_pressed))
}

#[cfg(test)]
mod tests {
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
}
