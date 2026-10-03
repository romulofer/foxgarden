
use super::*;

#[test]
fn java_major_version_reads_a_modern_jvm_banner() {
    assert_eq!(
        java_major_version("openjdk version \"21.0.2\" 2024-01-16 LTS\n"),
        Some(21)
    );
    assert_eq!(
        java_major_version("openjdk version \"17.0.4\" 2022-07-19 LTS"),
        Some(17)
    );
}

/// A Java 8 JVM reports `1.8.0_x`, where the major version is the
/// second component — reading the first would call it "Java 1" and
/// reject every JVM ever with a confusing message.
#[test]
fn java_major_version_reads_a_legacy_jvm_banner() {
    assert_eq!(java_major_version("java version \"1.8.0_292\""), Some(8));
}

#[test]
fn java_major_version_on_unparseable_output_is_none() {
    assert!(java_major_version("no version here").is_none());
    assert!(java_major_version("version \"nonsense\"").is_none());
}

use std::path::Path;

/// Deterministic regardless of this process's own ambient `JAVA_HOME`
/// (real dev machines commonly have one set, e.g. via SDKMAN) — an
/// explicit `java_home` always wins over it, so this doesn't need to
/// touch the environment at all to stay reliable.
#[test]
fn java_command_prefers_the_explicit_home_over_java_home_and_path() {
    assert_eq!(java_command("/opt/jdk21"), Path::new("/opt/jdk21/bin/java"));
}

#[test]
fn detect_major_version_on_a_nonexistent_binary_is_an_error_not_a_panic() {
    assert!(detect_major_version("/does/not/exist").is_err());
}
