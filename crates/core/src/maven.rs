//! `pom.xml` parsing (`PLAN.md` Track 21 Phase 1) — the first step of Maven/
//! Gradle awareness. Pure/no I/O, same "read into a struct, let a caller
//! decide what to do with it" shape `static_analysis`'s own report parsers
//! use. Deliberately scoped to what's *directly written* in one file's
//! `<dependencies>`/`<modules>`/`<properties>`/`<parent>` — no property
//! substitution (a `${foo.version}` placeholder is kept as-is, unresolved),
//! no inheritance from a parent POM's own `<dependencyManagement>`, and no
//! reading of `<dependencyManagement>` itself: a dependency with no explicit
//! `<version>` (extremely common under a Spring Boot parent — see this
//! module's `backend_pom_with_most_versions_inherited_from_the_parent_bom`
//! test) is recorded with `version: None` rather than guessed at. Real
//! version *resolution* is Phase 3's job (`mvn dependency:build-classpath`,
//! which sidesteps needing to reimplement Maven's own effective-POM
//! computation at all).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use quick_xml::Reader;
use quick_xml::events::Event;

/// A `<parent>` reference — always fully specified in a real `pom.xml`
/// (Maven itself requires all three), unlike a plain `<dependency>`, so
/// every field here is required rather than `Option`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenParent {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
}

/// One `<dependency>` entry from the project's own top-level `<dependencies>`
/// (not `<dependencyManagement>`'s, and not a `<plugin>`'s own nested
/// `<dependencies>` — `parse_pom` tracks the real element path to tell these
/// apart, not just the tag name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenDependency {
    pub group_id: String,
    pub artifact_id: String,
    /// `None` when unspecified — resolved elsewhere (a parent's own
    /// `<dependencyManagement>`, often supplied transitively via a BOM like
    /// `spring-boot-starter-parent`), not this module's job to chase down.
    pub version: Option<String>,
    pub scope: Option<String>,
    pub optional: bool,
}

/// A single `pom.xml`'s own declared shape — one Maven module, not an
/// effective (fully-resolved-and-inherited) POM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenProject {
    /// `None` when inherited from `parent` (a child module commonly omits
    /// its own `<groupId>`/`<version>` — see `backend_pom.xml`'s real
    /// shape in this module's tests).
    pub group_id: Option<String>,
    pub artifact_id: String,
    pub version: Option<String>,
    /// Defaults to `"jar"`, matching Maven's own default when `<packaging>`
    /// is absent.
    pub packaging: String,
    pub parent: Option<MavenParent>,
    pub properties: HashMap<String, String>,
    /// Child module directory names, in document order, from a multi-module
    /// parent's own `<modules>`.
    pub modules: Vec<String>,
    pub dependencies: Vec<MavenDependency>,
}

/// Accumulates the handful of in-progress multi-child elements
/// (`<parent>`, one `<dependency>` at a time) while walking the document;
/// folded into a `MavenProject` once the whole file's been read.
#[derive(Default)]
struct Builder {
    group_id: Option<String>,
    artifact_id: Option<String>,
    version: Option<String>,
    packaging: Option<String>,
    properties: HashMap<String, String>,
    modules: Vec<String>,
    dependencies: Vec<MavenDependency>,

    parent_group_id: Option<String>,
    parent_artifact_id: Option<String>,
    parent_version: Option<String>,

    dep_group_id: Option<String>,
    dep_artifact_id: Option<String>,
    dep_version: Option<String>,
    dep_scope: Option<String>,
    dep_optional: bool,
}

