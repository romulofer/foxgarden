//! Shared plumbing for shelling out to external static-analysis tools
//! (Checkstyle and PMD; SpotBugs lands in a later phase per `PLAN.md` Track
//! 5) and converting each finding into the same `Diagnostic` shape the
//! syntax-error squiggle pipeline already uses — a second diagnostic
//! *source* feeding the existing pipeline, not a new rendering path. Each
//! tool gets its own report parser (`SPEC.md` §5: "each needs its own
//! parser, not a shared one, the formats aren't related" — confirmed by
//! Checkstyle and PMD's real output: attribute-only vs. text-content
//! messages, a point column vs. a real begin/end range, `severity=".."` vs.
//! a numeric `priority`), but share the "read each referenced file once,
//! convert line/column into a byte range" tail end via
//! `diagnostics_from_findings`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use ropey::Rope;

use crate::diagnostic::{Diagnostic, Severity};

/// One `<error>` entry from a Checkstyle XML report, still in Checkstyle's
/// own line/column terms — not yet a byte range, since that needs the
/// referenced file's actual content (see `line_col_to_byte`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckstyleFinding {
    pub file: PathBuf,
    /// 1-based, as Checkstyle reports it.
    pub line: usize,
    /// 1-based character offset into `line`, as Checkstyle reports it —
    /// `None` for the handful of whole-file checks (e.g. a missing
    /// `package-info.java`) that have no specific column.
    pub column: Option<usize>,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug)]
