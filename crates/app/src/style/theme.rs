use egui::Color32;
use syntax::Scope;

/// raylib's `RAYWHITE` (245, 245, 245, 255) — used as the light theme's
/// background, per user request.
pub const RAYWHITE: Color32 = Color32::from_rgb(245, 245, 245);

/// Applies this app's light or dark visuals to `ctx`. Shared by startup
/// restoration (`app::FoxGardenApp::new`, once persisted settings are
/// loaded) and the Settings > Theme menu buttons, so both stay in sync
/// with exactly one place that knows what "light" means for this app — not
/// just `egui::Visuals::light()`, but that plus repainting the background
/// to `RAYWHITE`.
pub fn apply(ctx: &egui::Context, dark_mode: bool) {
    if dark_mode {
        ctx.set_visuals(egui::Visuals::dark());
    } else {
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = RAYWHITE;
        visuals.window_fill = RAYWHITE;
        visuals.extreme_bg_color = RAYWHITE;
        ctx.set_visuals(visuals);
    }
}

const DARK_TEXT: Color32 = Color32::from_rgb(171, 178, 191);
const LIGHT_TEXT: Color32 = Color32::from_rgb(56, 58, 66);

const DARK_ERROR_SQUIGGLE: Color32 = Color32::from_rgb(224, 82, 82);
const LIGHT_ERROR_SQUIGGLE: Color32 = Color32::from_rgb(202, 42, 42);

const DARK_LINE_NUMBER: Color32 = Color32::from_rgb(92, 99, 112);
const LIGHT_LINE_NUMBER: Color32 = Color32::from_rgb(160, 160, 160);

/// The editor's default (non-highlighted) text color, adapted for legibility
/// against the current theme's background — `color_for_scope`'s dark-theme
/// palette reads poorly against `RAYWHITE`, so both need the `dark_mode`
/// flag, not just the chrome that `egui::Visuals` already covers.
pub fn default_text(dark_mode: bool) -> Color32 {
    if dark_mode { DARK_TEXT } else { LIGHT_TEXT }
}

pub fn error_squiggle(dark_mode: bool) -> Color32 {
    if dark_mode {
        DARK_ERROR_SQUIGGLE
    } else {
        LIGHT_ERROR_SQUIGGLE
    }
}

/// Deliberately muted relative to `default_text` — line numbers are a
/// glance-able reference, not something that should compete with the code
/// itself for attention.
pub fn line_number(dark_mode: bool) -> Color32 {
    if dark_mode {
        DARK_LINE_NUMBER
    } else {
        LIGHT_LINE_NUMBER
    }
}

pub fn color_for_scope(scope: Scope, dark_mode: bool) -> Color32 {
    if dark_mode {
        match scope {
            Scope::Keyword => Color32::from_rgb(198, 120, 221),
            Scope::String => Color32::from_rgb(152, 195, 121),
            Scope::Comment => Color32::from_rgb(124, 132, 148),
            Scope::Type => Color32::from_rgb(229, 192, 123),
            Scope::Function => Color32::from_rgb(97, 175, 239),
            Scope::Property => Color32::from_rgb(224, 108, 117),
            Scope::Tag => Color32::from_rgb(86, 182, 194),
        }
    } else {
        match scope {
            Scope::Keyword => Color32::from_rgb(166, 38, 164),
            Scope::String => Color32::from_rgb(80, 138, 51),
            Scope::Comment => Color32::from_rgb(140, 140, 140),
            Scope::Type => Color32::from_rgb(152, 104, 1),
            Scope::Function => Color32::from_rgb(37, 106, 194),
            Scope::Property => Color32::from_rgb(228, 86, 73),
            Scope::Tag => Color32::from_rgb(24, 141, 148),
        }
    }
}
