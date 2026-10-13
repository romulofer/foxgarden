
use super::*;
use std::path::Path;

#[test]
fn each_language_gets_its_own_icon() {
    test_support::install_grammars();
    assert_eq!(for_file(Path::new("Main.java")), '\u{e738}');
    assert_eq!(for_file(Path::new("Main.kt")), '\u{e634}');
    assert_eq!(for_file(Path::new("pom.xml")), '\u{e619}');
    assert_eq!(for_file(Path::new("application.yml")), '\u{f0219}');
    assert_eq!(for_file(Path::new("application.properties")), '\u{f013}');
    assert_eq!(for_file(Path::new("Dockerfile")), '\u{f308}');
    assert_eq!(for_file(Path::new("Dockerfile.dev")), '\u{f308}');
    assert_eq!(for_file(Path::new("Main.JAVA")), '\u{e738}');
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
    let registry = test_support::languages();
    let contributed: Vec<char> = registry
        .languages()
        .filter_map(|language| registry.file_icon(&language.language.id))
        .collect();
    assert!(contributed.len() >= 6, "the shipped extensions contribute icons");
    for icon in contributed.into_iter().chain([
        FOLDER,
        FOLDER_OPEN,
        FILE,
        OPEN_FOLDER,
        NEW_FILE,
        TERMINAL,
        LOCK,
        UNSAVED,
        CLOSE,
        ERROR,
        WARNING,
    ]) {
        assert!(
            in_nerd_font_range(icon),
            "{icon:?} (U+{:04X}) is outside the bundled font",
            icon as u32
        );
    }
}
