
use super::*;

fn panel(first_line: usize, target_line: usize, line_count: usize) -> PanelContent {
    PanelContent {
        path: PathBuf::from("Foo.java"),
        first_line,
        target_line,
        lines: (0..line_count).map(|i| format!("line {i}")).collect(),
    }
}

#[test]
fn panel_from_text_centers_on_the_target_lines_own_line_number() {
    let text = "a\nb\nc\ntarget\nd\ne\nf";
    let byte_offset = text.find("target").unwrap();
    let content = panel_from_text(PathBuf::from("x"), text, byte_offset);
    assert_eq!(content.target_line, 4);
    assert_eq!(content.lines[content.target_line - content.first_line], "target");
}

#[test]
fn panel_from_text_clamps_the_window_at_the_start_of_a_short_file() {
    let text = "a\nb\ntarget";
    let byte_offset = text.find("target").unwrap();
    let content = panel_from_text(PathBuf::from("x"), text, byte_offset);
    assert_eq!(content.first_line, 1);
    assert_eq!(content.target_line, 3);
    assert_eq!(content.lines.len(), 3);
}

#[test]
fn visible_window_collapsed_is_narrower_than_expanded() {
    let content = panel(1, 20, 25);
    let (_, collapsed) = visible_window(&content, false);
    let (_, expanded) = visible_window(&content, true);
    assert!(collapsed.len() < expanded.len());
    assert_eq!(collapsed.len(), COLLAPSED_CONTEXT_LINES * 2 + 1);
}

#[test]
fn visible_window_reports_the_real_line_number_of_its_own_first_row() {
    let content = panel(10, 20, 25);
    let (first_line, lines) = visible_window(&content, false);
    assert_eq!(first_line, 20 - COLLAPSED_CONTEXT_LINES);
    assert_eq!(lines[0], content.lines[10 - COLLAPSED_CONTEXT_LINES]);
}

#[test]
fn visible_window_clamps_at_the_start_of_the_stored_window() {
    let content = panel(1, 1, 5);
    let (first_line, lines) = visible_window(&content, true);
    assert_eq!(first_line, 1);
    assert_eq!(lines.len(), 5);
}
