//! Shared plumbing for shelling out to external static-analysis tools
//! (Checkstyle, PMD, and SpotBugs — `PLAN.md` Track 5) and converting each
//! finding into the same `Diagnostic` shape the syntax-error squiggle
//! pipeline already uses — a third diagnostic *source* feeding the existing
//! pipeline, not a new rendering path. Each tool gets its own report parser
//! (`SPEC.md` §5: "each needs its own parser, not a shared one, the formats
//! aren't related" — confirmed by all three tools' real output: attribute-
//! only vs. text-content messages, a point column vs. a real begin/end
//! range vs. no column at all, `severity=".."` vs. a numeric `priority` vs.
//! SpotBugs' own numeric `priority` with a completely different report
//! shape — bug-pattern metadata plus a deeply-nested per-instance structure,
//! since it analyzes compiled bytecode rather than source text), but share
//! the "read each referenced file once, convert line/column into a byte
//! range" tail end via `diagnostics_from_findings`.

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
    diagnostics_from_findings(
        findings,
        |f| &f.file,
        |buffer, f| {
            let start = line_col_to_byte(buffer, f.line, f.column);
            let end = (start + 1).min(buffer.len_bytes());
            Diagnostic {
                range: start..end,
                severity: f.severity,
                message: f.message.clone(),
            }
        },
    )
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
                findings.push(CheckstyleFinding {
                    file,
                    line,
                    column,
                    severity,
                    message,
                });
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
    if priority <= 2 {
        Severity::Error
    } else {
        Severity::Warning
    }
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
    diagnostics_from_findings(
        findings,
        |f| &f.file,
        |buffer, f| {
            let start = line_col_to_byte(buffer, f.begin_line, Some(f.begin_column));
            // `end_column` is inclusive (verified against a real report — see
            // this module's tests), so querying one column past it lands right
            // after the violation's last character, matching `Diagnostic.range`'s
            // own exclusive-end convention.
            let end = line_col_to_byte(buffer, f.end_line, Some(f.end_column + 1)).max(start + 1);
            Diagnostic {
                range: start..end.min(buffer.len_bytes()),
                severity: pmd_severity(f.priority),
                message: f.message.clone(),
            }
        },
    )
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

/// Captures `<Class classname="..." primary="true">`/`<SourceLine
/// start="..." primary="true">` into `classname`/`line` when `e` is one of
/// those two tags and carries `primary="true"` — shared by
/// `parse_spotbugs_xml`'s `Start`/`Empty` arms, both of which need the
/// exact same check at depth `0`.
fn capture_if_primary(
    e: &BytesStart<'_>,
    classname: &mut Option<String>,
    line: &mut Option<usize>,
) -> Result<(), String> {
    match e.local_name().as_ref() {
        b"Class" if attr(e, b"primary")?.as_deref() == Some("true") => {
            *classname = attr(e, b"classname")?;
        }
        b"SourceLine" if attr(e, b"primary")?.as_deref() == Some("true") => {
            *line = attr(e, b"start")?.and_then(|s| s.parse().ok());
        }
        _ => {}
    }
    Ok(())
}

/// One `<BugInstance>` entry from a SpotBugs XML report, already reduced to
/// what a squiggle needs: `classname` is the bug's own *primary* `<Class>`
/// (`classname="..." primary="true"` — verified against a real report to be
/// present regardless of bug shape, whether the actual finding site is a
/// class-, method-, or field-level detector), `line` is the primary
/// `<SourceLine>`'s own `start` attribute (SpotBugs reports no column at
/// all — bytecode has no character offsets to report), and `priority` is
/// SpotBugs' own 1 (High) through at least 3 (Low) scale, read directly off
/// `<BugInstance priority="...">`. Unlike Checkstyle's/PMD's findings,
/// there's no real file path here yet — SpotBugs only knows a class name
/// and a bytecode-debug-info source filename, not where that source lives
/// on disk; resolving that is `spotbugs_source_file`'s job, done separately
/// since it needs a `project_root` this type has no reason to carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotBugsFinding {
    pub classname: String,
    pub line: usize,
    pub priority: u8,
    pub message: String,
}

