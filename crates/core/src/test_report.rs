//! Running a project's test task and parsing its own JUnit-XML test report
//! (`PLAN.md` Track 22 Phase 3 — "Test"): Maven Surefire's `target/
//! surefire-reports/TEST-<FQCN>.xml` and Gradle's `build/test-results/
//! test/TEST-<FQCN>.xml` are, once actually compared side by side against
//! real output (not assumed from either tool's docs), the *same* JUnit-XML
//! schema at the structural level both tools happen to have converged
//! on — one `<testsuite>` per test class, one `<testcase name="" classname=
//! "">` per test method, a nested `<failure>`/`<error>`/`<skipped>` marking
//! anything that isn't a plain pass. `parse_junit_xml` reads either shape
//! with the same code.
//!
//! Two real, found-not-assumed differences between the two tools' own
//! reports, both handled here:
//! - Maven wraps a `<failure>`/`<error>`'s own text in `<![CDATA[...]]>`;
//!   Gradle's is plain element text. `quick_xml` surfaces these as two
//!   different event kinds (`Event::CData` vs. `Event::Text`) — both are
//!   read into the same `detail` field.
//! - Gradle's own `<testcase name="...">` keeps JUnit 5's `()` suffix on a
//!   parameterless test method's display name (`"addIsBroken()"`); Maven's
//!   Surefire report strips it (`"addIsBroken"`). `parse_junit_xml` strips
//!   a trailing `"()"` unconditionally so a case's `name` reads the same
//!   regardless of which tool produced the report.

use std::path::{Path, PathBuf};
use std::process::Command;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::gradle::gradle_command;
use crate::scaffold::BuildTool;
use crate::static_analysis::attr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOutcome {
    Passed,
    Failed,
    Errored,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    pub classname: String,
    pub name: String,
    pub outcome: TestOutcome,
    /// The `<failure>`/`<error>` element's own message + stack trace text —
    /// `Some` exactly when `outcome` is `Failed`/`Errored`.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TestSummary {
    pub total: usize,
    pub failed: usize,
    pub errored: usize,
    pub skipped: usize,
}

impl TestSummary {
    pub fn passed(&self) -> usize {
        self.total.saturating_sub(self.failed + self.errored + self.skipped)
    }

    pub fn all_passed(&self) -> bool {
        self.failed == 0 && self.errored == 0
    }
}

pub fn summarize(cases: &[TestCase]) -> TestSummary {
    let mut summary = TestSummary {
        total: cases.len(),
        ..Default::default()
    };
    for case in cases {
        match case.outcome {
            TestOutcome::Failed => summary.failed += 1,
            TestOutcome::Errored => summary.errored += 1,
            TestOutcome::Skipped => summary.skipped += 1,
            TestOutcome::Passed => {}
        }
    }
    summary
}

/// Assembles (but does not spawn) the real test invocation for `tool`, cwd
/// already set to `project_root` — same shape as `build_output::
/// build_command`, sibling command.
pub fn test_command(project_root: &Path, tool: BuildTool) -> Command {
    let mut command = match tool {
        BuildTool::Maven => {
            let wrapper = project_root.join("mvnw");
            let mut c = if wrapper.is_file() { Command::new(wrapper) } else { Command::new("mvn") };
            c.arg("-B").arg("test");
            c
        }
        BuildTool::Gradle => {
            let mut c = gradle_command(project_root);
            c.arg("--console=plain").arg("test");
            c
        }
    };
    command.current_dir(project_root);
    command
}

/// Where `tool` writes its own JUnit-XML reports, relative to
/// `project_root` — verified against a real `mvn -B test`/`gradle
/// --console=plain test` run each (`TEST-<FQCN>.xml` per test class, both
/// tools, not just one directory guessed from the other's).
fn test_report_dir(project_root: &Path, tool: BuildTool) -> PathBuf {
    match tool {
        BuildTool::Maven => project_root.join("target").join("surefire-reports"),
        BuildTool::Gradle => project_root.join("build").join("test-results").join("test"),
    }
}

/// Reads and parses every `TEST-*.xml` report `tool`'s own last test run
/// left behind. A report that can't be read or fails to parse is silently
/// skipped rather than failing the whole scan — a leftover file from a
/// differently-shaped older run shouldn't hide every other class's real
/// results. Empty (no reports directory at all, e.g. the test task never
/// ran) degrades to an empty `Vec`, not an error.
pub fn scan_test_reports(project_root: &Path, tool: BuildTool) -> Vec<TestCase> {
    let dir = test_report_dir(project_root, tool);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut cases = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_report = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("TEST-") && n.ends_with(".xml"));
        if !is_report {
            continue;
        }
        if let Ok(xml) = std::fs::read_to_string(&path)
            && let Ok(mut parsed) = parse_junit_xml(&xml)
        {
            cases.append(&mut parsed);
        }
    }
    cases
}