pub enum StaticAnalysisError {
    /// The configured binary couldn't even be launched (not found, not
    /// executable, ...) — carries the tool's own display name ("Checkstyle"/
    /// "PMD") since both share this one error type.
    Spawn(&'static str, std::io::Error),
    /// The process ran, but its stdout wasn't a well-formed report — both
    /// tools' own exit codes double as their violation count, not a
    /// success/failure signal, so a non-zero exit alone is never this.
    Report(String),
}

impl std::fmt::Display for StaticAnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StaticAnalysisError::Spawn(tool, e) => write!(f, "failed to run {tool}: {e}"),
            StaticAnalysisError::Report(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for StaticAnalysisError {}

/// Runs `binary -c config -f xml <project_root>` and converts the resulting
/// report into `Diagnostic`s keyed by the absolute path of the file each
/// belongs to. Findings for a file that can no longer be read from disk
/// (deleted/renamed since Checkstyle ran) are silently dropped rather than
/// failing the whole batch.
pub fn checkstyle_diagnostics(
    binary: &Path,
    config: &Path,
    project_root: &Path,
) -> Result<Vec<(PathBuf, Diagnostic)>, StaticAnalysisError> {
    let stdout = run_checkstyle_process(binary, config, project_root)?;
    let findings = parse_checkstyle_xml(&stdout).map_err(StaticAnalysisError::Report)?;
    Ok(checkstyle_findings_to_diagnostics(findings))
}

/// The parse-and-convert tail of `checkstyle_diagnostics`, split out so it's
/// directly testable against a captured report without needing a real
/// Checkstyle process to run (see this module's tests).
fn checkstyle_findings_to_diagnostics(findings: Vec<CheckstyleFinding>) -> Vec<(PathBuf, Diagnostic)> {
    diagnostics_from_findings(findings, |f| &f.file, |buffer, f| {
        let start = line_col_to_byte(buffer, f.line, f.column);
        let end = (start + 1).min(buffer.len_bytes());
        Diagnostic { range: start..end, severity: f.severity, message: f.message.clone() }
    })
}

fn run_checkstyle_process(binary: &Path, config: &Path, project_root: &Path) -> Result<String, StaticAnalysisError> {
    let output = command_for_binary(binary)
        .arg("-c")
        .arg(config)
        .arg("-f")
        .arg("xml")
        .arg(project_root)
        .output()
        .map_err(|e| StaticAnalysisError::Spawn("Checkstyle", e))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Builds the `Command` to run a configured tool binary at `path`. A bare
/// `.jar` (what an in-app-downloaded Checkstyle install actually is — see
/// `crate` consumers of this module — since Checkstyle's own GitHub
/// release ships no launcher script, unlike PMD's/SpotBugs') needs `java
/// -jar` wrapped around it to be runnable at all; anything else (a real
/// executable/launcher script, e.g. an apt-installed `/usr/bin/checkstyle`
/// or PMD's/SpotBugs' own `bin/<script>`) is run directly. Requires a JVM
/// on `PATH` for the `.jar` case — not this app's concern to bundle one.
fn command_for_binary(path: &Path) -> Command {
    if path.extension().is_some_and(|ext| ext == "jar") {
        let mut cmd = Command::new("java");
        cmd.arg("-jar").arg(path);
        cmd
    } else {
        Command::new(path)
    }
}

/// Parses a Checkstyle XML report (the `-f xml` format) into one
/// `CheckstyleFinding` per `<error>` element, each `<file name="...">`
/// supplying its children's path. Pure/no I/O — verified against real
/// `checkstyle -c sun_checks.xml -f xml` output (see this module's tests),
/// not a guessed schema.
pub fn parse_checkstyle_xml(xml: &str) -> Result<Vec<CheckstyleFinding>, String> {
    let mut reader = Reader::from_str(xml);
    let mut findings = Vec::new();
    let mut current_file: Option<PathBuf> = None;

    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Eof => break,
            Event::Start(e) if e.local_name().as_ref() == b"file" => {
                current_file = attr(&e, b"name")?.map(PathBuf::from);
            }
            Event::Empty(e) if e.local_name().as_ref() == b"error" => {
                let file = current_file
                    .clone()
                    .ok_or_else(|| "<error> outside of any <file>".to_string())?;
                let line = attr(&e, b"line")?
                    .ok_or_else(|| "<error> missing a line attribute".to_string())?
                    .parse::<usize>()
                    .map_err(|e| e.to_string())?;
                let column = attr(&e, b"column")?
                    .map(|c| c.parse::<usize>())
                    .transpose()
                    .map_err(|e| e.to_string())?;
                let severity = match attr(&e, b"severity")?.as_deref() {
                    Some("error") => Severity::Error,
                    _ => Severity::Warning,
                };
                let message = attr(&e, b"message")?.unwrap_or_default();
                findings.push(CheckstyleFinding { file, line, column, severity, message });
            }
            _ => {}
        }
    }

    Ok(findings)
}

/// One `<violation>` entry from a PMD XML report, still in PMD's own
/// line/column terms. Unlike Checkstyle, PMD reports a real begin/end
/// range (both ends inclusive character columns — verified against a real
/// `pmd check -f xml` run, see this module's tests) rather than a single
/// point, and has no built-in error/warning distinction of its own, just a
/// 1 (highest) through 5 (lowest) `priority` — `pmd_severity` maps that
/// onto this codebase's binary `Severity` as a judgment call, not something
/// PMD itself defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmdFinding {
    pub file: PathBuf,
    pub begin_line: usize,
    pub begin_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub priority: u8,
    pub message: String,
}

/// PMD priority 1 (`HIGH`) and 2 (`MEDIUM_HIGH`) read as `Error`; 3
/// (`MEDIUM`) through 5 (`LOW`) as `Warning`. PMD itself has no
/// error/warning concept — this is this codebase's own threshold, not a
/// documented PMD convention, chosen because "compare objects with
/// reference equality" (a 3/`MEDIUM_HIGH`-adjacent style of finding) not
/// showing as a hard error in the common quickstart ruleset felt like the
/// right default; revisit if real usage disagrees.
fn pmd_severity(priority: u8) -> Severity {
    if priority <= 2 { Severity::Error } else { Severity::Warning }
}

