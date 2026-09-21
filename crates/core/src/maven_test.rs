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
        project
            .properties
            .get("project.build.sourceEncoding")
            .map(String::as_str),
        Some("UTF-8")
    );
    assert_eq!(
        project.properties.get("maven.compiler.source").map(String::as_str),
        Some("1.8")
    );

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
        vec![
            "app-bundle".to_string(),
            "api".to_string(),
            "backend".to_string(),
            "database".to_string()
        ]
    );

    assert_eq!(
        project.properties.get("kotlin.version").map(String::as_str),
        Some("2.1.21")
    );
    // A comment between two <properties> children must not corrupt either
    // one's own key/value.
    assert_eq!(
        project.properties.get("junit-jupiter.version").map(String::as_str),
        Some("5.9.1")
    );

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
    assert_eq!(
        project.dependencies[0].version, None,
        "version-less, resolved via the parent's BOM"
    );
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