/// SpotBugs priority 1 (`High`) reads as `Error`; 2 (`Normal`) and lower
/// (`Low`, `Experimental`, ...) as `Warning`. Same "this codebase's own
/// threshold, not a documented convention of the tool itself" judgment call
/// `pmd_severity` already makes, chosen for the same reason: a 2/`Normal`
/// finding (e.g. this module's own real-report fixture's "may fail to close
/// stream") not reading as a hard error by default felt like the right
/// starting point.
fn spotbugs_severity(priority: u8) -> Severity {
    if priority <= 1 {
        Severity::Error
    } else {
        Severity::Warning
    }
}

/// Runs `binary analyze -xml:withMessages -output <tmp> classes_dir` and
/// converts the resulting report into `Diagnostic`s keyed by the absolute
/// path of the file each belongs to, resolved via `spotbugs_source_file`
/// against `project_root`. A finding whose class can't be mapped back to a
/// real file on disk (a nonstandard source layout, a generated/synthetic
/// class with no `.java` of its own) is silently dropped rather than
/// failing the whole batch — same degrade `diagnostics_from_findings`
/// already establishes for an unreadable file.
///
/// Unlike Checkstyle/PMD (`SPEC.md` §5's own "judge success by whether
/// stdout parses, not the exit code" pattern — both tools' own exit codes
/// double as their violation count), SpotBugs' exit code *is* a real
/// success/failure signal: verified live (a real "no files to analyze"
/// run against a nonexistent classes directory) exits `1` with a Java stack
/// trace on stderr and no report written at all, while a real run with
/// findings — or with none — both exit `0`. So a non-zero exit here is
/// treated as a real failure (`StaticAnalysisError::Report`, carrying
/// stderr), not judged by whether the report parses.
pub fn spotbugs_diagnostics(
    binary: &Path,
    classes_dir: &Path,
    project_root: &Path,
) -> Result<Vec<(PathBuf, Diagnostic)>, StaticAnalysisError> {
    let xml = run_spotbugs_process(binary, classes_dir)?;
    let findings = parse_spotbugs_xml(&xml).map_err(StaticAnalysisError::Report)?;
    Ok(spotbugs_findings_to_diagnostics(project_root, findings))
}

/// The parse-and-convert tail of `spotbugs_diagnostics`, split out so it's
/// directly testable against a captured report without needing a real
/// SpotBugs process to run (see this module's tests).
fn spotbugs_findings_to_diagnostics(project_root: &Path, findings: Vec<SpotBugsFinding>) -> Vec<(PathBuf, Diagnostic)> {
    let resolved: Vec<(PathBuf, SpotBugsFinding)> = findings
        .into_iter()
        .filter_map(|f| spotbugs_source_file(project_root, &f.classname).map(|path| (path, f)))
        .collect();
    diagnostics_from_findings(
        resolved,
        |(path, _)| path.as_path(),
        |buffer, (_, f)| {
            let start = line_col_to_byte(buffer, f.line, None);
            let end = (start + 1).min(buffer.len_bytes());
            Diagnostic {
                range: start..end,
                severity: spotbugs_severity(f.priority),
                message: f.message.clone(),
            }
        },
    )
}

/// The most likely source file for a bug's own `classname` — standard
/// Maven/Gradle layout (`src/main/java/<package/path>/<ClassName>.java`),
/// the same convention `test_report::test_source_file` already established
/// for `src/test/java`. A nested/inner/anonymous class (`Outer$Inner`,
/// `Outer$1`) is reduced to its outer class first — Java always compiles
/// those into the *outer* class's own `.java` file, never their own.
/// `None` when that exact file doesn't exist, same "don't guess further"
/// degrade `test_source_file` already uses.
fn spotbugs_source_file(project_root: &Path, classname: &str) -> Option<PathBuf> {
    let outer = classname.split('$').next().unwrap_or(classname);
    let relative = outer.replace('.', "/");
    let candidate = project_root
        .join("src")
        .join("main")
        .join("java")
        .join(format!("{relative}.java"));
    candidate.is_file().then_some(candidate)
}

