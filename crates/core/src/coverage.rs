//! Running "Run with Coverage" (`PLAN.md` Track 13 Phase 1 — "Code coverage
//! overlay", Maven-only this phase) and parsing the resulting JaCoCo
//! `jacoco.xml` report into per-line hit/miss/partial marks.
//!
//! No `pom.xml` edits and no standalone jar downloads: `jacoco-maven-
//! plugin`'s own `prepare-agent`/`report` goals, invoked as bare plugin-
//! goal coordinates alongside `test` on one `mvn` command line, compute and
//! inject the `-javaagent:...=destfile=...` argument themselves and
//! convert the resulting `.exec` to XML — the same "just ask Maven for the
//! goal" approach `test_command`/`build_command` already use for
//! `test`/`compile`, extended to a plugin that isn't already declared in
//! the project's own `pom.xml`. Maven resolves and caches the plugin jar
//! (and its own JaCoCo agent dependency) into `~/.m2` itself, the same way
//! it resolves every other plugin any `mvn` invocation needs — nothing for
//! this app to download or manage.
//!
//! The line-status algorithm below (`counter_status`/`line_status`) was
//! decompiled from the real `org.jacoco.core.internal.analysis.
//! {CounterImpl,LineImpl}.getStatus()` bytecode in a genuine
//! `org.jacoco.core` jar, not assumed from the report DTD's attribute list
//! (which documents what `mi`/`ci`/`mb`/`cb` count, not how they combine
//! into a status): the overall status is the bitwise OR of the
//! instruction-counter status and the branch-counter status, so a line
//! with `mi=0` (instructions fully covered) but a split branch (`mb>0 &&
//! cb>0`) is still `Partial`, not `Covered` — an easy detail to get
//! backwards.

use std::path::{Path, PathBuf};
use std::process::Command;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::build_output::maven_command;
use crate::static_analysis::attr;

/// JaCoCo's own status classification for one source line — not a value
/// the XML report carries directly (the DTD only has raw `mi`/`ci`/`mb`/
/// `cb` counters), recomputed here from those counters via `line_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageStatus {
    Covered,
    Missed,
    Partial,
}

/// One JaCoCo-reported line's coverage status. `line` is already 0-based
/// (`nr - 1`), matching this codebase's own line-space convention
/// (`DiffHunk`'s own doc comment establishes the same thing) so
/// `coverage_gutter::paint_coverage_gutter` needs no special-casing to
/// look rows up in `out.row_galleys`, exactly like `paint_diff_gutter`
/// already does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCoverage {
    pub line: usize,
    pub status: CoverageStatus,
}

/// The pinned `jacoco-maven-plugin` version this invokes via bare
/// plugin-goal coordinates — a specific, verified-working release, not
/// "whatever's latest," same "pin, don't float" rationale
/// `tool_manager::Tool::recommended_version` already established for
/// Checkstyle/PMD/SpotBugs.
const JACOCO_MAVEN_PLUGIN_VERSION: &str = "0.8.12";

/// Assembles (but does not spawn) the real "Run with Coverage" invocation:
/// **one** `mvn` process running `prepare-agent`, `test`, then `report`
/// back to back in the same reactor session — Maven accepts a mixed goal/
/// phase list on one command line and executes it left to right within one
/// session, the same mechanism `mvn clean package` already relies on, so
/// this needs exactly one `BuildState` stage, not a chained pair.
///
/// `-Dmaven.test.failure.ignore=true` is required, not optional: without
/// it, a real test *failure* makes Maven's default fail-fast behavior
/// abort the whole multi-goal command before `report` ever runs, silently
/// producing no coverage data at all on the exact runs a user most wants
/// to see it for (some tests red, most green) — mirrors `test_command`'s
/// own sibling comment ("a test failure makes the process itself exit
/// non-zero, but the report is still written").
///
/// `-Djacoco.destFile`/`-Djacoco.dataFile` pin the `.exec` path explicitly
/// to `<project_root>/target/jacoco.exec` — explicit beats implicit for a
/// path nothing here reads back directly (only `coverage_report_path`'s
/// own file, written by the `report` goal, is read).
pub fn coverage_command(project_root: &Path) -> Command {
    let exec_file = project_root.join("target").join("jacoco.exec");
    let mut command = maven_command(project_root);
    command
        .arg("-B")
        .arg(format!("-Djacoco.destFile={}", exec_file.display()))
        .arg(format!("-Djacoco.dataFile={}", exec_file.display()))
        .arg("-Dmaven.test.failure.ignore=true")
        .arg(format!("org.jacoco:jacoco-maven-plugin:{JACOCO_MAVEN_PLUGIN_VERSION}:prepare-agent"))
        .arg("test")
        .arg(format!("org.jacoco:jacoco-maven-plugin:{JACOCO_MAVEN_PLUGIN_VERSION}:report"));
    command.current_dir(project_root);
    command
}

