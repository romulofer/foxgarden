use super::*;

const JACOCO_REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<!DOCTYPE report PUBLIC "-//JACOCO//DTD Report 1.1//EN" "report.dtd">
<report name="demo">
  <package name="com/example">
    <class name="com/example/Calc" sourcefilename="Calc.java">
      <counter type="INSTRUCTION" missed="3" covered="6"/>
    </class>
    <sourcefile name="Calc.java">
      <line nr="3" mi="0" ci="3" mb="0" cb="0"/>
      <line nr="7" mi="2" ci="0" mb="0" cb="0"/>
      <line nr="9" mi="1" ci="1" mb="0" cb="0"/>
      <line nr="12" mi="0" ci="2" mb="1" cb="1"/>
      <counter type="LINE" missed="2" covered="2"/>
    </sourcefile>
    <counter type="LINE" missed="2" covered="2"/>
  </package>
</report>"#;

#[test]
fn parses_a_real_jacoco_xml_report() {
    let files = parse_jacoco_xml(JACOCO_REPORT).unwrap();
    assert_eq!(files.len(), 1);
    let (path, lines) = &files[0];
    assert_eq!(path, "com/example/Calc.java");
    assert_eq!(lines.len(), 4);
    // nr=3 (0-based line 2): instructions fully covered, no branches -> Covered.
    assert_eq!(
        lines.iter().find(|l| l.line == 2).unwrap().status,
        CoverageStatus::Covered
    );
    // nr=7 (line 6): instructions fully missed -> Missed.
    assert_eq!(
        lines.iter().find(|l| l.line == 6).unwrap().status,
        CoverageStatus::Missed
    );
    // nr=9 (line 8): instructions split, zero branches -> Partial from
    // instruction counters alone.
    assert_eq!(
        lines.iter().find(|l| l.line == 8).unwrap().status,
        CoverageStatus::Partial
    );
    // nr=12 (line 11): instructions FULLY covered (mi=0) but its own
    // branch is split -> still Partial, proving "mi=0 => Covered" alone
    // is wrong without also checking the branch counters.
    assert_eq!(
        lines.iter().find(|l| l.line == 11).unwrap().status,
        CoverageStatus::Partial
    );
}

#[test]
fn line_status_matches_the_decompiled_jacoco_algorithm() {
    assert_eq!(line_status(0, 0, 0, 0), None); // EMPTY, real reports never emit this
    assert_eq!(line_status(1, 0, 0, 0), Some(CoverageStatus::Missed));
    assert_eq!(line_status(0, 1, 0, 0), Some(CoverageStatus::Covered));
    assert_eq!(line_status(1, 1, 0, 0), Some(CoverageStatus::Partial));
    assert_eq!(line_status(0, 1, 1, 1), Some(CoverageStatus::Partial)); // mi=0 but branch split
    assert_eq!(line_status(0, 1, 1, 0), Some(CoverageStatus::Partial)); // mi=0 but branch fully missed
}

#[test]
fn resolve_source_file_finds_the_standard_maven_layout() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src/main/java/com/example");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("Calc.java"), "").unwrap();
    assert_eq!(
        resolve_source_file(dir.path(), "com/example/Calc.java"),
        Some(src.join("Calc.java"))
    );
}

#[test]
fn resolve_source_file_is_none_when_nothing_matches() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(resolve_source_file(dir.path(), "com/example/Calc.java"), None);
}

#[test]
fn coverage_command_pins_the_jacoco_plugin_version_and_exact_exec_path() {
    let dir = tempfile::tempdir().unwrap();
    let command = coverage_command(dir.path());
    let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert!(
        args.iter()
            .any(|a| a.contains("jacoco-maven-plugin:0.8.12:prepare-agent"))
    );
    assert!(args.iter().any(|a| a == "test"));
    assert!(args.iter().any(|a| a.contains("jacoco-maven-plugin:0.8.12:report")));
    assert!(args.iter().any(|a| a.contains("maven.test.failure.ignore=true")));
}

#[test]
fn resolve_coverage_paths_drops_entries_that_dont_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src/main/java/com/example");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("Calc.java"), "").unwrap();
    let parsed = vec![
        (
            "com/example/Calc.java".to_string(),
            vec![LineCoverage {
                line: 0,
                status: CoverageStatus::Covered,
            }],
        ),
        (
            "com/example/Missing.java".to_string(),
            vec![LineCoverage {
                line: 0,
                status: CoverageStatus::Missed,
            }],
        ),
    ];
    let resolved = resolve_coverage_paths(dir.path(), parsed);
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].0, src.join("Calc.java"));
}
