
use super::*;

fn screen(rows: u16, cols: u16, text: &str) -> vt100::Parser {
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(text.replace('\n', "\r\n").as_bytes());
    parser
}

#[test]
fn cell_at_resolves_a_point_inside_the_grid() {
    let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(80.0, 40.0));
    assert_eq!(cell_at(rect, 8.0, 20.0, 2, 10, egui::pos2(34.0, 25.0)), (0, 3));
    assert_eq!(cell_at(rect, 8.0, 20.0, 2, 10, egui::pos2(34.0, 35.0)), (1, 3));
}

#[test]
fn cell_at_clamps_a_point_outside_the_grid_to_its_nearest_edge_cell() {
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 40.0));
    assert_eq!(cell_at(rect, 8.0, 20.0, 2, 10, egui::pos2(-50.0, -50.0)), (0, 0));
    assert_eq!(cell_at(rect, 8.0, 20.0, 2, 10, egui::pos2(500.0, 500.0)), (1, 9));
}

#[test]
fn ordered_normalizes_a_bottom_to_top_drag() {
    let selection = Selection {
        anchor: Some((3, 5)),
        current: Some((1, 2)),
    };
    assert_eq!(selection.ordered(), Some(((1, 2), (3, 5))));
}

#[test]
fn ordered_is_none_without_a_full_anchor_and_current_pair() {
    assert_eq!(Selection::default().ordered(), None);
}

#[test]
fn selection_text_reads_a_single_row_span() {
    let parser = screen(3, 10, "hello world");
    assert_eq!(selection_text(parser.screen(), (0, 0), (0, 4), 10), "hello");
}

#[test]
fn selection_text_trims_each_rows_trailing_padding_before_its_own_newline() {
    let parser = screen(3, 10, "hi\nbye");
    assert_eq!(selection_text(parser.screen(), (0, 0), (1, 2), 10), "hi\nbye");
}