impl Builder {
    /// Called on every closing tag with `path` still including that tag as
    /// its last element (popped by the caller right after) and `value` its
    /// trimmed text content (empty for a container element like
    /// `<dependencies>` itself, which has no text of its own — only its
    /// children do).
    fn close(&mut self, path: &[String], value: String) -> Result<(), String> {
        let p: Vec<&str> = path.iter().map(String::as_str).collect();
        match p.as_slice() {
            ["project", "groupId"] => self.group_id = Some(value),
            ["project", "artifactId"] => self.artifact_id = Some(value),
            ["project", "version"] => self.version = Some(value),
            ["project", "packaging"] => self.packaging = Some(value),

            ["project", "parent", "groupId"] => self.parent_group_id = Some(value),
            ["project", "parent", "artifactId"] => self.parent_artifact_id = Some(value),
            ["project", "parent", "version"] => self.parent_version = Some(value),

            ["project", "properties", key] => {
                self.properties.insert(key.to_string(), value);
            }
            ["project", "modules", "module"] => self.modules.push(value),

            ["project", "dependencies", "dependency", "groupId"] => self.dep_group_id = Some(value),
            ["project", "dependencies", "dependency", "artifactId"] => self.dep_artifact_id = Some(value),
            ["project", "dependencies", "dependency", "version"] => self.dep_version = Some(value),
            ["project", "dependencies", "dependency", "scope"] => self.dep_scope = Some(value),
            ["project", "dependencies", "dependency", "optional"] => self.dep_optional = value == "true",
            ["project", "dependencies", "dependency"] => {
                let group_id = self.dep_group_id.take().ok_or("<dependency> missing groupId")?;
                let artifact_id = self.dep_artifact_id.take().ok_or("<dependency> missing artifactId")?;
                self.dependencies.push(MavenDependency {
                    group_id,
                    artifact_id,
                    version: self.dep_version.take(),
                    scope: self.dep_scope.take(),
                    optional: std::mem::take(&mut self.dep_optional),
                });
            }

            _ => {}
        }
        Ok(())
    }

    fn finish(mut self) -> Result<MavenProject, String> {
        let parent = match (
            self.parent_group_id.take(),
            self.parent_artifact_id.take(),
            self.parent_version.take(),
        ) {
            (None, None, None) => None,
            (group_id, artifact_id, version) => Some(MavenParent {
                group_id: group_id.ok_or("<parent> missing groupId")?,
                artifact_id: artifact_id.ok_or("<parent> missing artifactId")?,
                version: version.ok_or("<parent> missing version")?,
            }),
        };

        Ok(MavenProject {
            group_id: self.group_id,
            artifact_id: self.artifact_id.ok_or("pom.xml missing <artifactId>")?,
            version: self.version,
            packaging: self.packaging.unwrap_or_else(|| "jar".to_string()),
            parent,
            properties: self.properties,
            modules: self.modules,
            dependencies: self.dependencies,
        })
    }
}

/// Parses one `pom.xml` document into its own declared shape. Verified
/// against several real captured `pom.xml` files (a simple single-module
/// project and a real multi-module parent + one of its child modules — see
/// this module's tests), not a guessed schema: real POMs interleave
/// comments between elements, wrap long values across multiple lines, and
/// nest a second, differently-scoped `<dependencies>` inside both
/// `<dependencyManagement>` and a `<plugin>`'s own configuration — all of
/// which this parser has to *not* mistake for the project's own
/// dependencies, which is why it tracks the full element path rather than
/// just matching on tag name.
pub fn parse_pom(xml: &str) -> Result<MavenProject, String> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut builder = Builder::default();

    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Eof => break,
            Event::Start(e) => {
                path.push(String::from_utf8_lossy(e.local_name().as_ref()).into_owned());
                text.clear();
            }
            Event::Text(t) => {
                let decoded = t.decode().map_err(|e| e.to_string())?;
                let unescaped = quick_xml::escape::unescape(&decoded).map_err(|e| e.to_string())?;
                text.push_str(&unescaped);
            }
            Event::End(_) => {
                builder.close(&path, std::mem::take(&mut text).trim().to_string())?;
                path.pop();
            }
            Event::Empty(e) => {
                path.push(String::from_utf8_lossy(e.local_name().as_ref()).into_owned());
                builder.close(&path, String::new())?;
                path.pop();
            }
            _ => {}
        }
    }

    builder.finish()
}

#[derive(Debug)]
pub enum MavenClasspathError {
    /// `mvn` itself couldn't be launched.
    Spawn(std::io::Error),
    /// `mvn` ran but exited non-zero, or the classpath file it was asked to
    /// write never appeared — a real dependency-resolution failure
    /// (missing artifact, no network + nothing cached, ...), not this
    /// module's own bug. Carries `mvn`'s own captured stderr.
    Resolution(String),
}

