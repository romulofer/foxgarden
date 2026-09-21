
use super::*;

#[test]
fn removal_at_the_very_start_of_the_file_targets_line_zeros_top_edge() {
    assert_eq!(removal_notch_target(0), (0, true));
}

#[test]
fn removal_in_the_middle_targets_the_preceding_lines_bottom_edge() {
    // Matches fg_core::diff's own captured "delete line 3 of 5"
    // fixture: the marker's `at` is 2, and the notch belongs right
    // below line 1 (the line now immediately preceding where the
    // deleted content used to be).
    assert_eq!(removal_notch_target(2), (1, false));
}

#[test]
fn removal_at_the_very_end_of_the_file_also_targets_the_preceding_lines_bottom_edge() {
    // Matches fg_core::diff's own "delete the last of 5 lines"
    // fixture (`at` = 4): lands on the new last line's (index 3)
    // bottom edge, with no separate "is this the last line" branch
    // needed.
    assert_eq!(removal_notch_target(4), (3, false));
}

/// Drives the real `text_area::show_interactive` pipeline (the same one
/// `widget.rs` itself calls) to get a genuine `TextAreaOutput`, then
/// inspects the actual `egui::Shape`s `paint_diff_gutter` produces —
/// this file's own painting had no coverage at all before this (every
/// other paint fn in this widget is likewise only "doesn't panic"
/// tested, or not tested directly — see `painting.rs`'s own tests), and
/// a user report that the removal notch wasn't showing up is what
/// prompted actually verifying the painted geometry rather than just
/// the pure targeting math above.
fn painted_rect_shapes(hunks: Vec<DiffHunk>, buffer_text: &str) -> Vec<(Color32, egui::Rect, egui::Rect)> {
    let ctx = egui::Context::default();
    let id = egui::Id::new("diff_gutter_test");
    let buffer = ropey::Rope::from_str(buffer_text);

    let raw_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    let output = ctx.run_ui(raw_input, |ui| {
        ui.memory_mut(|m| m.request_focus(id));
        egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
            let shell_out = super::super::text_area::show_interactive(
                ui,
                id,
                &buffer,
                0,
                buffer_text,
                egui::FontId::monospace(14.0),
                egui::Color32::WHITE,
                false,
                &[],
                &[],
                false,
                true,
            );
            paint_diff_gutter(ui, &shell_out.base, &hunks, 200.0, false);
        });
    });

    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(r) => Some((r.fill, r.rect, clipped.clip_rect)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_removed_hunk_paints_a_notch_fully_inside_its_visible_clip_rect() {
    let hunks = vec![DiffHunk {
        kind: DiffLineKind::Removed,
        lines: 2..2,
    }];
    let shapes = painted_rect_shapes(hunks, "l1\nl2\nl4\nl5\n");

    let red = theme::diff_removed(false);
    let notch = shapes
        .iter()
        .find(|(fill, ..)| *fill == red)
        .expect("a rect filled with the removal color");
    let (_, rect, clip) = notch;
    assert!(
        clip.contains_rect(*rect),
        "the notch must be fully visible, not clipped away at a row boundary"
    );
}

#[test]
fn an_added_hunk_paints_a_full_height_bar_on_its_own_line() {
    let hunks = vec![DiffHunk {
        kind: DiffLineKind::Added,
        lines: 1..2,
    }];
    let shapes = painted_rect_shapes(hunks, "l1\nNEW\nl3\n");

    let green = theme::diff_added(false);
    let bar = shapes
        .iter()
        .find(|(fill, ..)| *fill == green)
        .expect("a rect filled with the added color");
    let (_, rect, clip) = bar;
    assert!(clip.contains_rect(*rect), "the bar must be fully visible");
    assert!(
        rect.height() > NOTCH_HEIGHT,
        "an Added bar spans the full row height, not just a thin notch"
    );
}
