//! Unit tests for the virtualized editor's geometry core
//! ([`super`](text_area.rs)) — pure `usize`/`f32`/`Range` math, no frame.

// Fixtures like `[2..4]` are a one-element slice of hidden line *ranges*, not a
// mistaken range-of-ranges — the shape `FoldMap` genuinely takes.
#![allow(clippy::single_range_in_vec_init)]

use super::*;

/// A `RawInput` with a concrete screen rect, so a `ScrollArea` inside
/// `run_ui` gets a real viewport height to virtualize against (the default
/// `RawInput` leaves `screen_rect` unset, which collapses nested available
/// height to zero).
fn sized_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    }
}

#[test]
fn visible_rows_covers_only_the_viewport_span() {
    // 20px rows, viewport 100px tall (5 rows), scrolled to row 10's top.
    let r = visible_rows(200.0, 100.0, 20.0, 1000);
    // rows 10..=15 touch the viewport (10 at top, a sliver of 15 at bottom).
    assert_eq!(r.start, 10);
    assert_eq!(r.end, 15);
    // Whatever the buffer size, the range width stays viewport-bounded.
    assert!(r.len() <= 6);
}

#[test]
fn visible_rows_clamps_to_the_buffer_and_handles_degenerate_input() {
    // Past the end: clamped to total_rows, empty range.
    assert_eq!(visible_rows(10_000.0, 100.0, 20.0, 3), 3..3);
    // Zero rows / zero height never divides by zero.
    assert_eq!(visible_rows(0.0, 100.0, 20.0, 0), 0..0);
    assert_eq!(visible_rows(0.0, 100.0, 0.0, 10), 0..0);
    // Top of a small buffer: all rows visible.
    assert_eq!(visible_rows(0.0, 100.0, 20.0, 3), 0..3);
}

#[test]
fn content_height_is_rows_times_height() {
    assert_eq!(content_height(100, 18.0), 1800.0);
    assert_eq!(content_height(0, 18.0), 0.0);
}

#[test]
fn row_at_y_maps_a_click_to_its_row_and_clamps() {
    // content top at y=50, 20px rows.
    assert_eq!(row_at_y(50.0, 50.0, 20.0, 100), 0); // top of row 0
    assert_eq!(row_at_y(91.0, 50.0, 20.0, 100), 2); // 41px down → row 2
    assert_eq!(row_at_y(30.0, 50.0, 20.0, 100), 0); // above content → row 0
    assert_eq!(row_at_y(9_999.0, 50.0, 20.0, 5), 4); // past end → last row
    assert_eq!(row_at_y(100.0, 0.0, 20.0, 0), 0); // empty buffer
}

#[test]
fn prefix_rows_is_cumulative_and_one_longer_than_its_input() {
    assert_eq!(prefix_rows(&[1, 1, 1]), vec![0, 1, 2, 3]);
    assert_eq!(prefix_rows(&[2, 3, 1]), vec![0, 2, 5, 6]);
    // A folded-away line contributes nothing to the running total.
    assert_eq!(prefix_rows(&[1, 0, 0, 1]), vec![0, 1, 1, 1, 2]);
    assert_eq!(prefix_rows(&[]), vec![0]);
}

#[test]
fn visible_lines_matches_visible_rows_in_the_uniform_one_row_per_line_case() {
    // With every line contributing exactly one row, `visible_lines` over
    // `prefix_rows` of an all-1s array must answer exactly what `visible_
    // rows` (the no-wrap fast path it generalizes) already does — checked
    // across several scroll positions, not just one, since an off-by-one in
    // the binary search would likely only show up near a boundary.
    let total = 1000;
    let row_counts = vec![1; total];
    let prefix = prefix_rows(&row_counts);
    for scroll_y in [0.0, 17.0, 200.0, 199.9, 9_999.0, 20_000.0] {
        let expected = visible_rows(scroll_y, 100.0, 20.0, total);
        let actual = visible_lines(scroll_y, 100.0, 20.0, &prefix);
        // Both empty ranges count as a match regardless of exactly where
        // their (otherwise-meaningless) boundary landed — `visible_rows`
        // clamps an out-of-range scroll to `total..total`, `visible_lines`
        // short-circuits to `0..0`; either way nothing gets shaped, which is
        // the only thing virtualization actually depends on.
        if expected.is_empty() && actual.is_empty() {
            continue;
        }
        assert_eq!(actual, expected, "scroll_y={scroll_y}");
    }
}

