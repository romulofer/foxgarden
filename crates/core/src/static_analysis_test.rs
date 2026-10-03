use super::*;

#[test]
fn command_for_binary_wraps_a_jar_in_java_dash_jar() {
    let cmd = command_for_binary(Path::new("/tools/checkstyle-10.26.1-all.jar"));
    assert_eq!(cmd.get_program(), "java");
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, vec!["-jar", "/tools/checkstyle-10.26.1-all.jar"]);
}

#[test]
fn command_for_binary_runs_a_non_jar_path_directly() {
    let cmd = command_for_binary(Path::new("/tools/pmd-bin-7.26.0/bin/pmd"));
    assert_eq!(cmd.get_program(), "/tools/pmd-bin-7.26.0/bin/pmd");
}

/// Captured verbatim from a real `checkstyle -c sun_checks.xml -f xml`
/// run against a small fixture file with several real violations —
/// grammar shape verified fresh, not assumed, per this project's own
/// discipline for external report formats.
const REAL_REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<checkstyle version="8.36.1">
<file name="/tmp/fixture/Bad.java">
<error line="1" severity="error" message="Missing a package-info.java file." source="com.puppycrawl.tools.checkstyle.checks.javadoc.JavadocPackageCheck"/>
<error line="3" column="8" severity="error" message="Unused import - java.util.List." source="com.puppycrawl.tools.checkstyle.checks.imports.UnusedImportsCheck"/>
<error line="7" column="16" severity="warning" message="&apos;=&apos; is not preceded with whitespace." source="com.puppycrawl.tools.checkstyle.checks.whitespace.WhitespaceAroundCheck"/>
</file>
</checkstyle>
"#;

#[test]
fn parse_checkstyle_xml_extracts_every_error_with_its_file() {
    let findings = parse_checkstyle_xml(REAL_REPORT).expect("parses");
    assert_eq!(findings.len(), 3);
    assert_eq!(findings[0].file, PathBuf::from("/tmp/fixture/Bad.java"));
    assert_eq!(findings[0].line, 1);
    assert_eq!(findings[0].column, None);
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[1].column, Some(8));
}

#[test]
fn parse_checkstyle_xml_maps_non_error_severity_to_warning() {
    let findings = parse_checkstyle_xml(REAL_REPORT).expect("parses");
    assert_eq!(findings[2].severity, Severity::Warning);
}

#[test]
fn parse_checkstyle_xml_unescapes_xml_entities_in_the_message() {
    let findings = parse_checkstyle_xml(REAL_REPORT).expect("parses");
    assert_eq!(findings[2].message, "'=' is not preceded with whitespace.");
}

#[test]
fn parse_checkstyle_xml_rejects_an_error_outside_any_file() {
    let xml = r#"<checkstyle version="8.36.1"><error line="1" severity="error" message="x"/></checkstyle>"#;
    assert!(parse_checkstyle_xml(xml).is_err());
}

#[test]
fn line_col_to_byte_with_no_column_lands_at_line_start() {
    let buffer = Rope::from_str("first\nsecond\nthird");
    assert_eq!(line_col_to_byte(&buffer, 2, None), 6);
}

#[test]
fn line_col_to_byte_with_a_column_lands_inside_the_line() {
    let buffer = Rope::from_str("first\nsecond\nthird");
    // Line 2 ("second") starts at byte 6; column 3 is 1-based, so char
    // index 2 within the line -> byte 8.
    assert_eq!(line_col_to_byte(&buffer, 2, Some(3)), 8);
}

#[test]
fn line_col_to_byte_clamps_an_out_of_range_column_to_end_of_line() {
    let buffer = Rope::from_str("ab\ncd");
    assert_eq!(line_col_to_byte(&buffer, 1, Some(99)), 2);
}

#[test]
fn checkstyle_findings_to_diagnostics_reads_the_real_file_to_compute_byte_ranges() {
    let (_dir, path) = test_support::temp_file("Bad.java", "package demo;\nclass Bad {}\n");
    let findings = vec![CheckstyleFinding {
        file: path.clone(),
        line: 2,
        column: Some(7),
        severity: Severity::Warning,
        message: "example".to_string(),
    }];
    let diagnostics = checkstyle_findings_to_diagnostics(findings);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].0, path);
    // Line 2 ("class Bad {}") starts at byte 14; column 7 is char index
    // 6 within the line ('B' of "Bad") -> byte 20.
    assert_eq!(diagnostics[0].1.range, 20..21);
    assert_eq!(diagnostics[0].1.message, "example");
}

