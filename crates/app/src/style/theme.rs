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

/// Deliberately subtle — a passive, read-only "here's where else this word
/// appears" cue, not a selection, so it must never compete visually with
/// `egui::Visuals::selection`'s own background fill.
const DARK_OCCURRENCE_HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(140, 140, 140, 55);
const LIGHT_OCCURRENCE_HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(90, 90, 90, 35);

/// A matched bracket pair's outline — distinct from `occurrence_highlight`'s
/// fill (a box outline reads as "these two characters pair up," not "this
/// span is selected/repeated," so it shouldn't share that fill's visual
/// vocabulary) and, unlike it, the same color in both themes: it's already
/// deliberately muted via alpha rather than via a theme-specific hue pick.
const BRACKET_MATCH: Color32 = Color32::from_rgba_premultiplied(120, 120, 120, 110);

/// Whitespace markers and indentation guides are both "structural, not
/// content" cues — deliberately fainter than `line_number` (already the
/// dimmest text-like color in the palette) so a line dense with spaces
/// doesn't out-compete the code itself for attention.
const DARK_STRUCTURE: Color32 = Color32::from_rgba_premultiplied(120, 120, 120, 60);
const LIGHT_STRUCTURE: Color32 = Color32::from_rgba_premultiplied(120, 120, 120, 70);

/// Background band behind sticky-scroll's pinned header lines. Deliberately
/// **opaque** (unlike the alpha-blended structural cues above): its whole job
/// is to occlude the scrolled body text passing underneath the pinned
/// signatures, which a translucent fill couldn't do. A hair off the editor
/// background in each theme so the band still reads as pinned chrome rather
/// than blending invisibly into the content.
const DARK_STICKY_BACKGROUND: Color32 = Color32::from_rgb(45, 48, 56);
const LIGHT_STICKY_BACKGROUND: Color32 = Color32::from_rgb(235, 235, 238);

pub fn sticky_background(dark_mode: bool) -> Color32 {
    if dark_mode {
        DARK_STICKY_BACKGROUND
    } else {
        LIGHT_STICKY_BACKGROUND
    }
}

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
    if dark_mode { DARK_LINE_NUMBER } else { LIGHT_LINE_NUMBER }
}

pub fn occurrence_highlight(dark_mode: bool) -> Color32 {
    if dark_mode {
        DARK_OCCURRENCE_HIGHLIGHT
    } else {
        LIGHT_OCCURRENCE_HIGHLIGHT
    }
}

pub fn bracket_match(_dark_mode: bool) -> Color32 {
    BRACKET_MATCH
}

pub fn structure(dark_mode: bool) -> Color32 {
    if dark_mode { DARK_STRUCTURE } else { LIGHT_STRUCTURE }
}

/// `vt100`'s 16 indexed ANSI colors (0-7 normal, 8-15 bright), tinted per
/// theme rather than the raw xterm RGB values (`SPEC.md` §8.4: "map onto the
/// current theme's own color table ... not the raw ANSI 16-color palette
/// verbatim") — a colored shell prompt using stock ANSI red/green/etc. would
/// otherwise clash with `color_for_scope`'s own muted palette right above it.
/// Extended (256-color/truecolor) values are deliberately *not* remapped here
/// (see `terminal_fg`/`terminal_bg`) — those are a program's own explicit hue
/// choice, unlike the base 16, which every real terminal emulator already
/// treats as theme-customizable.
const DARK_ANSI: [Color32; 16] = [
    Color32::from_rgb(40, 44, 52),
    Color32::from_rgb(224, 82, 82),
    Color32::from_rgb(152, 195, 121),
    Color32::from_rgb(229, 192, 123),
    Color32::from_rgb(97, 175, 239),
    Color32::from_rgb(198, 120, 221),
    Color32::from_rgb(86, 182, 194),
    Color32::from_rgb(171, 178, 191),
    Color32::from_rgb(92, 99, 112),
    Color32::from_rgb(255, 110, 110),
    Color32::from_rgb(180, 220, 140),
    Color32::from_rgb(245, 216, 150),
    Color32::from_rgb(130, 200, 255),
    Color32::from_rgb(220, 150, 240),
    Color32::from_rgb(120, 210, 220),
    Color32::from_rgb(255, 255, 255),
];

