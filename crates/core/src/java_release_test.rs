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
    assert_eq!(
        release_from_version_file("17.0.9\n", ".java-version"),
        Some((17, ".java-version"))
    );
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