#[test]
fn checkstyle_findings_to_diagnostics_drops_findings_for_an_unreadable_file() {
    let findings = vec![CheckstyleFinding {
        file: PathBuf::from("/nonexistent/path/does/not/exist.java"),
        line: 1,
        column: None,
        severity: Severity::Error,
        message: "example".to_string(),
    }];
    assert!(checkstyle_findings_to_diagnostics(findings).is_empty());
}

/// Captured verbatim from a real
/// `pmd check -R rulesets/java/quickstart.xml -f xml --no-cache` run
/// (PMD 7.26.0) against a small fixture file with a real
/// `CompareObjectsWithEquals`/`UseEqualsToCompareStrings`/
/// `UnusedLocalVariable` violation each.
const REAL_PMD_REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<pmd xmlns="http://pmd.sourceforge.net/report/2.0.0" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://pmd.sourceforge.net/report/2.0.0 https://pmd.github.io/schema/report_2_0_0.xsd" version="7.26.0" timestamp="2026-07-28T07:04:02.976">
<file name="/tmp/fixture/Bad2.java">
<violation beginline="5" endline="5" begincolumn="13" endcolumn="19" rule="CompareObjectsWithEquals" ruleset="Error Prone" package="demo" class="Bad2" method="check" externalInfoUrl="https://docs.pmd-code.org/snapshot/pmd_rules_java_errorprone.html#compareobjectswithequals" priority="3">
Use equals() to compare object references.
</violation>
<violation beginline="5" endline="5" begincolumn="13" endcolumn="19" rule="UseEqualsToCompareStrings" ruleset="Error Prone" package="demo" class="Bad2" method="check" externalInfoUrl="https://docs.pmd-code.org/snapshot/pmd_rules_java_errorprone.html#useequalstocomparestrings" priority="3">
Use equals() to compare strings instead of '==' or '!='
</violation>
<violation beginline="8" endline="8" begincolumn="16" endcolumn="17" rule="UnusedLocalVariable" ruleset="Best Practices" package="demo" class="Bad2" method="check" variable="s" externalInfoUrl="https://docs.pmd-code.org/snapshot/pmd_rules_java_bestpractices.html#unusedlocalvariable" priority="3">
Avoid unused local variables such as 's'.
</violation>
</file>
</pmd>
"#;

#[test]
fn parse_pmd_xml_extracts_every_violation_with_its_file_and_range() {
    let findings = parse_pmd_xml(REAL_PMD_REPORT).expect("parses");
    assert_eq!(findings.len(), 3);
    assert_eq!(findings[0].file, PathBuf::from("/tmp/fixture/Bad2.java"));
    assert_eq!(findings[0].begin_line, 5);
    assert_eq!(findings[0].begin_column, 13);
    assert_eq!(findings[0].end_line, 5);
    assert_eq!(findings[0].end_column, 19);
    assert_eq!(findings[0].priority, 3);
}

#[test]
fn parse_pmd_xml_trims_the_message_from_the_element_text_content() {
    let findings = parse_pmd_xml(REAL_PMD_REPORT).expect("parses");
    assert_eq!(findings[0].message, "Use equals() to compare object references.");
    assert_eq!(findings[2].message, "Avoid unused local variables such as 's'.");
}

#[test]
fn parse_pmd_xml_rejects_a_violation_outside_any_file() {
    let xml = r#"<pmd version="7.26.0"><violation beginline="1" endline="1" begincolumn="1" endcolumn="1" priority="3">x</violation></pmd>"#;
    assert!(parse_pmd_xml(xml).is_err());
}

#[test]
fn parse_pmd_xml_with_no_violations_returns_an_empty_list() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<pmd xmlns="http://pmd.sourceforge.net/report/2.0.0" version="7.26.0" timestamp="2026-07-28T07:00:03.223">
</pmd>
"#;
    assert_eq!(parse_pmd_xml(xml).expect("parses"), vec![]);
}

