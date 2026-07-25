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
    /// Pins the enclosing class/method header line(s) to the top of the
    /// editor while scrolling through their body — "sticky scroll." Java only
    /// for now (needs a tree-sitter scope vocabulary; see
    /// `syntax::enclosing_scope_starts`), a no-op elsewhere.
    pub show_sticky_scroll: bool,
    /// Whether the text caret blinks (on the `Visuals::text_cursor` on/off
    /// timing — see `text_area::shell::caret_visible`) or stays solid.
    pub cursor_blink: bool,
    /// Whether the focused/unfocused border `widgets::editor::widget::show`
    /// paints around the active editor pane is shown at all.
    pub show_editor_outline: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            word_wrap: true,
            show_whitespace: false,
            show_indent_guides: false,
            show_sticky_scroll: false,
            cursor_blink: true,
            show_editor_outline: true,
        }
    }
}