#[test]
fn visible_lines_widens_around_a_wrapped_line() {
    // Lines 0,1 unwrapped (1 row each); line 2 wraps to 3 rows; line 3
    // unwrapped. Visual row layout: line0@0, line1@1, line2@2..5, line3@5.
    let prefix = prefix_rows(&[1, 1, 3, 1]);
    assert_eq!(prefix, vec![0, 1, 2, 5, 6]);

    // A viewport landing entirely inside line 2's wrapped block (rows 3..4)
    // must still report line 2 (not skip past it or split it).
    let visible = visible_lines(60.0, 20.0, 20.0, &prefix); // rows 3..4
    assert_eq!(visible, 2..3);

    // A viewport spanning from mid-line-2 through line 3.
    let visible = visible_lines(80.0, 40.0, 20.0, &prefix); // rows 4..6
    assert_eq!(visible, 2..4);
}

#[test]
fn visible_lines_includes_a_folded_line_in_range_with_nothing_to_paint_for_it() {
    // Line 1 is folded away (0 rows); its block collapses to nothing between
    // line 0 (row 0) and line 2 (row 1). `visible_lines` still reports it as
    // part of the touched *line* range (0..3, not a gap) — it's the caller's
    // shaping loop that skips a `row_counts[line] == 0` entry when it goes
    // to actually paint, the same way it'd skip any other zero-row line;
    // `visible_lines` itself only ever needs to answer "which lines' blocks
    // does the viewport touch," and a zero-width block trivially always does.
    let prefix = prefix_rows(&[1, 0, 1]);
    assert_eq!(prefix, vec![0, 1, 1, 2]);
    let visible = visible_lines(0.0, 100.0, 20.0, &prefix);
    assert_eq!(visible, 0..3);
}

#[test]
fn visible_lines_handles_degenerate_input() {
    assert_eq!(visible_lines(0.0, 100.0, 0.0, &prefix_rows(&[1, 1])), 0..0);
    assert_eq!(visible_lines(0.0, 100.0, 20.0, &prefix_rows(&[])), 0..0);
    assert_eq!(
        visible_lines(100_000.0, 100.0, 20.0, &prefix_rows(&[1, 1, 1])),
        0..0,
        "scrolled past the end is empty, same as visible_rows(100_000.0, 100.0, 20.0, 3)"
    );
}

#[test]
fn foldmap_with_no_folds_is_the_identity() {
    let map = FoldMap::new(&[]);
    assert_eq!(map.visual_count(10), 10);
    for line in 0..10 {
        assert_eq!(map.to_visual(line), Some(line));
        assert_eq!(map.to_logical(line), line);
    }
}

#[test]
fn foldmap_skips_a_single_hidden_range() {
    // Lines 2,3 hidden. Visible logical lines: 0,1,4,5 → visual 0,1,2,3.
    let hidden = [2..4];
    let map = FoldMap::new(&hidden);
    assert_eq!(map.visual_count(6), 4);

    assert_eq!(map.to_visual(0), Some(0));
    assert_eq!(map.to_visual(1), Some(1));
    assert_eq!(map.to_visual(2), None); // hidden
    assert_eq!(map.to_visual(3), None); // hidden
    assert_eq!(map.to_visual(4), Some(2));
    assert_eq!(map.to_visual(5), Some(3));

    assert_eq!(map.to_logical(0), 0);
    assert_eq!(map.to_logical(1), 1);
    assert_eq!(map.to_logical(2), 4);
    assert_eq!(map.to_logical(3), 5);
}

#[test]
fn foldmap_handles_multiple_ranges() {
    // Lines 2,3 and 6,7 hidden, 10 lines total.
    // Visible logical: 0,1,4,5,8,9 → visual 0,1,2,3,4,5.
    let hidden = [2..4, 6..8];
    let map = FoldMap::new(&hidden);
    assert_eq!(map.visual_count(10), 6);

    let expected_visible = [0, 1, 4, 5, 8, 9];
    for (visual, &logical) in expected_visible.iter().enumerate() {
        assert_eq!(map.to_logical(visual), logical, "visual {visual} -> logical");
        assert_eq!(map.to_visual(logical), Some(visual), "logical {logical} -> visual");
    }
    // Every hidden line reports None.
    for hidden_line in [2, 3, 6, 7] {
        assert_eq!(map.to_visual(hidden_line), None);
    }
}

#[test]
fn foldmap_to_logical_is_the_inverse_of_to_visual_for_visible_lines() {
    let hidden = [1..2, 4..7];
    let map = FoldMap::new(&hidden);
    let total = 10;
    for logical in 0..total {
        if let Some(visual) = map.to_visual(logical) {
            assert_eq!(map.to_logical(visual), logical);
        }
    }
}

