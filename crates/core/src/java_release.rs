//! Which Java release a project is written against — the number jdt.ls has
//! to compile and lint it at, so a Java 8 codebase is checked as Java 8
//! (`var` is an error, no records, no sealed types) even though jdt.ls
//! itself only runs on a JDK 21. Without this every project gets diagnosed
//! at whatever release jdt.ls' own JVM defaults to, which silently accepts
//! syntax the project's real compiler would reject and flags nothing when
//! an older toolchain would.
//!
//! Deliberately text-level and I/O-free at its core (`release_from_pom`,
//! `release_from_gradle`, `release_from_version_file`), the same shape
//! `maven::parse_pom` already takes: no Maven property resolution across a
//! parent POM, no Gradle evaluation. Both build tools state the compiler
//! release directly in the file that declares it in every layout checked
//! here, and guessing beyond that is the build tool's own job — `detect`
//! returning `None` is a fine answer, and simply leaves jdt.ls on its own
//! default.

use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::Event;

/// A detected release plus where it was read from, so the answer can be
/// explained rather than just applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRelease {
    /// The major version, always in modern form: `1.8` reads as `8`.
    pub major: u32,
    /// The file it came from, relative to the project root.
    pub file: String,
    /// The specific setting inside that file, e.g. `maven.compiler.release`.
    pub setting: &'static str,
}

/// The build files `detect` reads, in the order it tries them: a project
/// with both a `pom.xml` and a stray `.java-version` is a Maven project
/// first.
const BUILD_FILES: [&str; 5] = ["pom.xml", "build.gradle", "build.gradle.kts", ".java-version", ".sdkmanrc"];

/// The release `root`'s own build files declare, or `None` when they say
/// nothing about it. Only the project root is read — a multi-module build's
/// child modules are not walked, since the aggregator POM/`build.gradle` is
/// where a shared compiler release is declared in every real layout this was
/// checked against, and a per-module override is a question for the build
/// tool's own import (jdt.ls runs that itself).
pub fn detect(root: &Path) -> Option<JavaRelease> {
    for name in BUILD_FILES {
        let path = root.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let found = match name {
            "pom.xml" => release_from_pom(&text),
            "build.gradle" | "build.gradle.kts" => release_from_gradle(&text),
            _ => release_from_version_file(&text, name),
        };
        if let Some((major, setting)) = found {
            return Some(JavaRelease {
                major,
                file: name.to_string(),
                setting,
            });
        }
    }
    None
}

/// Reads a release out of whatever spelling a version string uses: `17`,
/// `17.0.2`, `1.8` (Java 8's legacy form — the leading `1.` is the epoch,
/// not the version), Gradle's `VERSION_17`/`VERSION_1_8` enum constants, and
/// the vendor-suffixed names version managers write (`21.0.3-zulu`,
/// `temurin-17.0.9`).
pub fn parse_release_token(token: &str) -> Option<u32> {
    let token = token.trim().trim_matches(['"', '\'']);
    // `VERSION_1_8`/`VERSION_17` — underscores are separators here exactly
    // as dots are elsewhere, so normalizing lets one parse handle both.
    let normalized = token.replace('_', ".");
    let mut numbers = normalized
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok());
    let first = numbers.next()?;
    // `1.8`/`1.7` — the real version is the second component. A bare `1`
    // with nothing after it is not a Java release anyone targets, and
    // returning it would claim "Java 1".
    if first == 1 { numbers.next() } else { Some(first) }
}