/// Where the `report` goal's XML output lands — its own default
/// (`target/site/jacoco/jacoco.xml`, from Maven's own Super POM default
/// `reporting.outputDirectory = ${project.build.directory}/site`), which
/// this can't override via `-D` when invoked as a bare goal (the real
/// `report` mojo's `outputDirectory`/`formats` have no bound user
/// property, only a `pom.xml` `<configuration>`, which this feature
/// deliberately never touches). Same "hardcode the tool's own well-known
/// convention" precedent `test_report::test_report_dir` already
/// established for Surefire's own `target/surefire-reports`.
pub fn coverage_report_path(project_root: &Path) -> PathBuf {
    project_root.join("target").join("site").join("jacoco").join("jacoco.xml")
}

/// `ICounter`'s own status bits, decompiled: `FULLY_COVERED = 2` when
/// `covered > 0`, OR'd with `NOT_COVERED = 1` when `missed > 0` — so
/// `covered=0,missed=0` is `0` (`EMPTY`), `covered>0,missed>0` is `3`.
fn counter_status(missed: u32, covered: u32) -> u8 {
    let mut status = if covered > 0 { 2 } else { 0 };
    if missed > 0 {
        status |= 1;
    }
    status
}

/// Decompiled from `LineImpl.getStatus()`: the overall line status is the
/// bitwise OR of the instruction-counter status and the branch-counter
/// status (a branchless line has `mb=cb=0`, contributing `0`/`EMPTY` to
/// the OR, so it never affects branch-free lines). `None` only for the
/// `EMPTY` case (`mi=ci=mb=cb=0`), which a real `jacoco.xml` never emits
/// as a `<line>` element at all — this only defends against a malformed
/// input, not something a genuine JaCoCo run produces.
fn line_status(mi: u32, ci: u32, mb: u32, cb: u32) -> Option<CoverageStatus> {
    match counter_status(mi, ci) | counter_status(mb, cb) {
        1 => Some(CoverageStatus::Missed),
        2 => Some(CoverageStatus::Covered),
        3 => Some(CoverageStatus::Partial),
        _ => None,
    }
}

