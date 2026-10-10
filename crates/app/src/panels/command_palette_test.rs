
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
    let label = Command::SaveAll.label();
    assert!(matches(label, ""));
    assert!(matches(label, &label.to_uppercase()));
    assert!(!matches(label, "definitely not a command"));
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

#[test]
fn analyzers_are_listed_right_after_run_tests() {
    let analyzers = vec!["Checkstyle".to_string(), "PMD".to_string()];
    let entries = entries(&analyzers);
    let tests = entries
        .iter()
        .position(|(picked, ..)| *picked == Picked::Command(Command::RunTests))
        .unwrap();

    assert_eq!(entries[tests + 1].0, Picked::Analyzer(0));
    assert_eq!(entries[tests + 1].1, fg_i18n::msg::run_analyzer("Checkstyle"));
    assert_eq!(entries[tests + 2].0, Picked::Analyzer(1));
    assert_eq!(entries.len(), Command::ALL.len() + 2);
}

#[test]
fn with_no_analyzers_the_list_is_just_the_fixed_commands() {
    assert_eq!(entries(&[]).len(), Command::ALL.len());
}
