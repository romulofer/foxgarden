
use super::*;

#[test]
fn every_command_has_a_name() {
    for command in Command::ALL {
        assert!(!command.label().is_empty(), "{command:?} has no label");
    }
}

/// A palette that lists the same action twice is worse than the menus:
/// the user has to guess which one they mean.
#[test]
fn no_two_commands_share_a_name() {
    let mut labels: Vec<&str> = Command::ALL.iter().map(|c| c.label()).collect();
    labels.sort_unstable();
    let before = labels.len();
    labels.dedup();
    assert_eq!(labels.len(), before, "two commands share a label");
}

#[test]
fn filtering_is_case_insensitive_and_matches_anywhere_in_the_name() {
    assert!(matches(Command::SaveAll, ""));
    assert!(matches(Command::SaveAll, &Command::SaveAll.label().to_uppercase()));
    assert!(!matches(Command::SaveAll, "definitely not a command"));
}

#[test]
fn toggle_opens_and_resets_the_query() {
    let mut state = CommandPaletteState::default();
    state.query.push_str("stale");
    state.selected = 7;

    state.toggle();

    assert!(state.open);
    assert!(state.query.is_empty());
    assert_eq!(state.selected, 0);
}