#[test]
fn pmd_severity_maps_high_and_medium_high_to_error_and_the_rest_to_warning() {
    assert_eq!(pmd_severity(1), Severity::Error);
    assert_eq!(pmd_severity(2), Severity::Error);
    assert_eq!(pmd_severity(3), Severity::Warning);
    assert_eq!(pmd_severity(4), Severity::Warning);
    assert_eq!(pmd_severity(5), Severity::Warning);
}

#[test]
fn pmd_findings_to_diagnostics_spans_from_begin_to_one_past_end_column() {
    let (_dir, path) = test_support::temp_file(
        "Bad2.java",
        "package demo;\n\nclass Bad2 {\n    void m() {\n        if (a == b) {}\n    }\n}\n",
    );
    let findings = vec![PmdFinding {
        file: path.clone(),
        begin_line: 5,
        begin_column: 13,
        end_line: 5,
        end_column: 19,
        priority: 3,
        message: "example".to_string(),
    }];
    let diagnostics = pmd_findings_to_diagnostics(findings);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].0, path);
    assert_eq!(diagnostics[0].1.severity, Severity::Warning);
    // Line 5 ("        if (a == b) {}") starts at byte 44; begincolumn
    // 13 is char index 12 ('a') -> byte 56; endcolumn 19 is inclusive
    // (char index 18, 'b') so the range extends one past it -> byte 63.
    let line5_start = "package demo;\n\nclass Bad2 {\n    void m() {\n".len();
    assert_eq!(diagnostics[0].1.range, (line5_start + 12)..(line5_start + 19));
}

#[test]
fn command_for_binary_runs_the_spotbugs_launcher_script_directly() {
    let cmd = command_for_binary(Path::new("/tools/spotbugs-4.10.3/bin/fb"));
    assert_eq!(cmd.get_program(), "/tools/spotbugs-4.10.3/bin/fb");
}