/// Reads the compiler release out of a `pom.xml`, preferring the settings
/// Maven itself prefers: `<maven.compiler.release>` (or the compiler
/// plugin's own `<release>`) outranks `<source>`, which outranks
/// `<target>` — `release` is the one that actually constrains the API a
/// build may use, `target` only the bytecode version. `<java.version>` is
/// not a Maven property at all but the one `spring-boot-starter-parent`
/// defines and wires into the compiler plugin, which makes it the single
/// most common way a real Spring project states its release.
pub fn release_from_pom(xml: &str) -> Option<(u32, &'static str)> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    // Highest-priority-first, filled as the document is walked; the whole
    // file is read before choosing, since a POM may state several of these
    // and document order says nothing about which Maven would honor.
    let mut found: [Option<u32>; 4] = [None; 4];

    loop {
        match reader.read_event().ok()? {
            Event::Eof => break,
            Event::Start(e) => {
                path.push(String::from_utf8_lossy(e.local_name().as_ref()).into_owned());
                text.clear();
            }
            Event::End(_) => {
                let value = std::mem::take(&mut text);
                let names: Vec<&str> = path.iter().map(String::as_str).collect();
                if let Some((rank, _)) = pom_setting_rank(&names) {
                    // First writer wins per rank: a plugin `<configuration>`
                    // repeated across profiles states the same release.
                    let slot = &mut found[rank];
                    if slot.is_none() {
                        *slot = parse_release_token(&value);
                    }
                }
                path.pop();
            }
            Event::Text(t) => {
                if let Ok(decoded) = t.decode() {
                    text.push_str(decoded.trim());
                }
            }
            _ => {}
        }
    }

    found
        .iter()
        .enumerate()
        .find_map(|(rank, major)| major.map(|major| (rank, major)))
        .map(|(rank, major)| (major, POM_SETTING_NAMES[rank]))
}

/// What each priority rank in `release_from_pom` means, for reporting.
const POM_SETTING_NAMES: [&str; 4] = [
    "maven.compiler.release",
    "java.version",
    "maven.compiler.source",
    "maven.compiler.target",
];

/// Which priority rank an element path counts as, if any. Both spellings of
/// each setting land on the same rank: the `<properties>` one and the
/// compiler plugin's own `<configuration>` element mean the same thing to
/// Maven.
fn pom_setting_rank(path: &[&str]) -> Option<(usize, &'static str)> {
    let rank = match path {
        ["project", "properties", "maven.compiler.release"] => 0,
        ["project", "properties", "java.version"] => 1,
        ["project", "properties", "maven.compiler.source"] => 2,
        ["project", "properties", "maven.compiler.target"] => 3,
        // `<plugin><configuration><release>` — matched on the tail rather
        // than the full path, since the plugin may sit under `<build>`,
        // `<build><pluginManagement>`, or a `<profile>`'s own copy of
        // either, and all three mean the same thing here.
        [.., "plugin", "configuration", "release"] => 0,
        [.., "plugin", "configuration", "source"] => 2,
        [.., "plugin", "configuration", "target"] => 3,
        _ => return None,
    };
    Some((rank, POM_SETTING_NAMES[rank]))
}

/// Reads the release out of a Groovy or Kotlin-DSL Gradle build file. Same
/// precedence idea as the POM reader: a toolchain (`java { toolchain {
/// languageVersion = JavaLanguageVersion.of(17) } }`, and Kotlin's
/// `jvmToolchain(17)` shorthand) is the modern, authoritative statement and
/// outranks the older `sourceCompatibility`, which outranks
/// `targetCompatibility`.
///
/// Text-scanned rather than parsed: a Gradle build file is a program, and
/// evaluating one means running Gradle. Every form matched here is the
/// literal-valued spelling real build files use; a release computed at build
/// time is simply not detected.
pub fn release_from_gradle(text: &str) -> Option<(u32, &'static str)> {
    let uncommented: String = text
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");

    let rules: [(&'static str, &[&str]); 4] = [
        ("java.toolchain.languageVersion", &["JavaLanguageVersion.of("]),
        ("kotlin.jvmToolchain", &["jvmToolchain("]),
        ("sourceCompatibility", &["sourceCompatibility"]),
        ("targetCompatibility", &["targetCompatibility"]),
    ];

    for (setting, needles) in rules {
        for needle in needles {
            let Some(at) = uncommented.find(needle) else { continue };
            let rest = &uncommented[at + needle.len()..];
            // The value is whatever follows, up to the end of that
            // statement: `= JavaVersion.VERSION_17`, `= 17`, `= '1.8'`,
            // `(17)`, `.set(JavaLanguageVersion.of(21))` — all of which
            // reduce to "the first version-shaped token after the setting".
            let value = rest
                .split(['\n', ';', '}'])
                .next()
                .unwrap_or_default()
                .trim_start_matches(['=', '(', ' ', '.'])
                .trim();
            if let Some(major) = parse_release_token(value) {
                return Some((major, setting));
            }
        }
    }
    None
}

/// Reads a bare version file: `.java-version` (jenv/asdf — `17`, `17.0.9`,
/// or `temurin-17.0.9`) or `.sdkmanrc` (a properties file whose `java=` line
/// names an installed candidate, e.g. `java=21.0.3-zulu`). A weaker signal
/// than a build file — it names the JDK a developer runs the build *with*,
/// not the release the code targets — which is why `detect` only reaches
/// these once no build file has answered.
pub fn release_from_version_file(text: &str, file: &str) -> Option<(u32, &'static str)> {
    if file == ".sdkmanrc" {
        let value = text
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix("java="))?;
        return parse_release_token(value).map(|major| (major, "java"));
    }
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    parse_release_token(line).map(|major| (major, ".java-version"))
}

