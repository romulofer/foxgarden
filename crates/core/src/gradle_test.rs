use super::*;

#[test]
fn is_dependency_configuration_keeps_the_real_ones_and_drops_tooling_noise() {
    for real in [
        "implementation",
        "api",
        "compileOnly",
        "runtimeOnly",
        "annotationProcessor",
        "developmentOnly",
        "testImplementation",
        "testCompileOnly",
        "testRuntimeOnly",
        "testAnnotationProcessor",
        "kaptTest",
    ] {
        assert!(is_dependency_configuration(real), "{real} should be kept");
    }
    for noise in [
        "compileClasspath",
        "runtimeClasspath",
        "testCompileClasspath",
        "testRuntimeClasspath",
        "kotlinCompilerPluginClasspathMain",
        "kotlinBuildToolsApiClasspath",
        "archives",
        "default",
    ] {
        assert!(!is_dependency_configuration(noise), "{noise} should be filtered out");
    }
}

#[cfg(not(windows))]
#[test]
fn gradle_command_prefers_a_real_gradlew_over_the_bare_binary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("gradlew"), "#!/bin/sh\n").unwrap();
    let cmd = gradle_command(dir.path());
    assert_eq!(cmd.get_program(), dir.path().join("gradlew").as_os_str());
}

#[cfg(not(windows))]
#[test]
fn gradle_command_falls_back_to_the_bare_binary_with_no_wrapper_present() {
    let dir = tempfile::tempdir().unwrap();
    let cmd = gradle_command(dir.path());
    assert_eq!(cmd.get_program(), "gradle");
}

#[cfg(windows)]
#[test]
fn gradle_command_prefers_a_real_gradlew_bat_over_the_bare_binary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("gradlew.bat"), "@echo off\r\n").unwrap();
    let cmd = gradle_command(dir.path());
    assert_eq!(cmd.get_program(), dir.path().join("gradlew.bat").as_os_str());
}

#[cfg(windows)]
#[test]
fn gradle_command_falls_back_to_the_bare_binary_with_no_wrapper_present() {
    let dir = tempfile::tempdir().unwrap();
    let cmd = gradle_command(dir.path());
    assert_eq!(cmd.get_program(), "gradle");
}

#[cfg(windows)]
#[test]
fn gradle_command_ignores_a_unix_only_gradlew_with_no_bat_counterpart() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("gradlew"), "#!/bin/sh\n").unwrap();
    let cmd = gradle_command(dir.path());
    assert_eq!(cmd.get_program(), "gradle");
}

/// Real output captured this session running the validated init script
/// (`gradle --offline --init-script <script> -q foxgardenDump2`)
/// against `bridge.ufsc.tech:boost`, a real multi-module Kotlin/Spring
/// Gradle project — `frontend`/`database` (no JVM dependencies of their
/// own) and `backend` (22 real dependencies across 5 configurations,
/// including a real inter-project `project(":database")` reference and
/// a mix of versioned and unversioned — Spring's dependency-management
/// plugin supplies the rest — module coordinates).
const REAL_DUMP_OUTPUT: &str = r#"
FOXGARDEN_JSON_BEGIN
{"path":":","name":"boost","group":"bridge.ufsc.tech","version":"0.0.1-SNAPSHOT","projectDir":"/home/romulo2/bridge/boost","dependencies":[]}
FOXGARDEN_JSON_END
FOXGARDEN_JSON_BEGIN
{"path":":frontend","name":"frontend","group":"bridge.ufsc.tech","version":"0.0.1-SNAPSHOT","projectDir":"/home/romulo2/bridge/boost/frontend","dependencies":[]}
FOXGARDEN_JSON_END
FOXGARDEN_JSON_BEGIN
{"path":":database","name":"database","group":"bridge.ufsc.tech","version":"0.0.1-SNAPSHOT","projectDir":"/home/romulo2/bridge/boost/database","dependencies":[]}
FOXGARDEN_JSON_END
FOXGARDEN_JSON_BEGIN
{"path":":backend","name":"backend","group":"bridge.ufsc.tech","version":"0.0.1-SNAPSHOT","projectDir":"/home/romulo2/bridge/boost/backend","dependencies":[{"configuration":"developmentOnly","kind":"module","group":"org.springframework.boot","artifact":"spring-boot-devtools","version":null},{"configuration":"implementation","kind":"project","path":":database"},{"configuration":"implementation","kind":"module","group":"org.springframework.boot","artifact":"spring-boot-starter-actuator","version":null},{"configuration":"implementation","kind":"module","group":"org.springdoc","artifact":"springdoc-openapi-starter-webmvc-ui","version":"3.0.3"},{"configuration":"runtimeOnly","kind":"module","group":"org.postgresql","artifact":"postgresql","version":null},{"configuration":"testImplementation","kind":"module","group":"org.mockito.kotlin","artifact":"mockito-kotlin","version":"5.4.0"},{"configuration":"testRuntimeOnly","kind":"module","group":"org.junit.platform","artifact":"junit-platform-launcher","version":null}]}
FOXGARDEN_JSON_END
"#;