/// Captured verbatim from a real
/// `fb analyze -xml:withMessages -output report.xml <classes_dir>` run
/// (SpotBugs 4.10.3) against a small fixture class compiled with
/// `javac`, with a real `ES_COMPARING_PARAMETER_STRING_WITH_EQ` and a
/// real `OBL_UNSATISFIED_OBLIGATION` violation. The latter is the
/// specific case that disproves this codebase's own earlier, unverified
/// guess (`PLAN.md` Track 5's "Deferred" note) that the useful
/// `<SourceLine>` is the *last* direct child of `<BugInstance>` — here
/// it's the *first* of three direct-child `<SourceLine>`s, distinguished
/// only by its own `primary="true"` attribute. Trimmed of a third,
/// redundant `OS_OPEN_STREAM` violation and the `<BugPattern>`/
/// `<BugCode>`/`<FindBugsProfile>` tail (real but not read by this
/// parser), same "trim what the parser doesn't touch, keep what it
/// does" discipline `maven.rs`'s own `SIMPLE_POM` fixture already uses.
const REAL_SPOTBUGS_REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<BugCollection version="4.10.3" sequence="0" timestamp="1787176559473" analysisTimestamp="1787176564882" release="">
  <Project projectName="">
    <Jar>/tmp/fixture/classes</Jar>
  </Project>
  <BugInstance type="ES_COMPARING_PARAMETER_STRING_WITH_EQ" priority="1" rank="14" abbrev="ES" category="BAD_PRACTICE" instanceHash="2718e3b7372e8143aea58a129489a108" instanceOccurrenceNum="0" instanceOccurrenceMax="0" cweid="595">
    <ShortMessage>Comparison of String parameter using == or !=</ShortMessage>
    <LongMessage>Comparison of String parameter using == or != in demo.Bad.compareStrings(String, String)</LongMessage>
    <Class classname="demo.Bad" primary="true">
      <SourceLine classname="demo.Bad" start="6" end="13" sourcefile="Bad.java" sourcepath="demo/Bad.java">
        <Message>At Bad.java:[lines 6-13]</Message>
      </SourceLine>
      <Message>In class demo.Bad</Message>
    </Class>
    <Method classname="demo.Bad" name="compareStrings" signature="(Ljava/lang/String;Ljava/lang/String;)Z" isStatic="true" primary="true">
      <SourceLine classname="demo.Bad" start="13" end="13" startBytecode="0" endBytecode="6" sourcefile="Bad.java" sourcepath="demo/Bad.java"/>
      <Message>In method demo.Bad.compareStrings(String, String)</Message>
    </Method>
    <Type descriptor="Ljava/lang/String;" role="TYPE_FOUND">
      <SourceLine classname="java.lang.String" start="140" end="4655" sourcefile="String.java" sourcepath="java/lang/String.java">
        <Message>At String.java:[lines 140-4655]</Message>
      </SourceLine>
      <Message>Actual type String</Message>
    </Type>
    <LocalVariable name="?" register="1" pc="1" role="LOCAL_VARIABLE_VALUE_OF">
      <Message>Value loaded from ?</Message>
    </LocalVariable>
    <SourceLine classname="demo.Bad" primary="true" start="13" end="13" startBytecode="2" endBytecode="2" sourcefile="Bad.java" sourcepath="demo/Bad.java">
      <Message>At Bad.java:[line 13]</Message>
    </SourceLine>
    <Property name="edu.umd.cs.findbugs.detect.RefComparisonWarningProperty.STATIC_AND_PARAMETER_IN_PUBLIC_METHOD" value="true"/>
  </BugInstance>
  <BugInstance type="OBL_UNSATISFIED_OBLIGATION" priority="2" rank="20" abbrev="OBL" category="EXPERIMENTAL" instanceHash="3c5b83a65d2ead90689f8a346a094d35" instanceOccurrenceNum="0" instanceOccurrenceMax="0">
    <ShortMessage>Method may fail to clean up stream or resource</ShortMessage>
    <LongMessage>demo.Bad.unclosedStream(String) may fail to clean up java.io.InputStream</LongMessage>
    <Class classname="demo.Bad" primary="true">
      <SourceLine classname="demo.Bad" start="6" end="13" sourcefile="Bad.java" sourcepath="demo/Bad.java">
        <Message>At Bad.java:[lines 6-13]</Message>
      </SourceLine>
      <Message>In class demo.Bad</Message>
    </Class>
    <Method classname="demo.Bad" name="unclosedStream" signature="(Ljava/lang/String;)V" isStatic="true" primary="true">
      <SourceLine classname="demo.Bad" start="8" end="10" startBytecode="0" endBytecode="46" sourcefile="Bad.java" sourcepath="demo/Bad.java"/>
      <Message>In method demo.Bad.unclosedStream(String)</Message>
    </Method>
    <Class classname="java.io.InputStream" role="CLASS_REFTYPE">
      <SourceLine classname="java.io.InputStream" start="61" end="786" sourcefile="InputStream.java" sourcepath="java/io/InputStream.java">
        <Message>At InputStream.java:[lines 61-786]</Message>
      </SourceLine>
      <Message>Reference type java.io.InputStream</Message>
    </Class>
    <Int value="1" role="INT_OBLIGATIONS_REMAINING">
      <Message>1 instances of obligation remaining</Message>
    </Int>
    <SourceLine classname="demo.Bad" primary="true" start="8" end="8" startBytecode="5" endBytecode="5" sourcefile="Bad.java" sourcepath="demo/Bad.java" role="SOURCE_LINE_OBLIGATION_CREATED">
      <Message>Obligation to clean up resource created at Bad.java:[line 8] is not discharged</Message>
    </SourceLine>
    <SourceLine classname="demo.Bad" start="9" end="9" startBytecode="9" endBytecode="9" sourcefile="Bad.java" sourcepath="demo/Bad.java" role="SOURCE_LINE_PATH_CONTINUES">
      <Message>Path continues at Bad.java:[line 9]</Message>
    </SourceLine>
    <SourceLine classname="demo.Bad" start="10" end="10" startBytecode="14" endBytecode="14" sourcefile="Bad.java" sourcepath="demo/Bad.java" role="SOURCE_LINE_PATH_CONTINUES">
      <Message>Path continues at Bad.java:[line 10]</Message>
    </SourceLine>
    <String value="{InputStream x 1}" role="STRING_REMAINING_OBLIGATIONS">
      <Message>Remaining obligations: {InputStream x 1}</Message>
    </String>
  </BugInstance>
  <Errors errors="0" missingClasses="0"></Errors>
