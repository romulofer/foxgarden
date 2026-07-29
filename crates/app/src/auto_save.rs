/// Settings > Auto-save — off by default, so nobody's existing "always
/// explicit Ctrl+S" habit changes without opting in. `PLAN.md` Track 6
/// Phase 1.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AutoSaveSettings {
    pub enabled: bool,
    pub mode: AutoSaveMode,
    /// Idle threshold for `AutoSaveMode::AfterIdle`; ignored under
    /// `OnFocusLoss`.
    pub idle_seconds: u32,
}

impl Default for AutoSaveSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: AutoSaveMode::OnFocusLoss,
            idle_seconds: 30,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoSaveMode {
    OnFocusLoss,
    AfterIdle,
}

/// Per-frame tracking auto-save needs that isn't itself a persisted
/// setting: the previous frame's OS-focus state (to catch the *edge*, not
/// just the level — otherwise every frame the window happens to be
/// unfocused would re-fire, not just the frame it lost focus) and when the
/// user was last seen doing anything (`record_activity`).
#[derive(Default)]
pub struct AutoSaveState {
    was_focused: bool,
    last_activity: f64,
}

impl AutoSaveState {
    /// Call whenever the app sees real user input this frame (a key press,
    /// pointer move/click, scroll, ...) — resets the idle clock.
    pub fn record_activity(&mut self, now: f64) {
        self.last_activity = now;
    }

    /// Call once per frame, regardless of `settings.enabled`, so
    /// `was_focused` never goes stale while auto-save is off — otherwise
    /// turning it on right after the window regained focus would read the
    /// *next* loss as if it were the first one since launch. Returns
    /// whether `settings.mode`'s trigger condition just fired this frame.
    pub fn tick(&mut self, settings: AutoSaveSettings, focused: bool, now: f64) -> bool {
        let focus_lost = self.was_focused && !focused;
        self.was_focused = focused;
        if !settings.enabled {
            return false;
        }
        match settings.mode {
            AutoSaveMode::OnFocusLoss => focus_lost,
            AutoSaveMode::AfterIdle => now - self.last_activity >= f64::from(settings.idle_seconds),
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert!(!state.tick(settings, true, 15.0), "activity reset the clock: no trigger");
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
}
