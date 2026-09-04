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
            let is_dockerfile = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| fg_core::Language::from_filename(n).is_some());
            if is_dockerfile { DOCKER } else { FILE }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn each_language_gets_its_own_icon() {
        assert_eq!(for_file(Path::new("Main.java")), JAVA);
        assert_eq!(for_file(Path::new("Main.kt")), KOTLIN);
        assert_eq!(for_file(Path::new("pom.xml")), XML);
        assert_eq!(for_file(Path::new("application.yml")), YAML);
        assert_eq!(for_file(Path::new("application.properties")), PROPERTIES);
        assert_eq!(for_file(Path::new("Dockerfile")), DOCKER);
        assert_eq!(for_file(Path::new("README.md")), FILE);
        assert_eq!(for_file(Path::new("no-extension")), FILE);
    }

    /// The whole point of moving off emoji: every glyph must exist in the
    /// font this app embeds, not in whatever the host happens to have.
    #[test]
    fn every_icon_is_in_the_bundled_symbols_font() {
        // Private Use Area blocks the Nerd Fonts symbols live in.
        let in_nerd_font_range = |c: char| {
            let cp = c as u32;
            (0xe000..=0xf8ff).contains(&cp) || (0xf0000..=0xfffff).contains(&cp)
        };
        for icon in [
            FOLDER, FOLDER_OPEN, FILE, JAVA, KOTLIN, XML, YAML, PROPERTIES, DOCKER, OPEN_FOLDER, NEW_FILE, TERMINAL,
            LOCK, UNSAVED, CLOSE, ERROR, WARNING,
        ] {
            assert!(in_nerd_font_range(icon), "{icon:?} (U+{:04X}) is outside the bundled font", icon as u32);
        }
    }
}