</BugCollection>
"#;

#[test]
fn parse_spotbugs_xml_extracts_every_bug_instance() {
    let findings = parse_spotbugs_xml(REAL_SPOTBUGS_REPORT).expect("parses");
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].classname, "demo.Bad");
    assert_eq!(findings[0].priority, 1);
    assert_eq!(
        findings[0].message,
        "Comparison of String parameter using == or != in demo.Bad.compareStrings(String, String)"
    );
}

#[test]
fn parse_spotbugs_xml_picks_the_primary_source_line_not_the_last_direct_child() {
    let findings = parse_spotbugs_xml(REAL_SPOTBUGS_REPORT).expect("parses");
    // The OBL_UNSATISFIED_OBLIGATION finding has three direct-child
    // <SourceLine>s (lines 8, 9, 10) — only the first (line 8) carries
    // primary="true"; a "last direct child" heuristic would wrongly
    // pick line 10.
    assert_eq!(findings[1].classname, "demo.Bad");
    assert_eq!(findings[1].line, 8);
    assert_eq!(findings[1].priority, 2);
}

#[test]
fn parse_spotbugs_xml_ignores_source_lines_nested_inside_class_and_method() {
    let findings = parse_spotbugs_xml(REAL_SPOTBUGS_REPORT).expect("parses");
    // The ES_COMPARING_PARAMETER_STRING_WITH_EQ finding's own primary
    // <SourceLine> (line 13) is a direct child; several other
    // <SourceLine>s nested inside <Class>/<Method>/<Type> (lines 6, 13
    // again, 140) must not be picked up as separate findings or override
    // the real one.
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].line, 13);
}

#[test]
fn spotbugs_severity_maps_high_to_error_and_the_rest_to_warning() {
    assert_eq!(spotbugs_severity(1), Severity::Error);
    assert_eq!(spotbugs_severity(2), Severity::Warning);
    assert_eq!(spotbugs_severity(3), Severity::Warning);
}

#[test]
fn spotbugs_source_file_finds_the_standard_main_layout_path() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src/main/java/demo");
    std::fs::create_dir_all(&src_dir).unwrap();
    let file = src_dir.join("Bad.java");
    std::fs::write(&file, "package demo;\nclass Bad {}\n").unwrap();

    assert_eq!(spotbugs_source_file(dir.path(), "demo.Bad"), Some(file));
}

#[test]
fn spotbugs_source_file_reduces_a_nested_class_to_its_outer_java_file() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src/main/java/demo");
    std::fs::create_dir_all(&src_dir).unwrap();
    let file = src_dir.join("Bad.java");
    std::fs::write(&file, "package demo;\nclass Bad { class Inner {} }\n").unwrap();

    assert_eq!(spotbugs_source_file(dir.path(), "demo.Bad$Inner"), Some(file));
}

#[test]
fn spotbugs_source_file_is_none_when_the_file_does_not_exist() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(spotbugs_source_file(dir.path(), "demo.DoesNotExist"), None);
}

#[test]
fn spotbugs_findings_to_diagnostics_resolves_against_the_real_file_and_drops_unresolvable_ones() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src/main/java/demo");
    std::fs::create_dir_all(&src_dir).unwrap();
    let file = src_dir.join("Bad.java");
    std::fs::write(
        &file,
        "package demo;\n\nclass Bad {\n    void m() {\n        int x = 1;\n    }\n}\n",
    )
    .unwrap();

    let findings = vec![
        SpotBugsFinding {
            classname: "demo.Bad".to_string(),
            line: 5,
            priority: 1,
            message: "found".to_string(),
        },
        SpotBugsFinding {
            classname: "demo.Ghost".to_string(),
            line: 1,
            priority: 1,
            message: "unresolvable".to_string(),
        },
    ];
    let diagnostics = spotbugs_findings_to_diagnostics(dir.path(), findings);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].0, file);
    assert_eq!(diagnostics[0].1.severity, Severity::Error);
    assert_eq!(diagnostics[0].1.message, "found");
}
