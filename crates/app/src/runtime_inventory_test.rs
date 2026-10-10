use std::collections::HashMap;

use eframe::Storage as _;

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

fn known(major: u32) -> RegisteredRuntime {
    RegisteredRuntime {
        label: format!("Java {major}"),
        home: PathBuf::from(format!("/jdk{major}")),
        major_version: Some(major),
    }
}

fn install(major: u32) -> RuntimeInstall {
    RuntimeInstall {
        home: PathBuf::from(format!("/jdk{major}")),
        label: format!("Java {major}"),
        major_version: major,
    }
}

fn inventory(majors: &[u32]) -> RuntimeInventory {
    RuntimeInventory {
        runtimes: majors.iter().map(|m| known(*m)).collect(),
    }
}

#[test]
fn closest_for_prefers_an_exact_match() {
    assert_eq!(inventory(&[8, 17, 21]).closest_for(17).unwrap().major_version, Some(17));
}

#[test]
fn closest_for_falls_back_to_the_smallest_newer_version() {
    // No JDK 11 registered — 21 is the smallest one still >= 11.
    assert_eq!(inventory(&[8, 21]).closest_for(11).unwrap().major_version, Some(21));
}

#[test]
fn closest_for_falls_back_to_the_largest_available_when_everything_is_older() {
    // Nothing registered is >= 21 — 11 (the largest available) beats
    // refusing to suggest anything at all.
    assert_eq!(inventory(&[8, 11]).closest_for(21).unwrap().major_version, Some(11));
}

#[test]
fn closest_for_on_an_empty_inventory_is_none() {
    assert!(RuntimeInventory::default().closest_for(17).is_none());
}

#[test]
fn closest_for_ignores_entries_with_an_unverified_version() {
    let inventory = RuntimeInventory {
        runtimes: vec![RegisteredRuntime {
            label: "mystery".to_string(),
            home: PathBuf::from("/mystery"),
            major_version: None,
        }],
    };
    assert!(inventory.closest_for(17).is_none());
}

#[test]
fn json_round_trips_through_to_json_and_from_json() {
    let inventory = inventory(&[17, 21]);
    assert_eq!(inventory, RuntimeInventory::from_json(&inventory.to_json()));
}

#[test]
fn from_json_on_malformed_input_is_an_empty_inventory_not_a_panic() {
    assert_eq!(RuntimeInventory::from_json("not json"), RuntimeInventory::default());
    assert_eq!(RuntimeInventory::from_json(""), RuntimeInventory::default());
}

/// What `JdkRegistry` serialized to before the inventory was generic.
#[test]
fn the_old_jdks_field_still_decodes() {
    let old = r#"{"jdks":[{"label":"Java 21","home":"/jdk21","major_version":21}]}"#;
    assert_eq!(RuntimeInventory::from_json(old), inventory(&[21]));
}

#[test]
fn add_registers_the_install_as_the_extension_reported_it() {
    let mut inventory = RuntimeInventory::default();
    inventory.add(install(21));
    assert_eq!(inventory, RuntimeInventory { runtimes: vec![known(21)] });
}

#[test]
fn add_is_a_no_op_when_the_same_home_is_already_registered() {
    let mut inventory = inventory(&[21]);
    // A rescan reporting the same home again, even at a different
    // (impossible in practice, but the point stands) major version,
    // must not duplicate or overwrite the existing entry.
    inventory.add(RuntimeInstall {
        major_version: 99,
        ..install(21)
    });
    assert_eq!(inventory, RuntimeInventory { runtimes: vec![known(21)] });
}

#[test]
fn remove_drops_the_entry_at_the_given_index() {
    let mut inventory = inventory(&[8, 17]);
    inventory.remove(0);
    assert_eq!(inventory.runtimes, vec![known(17)]);
}

#[test]
fn remove_on_an_out_of_range_index_is_a_no_op_not_a_panic() {
    let mut inventory = inventory(&[8]);
    inventory.remove(5);
    assert_eq!(inventory.runtimes.len(), 1);
}

#[test]
fn an_inventory_survives_a_save_and_restore() {
    let mut storage = FakeStorage::default();
    persist(&mut storage, "jdk", &inventory(&[17, 21]));
    assert_eq!(restore(&storage, "jdk"), inventory(&[17, 21]));
    assert!(restore(&storage, "node").runtimes.is_empty());
}

#[test]
fn an_install_saved_under_the_old_key_is_still_found() {
    let mut storage = FakeStorage::default();
    storage.set_string(
        "jdk_registry",
        r#"{"jdks":[{"label":"Java 8","home":"/jdk8","major_version":8}]}"#.to_string(),
    );
    assert_eq!(restore(&storage, "jdk"), inventory(&[8]));

    persist(&mut storage, "jdk", &inventory(&[21]));
    assert_eq!(restore(&storage, "jdk"), inventory(&[21]), "the new key wins once written");
}
