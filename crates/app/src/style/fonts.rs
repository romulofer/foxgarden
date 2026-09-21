const JETBRAINS_MONO_KEY: &str = "JetBrainsMono";
const NERD_FONT_SYMBOLS_KEY: &str = "NerdFontSymbols";

/// Registers the bundled JetBrains Mono under its own font family, alongside
/// egui's built-in families (left untouched, reachable as `EditorFont::Default`).
/// Also registers the bundled Nerd Font "Symbols Only" font (the official
/// `ryanoasis/nerd-fonts` release meant exactly for this — a fallback
/// alongside any regular text font, not a full patched font family) as the
/// *last* fallback on both `EditorFont` chains: the embedded terminal
/// (`terminal_widget::show`) renders with whatever `EditorFont` the editor
/// itself uses, and a real shell prompt (Powerlevel10k, Starship, Oh My
/// Zsh's git plugin, ...) routinely emits Powerline/Nerd Font glyphs
/// (U+E0A0–E0D4, U+E700–E7C5, U+F000–F2E0) that neither JetBrains Mono nor
/// egui's own bundled Hack/Ubuntu-Light/NotoEmoji contain — without a font
/// somewhere in the chain covering them, egui/epaint substitutes a literal
/// `?` per missing glyph (confirmed: this is exactly what a user reported
/// seeing in their own git-aware zsh prompt inside the terminal panel).
/// `SymbolsNerdFontMono-Regular.ttf` specifically (not the proportional
/// `SymbolsNerdFont-Regular.ttf` variant) — its glyphs are fixed-width,
/// matching a monospace grid the way the terminal's own cell layout needs,
/// where a variable-width icon glyph would misalign the grid.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        JETBRAINS_MONO_KEY.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        NERD_FONT_SYMBOLS_KEY.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/SymbolsNerdFontMono-Regular.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Name(JETBRAINS_MONO_KEY.into()),
        vec![
            JETBRAINS_MONO_KEY.to_owned(),
            "Hack".to_owned(),
            NERD_FONT_SYMBOLS_KEY.to_owned(),
        ],
    );
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push(NERD_FONT_SYMBOLS_KEY.to_owned());
    // Proportional too, because the *UI* draws icons from this same font
    // (`style::icons`) — the project tree, tab bar, side-panel toolbar and
    // status bar all render in the proportional family, and without this
    // every one of those icons would be a missing glyph.
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .push(NERD_FONT_SYMBOLS_KEY.to_owned());
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

    /// A stable identifier for persisted storage — deliberately distinct
    /// from `label()`, which is the user-facing display text and free to
    /// change (e.g. a typo fix) without silently breaking every saved
    /// session that picked that font.
    pub fn storage_key(self) -> &'static str {
        match self {
            EditorFont::JetBrainsMono => "JetBrainsMono",
            EditorFont::Default => "Default",
        }
    }

    pub fn from_storage_key(key: &str) -> Option<EditorFont> {
        match key {
            "JetBrainsMono" => Some(EditorFont::JetBrainsMono),
            "Default" => Some(EditorFont::Default),
            _ => None,
        }
    }
}
