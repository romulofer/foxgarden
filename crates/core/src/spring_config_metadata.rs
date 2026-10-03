//! Spring config property metadata extraction (`PLAN.md` Track 21's own
//! Track 12 — "Spring config property autocomplete" — Phase 1). Scans a
//! resolved classpath's own jars (`maven_classpath`/`gradle_classpaths`,
//! Track 21 Phase 3) for each one's bundled `META-INF/
//! spring-configuration-metadata.json` — the same file Spring Boot's own
//! annotation processor (`spring-boot-configuration-processor`) generates
//! and IntelliJ/VS Code's own Spring tooling already reads for
//! `application.properties`/`.yml` autocomplete — and parses it into
//! completion candidates. Verified against a real, currently-cached
//! `spring-boot-autoconfigure-4.0.6.jar` (this module's own tests embed a
//! real captured excerpt of its actual `spring-configuration-metadata.json`,
//! not a guessed shape): `properties[].defaultValue` is genuinely
//! heterogeneous JSON (a bool, a string, or a number depending on the
//! property's own type), which is why it isn't a plain `Option<String>`
//! field directly.

use std::io::Read;
use std::path::Path;

use serde::Deserialize;

/// One `spring-configuration-metadata.json` `properties[]` entry, the only
/// part of the file's real shape (`groups`/`properties`/`hints`/`ignored`)
/// this module's own completion-candidate scope needs — `groups` describes
/// nested `@ConfigurationProperties` prefixes rather than a leaf key a user
/// would actually type, and `hints`/`ignored` are a further refinement on
/// top of `properties` (enum-like value suggestions, deprecated-property
/// suppression) out of this phase's stated scope.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SpringConfigProperty {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    pub description: Option<String>,
    /// Stringified from whatever real JSON value the metadata carries
    /// (`true`, `"UTF-8"`, `8080`, ...) — kept as a display-ready `String`
    /// rather than a `serde_json::Value` since nothing downstream needs to
    /// distinguish a numeric default from a string one, only show it.
    #[serde(
        rename = "defaultValue",
        default,
        deserialize_with = "default_value_as_display_string"
    )]
    pub default_value: Option<String>,
}

fn default_value_as_display_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.map(|v| match v {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    }))
}

#[derive(Debug, Deserialize)]
struct RawMetadata {
    #[serde(default)]
    properties: Vec<SpringConfigProperty>,
}

/// Parses one already-extracted `spring-configuration-metadata.json`
/// document's `properties[]` into candidates. Pure/no I/O.
pub fn parse_metadata_json(json: &str) -> Result<Vec<SpringConfigProperty>, String> {
    let raw: RawMetadata = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(raw.properties)
}

const METADATA_ENTRY: &str = "META-INF/spring-configuration-metadata.json";

/// Reads and parses `jar_path`'s own bundled metadata, if it has any. Most
/// jars on a real classpath don't (verified this session: a plain
/// `commons-io` jar has no such entry at all) — that's the ordinary case,
/// not an error, so it returns `Ok(vec![])` rather than `Err`; only a jar
/// that exists but isn't a valid zip at all is a real error.
pub fn scan_jar_for_metadata(jar_path: &Path) -> Result<Vec<SpringConfigProperty>, String> {
    let file = std::fs::File::open(jar_path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("{}: not a valid zip: {e}", jar_path.display()))?;

    let mut entry = match archive.by_name(METADATA_ENTRY) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", jar_path.display())),
    };
    let mut contents = String::new();
    entry
        .read_to_string(&mut contents)
        .map_err(|e| format!("{}: {METADATA_ENTRY} isn't valid UTF-8: {e}", jar_path.display()))?;

    parse_metadata_json(&contents)
}

/// Scans every jar in `classpath` and collects every property found across
/// all of them (typically just the one or two jars that bundle Spring's own
/// autoconfigure metadata, out of a real project's often much longer
/// resolved classpath). A single jar that can't be opened at all (a stale
/// resolved path pointing at a since-deleted cache entry, say) is silently
/// skipped rather than failing the whole scan — the same "one bad input
/// doesn't sink an otherwise-good batch" reasoning `static_analysis`'s own
/// `diagnostics_from_findings` established for a file that can no longer
/// be read from disk.
pub fn scan_classpath_for_metadata(classpath: &[std::path::PathBuf]) -> Vec<SpringConfigProperty> {
    classpath
        .iter()
        .filter_map(|jar| scan_jar_for_metadata(jar).ok())
        .flatten()
        .collect()
}

#[cfg(test)]
#[path = "spring_config_metadata_test.rs"]
mod spring_config_metadata_test;
