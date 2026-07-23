/// Shows a modal with `id` if `guard` is `Some`, calling `body` with the
/// guard's contents to draw the label/buttons. Returns whatever `body`
/// returns, so callers can signal outcomes (a dismissal, a confirmed
/// action) back out without needing interior mutability inside the
/// closure — `body` is free to mutate whatever state it captures by
/// reference from the caller's scope, same as it could inside a bare
/// `egui::Modal::show` call.
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
) -> Option<R> {
    let data = guard?;
    let ctx = ui.ctx().clone();
    Some(egui::Modal::new(egui::Id::new(id)).show(&ctx, |ui| body(ui, &data)).inner)
}