/// Parses `jacoco.xml` into `(package/sourcefile.ext, lines)` pairs — the
/// first half already slash-joined VM notation straight from `<package
/// name="com/example">` + `<sourcefile name="Calc.java">`
/// (`"com/example/Calc.java"`), **not** a dotted classname needing
/// `.replace('.', "/")` the way `test_source_file`'s own classname-based
/// resolver needs — an easy detail to get backwards since every other
/// source-file resolver in this codebase starts from a dotted FQCN
/// instead. Pure/no I/O, `quick_xml::Reader` streaming parser, same idiom
/// as `parse_junit_xml`: `<class>`/`<method>`/`<counter>` elements are
/// read past via the catch-all arm, since only `<package>`, `<sourcefile>`,
/// and `<line>` carry anything this needs.
pub fn parse_jacoco_xml(xml: &str) -> Result<Vec<(String, Vec<LineCoverage>)>, String> {
    let mut reader = Reader::from_str(xml);
    let mut results = Vec::new();
    let mut package = String::new();
    let mut current: Option<(String, Vec<LineCoverage>)> = None;

    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Eof => break,
            Event::Start(e) if e.local_name().as_ref() == b"package" => {
                package = attr(&e, b"name")?.unwrap_or_default();
            }
            Event::Start(e) if e.local_name().as_ref() == b"sourcefile" => {
                let name = attr(&e, b"name")?.unwrap_or_default();
                let path = if package.is_empty() { name } else { format!("{package}/{name}") };
                current = Some((path, Vec::new()));
            }
            Event::Empty(e) if e.local_name().as_ref() == b"line" && current.is_some() => {
                let count = |a: &[u8]| -> Result<u32, String> { Ok(attr(&e, a)?.and_then(|v| v.parse().ok()).unwrap_or(0)) };
                let Some(nr) = attr(&e, b"nr")?.and_then(|v| v.parse::<usize>().ok()) else {
                    continue;
                };
                if let Some(status) = line_status(count(b"mi")?, count(b"ci")?, count(b"mb")?, count(b"cb")?)
                    && let Some((_, lines)) = current.as_mut()
                {
                    lines.push(LineCoverage { line: nr.saturating_sub(1), status });
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"sourcefile" => {
                if let Some(entry) = current.take() {
                    results.push(entry);
                }
            }
            _ => {}
        }
    }

    Ok(results)
}

/// The most likely real file for one JaCoCo-reported `relative` path
/// (already slash-joined, e.g. `"com/example/Calc.java"`) — tries every
/// standard Maven Java/Kotlin main/test source root, main before test
/// since JaCoCo instruments whatever got compiled and covered, most
/// commonly production code. `None` when none of the four exist — same
/// "don't guess further" degrade `test_report::test_source_file` already
/// uses.
pub fn resolve_source_file(project_root: &Path, relative: &str) -> Option<PathBuf> {
    for root in ["src/main/java", "src/main/kotlin", "src/test/java", "src/test/kotlin"] {
        let candidate = project_root.join(root).join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolves every parsed entry's own relative path via
/// `resolve_source_file`, dropping any that don't resolve to a real file
/// (a generated/annotation-processed class, or a nonstandard layout).
pub fn resolve_coverage_paths(project_root: &Path, parsed: Vec<(String, Vec<LineCoverage>)>) -> Vec<(PathBuf, Vec<LineCoverage>)> {
    parsed
        .into_iter()
        .filter_map(|(relative, lines)| resolve_source_file(project_root, &relative).map(|path| (path, lines)))
        .collect()
}

#[cfg(test)]
mod tests {
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
        assert_eq!(lines.iter().find(|l| l.line == 2).unwrap().status, CoverageStatus::Covered);
        // nr=7 (line 6): instructions fully missed -> Missed.
        assert_eq!(lines.iter().find(|l| l.line == 6).unwrap().status, CoverageStatus::Missed);
        // nr=9 (line 8): instructions split, zero branches -> Partial from
        // instruction counters alone.
        assert_eq!(lines.iter().find(|l| l.line == 8).unwrap().status, CoverageStatus::Partial);
        // nr=12 (line 11): instructions FULLY covered (mi=0) but its own
        // branch is split -> still Partial, proving "mi=0 => Covered" alone
        // is wrong without also checking the branch counters.
        assert_eq!(lines.iter().find(|l| l.line == 11).unwrap().status, CoverageStatus::Partial);
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
        assert_eq!(resolve_source_file(dir.path(), "com/example/Calc.java"), Some(src.join("Calc.java")));
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
        assert!(args.iter().any(|a| a.contains("jacoco-maven-plugin:0.8.12:prepare-agent")));
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
            ("com/example/Calc.java".to_string(), vec![LineCoverage { line: 0, status: CoverageStatus::Covered }]),
            ("com/example/Missing.java".to_string(), vec![LineCoverage { line: 0, status: CoverageStatus::Missed }]),
        ];
        let resolved = resolve_coverage_paths(dir.path(), parsed);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0, src.join("Calc.java"));
    }
}
