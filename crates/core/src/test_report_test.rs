use super::*;

const MAVEN_REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="com.example.CalcTest" time="0.041" tests="2" errors="0" skipped="0" failures="1">
  <testcase name="addIsBroken" classname="com.example.CalcTest" time="0.022">
    <failure message="expected: &lt;99&gt; but was: &lt;5&gt;" type="org.opentest4j.AssertionFailedError"><![CDATA[org.opentest4j.AssertionFailedError: expected: <99> but was: <5>
	at org.junit.jupiter.api.Assertions.assertEquals(Assertions.java:531)
	at com.example.CalcTest.addIsBroken(CalcTest.java:14)
]]></failure>
  </testcase>
  <testcase name="addWorks" classname="com.example.CalcTest" time="0.001"/>
</testsuite>"#;

const GRADLE_REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="com.example.CalcTest" tests="2" skipped="0" failures="1" errors="0" time="0.025">
  <properties/>
  <testcase name="addIsBroken()" classname="com.example.CalcTest" time="0.015">
    <failure message="org.opentest4j.AssertionFailedError: expected: &lt;99&gt; but was: &lt;5&gt;" type="org.opentest4j.AssertionFailedError">org.opentest4j.AssertionFailedError: expected: &lt;99&gt; but was: &lt;5&gt;
	at org.junit.jupiter.api.Assertions.assertEquals(Assertions.java:531)
	at com.example.CalcTest.addIsBroken(CalcTest.java:14)
</failure>
  </testcase>
  <testcase name="addWorks()" classname="com.example.CalcTest" time="0.001"/>
  <system-out><![CDATA[]]></system-out>
  <system-err><![CDATA[]]></system-err>
</testsuite>"#;

#[test]
fn parses_a_real_maven_surefire_report() {
    let cases = parse_junit_xml(MAVEN_REPORT).unwrap();
    assert_eq!(cases.len(), 2);
    let failed = cases.iter().find(|c| c.name == "addIsBroken").unwrap();
    assert_eq!(failed.classname, "com.example.CalcTest");
    assert_eq!(failed.outcome, TestOutcome::Failed);
    assert!(failed.detail.as_ref().unwrap().contains("CalcTest.java:14"));
    let passed = cases.iter().find(|c| c.name == "addWorks").unwrap();
    assert_eq!(passed.outcome, TestOutcome::Passed);
    assert!(passed.detail.is_none());
}

#[test]
fn parses_a_real_gradle_test_report() {
    let cases = parse_junit_xml(GRADLE_REPORT).unwrap();
    assert_eq!(cases.len(), 2);
    // Gradle's own `()`-suffixed name is stripped to match Maven's.
    let failed = cases.iter().find(|c| c.name == "addIsBroken").unwrap();
    assert_eq!(failed.outcome, TestOutcome::Failed);
    assert!(failed.detail.as_ref().unwrap().contains("CalcTest.java:14"));
    let passed = cases.iter().find(|c| c.name == "addWorks").unwrap();
    assert_eq!(passed.outcome, TestOutcome::Passed);
}

#[test]
fn summarize_counts_each_outcome() {
    let cases = parse_junit_xml(MAVEN_REPORT).unwrap();
    let summary = summarize(&cases);
    assert_eq!(summary.total, 2);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.errored, 0);
    assert_eq!(summary.skipped, 0);
    assert_eq!(summary.passed(), 1);
    assert!(!summary.all_passed());
}

#[test]
fn empty_report_directory_yields_no_cases() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(scan_test_reports(dir.path(), BuildTool::Maven), Vec::new());
}

#[test]
fn failure_line_finds_the_first_frame_naming_the_tests_own_class() {
    let detail = "org.opentest4j.AssertionFailedError: expected: <99> but was: <5>\n\tat org.junit.jupiter.api.Assertions.assertEquals(Assertions.java:531)\n\tat com.example.CalcTest.addIsBroken(CalcTest.java:14)\n\tat java.base/java.lang.reflect.Method.invoke(Method.java:568)\n";
    assert_eq!(failure_line(detail, "com.example.CalcTest"), Some(14));
}

#[test]
fn failure_line_is_none_without_a_matching_frame() {
    assert_eq!(failure_line("no stack trace here", "com.example.CalcTest"), None);
}

#[test]
fn test_source_file_finds_the_standard_layout_path() {
    let dir = tempfile::tempdir().unwrap();
    let test_dir = dir.path().join("src/test/java/com/example");
    std::fs::create_dir_all(&test_dir).unwrap();
    std::fs::write(test_dir.join("CalcTest.java"), "").unwrap();
    assert_eq!(
        test_source_file(dir.path(), "com.example.CalcTest"),
        Some(test_dir.join("CalcTest.java"))
    );
}

#[test]
fn test_source_file_is_none_when_the_file_does_not_exist() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(test_source_file(dir.path(), "com.example.CalcTest"), None);
}