/// Spawns a real SpotBugs `analyze` run against `classes_dir`, writing its
/// XML report to a process-scoped temp file (`-output`, not stdout — the
/// same "a real file on disk, not piped stdout" shape `maven_classpath`'s
/// own `-Dmdep.outputFile` already established, and for the same reason:
/// the report format wasn't designed to also be a clean stdout stream) and
/// reading it back once the process exits successfully. `-xml:withMessages`
/// (not bare `-xml`) is required for a `<LongMessage>` to be present at all
/// — verified live; the bare `-xml` form omits it entirely, which would
/// leave every `Diagnostic` with no message text.
fn run_spotbugs_process(binary: &Path, classes_dir: &Path) -> Result<String, StaticAnalysisError> {
    let output_file = std::env::temp_dir().join(format!("foxgarden-spotbugs-report-{}.xml", std::process::id()));

    let result = command_for_binary(binary)
        .arg("analyze")
        .arg("-xml:withMessages")
        .arg("-output")
        .arg(&output_file)
        .arg(classes_dir)
        .output();
    let result = (|| -> Result<String, StaticAnalysisError> {
        let output = result.map_err(|e| StaticAnalysisError::Spawn("SpotBugs", e))?;
        if !output.status.success() {
            return Err(StaticAnalysisError::Report(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        std::fs::read_to_string(&output_file).map_err(|e| {
            StaticAnalysisError::Report(format!(
                "SpotBugs exited successfully but its own -output report was never written: {e}"
            ))
        })
    })();
    let _ = std::fs::remove_file(&output_file);
    result
}

/// Parses a SpotBugs XML report (the `-xml:withMessages` format) into one
/// `SpotBugsFinding` per `<BugInstance>`. Pure/no I/O — verified against a
/// real `fb analyze -xml:withMessages` run (SpotBugs 4.10.3, see this
/// module's tests), not a guessed schema. Unlike Checkstyle's/PMD's flat
/// finding shape, a `<BugInstance>` nests several *other* elements
/// (`<Class>`, `<Method>`, `<Type>`, `<Int>`, `<String>`, ...) that carry
/// their *own* nested `<SourceLine>`/`<Message>` children describing
/// secondary/contextual locations, not the bug's own primary one — a naive
/// "first `<SourceLine>` seen" or "last direct child" (an earlier, *wrong*
/// guess this project made before checking a real report side by side: a
/// bug with more than one direct-child `<SourceLine>` — e.g. an
/// `OBL_UNSATISFIED_OBLIGATION` finding's own "obligation created" plus
/// "path continues" trail — has its real primary line *first*, not last)
/// would as often as not pick a wrong location. This tracks nesting depth
/// relative to the current `<BugInstance>` and only accepts a `<Class>`/
/// `<SourceLine>` reading at depth `0` (a *direct* child) that also carries
/// `primary="true"` — the one attribute SpotBugs itself uses to mark which
/// of several same-shaped elements is the real one.
pub fn parse_spotbugs_xml(xml: &str) -> Result<Vec<SpotBugsFinding>, String> {
    let mut reader = Reader::from_str(xml);
    let mut findings = Vec::new();

    let mut in_bug = false;
    let mut depth: i32 = 0;
    let mut priority: Option<u8> = None;
    let mut classname: Option<String> = None;
    let mut line: Option<usize> = None;
    let mut message = String::new();
    let mut in_long_message = false;

    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Eof => break,

            Event::Start(e) if !in_bug && e.local_name().as_ref() == b"BugInstance" => {
                in_bug = true;
                depth = 0;
                priority = Some(
                    attr(&e, b"priority")?
                        .ok_or_else(|| "<BugInstance> missing priority".to_string())?
                        .parse::<u8>()
                        .map_err(|e| e.to_string())?,
                );
                classname = None;
                line = None;
                message.clear();
            }

            Event::Start(e) if in_bug => {
                if depth == 0 {
                    if e.local_name().as_ref() == b"LongMessage" {
                        in_long_message = true;
                    }
                    capture_if_primary(&e, &mut classname, &mut line)?;
                }
                depth += 1;
            }

            Event::Empty(e) if in_bug && depth == 0 => {
                capture_if_primary(&e, &mut classname, &mut line)?;
            }

            Event::Text(t) if in_long_message => {
                let text = t.decode().map_err(|e| e.to_string())?;
                let unescaped = quick_xml::escape::unescape(&text).map_err(|e| e.to_string())?;
                message.push_str(unescaped.trim());
            }

            Event::End(e) if in_bug => {
                depth -= 1;
                if e.local_name().as_ref() == b"LongMessage" {
                    in_long_message = false;
                }
                if e.local_name().as_ref() == b"BugInstance" {
                    in_bug = false;
                    if let (Some(classname), Some(line)) = (classname.take(), line.take()) {
                        findings.push(SpotBugsFinding {
                            classname,
                            line,
                            priority: priority.expect("set when entering <BugInstance>"),
                            message: message.clone(),
                        });
                    }
                }
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
#[path = "static_analysis_test.rs"]
mod static_analysis_test;
