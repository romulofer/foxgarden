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

/// Files, by language.
pub const FILE: char = '\u{f15b}';
pub const JAVA: char = '\u{e738}';
pub const KOTLIN: char = '\u{e634}';
pub const XML: char = '\u{e619}';
pub const YAML: char = '\u{f0219}';
pub const PROPERTIES: char = '\u{f013}';
pub const DOCKER: char = '\u{f308}';

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

/// The icon for `path`, chosen by extension, with `Dockerfile`-style
/// extensionless names falling back to a name check the same way
/// `Document::open` recognizes them.
pub fn for_file(path: &std::path::Path) -> char {
    match path.extension().and_then(|e| e.to_str()) {
        Some("java") => JAVA,
        Some("kt") => KOTLIN,
        Some("properties") => PROPERTIES,
        Some("yml" | "yaml") => YAML,
        Some("xml") => XML,
        _ => {
            // Checked by name rather than through the language registry:
            // this whole function is a hardcoded extension-to-glyph table
            // (see the arms above), so routing one of its cases through a
            // registry it otherwise never consults would be inconsistent
            // rather than more correct. Contributed icons are a real idea,
            // but they belong with the contributed-UI question `SPEC.md`
            // §24 defers until a second extension exists to shape it.
            let is_dockerfile = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| {
                    let lower = n.to_lowercase();
                    lower == "dockerfile" || lower.starts_with("dockerfile.") || lower.ends_with(".dockerfile")
                });
            if is_dockerfile { DOCKER } else { FILE }
        }
    }
}

#[cfg(test)]
#[path = "icons_test.rs"]
mod icons_test;