const LIGHT_ANSI: [Color32; 16] = [
    Color32::from_rgb(56, 58, 66),
    Color32::from_rgb(202, 42, 42),
    Color32::from_rgb(80, 138, 51),
    Color32::from_rgb(152, 104, 1),
    Color32::from_rgb(37, 106, 194),
    Color32::from_rgb(166, 38, 164),
    Color32::from_rgb(24, 141, 148),
    Color32::from_rgb(160, 160, 160),
    Color32::from_rgb(110, 110, 110),
    Color32::from_rgb(230, 80, 80),
    Color32::from_rgb(110, 170, 70),
    Color32::from_rgb(190, 140, 20),
    Color32::from_rgb(60, 130, 220),
    Color32::from_rgb(190, 60, 190),
    Color32::from_rgb(40, 165, 175),
    Color32::from_rgb(30, 30, 30),
];

/// The 6x6x6 color cube xterm-256 defines for indices 16-231 — the
/// standard, fixed RGB levels every terminal emulator agrees on (not a value
/// this app gets to reinterpret per theme, unlike the base 16 above).
const ANSI256_CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Resolves an indexed `vt100::Color::Idx` (0-255) to a concrete `Color32`:
/// 0-15 through this theme's own `DARK_ANSI`/`LIGHT_ANSI` (bold promotes a
/// normal color, 0-7, to its bright counterpart, 8-15 — the standard
/// "bold-as-bright" fallback terminals use with no separate bold font
/// loaded, and the only weight this app's bundled `JetBrainsMono-Regular.ttf`
/// has), 16-231 through the fixed xterm color cube, 232-255 through its
/// grayscale ramp.
fn indexed_color(idx: u8, bold: bool, dark_mode: bool) -> Color32 {
    if idx < 16 {
        let palette = if dark_mode { &DARK_ANSI } else { &LIGHT_ANSI };
        let idx = if bold && idx < 8 { idx + 8 } else { idx };
        palette[usize::from(idx)]
    } else if idx < 232 {
        let cube = idx - 16;
        let r = ANSI256_CUBE_LEVELS[usize::from(cube / 36)];
        let g = ANSI256_CUBE_LEVELS[usize::from((cube / 6) % 6)];
        let b = ANSI256_CUBE_LEVELS[usize::from(cube % 6)];
        Color32::from_rgb(r, g, b)
    } else {
        let level = 8 + (idx - 232) * 10;
        Color32::from_rgb(level, level, level)
    }
}

