//! Generic JDK discovery/verification (`PLAN.md` Track 29 Phase 1) —
//! shared by `lsp_manager` (which JVM jdt.ls itself must run under, always
//! 21+) and `jdk_registry` (which JDKs exist on this machine to *target*
//! when analyzing or scaffolding a project, any version). Neither owns
//! this logic; both need it, so it lives here instead of one being built
//! on top of the other's private internals.

use std::path::PathBuf;
use std::process::Command;

/// Resolves `java_home` (a `JAVA_HOME`-shaped directory, or empty for
/// "auto-detect") to the actual `java` executable to run: `java_home`'s own
/// `bin/java` if given, else `$JAVA_HOME`'s, else whatever `java` resolves
/// to on `PATH` — the same fallback rule `jdt.ls`'s own `bin/jdtls`
/// launcher applies internally.
pub fn java_command(java_home: &str) -> PathBuf {
    let home = if java_home.trim().is_empty() {
        std::env::var_os("JAVA_HOME").map(PathBuf::from)
    } else {
        Some(PathBuf::from(java_home.trim()))
    };
    match home {
        Some(home) => home.join("bin").join("java"),
        None => PathBuf::from("java"),
    }
}

/// The major version out of `java -version`'s own output. Handles both
/// shapes a real JVM prints: modern `openjdk version "21.0.2"` and the
/// legacy `java version "1.8.0_292"`, where the major version is the
/// *second* component.
pub fn java_major_version(version_output: &str) -> Option<u32> {
    let quoted = version_output.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '_', '-', '+']);
    let first = parts.next()?;
    if first == "1" {
        parts.next()?.parse().ok()
    } else {
        first.parse().ok()
    }
}

/// Runs `java_command(java_home) -version` and reads its major version —
/// no minimum enforced, no caller-specific wording. `lsp_manager::
/// check_java` wraps this with jdt.ls's own minimum-version message on
/// top; `JdkRegistry::detect_and_add` uses it directly to label a newly
/// registered JDK.
pub fn detect_major_version(java_home: &str) -> Result<u32, String> {
    let java = java_command(java_home);
    let output =
        Command::new(&java).arg("-version").output().map_err(|e| format!("couldn't run {}: {e}", java.display()))?;
    // Every JVM prints its version banner on stderr, not stdout.
    let banner = String::from_utf8_lossy(&output.stderr);
    java_major_version(&banner).ok_or_else(|| format!("couldn't read a version out of `{} -version`", java.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_major_version_reads_a_modern_jvm_banner() {
        assert_eq!(java_major_version("openjdk version \"21.0.2\" 2024-01-16 LTS\n"), Some(21));
        assert_eq!(java_major_version("openjdk version \"17.0.4\" 2022-07-19 LTS"), Some(17));
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
}
