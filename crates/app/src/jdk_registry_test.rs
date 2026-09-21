
use super::*;

fn fake_jdk(dir: &std::path::Path, banner: &str) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = bin.join("java");
    std::fs::write(&script, format!("#!/bin/sh\necho '{banner}' >&2\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }
    dir.to_path_buf()
}

#[test]
fn detect_and_add_records_the_real_detected_version() {
    let dir = test_support::tempdir();
    let home = fake_jdk(dir.path(), "openjdk version \"17.0.4\" 2022-07-19 LTS");

    let mut registry = JdkRegistry::default();
    registry.detect_and_add(home.clone()).expect("detects a fake JDK 17");

    assert_eq!(registry.jdks.len(), 1);
    assert_eq!(registry.jdks[0].major_version, Some(17));
    assert_eq!(registry.jdks[0].home, home);
    assert_eq!(registry.jdks[0].label, "Java 17");
}

#[test]
fn detect_and_add_on_an_unreadable_home_is_an_error_and_adds_nothing() {
    let mut registry = JdkRegistry::default();
    let error = registry
        .detect_and_add(PathBuf::from("/does/not/exist"))
        .expect_err("no java there");
    assert!(!error.is_empty());
    assert!(registry.jdks.is_empty());
}

fn known(major: u32) -> RegisteredJdk {
    RegisteredJdk {
        label: format!("Java {major}"),
        home: PathBuf::from(format!("/jdk{major}")),
        major_version: Some(major),
    }
}

#[test]
fn closest_for_prefers_an_exact_match() {
    let registry = JdkRegistry {
        jdks: vec![known(8), known(17), known(21)],
    };
    assert_eq!(registry.closest_for(17).unwrap().major_version, Some(17));
}

#[test]
fn closest_for_falls_back_to_the_smallest_newer_version() {
    let registry = JdkRegistry {
        jdks: vec![known(8), known(21)],
    };
    // No JDK 11 registered — 21 is the smallest one still >= 11.
    assert_eq!(registry.closest_for(11).unwrap().major_version, Some(21));
}

#[test]
fn closest_for_falls_back_to_the_largest_available_when_everything_is_older() {
    let registry = JdkRegistry {
        jdks: vec![known(8), known(11)],
    };
    // Nothing registered is >= 21 — 11 (the largest available) beats
    // refusing to suggest anything at all.
    assert_eq!(registry.closest_for(21).unwrap().major_version, Some(11));
}

#[test]
fn closest_for_on_an_empty_registry_is_none() {
    assert!(JdkRegistry::default().closest_for(17).is_none());
}

#[test]
fn closest_for_ignores_entries_with_an_unverified_version() {
    let registry = JdkRegistry {
        jdks: vec![RegisteredJdk {
            label: "mystery".to_string(),
            home: PathBuf::from("/mystery"),
            major_version: None,
        }],
    };
    assert!(registry.closest_for(17).is_none());
}

#[test]
fn json_round_trips_through_to_json_and_from_json() {
    let registry = JdkRegistry {
        jdks: vec![known(17), known(21)],
    };
    let restored = JdkRegistry::from_json(&registry.to_json());
    assert_eq!(registry, restored);
}

#[test]
fn from_json_on_malformed_input_is_an_empty_registry_not_a_panic() {
    assert_eq!(JdkRegistry::from_json("not json"), JdkRegistry::default());
    assert_eq!(JdkRegistry::from_json(""), JdkRegistry::default());
}

#[test]
fn add_known_registers_a_jdk_at_the_given_version_without_probing_it() {
    let mut registry = JdkRegistry::default();
    registry.add_known(PathBuf::from("/jdk21"), 21);
    assert_eq!(registry.jdks, vec![known(21)]);
}

#[test]
fn add_known_is_a_no_op_when_the_same_home_is_already_registered() {
    let mut registry = JdkRegistry { jdks: vec![known(21)] };
    // A rescan reporting the same home again, even at a different
    // (impossible in practice, but the point stands) major version,
    // must not duplicate or overwrite the existing entry.
    registry.add_known(PathBuf::from("/jdk21"), 99);
    assert_eq!(registry.jdks, vec![known(21)]);
}

#[test]
fn remove_drops_the_entry_at_the_given_index() {
    let mut registry = JdkRegistry {
        jdks: vec![known(8), known(17)],
    };
    registry.remove(0);
    assert_eq!(registry.jdks, vec![known(17)]);
}

#[test]
fn remove_on_an_out_of_range_index_is_a_no_op_not_a_panic() {
    let mut registry = JdkRegistry { jdks: vec![known(8)] };
    registry.remove(5);
    assert_eq!(registry.jdks.len(), 1);
}
