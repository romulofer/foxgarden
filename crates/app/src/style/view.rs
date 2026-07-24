/// Editor display toggles, selectable via Settings/View — separate from
/// `IndentSettings` (which governs indentation *behavior*: what one level
/// inserts) since these three control what's *rendered*, not what typing
/// does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewSettings {
    /// Whether long lines wrap to the viewport width. When `false`, lines
    /// run past the visible width and the editor scrolls horizontally to
    /// reach them instead.
    pub word_wrap: bool,
    /// Paints a small dot for each space and an arrow for each tab.
    pub show_whitespace: bool,
    /// Paints a thin vertical line through each indent level a line's
    /// leading whitespace spans.
    pub show_indent_guides: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self { word_wrap: true, show_whitespace: false, show_indent_guides: false }
    }
}
