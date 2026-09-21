
use egui::Color32;

use super::*;

fn painted_rect_shapes(lines: Vec<LineCoverage>, buffer_text: &str) -> Vec<(Color32, egui::Rect, egui::Rect)> {
    let ctx = egui::Context::default();
    let id = egui::Id::new("coverage_gutter_test");
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
            paint_coverage_gutter(ui, &shell_out.base, &lines, 200.0, false);
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
fn a_covered_line_paints_a_full_height_bar_in_the_covered_color() {
    let lines = vec![LineCoverage {
        line: 1,
        status: CoverageStatus::Covered,
    }];
    let shapes = painted_rect_shapes(lines, "l1\nl2\nl3\n");

    let green = theme::coverage_covered(false);
    let bar = shapes
        .iter()
        .find(|(fill, ..)| *fill == green)
        .expect("a rect filled with the covered color");
    let (_, rect, clip) = bar;
    assert!(clip.contains_rect(*rect), "the bar must be fully visible");
}

#[test]
fn a_missed_line_paints_in_the_missed_color_not_the_covered_one() {
    let lines = vec![LineCoverage {
        line: 0,
        status: CoverageStatus::Missed,
    }];
    let shapes = painted_rect_shapes(lines, "l1\nl2\n");

    let red = theme::coverage_missed(false);
    assert!(
        shapes.iter().any(|(fill, ..)| *fill == red),
        "expected a rect filled with the missed color"
    );
}

#[test]
fn no_coverage_data_paints_nothing() {
    let shapes = painted_rect_shapes(Vec::new(), "l1\nl2\n");
    let green = theme::coverage_covered(false);
    let red = theme::coverage_missed(false);
    let amber = theme::coverage_partial(false);
    assert!(
        !shapes
            .iter()
            .any(|(fill, ..)| *fill == green || *fill == red || *fill == amber)
    );
}