/// Runs `binary check -d project_root -R ruleset -f xml --no-cache` and
/// converts the resulting report into `Diagnostic`s keyed by the absolute
/// path of the file each belongs to. `--no-cache` always disabled:
/// PMD's incremental-analysis cache is meant for repeated runs against an
/// unchanged codebase, which doesn't fit this feature's "batch job on
/// demand" shape (`SPEC.md` §5's own non-goal — no live/on-type analysis),
/// and a stale cache silently under-reporting would be a worse failure
/// mode than the extra cost of a fresh run every time.
pub fn pmd_diagnostics(
    binary: &Path,
    ruleset: &str,
    project_root: &Path,
) -> Result<Vec<(PathBuf, Diagnostic)>, StaticAnalysisError> {
    let stdout = run_pmd_process(binary, ruleset, project_root)?;
    let findings = parse_pmd_xml(&stdout).map_err(StaticAnalysisError::Report)?;
    Ok(pmd_findings_to_diagnostics(findings))
}

/// The parse-and-convert tail of `pmd_diagnostics`, split out so it's
/// directly testable against a captured report without needing a real PMD
/// process to run (see this module's tests).
fn pmd_findings_to_diagnostics(findings: Vec<PmdFinding>) -> Vec<(PathBuf, Diagnostic)> {
    diagnostics_from_findings(findings, |f| &f.file, |buffer, f| {
        let start = line_col_to_byte(buffer, f.begin_line, Some(f.begin_column));
        // `end_column` is inclusive (verified against a real report — see
        // this module's tests), so querying one column past it lands right
        // after the violation's last character, matching `Diagnostic.range`'s
        // own exclusive-end convention.
        let end = line_col_to_byte(buffer, f.end_line, Some(f.end_column + 1)).max(start + 1);
        Diagnostic { range: start..end.min(buffer.len_bytes()), severity: pmd_severity(f.priority), message: f.message.clone() }
    })
}

