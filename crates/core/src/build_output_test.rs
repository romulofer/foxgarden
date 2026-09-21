use super::*;

#[test]
fn detects_maven_by_pom_xml() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pom.xml"), "").unwrap();
    assert_eq!(detect_build_tool(dir.path()), Some(BuildTool::Maven));
}

#[test]
fn detects_gradle_by_build_gradle_kts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();
    assert_eq!(detect_build_tool(dir.path()), Some(BuildTool::Gradle));
}

#[test]
fn detects_gradle_by_plain_build_gradle() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("build.gradle"), "").unwrap();
    assert_eq!(detect_build_tool(dir.path()), Some(BuildTool::Gradle));
}

#[test]
fn detects_neither_with_no_build_file() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(detect_build_tool(dir.path()), None);
}

#[cfg(not(windows))]
#[test]
fn maven_command_prefers_a_real_mvnw_over_the_bare_binary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mvnw"), "#!/bin/sh\n").unwrap();
    let cmd = maven_command(dir.path());
    assert_eq!(cmd.get_program(), dir.path().join("mvnw").as_os_str());
}

#[cfg(not(windows))]
#[test]
fn maven_command_falls_back_to_the_bare_binary_with_no_wrapper_present() {
    let dir = tempfile::tempdir().unwrap();
    let cmd = maven_command(dir.path());
    assert_eq!(cmd.get_program(), "mvn");
}

#[cfg(windows)]
#[test]
fn maven_command_prefers_a_real_mvnw_cmd_over_the_bare_binary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mvnw.cmd"), "@echo off\r\n").unwrap();
    let cmd = maven_command(dir.path());
    assert_eq!(cmd.get_program(), dir.path().join("mvnw.cmd").as_os_str());
}

#[cfg(windows)]
#[test]
fn maven_command_falls_back_to_the_bare_binary_with_no_wrapper_present() {
    let dir = tempfile::tempdir().unwrap();
    let cmd = maven_command(dir.path());
    assert_eq!(cmd.get_program(), "mvn");
}

#[cfg(windows)]
#[test]
fn maven_command_ignores_a_unix_only_mvnw_with_no_cmd_counterpart() {
    // A wrapper-generated project always ships both `mvnw`/`mvnw.cmd`
    // together, but a hand-rolled or stripped-down one might not —
    // `Command::new`ing the extensionless POSIX script directly would
    // fail outright on Windows (no shebang support), so this must fall
    // back to the bare `mvn` on PATH instead of trying it.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mvnw"), "#!/bin/sh\n").unwrap();
    let cmd = maven_command(dir.path());
    assert_eq!(cmd.get_program(), "mvn");
}

// --- Maven: real `mvn -B compile` output against a deliberately broken
// two-error fixture, captured verbatim (see PLAN.md Track 22 Phase 1) ---

#[test]
fn parses_a_real_maven_compiler_error_line() {
    let line = "[ERROR] /home/user/project/src/main/java/com/example/Foo.java:[5,17] cannot find symbol";
    let problem = parse_build_output_line(line).unwrap();
    assert_eq!(
        problem.path,
        PathBuf::from("/home/user/project/src/main/java/com/example/Foo.java")
    );
    assert_eq!(problem.line, 5);
    assert_eq!(problem.column, 17);
    assert_eq!(problem.severity, Severity::Error);
    assert_eq!(problem.message, "cannot find symbol");
}

#[test]
fn parses_a_real_maven_compiler_warning_line() {
    let line = "[WARNING] /home/user/project/src/main/java/Foo.java:[3,1] some warning text";
    let problem = parse_build_output_line(line).unwrap();
    assert_eq!(problem.severity, Severity::Warning);
    assert_eq!(problem.line, 3);
    assert_eq!(problem.column, 1);
}

#[test]
fn ignores_maven_lines_with_no_source_location() {
    assert!(
        parse_build_output_line(
            "[ERROR] Failed to execute goal org.apache.maven.plugins:maven-compiler-plugin:3.11.0:compile"
        )
        .is_none()
    );
    assert!(parse_build_output_line("[ERROR] COMPILATION ERROR : ").is_none());
    assert!(parse_build_output_line("[ERROR]   symbol:   variable undefinedSymbol").is_none());
}

// --- Gradle: real `gradle --console=plain compileJava` output (plain
// javac shape, no Maven wrapper) ---

#[test]
fn parses_a_real_gradle_javac_error_line() {
    let line = "/home/user/project/src/main/java/com/example/Foo.java:5: error: cannot find symbol";
    let problem = parse_build_output_line(line).unwrap();
    assert_eq!(
        problem.path,
        PathBuf::from("/home/user/project/src/main/java/com/example/Foo.java")
    );
    assert_eq!(problem.line, 5);
    assert_eq!(problem.column, 1);
    assert_eq!(problem.severity, Severity::Error);
    assert_eq!(problem.message, "cannot find symbol");
}

#[test]
fn parses_the_indented_copy_gradles_own_summary_repeats() {
    // The exact 2-space-indented repetition Gradle's own "* What went
    // wrong" section prints for the same error.
    let line = "  /home/user/project/src/main/java/com/example/Foo.java:5: error: cannot find symbol";
    let problem = parse_build_output_line(line).unwrap();
    assert_eq!(problem.line, 5);
}

#[test]
fn parses_a_real_gradle_javac_warning_line() {
    let line = "/home/user/project/src/main/java/Foo.java:9: warning: some warning text";
    let problem = parse_build_output_line(line).unwrap();
    assert_eq!(problem.severity, Severity::Warning);
}

#[test]
fn ignores_gradle_source_echo_and_caret_lines() {
    assert!(parse_build_output_line("        int x = undefinedSymbol;").is_none());
    assert!(parse_build_output_line("                ^").is_none());
    assert!(parse_build_output_line("  symbol:   variable undefinedSymbol").is_none());
    assert!(parse_build_output_line("2 errors").is_none());
}

#[test]
fn ignores_plain_prose_lines() {
    assert!(parse_build_output_line("BUILD FAILED in 3s").is_none());
    assert!(parse_build_output_line("").is_none());
}