/// Parses one JUnit-XML report (either tool's own shape — see this
/// module's own doc comment) into its `TestCase`s. Pure/no I/O.
pub fn parse_junit_xml(xml: &str) -> Result<Vec<TestCase>, String> {
    let mut reader = Reader::from_str(xml);
    let mut cases = Vec::new();
    let mut pending: Option<TestCase> = None;
    let mut capturing_detail = false;

    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Eof => break,
            Event::Start(e) if e.local_name().as_ref() == b"testcase" => {
                let classname = attr(&e, b"classname")?.unwrap_or_default();
                let name = attr(&e, b"name")?.unwrap_or_default();
                pending = Some(TestCase {
                    classname,
                    name: strip_parens(&name),
                    outcome: TestOutcome::Passed,
                    detail: None,
                });
            }
            Event::Empty(e) if e.local_name().as_ref() == b"testcase" => {
                let classname = attr(&e, b"classname")?.unwrap_or_default();
                let name = attr(&e, b"name")?.unwrap_or_default();
                cases.push(TestCase {
                    classname,
                    name: strip_parens(&name),
                    outcome: TestOutcome::Passed,
                    detail: None,
                });
            }
            Event::Start(e) if matches!(e.local_name().as_ref(), b"failure" | b"error") && pending.is_some() => {
                let outcome = if e.local_name().as_ref() == b"failure" { TestOutcome::Failed } else { TestOutcome::Errored };
                if let Some(case) = pending.as_mut() {
                    case.outcome = outcome;
                    case.detail = Some(String::new());
                }
                capturing_detail = true;
            }
            Event::Empty(e) if matches!(e.local_name().as_ref(), b"failure" | b"error") && pending.is_some() => {
                let outcome = if e.local_name().as_ref() == b"failure" { TestOutcome::Failed } else { TestOutcome::Errored };
                if let Some(case) = pending.as_mut() {
                    case.outcome = outcome;
                    case.detail = Some(attr(&e, b"message")?.unwrap_or_default());
                }
            }
            Event::Empty(e) if e.local_name().as_ref() == b"skipped" && pending.is_some() => {
                if let Some(case) = pending.as_mut() {
                    case.outcome = TestOutcome::Skipped;
                }
            }
            Event::Text(t) if capturing_detail => {
                let text = t.decode().map_err(|e| e.to_string())?;
                if let Some(case) = pending.as_mut()
                    && let Some(detail) = case.detail.as_mut()
                {
                    detail.push_str(&text);
                }
            }
            Event::CData(t) if capturing_detail => {
                let text = t.decode().map_err(|e| e.to_string())?;
                if let Some(case) = pending.as_mut()
                    && let Some(detail) = case.detail.as_mut()
                {
                    detail.push_str(&text);
                }
            }
            Event::End(e) if matches!(e.local_name().as_ref(), b"failure" | b"error") => {
                capturing_detail = false;
            }
            Event::End(e) if e.local_name().as_ref() == b"testcase" => {
                if let Some(case) = pending.take() {
                    cases.push(case);
                }
            }
            _ => {}
        }
    }

    Ok(cases)
}

fn strip_parens(name: &str) -> String {
    name.strip_suffix("()").unwrap_or(name).to_string()
}

/// The most likely source file for `classname`'s own test — standard
/// Maven/Gradle layout (`src/test/java/<package/path>/<ClassName>.java`),
/// the same convention both tools' scaffolding (and this app's own, `PLAN.
/// md` Track 29) already assumes elsewhere. `None` when that exact file
/// doesn't exist rather than guessing further (a nonstandard source layout,
/// or a generated/kotlin test class) — the caller shows the pass/fail
/// summary regardless, just without a click-to-jump for that one row.
pub fn test_source_file(project_root: &Path, classname: &str) -> Option<PathBuf> {
    let relative = classname.replace('.', "/");
    let candidate = project_root.join("src").join("test").join("java").join(format!("{relative}.java"));
    candidate.is_file().then_some(candidate)
}

/// The 1-based line a failing/errored test's own stack trace blames,
/// within its own class — the first stack-frame line naming `classname`'s
/// own simple (unqualified) name followed by `.java:<line>`, e.g. `at
/// com.example.CalcTest.addIsBroken(CalcTest.java:14)`. Verified against
/// real JUnit 5 traces from both a real `mvn test`/`gradle test` failure
/// (both tools produce this exact frame shape — a JUnit/AssertionFailedError
/// stack trace, not a tool-specific format): the *first* such frame is the
/// right one, since every frame above it in the trace belongs to the
/// assertion framework's own internals, not the test itself. `None` when no
/// such frame is found (an exception with no stack trace, or one that never
/// actually re-enters the test's own class — both possible, if rare).
pub fn failure_line(detail: &str, classname: &str) -> Option<usize> {
    let simple_name = classname.rsplit('.').next().unwrap_or(classname);
    let marker = format!("({simple_name}.java:");
    let start = detail.find(&marker)? + marker.len();
    let rest = &detail[start..];
    let end = rest.find(')')?;
    rest[..end].trim().parse().ok()
}

#[cfg(test)]
mod tests {
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
}