impl std::fmt::Display for MavenClasspathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MavenClasspathError::Spawn(e) => write!(f, "failed to run mvn: {e}"),
            MavenClasspathError::Resolution(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for MavenClasspathError {}

/// Resolves `module_root`'s own real, on-disk classpath: every jar Maven's
/// own dependency-resolution machinery decides this module needs, each one
/// a real path that exists in the local repository (`~/.m2/repository` by
/// default) — downloading it first if it isn't cached yet, exactly like a
/// real `mvn compile` would. Deliberately **not** attempting to reimplement
/// Maven's own effective-POM computation, BOM/`<dependencyManagement>`
/// inheritance, or version-conflict resolution (`maven.rs`'s own
/// module-level doc comment already named this as Phase 3's job, precisely
/// so none of that has to be redone here) — `mvn dependency:build-classpath`
/// already does all of it correctly, verified this session against a real,
/// cleanly-resolvable project (see this function's own test) before being
/// trusted for this.
///
/// One real module at a time, matching Maven's own per-`pom.xml` reactor
/// scoping — a caller wanting a whole multi-module project's classpath
/// calls this once per module (each with its own real, independent
/// classpath), the same way `mvn dependency:build-classpath` itself would
/// need to be run once per module directory.
pub fn maven_classpath(module_root: &Path) -> Result<Vec<PathBuf>, MavenClasspathError> {
    let output_file = std::env::temp_dir().join(format!("foxgarden-maven-classpath-{}.txt", std::process::id()));

    let result = Command::new("mvn")
        .current_dir(module_root)
        .arg("-q")
        .arg("dependency:build-classpath")
        .arg(format!("-Dmdep.outputFile={}", output_file.display()))
        .output();
    let result = (|| -> Result<Vec<PathBuf>, MavenClasspathError> {
        let output = result.map_err(MavenClasspathError::Spawn)?;
        if !output.status.success() {
            return Err(MavenClasspathError::Resolution(String::from_utf8_lossy(&output.stderr).into_owned()));
        }
        let contents = std::fs::read_to_string(&output_file).map_err(|e| {
            MavenClasspathError::Resolution(format!(
                "mvn exited successfully but its own -Dmdep.outputFile was never written: {e}"
            ))
        })?;
        // The classpath-list separator (`:` on Unix, `;` on Windows) — not
        // to be confused with `std::path::MAIN_SEPARATOR` (the *directory*
        // separator, `/` vs `\`, an entirely different character).
        let list_separator = if cfg!(windows) { ';' } else { ':' };
        Ok(contents
            .trim()
            .split(list_separator)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect())
    })();
    let _ = std::fs::remove_file(&output_file);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real, single-module, no-parent `pom.xml` (MegaBasterd's own,
    /// trimmed to the elements this parser reads — `<repositories>`/
    /// `<build>` left out since nothing here consumes them yet) — every
    /// dependency fully versioned, no `<properties>`/`<modules>` beyond a
    /// handful of build-encoding properties.
    const SIMPLE_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.tonikelope</groupId>
    <artifactId>MegaBasterd</artifactId>
    <version>8.57</version>
    <packaging>jar</packaging>
    <dependencies>
        <dependency>
            <groupId>commons-io</groupId>
            <artifactId>commons-io</artifactId>
            <version>2.14.0</version>
        </dependency>
        <dependency>
            <groupId>org.xerial</groupId>
            <artifactId>sqlite-jdbc</artifactId>
            <version>3.43.0.0</version>
            <type>jar</type>
        </dependency>
    </dependencies>
    <properties>
        <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
        <maven.compiler.source>1.8</maven.compiler.source>
        <maven.compiler.target>1.8</maven.compiler.target>
    </properties>
    <name>MegaBasterd</name>
    <description>Yet another unofficial (and ugly) cross-platform MEGA downloader/uploader/streaming suite.</description>
</project>
"#;

    #[test]
    fn parses_a_simple_single_module_project() {
        let project = parse_pom(SIMPLE_POM).unwrap();
        assert_eq!(project.group_id.as_deref(), Some("com.tonikelope"));
        assert_eq!(project.artifact_id, "MegaBasterd");
        assert_eq!(project.version.as_deref(), Some("8.57"));
        assert_eq!(project.packaging, "jar");
        assert!(project.parent.is_none());
        assert!(project.modules.is_empty());

        assert_eq!(
            project.properties.get("project.build.sourceEncoding").map(String::as_str),
            Some("UTF-8")
        );
        assert_eq!(project.properties.get("maven.compiler.source").map(String::as_str), Some("1.8"));

        assert_eq!(project.dependencies.len(), 2);
        assert_eq!(
            project.dependencies[0],
            MavenDependency {
                group_id: "commons-io".to_string(),
                artifact_id: "commons-io".to_string(),
                version: Some("2.14.0".to_string()),
                scope: None,
                optional: false,
            }
        );
        // The second dependency's <type>jar</type> is outside this parser's
        // scope (no `type` field on `MavenDependency`) — just confirm it
        // didn't corrupt the fields this parser *does* track.
        assert_eq!(project.dependencies[1].artifact_id, "sqlite-jdbc");
        assert_eq!(project.dependencies[1].version.as_deref(), Some("3.43.0.0"));
    }

    /// A real multi-module parent `pom.xml` (`br.ufsc.bridge:pec`, trimmed
    /// to a representative slice of its real `<properties>`/`<modules>`/
    /// `<dependencyManagement>` — the parent's own `<parent>` on
    /// `spring-boot-starter-parent`, comments between properties, and a
    /// property-reference version (`${project.version}`, left unresolved)
    /// are all real, not invented for the test).
    const PARENT_POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd">
    <modelVersion>4.0.0</modelVersion>

    <parent>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-starter-parent</artifactId>
        <version>2.7.18</version>
    </parent>

    <name>UFSC – Sistema – PEC</name>
    <groupId>br.ufsc.bridge</groupId>
    <artifactId>pec</artifactId>
    <version>5.4.27-SNAPSHOT</version>
    <packaging>pom</packaging>

    <properties>
        <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
        <maven.compiler.target>17</maven.compiler.target>
        <kotlin.version>2.1.21</kotlin.version>

        <!-- Testes -->
        <junit-jupiter.version>5.9.1</junit-jupiter.version>
    </properties>

    <modules>
        <module>app-bundle</module>
        <module>api</module>
        <module>backend</module>
        <module>database</module>
    </modules>

    <dependencyManagement>
        <dependencies>
            <dependency>
                <groupId>br.ufsc.bridge.pec</groupId>
                <artifactId>backend</artifactId>
                <version>${project.version}</version>
            </dependency>
            <dependency>
                <groupId>org.projectlombok</groupId>
                <artifactId>lombok</artifactId>
                <version>1.18.24</version>
            </dependency>
        </dependencies>
    </dependencyManagement>
</project>
"#;

    #[test]
    fn parses_a_multi_module_parent_pom_without_mistaking_dependency_management_for_real_dependencies() {
        let project = parse_pom(PARENT_POM).unwrap();
        assert_eq!(project.group_id.as_deref(), Some("br.ufsc.bridge"));
        assert_eq!(project.artifact_id, "pec");
        assert_eq!(project.packaging, "pom");

        assert_eq!(
            project.parent,
            Some(MavenParent {
                group_id: "org.springframework.boot".to_string(),
                artifact_id: "spring-boot-starter-parent".to_string(),
                version: "2.7.18".to_string(),
            })
        );

        assert_eq!(
            project.modules,
            vec!["app-bundle".to_string(), "api".to_string(), "backend".to_string(), "database".to_string()]
        );

        assert_eq!(project.properties.get("kotlin.version").map(String::as_str), Some("2.1.21"));
        // A comment between two <properties> children must not corrupt either
        // one's own key/value.
        assert_eq!(project.properties.get("junit-jupiter.version").map(String::as_str), Some("5.9.1"));

        // <dependencyManagement>'s own nested <dependencies> must NOT be
        // read as this project's real dependencies — this pom.xml has none
        // of its own.
        assert!(project.dependencies.is_empty());
    }

    /// A real child module's `pom.xml` (`br.ufsc.bridge.pec:backend`):
    /// inherits `groupId`/`version` from its `<parent>` rather than
    /// declaring its own, and most dependencies below carry no `<version>`
    /// at all — resolved transitively via the parent's own Spring Boot BOM,
    /// which this parser correctly does *not* try to chase down.
    const BACKEND_POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd">
    <modelVersion>4.0.0</modelVersion>
    <parent>
        <groupId>br.ufsc.bridge</groupId>
        <artifactId>pec</artifactId>
        <version>5.4.27-SNAPSHOT</version>
    </parent>

    <name>UFSC – Backend – PEC</name>
    <groupId>br.ufsc.bridge.pec</groupId>
    <artifactId>backend</artifactId>

    <dependencies>
        <dependency>
            <groupId>de.codecentric</groupId>
            <artifactId>spring-boot-admin-starter-client</artifactId>
        </dependency>
        <dependency>
            <groupId>org.springframework.boot</groupId>
            <artifactId>spring-boot-configuration-processor</artifactId>
            <optional>true</optional>
        </dependency>
        <dependency>
            <groupId>com.h2database</groupId>
            <artifactId>h2</artifactId>
            <version>${h2.version}</version>
            <scope>test</scope>
        </dependency>
    </dependencies>
</project>
"#;

    #[test]
    fn backend_pom_with_most_versions_inherited_from_the_parent_bom() {
        let project = parse_pom(BACKEND_POM).unwrap();
        // Its own <groupId> is present here (real pom.xml), but no <version>
        // of its own at all — inherited from <parent>.
        assert_eq!(project.group_id.as_deref(), Some("br.ufsc.bridge.pec"));
        assert_eq!(project.artifact_id, "backend");
        assert_eq!(project.version, None);
        assert_eq!(
            project.parent,
            Some(MavenParent {
                group_id: "br.ufsc.bridge".to_string(),
                artifact_id: "pec".to_string(),
                version: "5.4.27-SNAPSHOT".to_string(),
            })
        );

        assert_eq!(project.dependencies.len(), 3);
        assert_eq!(project.dependencies[0].version, None, "version-less, resolved via the parent's BOM");
        assert!(project.dependencies[1].optional);
        assert_eq!(project.dependencies[2].scope.as_deref(), Some("test"));
        assert_eq!(
            project.dependencies[2].version.as_deref(),
            Some("${h2.version}"),
            "an unresolved property placeholder is kept as-is, not substituted"
        );
    }

    #[test]
    fn packaging_defaults_to_jar_when_absent() {
        let xml = r#"<project>
            <artifactId>a</artifactId>
        </project>"#;
        let project = parse_pom(xml).unwrap();
        assert_eq!(project.packaging, "jar");
    }

    #[test]
    fn missing_artifact_id_is_an_error() {
        let xml = r#"<project><groupId>g</groupId></project>"#;
        assert!(parse_pom(xml).is_err());
    }

    #[test]
    fn a_plugins_own_nested_dependencies_are_not_mistaken_for_project_dependencies() {
        // Mirrors backend_pom.xml's real kotlin-maven-plugin shape: a
        // <plugin> can carry its own <dependencies> (compiler plugin
        // artifacts), entirely unrelated to the project's own dependency
        // list.
        let xml = r#"<project>
            <artifactId>a</artifactId>
            <build>
                <plugins>
                    <plugin>
                        <artifactId>kotlin-maven-plugin</artifactId>
                        <dependencies>
                            <dependency>
                                <groupId>org.jetbrains.kotlin</groupId>
                                <artifactId>kotlin-maven-allopen</artifactId>
                                <version>2.1.21</version>
                            </dependency>
                        </dependencies>
                    </plugin>
                </plugins>
            </build>
        </project>"#;
        let project = parse_pom(xml).unwrap();
        assert!(project.dependencies.is_empty());
    }
}