#[test]
fn readonly_render_shapes_only_visible_rows_not_the_whole_buffer() {
    // A 500-line buffer inside a 100px-tall scroll viewport must shape only
    // the handful of rows that fit — the whole point of virtualization.
    let text: String = (0..500).map(|i| format!("line {i}\n")).collect();
    let buffer = ropey::Rope::from_str(&text);
    let ctx = egui::Context::default();

    let mut shaped = 0;
    let mut in_bounds = true;
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
            let out = super::render::show_readonly(
                ui,
                egui::Id::new("test"),
                &buffer,
                egui::FontId::monospace(14.0),
                egui::Color32::WHITE,
                &[],
            );
            shaped = out.row_galleys.len();
            in_bounds = out.row_galleys.iter().all(|(logical, _)| *logical < buffer.len_lines())
                && out.row_galleys.len() == out.visible_rows.len();
        });
    });

    assert!(
        in_bounds,
        "every shaped row maps to a real line, one galley per visible row"
    );
    assert!(shaped > 0, "some rows should render into the viewport");
    // The load-bearing invariant: a bounded viewport shapes strictly fewer
    // rows than the whole 500-line buffer. (Exact count depends on the test
    // harness's row height, so this stays a "not everything" bound.)
    assert!(shaped < 500, "virtualization must not shape all 500 rows, got {shaped}");
}

#[test]
fn highlight_spans_bake_their_color_and_leave_the_rest_placeholder() {
    // "let x" with "let" (bytes 0..3) highlighted red — the rest of the line
    // ("x", byte 3 on) should fall back to `Color32::PLACEHOLDER`, resolved
    // to whatever color the caller passes `Painter::galley` at paint time.
    let buffer = ropey::Rope::from_str("let x");
    let ctx = egui::Context::default();
    let spans = [super::render::HighlightSpan {
        range: 0..3,
        color: egui::Color32::RED,
    }];

    let mut sections = Vec::new();
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
            let out = super::render::layout_visible(
                ui,
                egui::Id::new("test"),
                &buffer,
                egui::FontId::monospace(14.0),
                &[],
                &spans,
            );
            let (_, galley) = &out.row_galleys[0];
            sections = galley
                .job
                .sections
                .iter()
                .map(|s| (s.byte_range.start.0..s.byte_range.end.0, s.format.color))
                .collect();
        });
    });

    assert_eq!(
        sections.len(),
        2,
        "one highlighted section plus one placeholder gap, got {sections:?}"
    );
    assert_eq!(sections[0], (0..3, egui::Color32::RED));
    assert_eq!(sections[1], (3..5, egui::Color32::PLACEHOLDER));
}

#[test]
fn a_line_with_no_matching_spans_still_shapes_a_single_placeholder_section() {
    let buffer = ropey::Rope::from_str("plain");
    let ctx = egui::Context::default();
    // A span on a different line entirely shouldn't leave line 0 without any
    // section at all.
    let spans = [super::render::HighlightSpan {
        range: 100..103,
        color: egui::Color32::RED,
    }];

    let mut sections = Vec::new();
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
            let out = super::render::layout_visible(
                ui,
                egui::Id::new("test"),
                &buffer,
                egui::FontId::monospace(14.0),
                &[],
                &spans,
            );
            let (_, galley) = &out.row_galleys[0];
            sections = galley
                .job
                .sections
                .iter()
                .map(|s| (s.byte_range.start.0..s.byte_range.end.0, s.format.color))
                .collect();
        });
    });

    assert_eq!(sections, vec![(0..5, egui::Color32::PLACEHOLDER)]);
}

#[test]
fn char_rect_locates_visible_chars_and_skips_offscreen_ones() {
    let text: String = (0..500).map(|i| format!("line {i}\n")).collect();
    let buffer = ropey::Rope::from_str(&text);
    let ctx = egui::Context::default();

    let mut on_screen_has_rect = false;
    let mut last_visible_has_rect = false;
    let mut off_screen_is_none = false;
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
            let out = super::render::show_readonly(
                ui,
                egui::Id::new("test"),
                &buffer,
                egui::FontId::monospace(14.0),
                egui::Color32::WHITE,
                &[],
            );
            // A char on the first shaped row resolves to a rect...
            if let Some((first_line, _)) = out.row_galleys.first() {
                let char0 = buffer.line_to_char(*first_line);
                on_screen_has_rect = out.char_rect(&buffer, char0).is_some();
            }
            // ...and so does one on the *last* shaped row — `char_rect`'s
            // binary search (SPEC.md §8) over `row_galleys` needs to
            // resolve correctly at both ends of the visible range, not
            // just index 0 (which a broken search could satisfy by
            // accident).
            if let Some((last_line, _)) = out.row_galleys.last() {
                let char0 = buffer.line_to_char(*last_line);
                last_visible_has_rect = out.char_rect(&buffer, char0).is_some();
            }
            // ...while a char on a line far below the viewport does not.
            let far = buffer.line_to_char(400);
            off_screen_is_none = out.char_rect(&buffer, far).is_none();
        });
    });

    assert!(on_screen_has_rect, "a visible char should resolve to a screen rect");
    assert!(
        last_visible_has_rect,
        "the last visible row's char should also resolve to a screen rect"
    );
    assert!(off_screen_is_none, "an off-screen char should resolve to None");
}