fn run_pmd_process(binary: &Path, ruleset: &str, project_root: &Path) -> Result<String, StaticAnalysisError> {
    let output = command_for_binary(binary)
        .arg("check")
        .arg("-d")
        .arg(project_root)
        .arg("-R")
        .arg(ruleset)
        .arg("-f")
        .arg("xml")
        .arg("--no-cache")
        .output()
        .map_err(|e| StaticAnalysisError::Spawn("PMD", e))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parses a PMD XML report (the `-f xml` format) into one `PmdFinding` per
/// `<violation>` element, each `<file name="...">` supplying its
/// children's path. Pure/no I/O — verified against real
/// `pmd check -R rulesets/java/quickstart.xml -f xml` output (see this
/// module's tests), not a guessed schema. Unlike Checkstyle's `<error/>`
/// (self-closing, message as an attribute), PMD's `<violation>` wraps its
/// message as element text content, so this tracks an in-progress
/// violation's attributes across `Start`/`Text`/`End` rather than reading
/// everything off one `Empty` event.
pub fn parse_pmd_xml(xml: &str) -> Result<Vec<PmdFinding>, String> {
    let mut reader = Reader::from_str(xml);
    let mut findings = Vec::new();
    let mut current_file: Option<PathBuf> = None;
    let mut pending: Option<PmdFinding> = None;

    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Eof => break,
            Event::Start(e) if e.local_name().as_ref() == b"file" => {
                current_file = attr(&e, b"name")?.map(PathBuf::from);
            }
            Event::Start(e) if e.local_name().as_ref() == b"violation" => {
                let file = current_file
                    .clone()
                    .ok_or_else(|| "<violation> outside of any <file>".to_string())?;
                let usize_attr = |name: &[u8]| -> Result<usize, String> {
                    attr(&e, name)?
                        .ok_or_else(|| format!("<violation> missing {}", String::from_utf8_lossy(name)))?
                        .parse::<usize>()
                        .map_err(|e| e.to_string())
                };
                let priority = attr(&e, b"priority")?
                    .ok_or_else(|| "<violation> missing priority".to_string())?
                    .parse::<u8>()
                    .map_err(|e| e.to_string())?;
                pending = Some(PmdFinding {
                    file,
                    begin_line: usize_attr(b"beginline")?,
                    begin_column: usize_attr(b"begincolumn")?,
                    end_line: usize_attr(b"endline")?,
                    end_column: usize_attr(b"endcolumn")?,
                    priority,
                    message: String::new(),
                });
            }
            Event::Text(t) if pending.is_some() => {
                let text = t.decode().map_err(|e| e.to_string())?;
                let unescaped = quick_xml::escape::unescape(&text).map_err(|e| e.to_string())?;
                if let Some(finding) = pending.as_mut() {
                    finding.message.push_str(unescaped.trim());
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"violation" => {
                let finding = pending
                    .take()
                    .ok_or_else(|| "</violation> without a matching start".to_string())?;
                findings.push(finding);
            }
            _ => {}
        }
    }

    Ok(findings)
}

pub(crate) fn attr(start: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>, String> {
    for a in start.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        if a.key.as_ref() == name {
            return a
                .unescape_value()
                .map(|v| Some(v.into_owned()))
                .map_err(|e| e.to_string());
        }
    }
    Ok(None)
}

/// Converts 1-based Checkstyle line/column terms into a byte offset into
/// `buffer`. `column` is a 1-based *character* offset within the line (not
/// byte), so the two only coincide for a pure-ASCII line. Out-of-range
/// input clamps to the nearest valid position rather than panicking, since
/// a stale report (the file changed since Checkstyle read it) is expected,
/// not exceptional.
pub fn line_col_to_byte(buffer: &Rope, line: usize, column: Option<usize>) -> usize {
    let line_idx = line.saturating_sub(1).min(buffer.len_lines().saturating_sub(1));
    let line_start = buffer.line_to_byte(line_idx);
    let Some(column) = column else {
        return line_start;
    };
    let line_slice = buffer.line(line_idx);
    // `line_slice` includes its own trailing line terminator (ropey's own
    // convention) — excluded from the clamp bound so an out-of-range
    // column lands at the end of the line's real content, not at the start
    // of the next line.
    let mut content_chars = line_slice.len_chars();
    if content_chars > 0 && line_slice.char(content_chars - 1) == '\n' {
        content_chars -= 1;
        if content_chars > 0 && line_slice.char(content_chars - 1) == '\r' {
            content_chars -= 1;
        }
    }
    let char_idx = column.saturating_sub(1).min(content_chars);
    line_start + line_slice.char_to_byte(char_idx)
}

/// Reads each finding's referenced file from disk once (grouped by path,
/// not once per finding) and converts every finding into a `Diagnostic` via
/// `to_diagnostic`, which gets that file's parsed `Rope` to turn line/column
/// terms into a real byte range (`line_col_to_byte`). Shared by Checkstyle's
/// and PMD's own `*_diagnostics` — reading-and-grouping is identical between
/// the two, only each report's own finding shape and range computation
/// differ. A file that fails to read (deleted/unreadable since the tool
/// ran) just loses its findings rather than failing the whole conversion.
fn diagnostics_from_findings<T>(
    findings: Vec<T>,
    file_of: impl Fn(&T) -> &Path,
    to_diagnostic: impl Fn(&Rope, &T) -> Diagnostic,
) -> Vec<(PathBuf, Diagnostic)> {
    let mut by_file: HashMap<PathBuf, Vec<T>> = HashMap::new();
    for finding in findings {
        let path = file_of(&finding).to_path_buf();
        by_file.entry(path).or_default().push(finding);
    }

    let mut result = Vec::new();
    for (path, findings) in by_file {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let buffer = Rope::from_str(&content);
        for finding in &findings {
            result.push((path.clone(), to_diagnostic(&buffer, finding)));
        }
    }
    result
}

#[cfg(test)]
mod tests {
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
        let (_dir, path) = test_support::temp_file("Bad2.java", "package demo;\n\nclass Bad2 {\n    void m() {\n        if (a == b) {}\n    }\n}\n");
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
}
