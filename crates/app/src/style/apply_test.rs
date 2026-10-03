
use super::*;

/// `apply` must assert the hover-grows scroll bar itself, not just trust
/// whatever `Style` a prior session's persisted `egui::Memory` restored
/// — regresses if a future edit ever drops the `all_styles_mut` call and
/// leaves it to chance again.
#[test]
fn apply_forces_a_floating_hover_grow_scrollbar_in_both_themes() {
    let ctx = egui::Context::default();
    // Simulate a stale persisted style that predates hover-grow — a
    // solid, fixed-width scroll bar in both themes.
    ctx.all_styles_mut(|style| style.spacing.scroll = egui::style::ScrollStyle::solid());

    apply(&ctx, true);
    apply(&ctx, false);

    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        let scroll = ctx.style_of(theme).spacing.scroll;
        assert!(scroll.floating, "{theme:?} scroll bar should float/hover-grow");
        assert!(
            scroll.bar_width > scroll.floating_width,
            "{theme:?} hovered width ({}) should exceed the resting width ({})",
            scroll.bar_width,
            scroll.floating_width
        );
    }
}
