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
    // Capped to the window and scrollable inside that cap. A modal sizes
    // itself to its content, which is fine at 100% and stops being fine
    // under Settings > Accessibility…'s interface zoom: at 300% this
    // dialog's own buttons rendered past the right and bottom edges of the
    // window, including the "Back to 100%" that undoes the zoom — found
    // live, and a trap rather than a cosmetic problem, since the setting
    // that caused it was then unreachable. Every modal gets the same
    // treatment because every modal has the same failure mode.
    let screen = ctx.content_rect();
    let max_size = egui::vec2(screen.width() * 0.9, screen.height() * 0.85);
    let response = egui::Modal::new(egui::Id::new(id)).show(&ctx, |ui| {
        ui.set_max_size(max_size);
        egui::ScrollArea::both()
            .id_salt((id, "modal_scroll"))
            .show(ui, |ui| body(ui, &data))
            .inner
    });
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
#[path = "modal_test.rs"]
mod modal_test;