/// A terminal cell's foreground color (`SPEC.md` §8.4) — `vt100::Color::
/// Default` falls back to `default_text` (the same color the editor itself
/// uses for unhighlighted text), `Idx`/`Rgb` resolve via `indexed_color`/
/// passthrough respectively.
pub fn terminal_fg(color: vt100::Color, bold: bool, dark_mode: bool) -> Color32 {
    match color {
        vt100::Color::Default => default_text(dark_mode),
        vt100::Color::Idx(idx) => indexed_color(idx, bold, dark_mode),
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// The background counterpart of `terminal_fg`. `Color::Default` resolves to
/// `Color32::TRANSPARENT` rather than an opaque fill matching the panel's own
/// background — painting nothing for the common case (most cells never set
/// an explicit background) is equivalent to painting the panel's own fill,
/// without needing this theme-only module to know what that fill is.
pub fn terminal_bg(color: vt100::Color, dark_mode: bool) -> Color32 {
    match color {
        vt100::Color::Default => Color32::TRANSPARENT,
        vt100::Color::Idx(idx) => indexed_color(idx, false, dark_mode),
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
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
            Scope::Constant => Color32::from_rgb(209, 154, 102),
            // A dusty rose close to (but distinct from) `Property`'s coral
            // red — a parameter and a field are closely related lexical
            // roles (both "a name you read/write, not code structure"), so
            // a related-but-different hue reads as "kin to Property," not
            // an arbitrary new color competing with it.
            Scope::Parameter => Color32::from_rgb(198, 145, 145),
            // Deliberately muted — operators appear on nearly every line,
            // so a saturated color here would compete with the code itself
            // far more than any other scope's own frequency ever would.
            Scope::Operator => Color32::from_rgb(130, 145, 158),
            // A rare token (one `labeled_statement` per file at most, in
            // practice) can afford a more distinctive color; warm gold
            // keeps it out of every other scope's own hue family.
            Scope::Label => Color32::from_rgb(216, 176, 92),
            // A desaturated green — between `Comment`'s gray and
            // `String`'s fuller green — so a doc comment reads as "still a
            // comment" while standing out as more structured/significant
            // than a plain one.
            Scope::DocComment => Color32::from_rgb(109, 145, 120),
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
            Scope::Constant => Color32::from_rgb(193, 132, 1),
            Scope::Parameter => Color32::from_rgb(166, 92, 92),
            Scope::Operator => Color32::from_rgb(95, 110, 125),
            Scope::Label => Color32::from_rgb(150, 110, 20),
            Scope::DocComment => Color32::from_rgb(70, 115, 75),
        }
    }
}

#[cfg(test)]
mod terminal_color_tests {
    use super::*;

    #[test]
    fn default_fg_matches_editor_text_color() {
        assert_eq!(terminal_fg(vt100::Color::Default, false, true), default_text(true));
        assert_eq!(terminal_fg(vt100::Color::Default, false, false), default_text(false));
    }

    #[test]
    fn default_bg_is_transparent() {
        assert_eq!(terminal_bg(vt100::Color::Default, true), Color32::TRANSPARENT);
        assert_eq!(terminal_bg(vt100::Color::Default, false), Color32::TRANSPARENT);
    }

    #[test]
    fn indexed_fg_resolves_through_this_themes_own_palette_not_raw_ansi() {
        // Red (Idx(1)) is theme-tinted, not xterm's raw (205, 49, 49)-ish red.
        assert_eq!(terminal_fg(vt100::Color::Idx(1), false, true), DARK_ANSI[1]);
        assert_eq!(terminal_fg(vt100::Color::Idx(1), false, false), LIGHT_ANSI[1]);
    }

    #[test]
    fn bold_promotes_normal_idx_to_its_bright_counterpart() {
        assert_eq!(terminal_fg(vt100::Color::Idx(1), true, true), DARK_ANSI[9]);
        assert_eq!(terminal_fg(vt100::Color::Idx(1), true, false), LIGHT_ANSI[9]);
    }

    #[test]
    fn bold_does_not_wrap_an_already_bright_idx() {
        assert_eq!(terminal_fg(vt100::Color::Idx(9), true, true), DARK_ANSI[9]);
    }

    #[test]
    fn bold_is_ignored_for_backgrounds() {
        assert_eq!(terminal_bg(vt100::Color::Idx(1), true), DARK_ANSI[1]);
    }

    #[test]
    fn rgb_passes_through_unchanged_regardless_of_theme() {
        let rgb = vt100::Color::Rgb(10, 20, 30);
        assert_eq!(terminal_fg(rgb, false, true), Color32::from_rgb(10, 20, 30));
        assert_eq!(terminal_bg(rgb, false), Color32::from_rgb(10, 20, 30));
    }

    #[test]
    fn extended_256_color_cube_is_not_theme_tinted() {
        // Idx(196) is the cube's own pure red (r=5,g=0,b=0 -> 255,0,0),
        // same value in both themes since only the base 16 get remapped.
        assert_eq!(terminal_fg(vt100::Color::Idx(196), false, true), Color32::from_rgb(255, 0, 0));
        assert_eq!(terminal_fg(vt100::Color::Idx(196), false, false), Color32::from_rgb(255, 0, 0));
    }

    #[test]
    fn extended_grayscale_ramp_endpoints() {
        assert_eq!(terminal_fg(vt100::Color::Idx(232), false, true), Color32::from_rgb(8, 8, 8));
        assert_eq!(terminal_fg(vt100::Color::Idx(255), false, true), Color32::from_rgb(238, 238, 238));
    }
}
