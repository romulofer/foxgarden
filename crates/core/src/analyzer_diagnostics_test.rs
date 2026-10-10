use super::*;

fn finding(file: PathBuf, line: usize, column: Option<usize>, end: Option<(usize, usize)>) -> AnalyzerFinding {
    AnalyzerFinding {
        file,
        line,
        column,
        end,
        severity: ProblemSeverity::Warning,
        message: "example".to_string(),
    }
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
fn a_point_finding_is_one_character_wide_at_the_real_files_position() {
    let (_dir, path) = test_support::temp_file("Bad.java", "package demo;\nclass Bad {}\n");
    let diagnostics = analyzer_diagnostics(vec![finding(path.clone(), 2, Some(7), None)]);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].0, path);
    // Line 2 ("class Bad {}") starts at byte 14; column 7 is char index
    // 6 within the line ('B' of "Bad") -> byte 20.
    assert_eq!(diagnostics[0].1.range, 20..21);
    assert_eq!(diagnostics[0].1.message, "example");
    assert_eq!(diagnostics[0].1.severity, Severity::Warning);
}

#[test]
fn a_range_finding_ends_one_past_its_inclusive_end_column() {
    let (_dir, path) = test_support::temp_file(
        "Bad2.java",
        "package demo;\n\nclass Bad2 {\n    void m() {\n        if (a == b) {}\n    }\n}\n",
    );
    let diagnostics = analyzer_diagnostics(vec![finding(path, 5, Some(13), Some((5, 19)))]);
    // Line 5 ("        if (a == b) {}"): column 13 is char index 12 ('a'); end
    // column 19 is inclusive (char index 18, 'b') so the range extends one
    // past it.
    let line5_start = "package demo;\n\nclass Bad2 {\n    void m() {\n".len();
    assert_eq!(diagnostics[0].1.range, (line5_start + 12)..(line5_start + 19));
}

#[test]
fn a_whole_line_finding_starts_at_the_line_start() {
    let (_dir, path) = test_support::temp_file("Bad3.java", "one\ntwo\n");
    let diagnostics = analyzer_diagnostics(vec![finding(path, 2, None, None)]);
    assert_eq!(diagnostics[0].1.range, 4..5);
}

#[test]
fn findings_for_an_unreadable_file_are_dropped() {
    let missing = PathBuf::from("/nonexistent/path/does/not/exist.java");
    assert!(analyzer_diagnostics(vec![finding(missing, 1, None, None)]).is_empty());
}

#[test]
fn error_severity_is_carried_over() {
    let (_dir, path) = test_support::temp_file("Bad4.java", "x\n");
    let mut error = finding(path, 1, None, None);
    error.severity = ProblemSeverity::Error;
    assert_eq!(analyzer_diagnostics(vec![error])[0].1.severity, Severity::Error);
}
