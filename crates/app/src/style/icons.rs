//! Every icon glyph the UI draws, in one place.
//!
//! These used to be emoji (`📁`, `☕`, `🔷`, …), which reads fine on a
//! machine that happens to have a colour emoji font installed and renders
//! as a row of empty boxes — file names, tabs, the whole project tree —
//! on one that doesn't. That isn't hypothetical: a clean environment
//! (verified in a bare X server this session) shows tofu for every one of
//! them, because egui's own bundled fonts cover only a small emoji subset
//! and nothing else in the chain covered the rest.
//!
//! The glyphs here come from `SymbolsNerdFontMono`, which this app already
//! embeds (`style::fonts`) for the terminal panel's sake — so they ship
//! with the binary and look identical everywhere, with no dependency on
//! what the host has installed. Codepoints are from the Nerd Fonts cheat
//! sheet and were each verified present in the embedded font file.

/// A directory in the project tree, closed and open.
pub const FOLDER: char = '\u{f07b}';
pub const FOLDER_OPEN: char = '\u{f07c}';

/// A file no extension gave an icon to. Per-language icons are contributed
/// (`Contributions::file_icons`).
pub const FILE: char = '\u{f15b}';

/// Side-panel toolbar actions.
pub const OPEN_FOLDER: char = '\u{f07c}';
pub const NEW_FILE: char = '\u{f0214}';
pub const TERMINAL: char = '\u{f120}';

/// A read-only tab, and the dot marking one with unsaved changes.
pub const LOCK: char = '\u{f023}';
pub const UNSAVED: char = '\u{f111}';
/// A tab's close button.
pub const CLOSE: char = '\u{f00d}';

/// Status-bar diagnostics counters.
pub const ERROR: char = '\u{f057}';
pub const WARNING: char = '\u{f071}';

/// The icon for `path`: the one its language's extension contributed, else
/// the generic file glyph.
pub fn for_file(path: &std::path::Path) -> char {
    syntax::file_icon(path).unwrap_or(FILE)
}

#[cfg(test)]
#[path = "icons_test.rs"]
mod icons_test;
