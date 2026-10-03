
use super::*;

#[test]
fn disabled_never_fires() {
    let mut state = AutoSaveState::default();
    let settings = AutoSaveSettings {
        enabled: false,
        ..AutoSaveSettings::default()
    };
    assert!(!state.tick(settings, true, 0.0));
    assert!(!state.tick(settings, false, 1.0));
}

#[test]
fn focus_loss_mode_fires_only_on_the_losing_edge() {
    let mut state = AutoSaveState::default();
    let settings = AutoSaveSettings {
        enabled: true,
        mode: AutoSaveMode::OnFocusLoss,
        idle_seconds: 30,
    };
    assert!(!state.tick(settings, true, 0.0), "still focused: no trigger");
    assert!(state.tick(settings, false, 1.0), "just lost focus: trigger");
    assert!(!state.tick(settings, false, 2.0), "still unfocused: no repeat trigger");
    assert!(!state.tick(settings, true, 3.0), "regaining focus: no trigger");
}

#[test]
fn idle_mode_fires_once_the_threshold_elapses_since_last_activity() {
    let mut state = AutoSaveState::default();
    let settings = AutoSaveSettings {
        enabled: true,
        mode: AutoSaveMode::AfterIdle,
        idle_seconds: 10,
    };
    state.record_activity(0.0);
    assert!(!state.tick(settings, true, 5.0), "only 5s idle: no trigger yet");
    assert!(state.tick(settings, true, 10.0), "10s idle: threshold reached");
    state.record_activity(10.0);
    assert!(
        !state.tick(settings, true, 15.0),
        "activity reset the clock: no trigger"
    );
}

#[test]
fn idle_mode_ignores_focus_state() {
    let mut state = AutoSaveState::default();
    let settings = AutoSaveSettings {
        enabled: true,
        mode: AutoSaveMode::AfterIdle,
        idle_seconds: 10,
    };
    state.record_activity(0.0);
    assert!(
        state.tick(settings, false, 10.0),
        "idle mode fires even while unfocused, unlike OnFocusLoss"
    );
}