/// Every build file `detect` reads, as absolute paths under `root` — for a
/// caller that wants to know when the answer might have changed without
/// re-reading them all.
pub fn build_files(root: &Path) -> Vec<PathBuf> {
    BUILD_FILES.iter().map(|name| root.join(name)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_tokens_cover_every_spelling_a_build_file_uses() {
        assert_eq!(parse_release_token("17"), Some(17));
        assert_eq!(parse_release_token("21.0.3"), Some(21));
        assert_eq!(parse_release_token(" '1.8' "), Some(8));
        assert_eq!(parse_release_token("VERSION_1_8"), Some(8));
        assert_eq!(parse_release_token("VERSION_17"), Some(17));
        assert_eq!(parse_release_token("JavaVersion.VERSION_11"), Some(11));
        assert_eq!(parse_release_token("21.0.3-zulu"), Some(21));
        assert_eq!(parse_release_token("temurin-17.0.9"), Some(17));
    }

    /// `1` alone is the legacy epoch with nothing after it — reporting
    /// "Java 1" would be worse than reporting nothing.
    #[test]
    fn a_release_token_with_no_version_in_it_is_none() {
        assert_eq!(parse_release_token("1"), None);
        assert_eq!(parse_release_token(""), None);
        assert_eq!(parse_release_token("stable"), None);
    }

    #[test]
    fn pom_java_version_property_is_read() {
        let xml = r#"
            <project>
              <artifactId>demo</artifactId>
              <properties><java.version>17</java.version></properties>
            </project>"#;
        assert_eq!(release_from_pom(xml), Some((17, "java.version")));
    }

    #[test]
    fn pom_legacy_source_property_reads_as_java_8() {
        let xml = r#"
            <project>
              <properties>
                <maven.compiler.source>1.8</maven.compiler.source>
                <maven.compiler.target>1.8</maven.compiler.target>
              </properties>
            </project>"#;
        assert_eq!(release_from_pom(xml), Some((8, "maven.compiler.source")));
    }

    /// `release` constrains the API a build may use; `source`/`target` don't.
    /// A POM stating all three has to be read as its `release`.
    #[test]
    fn pom_release_outranks_source_target_and_java_version() {
        let xml = r#"
            <project>
              <properties>
                <java.version>11</java.version>
                <maven.compiler.target>11</maven.compiler.target>
                <maven.compiler.release>21</maven.compiler.release>
                <maven.compiler.source>11</maven.compiler.source>
              </properties>
            </project>"#;
        assert_eq!(release_from_pom(xml), Some((21, "maven.compiler.release")));
    }

    /// The compiler plugin's own configuration is the other half of how
    /// every real POM states this — including from inside `<profiles>` and
    /// `<pluginManagement>`, which is why the path is matched on its tail.
    #[test]
    fn pom_compiler_plugin_configuration_is_read_wherever_the_plugin_sits() {
        let xml = r#"
            <project>
              <build><plugins><plugin>
                <artifactId>maven-compiler-plugin</artifactId>
                <configuration><release>17</release></configuration>
              </plugin></plugins></build>
            </project>"#;
        assert_eq!(release_from_pom(xml), Some((17, "maven.compiler.release")));

        let profiled = r#"
            <project>
              <profiles><profile><build><pluginManagement><plugins><plugin>
                <artifactId>maven-compiler-plugin</artifactId>
                <configuration><source>1.7</source></configuration>
              </plugin></plugins></pluginManagement></build></profile></profiles>
            </project>"#;
        assert_eq!(release_from_pom(profiled), Some((7, "maven.compiler.source")));
    }

    #[test]
    fn a_pom_that_says_nothing_about_the_compiler_is_none() {
        let xml = "<project><artifactId>demo</artifactId></project>";
        assert_eq!(release_from_pom(xml), None);
    }

    #[test]
    fn gradle_groovy_source_compatibility_is_read() {
        let build = "plugins { id 'java' }\nsourceCompatibility = JavaVersion.VERSION_1_8\n";
        assert_eq!(release_from_gradle(build), Some((8, "sourceCompatibility")));

        let bare = "sourceCompatibility = 11\n";
        assert_eq!(release_from_gradle(bare), Some((11, "sourceCompatibility")));
    }

    #[test]
    fn gradle_toolchain_outranks_source_compatibility() {
        let build = r#"
            sourceCompatibility = 11
            java {
                toolchain {
                    languageVersion = JavaLanguageVersion.of(21)
                }
            }
        "#;
        assert_eq!(release_from_gradle(build), Some((21, "java.toolchain.languageVersion")));
    }

    #[test]
    fn gradle_kotlin_dsl_jvm_toolchain_is_read() {
        let build = "kotlin {\n    jvmToolchain(17)\n}\n";
        assert_eq!(release_from_gradle(build), Some((17, "kotlin.jvmToolchain")));
    }

    /// A commented-out setting is not the project's setting.
    #[test]
    fn gradle_ignores_a_commented_out_setting() {
        let build = "// sourceCompatibility = 8\nsourceCompatibility = 17\n";
        assert_eq!(release_from_gradle(build), Some((17, "sourceCompatibility")));
    }

    #[test]
    fn version_files_are_read_in_their_own_formats() {
        assert_eq!(release_from_version_file("17.0.9\n", ".java-version"), Some((17, ".java-version")));
        assert_eq!(
            release_from_version_file("temurin-11.0.22\n", ".java-version"),
            Some((11, ".java-version"))
        );
        assert_eq!(
            release_from_version_file("# comment\njava=21.0.3-zulu\n", ".sdkmanrc"),
            Some((21, "java"))
        );
    }

    /// A `pom.xml` answers before a `.java-version` that happens to sit
    /// beside it: the build file states what the code targets, the version
    /// file only what JDK the developer runs.
    #[test]
    fn detect_prefers_the_build_file_over_a_version_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pom.xml"),
            "<project><properties><java.version>8</java.version></properties></project>",
        )
        .unwrap();
        std::fs::write(dir.path().join(".java-version"), "21\n").unwrap();

        let found = detect(dir.path()).expect("detected");
        assert_eq!(found.major, 8);
        assert_eq!(found.file, "pom.xml");
        assert_eq!(found.setting, "java.version");
    }

    #[test]
    fn detect_falls_back_to_a_version_file_and_then_to_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect(dir.path()), None);

        std::fs::write(dir.path().join(".java-version"), "11\n").unwrap();
        assert_eq!(detect(dir.path()).map(|found| found.major), Some(11));
    }

    /// A Gradle project with no Java statement at all must not be reported
    /// as some default release — `None` leaves jdt.ls on its own default,
    /// which is the honest answer.
    #[test]
    fn detect_on_a_gradle_build_that_says_nothing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "plugins { id 'java' }\n").unwrap();
        assert_eq!(detect(dir.path()), None);
    }
}
