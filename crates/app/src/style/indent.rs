/// The editor's indentation style, selectable via Settings > Indentation.
/// Drives what one indent level looks like for auto-indent (Enter after
/// `{`), Tab with no selection, and Tab/Shift+Tab block indent/dedent over
/// a selection — every place `widgets::editor` decides "insert one level
/// of indentation" reads from this instead of a hardcoded 4-space string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IndentSettings {
    pub use_tabs: bool,
    /// Spaces per indent level when `use_tabs` is false. Ignored when it's
    /// true — a literal tab is always exactly one level regardless of width.
    pub width: usize,
}

impl Default for IndentSettings {
    fn default() -> Self {
        Self {
            use_tabs: false,
            width: 4,
        }
    }
}

impl IndentSettings {
    /// The literal string one indent level inserts.
    pub fn unit(self) -> String {
        if self.use_tabs {
            "\t".to_string()
        } else {
            " ".repeat(self.width.max(1))
        }
    }
}

#[cfg(test)]
#[path = "indent_test.rs"]
mod indent_test;
