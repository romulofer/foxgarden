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
mod tests {
    use super::*;

    #[test]
    fn unit_is_a_single_tab_when_use_tabs_is_set() {
        let settings = IndentSettings {
            use_tabs: true,
            width: 4,
        };
        assert_eq!(settings.unit(), "\t");
    }

    #[test]
    fn unit_is_width_spaces_when_use_tabs_is_unset() {
        let settings = IndentSettings {
            use_tabs: false,
            width: 2,
        };
        assert_eq!(settings.unit(), "  ");
    }

    #[test]
    fn width_zero_still_produces_at_least_one_space() {
        // A zero-width "indent" would make Tab a visible no-op, which reads
        // as broken rather than as a deliberate user choice — floor it at 1.
        let settings = IndentSettings {
            use_tabs: false,
            width: 0,
        };
        assert_eq!(settings.unit(), " ");
    }
}
