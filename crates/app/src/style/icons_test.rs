
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
        FOLDER,
        FOLDER_OPEN,
        FILE,
        JAVA,
        KOTLIN,
        XML,
        YAML,
        PROPERTIES,
        DOCKER,
        OPEN_FOLDER,
        NEW_FILE,
        TERMINAL,
        LOCK,
        UNSAVED,
        CLOSE,
        ERROR,
        WARNING,
    ] {
        assert!(
            in_nerd_font_range(icon),
            "{icon:?} (U+{:04X}) is outside the bundled font",
            icon as u32
        );
    }
}
