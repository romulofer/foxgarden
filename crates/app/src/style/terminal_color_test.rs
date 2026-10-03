
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
    assert_eq!(
        terminal_fg(vt100::Color::Idx(196), false, true),
        Color32::from_rgb(255, 0, 0)
    );
    assert_eq!(
        terminal_fg(vt100::Color::Idx(196), false, false),
        Color32::from_rgb(255, 0, 0)
    );
}

#[test]
fn extended_grayscale_ramp_endpoints() {
    assert_eq!(
        terminal_fg(vt100::Color::Idx(232), false, true),
        Color32::from_rgb(8, 8, 8)
    );
    assert_eq!(
        terminal_fg(vt100::Color::Idx(255), false, true),
        Color32::from_rgb(238, 238, 238)
    );
}
