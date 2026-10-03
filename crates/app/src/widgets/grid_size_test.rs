
use super::*;

#[test]
fn exact_multiple_fits_with_no_remainder() {
    assert_eq!(grid_size(egui::vec2(800.0, 480.0), 20.0, 8.0), (24, 100));
}

#[test]
fn a_partial_trailing_row_or_column_is_floored_away() {
    // 479px / 20px-tall rows only fully fits 23, not 24 — the 24th
    // row would be clipped, so it shouldn't count as fitting.
    assert_eq!(grid_size(egui::vec2(799.0, 479.0), 20.0, 8.0), (23, 99));
}

#[test]
fn a_rect_smaller_than_one_cell_still_floors_to_a_usable_grid() {
    assert_eq!(grid_size(egui::vec2(2.0, 2.0), 20.0, 8.0), (2, 2));
}

/// A single-row (or single-column) grid is what `vt100`'s own
/// `col_wrap` underflows on, so a squeezed-shut panel must never
/// produce one — this is the regression guard for a real crash found
/// live, not a style preference.
#[test]
fn a_squeezed_panel_never_produces_a_single_row_or_column_grid() {
    for size in [0.0, 1.0, 7.9, 8.0, 15.9, 19.9] {
        let (rows, cols) = grid_size(egui::vec2(size, size), 20.0, 8.0);
        assert!(rows >= 2 && cols >= 2, "{size}px produced a {rows}x{cols} grid");
    }
}

#[test]
fn a_wider_font_yields_fewer_columns_for_the_same_width() {
    assert_eq!(grid_size(egui::vec2(800.0, 480.0), 20.0, 16.0), (24, 50));
}
