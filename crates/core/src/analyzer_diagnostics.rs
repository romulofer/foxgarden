//! Turning an external analyzer's findings (`fg_extension::AnalyzerFinding`)
//! into the `Diagnostic`s the squiggle pipeline already paints — a third
//! diagnostic *source* feeding the existing pipeline, not a new rendering
//! path. Running the tools and reading their reports is the contributing
//! extension's; what is shared is the tail end: read each referenced file
//! once and convert line/column into a byte range.

use std::collections::HashMap;
use std::path::PathBuf;

use fg_extension::{AnalyzerFinding, ProblemSeverity};
use ropey::Rope;

use crate::diagnostic::{Diagnostic, Severity};

/// Converts findings into `Diagnostic`s keyed by the absolute path of the
/// file each belongs to. Reads each referenced file from disk once (grouped
/// by path, not once per finding). A file that fails to read (deleted or
/// unreadable since the tool ran) just loses its findings rather than
/// failing the whole batch.
pub fn analyzer_diagnostics(findings: Vec<AnalyzerFinding>) -> Vec<(PathBuf, Diagnostic)> {
    let mut by_file: HashMap<PathBuf, Vec<AnalyzerFinding>> = HashMap::new();
    for finding in findings {
        by_file.entry(finding.file.clone()).or_default().push(finding);
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

fn to_diagnostic(buffer: &Rope, finding: &AnalyzerFinding) -> Diagnostic {
    let start = line_col_to_byte(buffer, finding.line, finding.column);
    let end = match finding.end {
        // The end is inclusive, so one column past it lands right after the
        // finding's last character, matching `Diagnostic.range`'s own
        // exclusive-end convention.
        Some((line, column)) => line_col_to_byte(buffer, line, Some(column + 1)),
        None => start,
    };
    Diagnostic {
        range: start..end.max(start + 1).min(buffer.len_bytes()),
        severity: match finding.severity {
            ProblemSeverity::Error => Severity::Error,
            ProblemSeverity::Warning => Severity::Warning,
        },
        message: finding.message.clone(),
        server: None,
    }
}

/// Converts 1-based line/column terms into a byte offset into `buffer`.
/// `column` is a 1-based *character* offset within the line (not byte), so
/// the two only coincide for a pure-ASCII line. Out-of-range input clamps to
/// the nearest valid position rather than panicking, since a stale report
/// (the file changed since the tool read it) is expected, not exceptional.
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

#[cfg(test)]
#[path = "analyzer_diagnostics_test.rs"]
mod analyzer_diagnostics_test;
