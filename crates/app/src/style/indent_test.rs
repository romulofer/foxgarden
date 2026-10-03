
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
