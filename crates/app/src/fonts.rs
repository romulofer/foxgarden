const JETBRAINS_MONO_KEY: &str = "JetBrainsMono";

/// Registers the bundled JetBrains Mono under its own font family, alongside
/// egui's built-in families (left untouched, reachable as `EditorFont::Default`).
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        JETBRAINS_MONO_KEY.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Name(JETBRAINS_MONO_KEY.into()),
        vec![JETBRAINS_MONO_KEY.to_owned(), "Hack".to_owned()],
    );
    ctx.set_fonts(fonts);
}

/// The editor's code-font choice, selectable via Settings > Font.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EditorFont {
    #[default]
    JetBrainsMono,
    Default,
}

impl EditorFont {
    pub const ALL: [EditorFont; 2] = [EditorFont::JetBrainsMono, EditorFont::Default];

    pub fn family(self) -> egui::FontFamily {
        match self {
            EditorFont::JetBrainsMono => egui::FontFamily::Name(JETBRAINS_MONO_KEY.into()),
            EditorFont::Default => egui::FontFamily::Monospace,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EditorFont::JetBrainsMono => "JetBrains Mono",
            EditorFont::Default => "Default",
        }
    }
}