#[test]
fn readonly_render_handles_an_empty_buffer() {
    let buffer = ropey::Rope::from_str("");
    let ctx = egui::Context::default();
    let mut rows = usize::MAX;
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
            let out = super::render::show_readonly(
                ui,
                egui::Id::new("test"),
                &buffer,
                egui::FontId::monospace(14.0),
                egui::Color32::WHITE,
                &[],
            );
            rows = out.row_galleys.len();
        });
    });
    // An empty buffer is one (empty) logical line — never a panic, never a
    // negative/huge row count.
    assert!(rows <= 1, "empty buffer shaped {rows} rows");
}

// --- Track 19 Phase 1: bounded word-wrap row-count computation ---

#[test]
fn default_row_counts_assumes_one_row_and_zeroes_hidden_lines() {
    assert_eq!(super::render::default_row_counts(5, &[]), vec![1, 1, 1, 1, 1]);
    assert_eq!(super::render::default_row_counts(6, &[2..4]), vec![1, 1, 0, 0, 1, 1]);
    assert_eq!(super::render::default_row_counts(0, &[]), Vec::<usize>::new());
}

/// A long-line buffer, run through `layout_visible_wrapped` inside a
/// narrowed `ui` so word-wrap actually triggers, returning how many logical
/// lines got shaped (`out.row_galleys.len()`) — a direct proxy for how many
/// real `shape_line` calls this frame made.
fn wrapped_shaped_count(total_lines: usize) -> usize {
    let text: String = (0..total_lines)
        .map(|i| format!("line {i} {}\n", "x".repeat(120)))
        .collect();
    let buffer = ropey::Rope::from_str(&text);
    let ctx = egui::Context::default();
    let mut shaped = 0;
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
            ui.set_max_width(100.0);
            let out = super::render::layout_visible_wrapped(
                ui,
                egui::Id::new("test"),
                &buffer,
                super::render::ContentKey::revision(0),
                egui::FontId::monospace(14.0),
                &[],
                &[],
            );
            shaped = out.row_galleys.len();
        });
    });
    shaped
}

#[test]
fn wrapped_layout_shapes_only_visible_lines_not_the_whole_buffer() {
    // The load-bearing claim of this phase: how many lines get shaped
    // depends on the viewport, not on total file size.
    let shaped_small = wrapped_shaped_count(2_000);
    let shaped_huge = wrapped_shaped_count(200_000);
    assert_eq!(shaped_small, shaped_huge, "shaping must not scale with total file size");
    assert!(shaped_huge > 0, "some lines should still shape into the viewport");
    assert!(
        shaped_huge < 2_000,
        "virtualization must not shape every line, got {shaped_huge}"
    );
}

