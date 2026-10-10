use std::collections::HashMap;

use super::*;

#[derive(Default)]
struct FakeStorage(HashMap<String, String>);

impl eframe::Storage for FakeStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }
    fn flush(&mut self) {}
}

fn state(disabled: &[&str]) -> ExtensionsState {
    ExtensionsState::new(
        fg_languages::shipped_extensions(),
        disabled.iter().map(|id| (*id).to_string()).collect(),
    )
}

#[test]
fn everything_is_enabled_without_stored_settings() {
    assert!(stored_disabled(None).is_empty());
    assert!(stored_disabled(Some(&FakeStorage::default())).is_empty());
    assert!(state(&[]).is_enabled("spring"));
}

#[test]
fn the_disabled_list_survives_a_save_and_restore() {
    let mut storage = FakeStorage::default();
    persist_disabled(&mut storage, &["spring".to_string(), "file-types".to_string()]);
    assert_eq!(stored_disabled(Some(&storage)), vec!["spring", "file-types"]);

    persist_disabled(&mut storage, &[]);
    assert!(stored_disabled(Some(&storage)).is_empty());
}

#[test]
fn toggling_flips_one_extension_and_never_duplicates_it() {
    let mut state = state(&[]);
    state.set_enabled("spring", false);
    state.set_enabled("spring", false);
    assert_eq!(state.disabled(), ["spring"]);
    assert!(!state.is_enabled("spring"));
    assert!(state.is_enabled("file-types"));

    state.set_enabled("spring", true);
    assert!(state.disabled().is_empty());
}

#[test]
fn a_restart_is_needed_only_while_the_choice_differs_from_the_running_registry() {
    let mut state = state(&["spring"]);
    assert!(!state.restart_required());

    state.set_enabled("spring", true);
    assert!(state.restart_required());

    state.set_enabled("spring", false);
    assert!(!state.restart_required(), "switching back needs no restart");
}

#[test]
fn restart_check_ignores_order() {
    let mut state = state(&["spring", "file-types"]);
    state.set_enabled("spring", true);
    state.set_enabled("spring", false);
    assert_eq!(state.disabled(), ["file-types", "spring"]);
    assert!(!state.restart_required());
}