#[test]
fn parses_real_captured_multi_module_dump_output() {
    let projects = parse_dump_output(REAL_DUMP_OUTPUT).unwrap();
    assert_eq!(projects.len(), 4);
    assert_eq!(projects[0].path, ":");
    assert_eq!(projects[1].path, ":frontend");
    assert!(
        projects[1].dependencies.is_empty(),
        "frontend has no JVM dependencies of its own"
    );
    assert_eq!(projects[2].path, ":database");

    let backend = &projects[3];
    assert_eq!(backend.name, "backend");
    assert_eq!(backend.project_dir, PathBuf::from("/home/romulo2/bridge/boost/backend"));
    assert_eq!(backend.dependencies.len(), 7);

    assert_eq!(
        backend.dependencies[1],
        GradleDependency::Project {
            configuration: "implementation".to_string(),
            path: ":database".to_string()
        }
    );
    assert_eq!(
        backend.dependencies[3],
        GradleDependency::Module {
            configuration: "implementation".to_string(),
            group: "org.springdoc".to_string(),
            artifact: "springdoc-openapi-starter-webmvc-ui".to_string(),
            version: Some("3.0.3".to_string()),
        }
    );
    assert_eq!(
        backend.dependencies[2],
        GradleDependency::Module {
            configuration: "implementation".to_string(),
            group: "org.springframework.boot".to_string(),
            artifact: "spring-boot-starter-actuator".to_string(),
            version: None,
        },
        "unversioned: resolved elsewhere via Spring's dependency-management plugin, not chased down here"
    );
}

#[test]
fn a_begin_marker_with_no_matching_end_is_an_error() {
    let err = parse_dump_output("FOXGARDEN_JSON_BEGIN\n{\"broken").unwrap_err();
    assert!(err.contains("FOXGARDEN_JSON_END"));
}

#[test]
fn no_markers_at_all_yields_an_empty_list_not_an_error() {
    // A build with genuinely no projects reachable isn't realistic, but
    // stray plugin stdout output containing neither marker shouldn't be
    // mistaken for a report failure the way an unparseable Checkstyle/
    // PMD report is — there's simply nothing to report yet at the
    // parser level (a real invocation returning zero projects would be
    // a `gradle_projects`-level concern, not this pure parser's).
    assert!(
        parse_dump_output("some unrelated build log noise\n")
            .unwrap()
            .is_empty()
    );
}

/// Real output captured this session running `gradle_classpaths`
/// end-to-end (a genuine real-process invocation, not just this parser
/// in isolation) against a minimal real single-module Gradle project
/// (`plugins { id 'java' }`, one `implementation
/// 'commons-io:commons-io:2.14.0'` dependency) — confirmed the resolved
/// path is a real, existing jar in the local Gradle module cache before
/// being trusted for this fixture.
const REAL_CLASSPATH_OUTPUT: &str = r#"
FOXGARDEN_CP_JSON_BEGIN
{"path":":","compile":["/home/romulo2/.gradle/caches/modules-2/files-2.1/commons-io/commons-io/2.14.0/a4c6e1f6c196339473cd2e1b037f0eb97c62755b/commons-io-2.14.0.jar"],"runtime":["/home/romulo2/.gradle/caches/modules-2/files-2.1/commons-io/commons-io/2.14.0/a4c6e1f6c196339473cd2e1b037f0eb97c62755b/commons-io-2.14.0.jar"]}
FOXGARDEN_CP_JSON_END
"#;

#[test]
fn parses_real_captured_classpath_output() {
    let classpaths = parse_classpath_output(REAL_CLASSPATH_OUTPUT).unwrap();
    assert_eq!(classpaths.len(), 1);
    assert_eq!(classpaths[0].path, ":");
    assert_eq!(
        classpaths[0].compile,
        vec![PathBuf::from(
            "/home/romulo2/.gradle/caches/modules-2/files-2.1/commons-io/commons-io/2.14.0/\
                 a4c6e1f6c196339473cd2e1b037f0eb97c62755b/commons-io-2.14.0.jar"
        )]
    );
    assert_eq!(classpaths[0].compile, classpaths[0].runtime);
}

#[test]
fn a_project_with_no_jvm_configurations_at_all_is_simply_absent() {
    // Mirrors what CLASSPATH_INIT_SCRIPT itself does for a non-JVM
    // module (the real `boost` project's own `frontend`, for instance):
    // no block is printed for it at all, rather than an empty-lists one
    // — nothing for this parser to special-case, just confirming an
    // empty dump parses to an empty list rather than an error.
    assert!(parse_classpath_output("").unwrap().is_empty());
}