#[test]
fn wrapped_layout_reuses_a_learned_row_count_across_scroll_only_frames() {
    // Line 0 is long enough to wrap across several rows; everything after it
    // is short (1 row each). First frame shapes line 0 (learning its real
    // row count) plus whatever else is visible near the top.
    let mut text = format!("start {}\n", "x".repeat(400));
    text.push_str(&(1..2_000).map(|i| format!("line {i}\n")).collect::<String>());
    let buffer = ropey::Rope::from_str(&text);
    let ctx = egui::Context::default();
    let id = egui::Id::new("scroll-reuse");

    let mut first_prefix_at_line_1 = 0;
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
            ui.set_max_width(100.0);
            let _ = super::render::layout_visible_wrapped(
                ui,
                id,
                &buffer,
                super::render::ContentKey::revision(0),
                egui::FontId::monospace(14.0),
                &[],
                &[],
            );
            let counts = super::render::cached_row_counts(
                ui,
                id,
                super::render::ContentKey::revision(0),
                &egui::FontId::monospace(14.0),
                100.0,
                &[],
                2_000,
            );
            first_prefix_at_line_1 = prefix_rows(&counts)[1];
        });
    });

    // Line 0 really does wrap to more than one row — otherwise this test
    // isn't exercising the correction path at all.
    assert!(
        first_prefix_at_line_1 > 1,
        "expected line 0 to wrap to multiple rows, prefix at line 1 was {first_prefix_at_line_1}"
    );

    // A second frame, no edit (same buffer, same id): the cache key is
    // unchanged, so the learned correction from the first frame must still
    // be there without line 0 needing to be shaped again.
    let mut second_prefix_at_line_1 = 0;
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
            ui.set_max_width(100.0);
            let counts = super::render::cached_row_counts(
                ui,
                id,
                super::render::ContentKey::revision(0),
                &egui::FontId::monospace(14.0),
                100.0,
                &[],
                2_000,
            );
            second_prefix_at_line_1 = prefix_rows(&counts)[1];
        });
    });
    assert_eq!(
        first_prefix_at_line_1, second_prefix_at_line_1,
        "a learned row count must survive a scroll-only frame with no edit"
    );
}

#[test]
fn wrapped_layout_after_an_edit_reshapes_only_the_new_visible_slice() {
    let text: String = (0..2_000).map(|i| format!("line {i} {}\n", "x".repeat(120))).collect();
    let buffer = ropey::Rope::from_str(&text);
    let ctx = egui::Context::default();
    let id = egui::Id::new("edit-reshape");

    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
            ui.set_max_width(100.0);
            let _ = super::render::layout_visible_wrapped(
                ui,
                id,
                &buffer,
                super::render::ContentKey::revision(0),
                egui::FontId::monospace(14.0),
                &[],
                &[],
            );
        });
    });

    // Simulate an edit: a different buffer, same widget id.
    let edited = ropey::Rope::from_str(&(text + "one more line\n"));
    let mut shaped_after_edit = 0;
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
            ui.set_max_width(100.0);
            let out = super::render::layout_visible_wrapped(
                ui,
                id,
                &edited,
                super::render::ContentKey::edited(0, &edited),
                egui::FontId::monospace(14.0),
                &[],
                &[],
            );
            shaped_after_edit = out.row_galleys.len();
        });
    });
    assert!(
        shaped_after_edit < 2_000,
        "post-edit reshape scaled with file size: {shaped_after_edit}"
    );
}

#[test]
fn huge_file_first_open_and_per_keystroke_cost_stays_bounded() {
    // 200_000 deliberately long lines so word-wrap actually triggers.
    let huge_text: String = (0..200_000)
        .map(|i| format!("line {i} {}\n", "x".repeat(120)))
        .collect();
    let buffer = ropey::Rope::from_str(&huge_text);
    let ctx = egui::Context::default();
    let id = egui::Id::new("huge");

    let mut first_open_shaped = 0;
    let mut first_open_ms = 0.0;
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            ui.set_max_width(100.0);
            let start = std::time::Instant::now();
            let out = super::render::layout_visible_wrapped(
                ui,
                id,
                &buffer,
                super::render::ContentKey::revision(0),
                egui::FontId::monospace(14.0),
                &[],
                &[],
            );
            first_open_ms = start.elapsed().as_secs_f64() * 1000.0;
            first_open_shaped = out.row_galleys.len();
        });
    });

    // Hard, deterministic assertion: shaping is viewport-bounded, not
    // file-size-bounded, regardless of the machine running this test.
    assert!(
        first_open_shaped < 200,
        "first open shaped {first_open_shaped} lines out of 200_000 — virtualization regressed"
    );
    // Documented, not asserted (machine/CI-load dependent) — see PLAN.md's
    // Track 19 Checkpoint 1 note for a real measured number.
    println!("first_open_shaped={first_open_shaped} first_open_ms={first_open_ms:.3}");

    // Per-keystroke: a simulated edit (new Rope, same id) must be equally
    // bounded, not cumulative with the first open.
    let edited = ropey::Rope::from_str(&(huge_text + "x"));
    let mut edit_shaped = 0;
    let _ = ctx.run_ui(sized_input(), |ui| {
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            ui.set_max_width(100.0);
            let out = super::render::layout_visible_wrapped(
                ui,
                id,
                &edited,
                super::render::ContentKey::edited(0, &edited),
                egui::FontId::monospace(14.0),
                &[],
                &[],
            );
            edit_shaped = out.row_galleys.len();
        });
    });
    assert!(
        edit_shaped < 200,
        "post-edit reshape scaled with file size: {edit_shaped}"
    );
}
