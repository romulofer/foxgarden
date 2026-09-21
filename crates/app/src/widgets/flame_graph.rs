//! `PLAN.md` Track 26 Phase 2: a custom-painted, interactive flame graph for
//! an async-profiler capture (`fg_core::FlameNode`, produced by Track 26
//! Phase 1's own `parse_collapsed`).
//!
//! Icicle layout — the focused frame spans the full width at the top, its
//! callees stacked directly beneath, each child's width proportional to its
//! inclusive sample count. Root-at-top rather than the classic root-at-bottom
//! is a deliberate choice: it lays out top-down the same direction a
//! `ScrollArea` scrolls, so a very deep stack scrolls naturally without the
//! whole graph having to be measured and flipped first.
//!
//! Interaction: hovering a frame shows its name, sample count, and share of
//! both the current focus and the whole profile; clicking a frame zooms into
//! it (that subtree fills the width); clicking the top (focused) frame zooms
//! back out one level. The layout itself (`layout_frames`) is a pure function
//! of the tree and the target rectangle, so it's unit-tested for correct
//! proportional widths without a live `egui` context.

use egui::{Color32, FontId, Rect, Sense, Stroke, pos2, vec2};

use fg_core::FlameNode;

/// The zoom state a flame graph carries between frames: the child-index path
/// from the tree root down to the currently-focused (full-width) node. Empty
/// means the root itself is focused — the whole profile.
#[derive(Default)]
pub struct FlameGraphState {
    focus: Vec<usize>,
}

impl FlameGraphState {
    /// Resets the zoom back to the whole profile — called when a fresh capture
    /// replaces the one a stale path was pointing into, so an old focus can't
    /// dangle over a different tree.
    pub fn reset(&mut self) {
        self.focus.clear();
    }
}

/// Height of one stack frame's row, in points — tall enough for a readable
/// label, short enough to fit a deep stack without excessive scrolling.
const ROW_HEIGHT: f32 = 18.0;

/// Frames narrower than this (in points) are dropped rather than painted: at
/// sub-pixel widths they'd be invisible anyway, and skipping them bounds the
/// paint/hit-test work on a profile with tens of thousands of tiny leaf
/// stacks.
const MIN_FRAME_WIDTH: f32 = 1.5;

/// One laid-out frame ready to paint and hit-test: where it sits, its
/// full child-index path from the root (what a click zooms to), and the label
/// data.
struct LaidOutFrame {
    rect: Rect,
    path: Vec<usize>,
    name: String,
    total: u64,
}

/// Walks `node` (the focused subtree) into a flat list of rectangles within
/// `area`, the focused node spanning the full width at the top. `focus_path`
/// is the path from the real tree root to `node`, prepended to every emitted
/// frame's own path so a click resolves against the whole tree, not just the
/// zoomed subtree. Pure and context-free, so proportional widths are testable.
fn layout_frames(node: &FlameNode, focus_path: &[usize], area: Rect) -> Vec<LaidOutFrame> {
    let mut frames = Vec::new();
    layout_into(
        node,
        focus_path.to_vec(),
        0,
        area.left(),
        area.width(),
        area.top(),
        &mut frames,
    );
    frames
}

fn layout_into(
    node: &FlameNode,
    path: Vec<usize>,
    depth: usize,
    x: f32,
    width: f32,
    top: f32,
    out: &mut Vec<LaidOutFrame>,
) {
    let y = top + depth as f32 * ROW_HEIGHT;
    out.push(LaidOutFrame {
        rect: Rect::from_min_size(pos2(x, y), vec2(width, ROW_HEIGHT)),
        path: path.clone(),
        name: node.name.clone(),
        total: node.total,
    });

    if node.total == 0 {
        return;
    }
    let mut child_x = x;
    for (index, child) in node.children.iter().enumerate() {
        let child_width = width * (child.total as f32 / node.total as f32);
        if child_width >= MIN_FRAME_WIDTH {
            let mut child_path = path.clone();
            child_path.push(index);
            layout_into(child, child_path, depth + 1, child_x, child_width, top, out);
        }
        child_x += child_width;
    }
}

/// The greatest stack depth reachable under `node` within `area` (respecting
/// the same `MIN_FRAME_WIDTH` cull as the layout), so the caller can size the
/// scroll region to exactly the painted height.
fn max_depth(node: &FlameNode, width: f32) -> usize {
    if node.total == 0 {
        return 0;
    }
    node.children
        .iter()
        .map(|child| {
            let child_width = width * (child.total as f32 / node.total as f32);
            if child_width >= MIN_FRAME_WIDTH {
                1 + max_depth(child, child_width)
            } else {
                0
            }
        })
        .max()
        .unwrap_or(0)
}

/// Resolves the focused node by walking `state.focus` from the root, returning
/// it plus the valid prefix of the path actually followed. A path that no
/// longer resolves (a stale focus over a fresh capture) falls back to the
/// deepest node it could reach, and the caller resets — so a click never
/// indexes past the end of a shorter tree.
fn resolve_focus<'a>(root: &'a FlameNode, focus: &[usize]) -> (&'a FlameNode, Vec<usize>, bool) {
    let mut node = root;
    let mut valid = Vec::new();
    let mut intact = true;
    for &index in focus {
        match node.children.get(index) {
            Some(child) => {
                node = child;
                valid.push(index);
            }
            None => {
                intact = false;
                break;
            }
        }
    }
    (node, valid, intact)
}

/// A warm flame-ish fill for `name`, deterministic per symbol so the same
/// frame keeps its color across zooms and re-captures. Hue spans the
/// yellow→red band (the palette the FlameGraph tool established), varied by a
/// cheap hash of the name.
fn frame_color(name: &str) -> Color32 {
    let hash = name
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    let red = 205 + (hash % 50) as u8; // 205..=254
    let green = 80 + (hash / 50 % 110) as u8; // 80..=189
    let blue = 30 + (hash / 5000 % 30) as u8; // 30..=59
    Color32::from_rgb(red, green, blue)
}

/// Renders the flame graph for `root`, returning nothing — all state (the zoom
/// focus) lives in `state`. Assumes a non-empty profile; the caller decides
/// whether there's anything to show (an empty-state message otherwise).
pub fn show(ui: &mut egui::Ui, root: &FlameNode, state: &mut FlameGraphState) {
    let (focus_node_total, valid_focus, intact) = {
        let (node, valid, intact) = resolve_focus(root, &state.focus);
        (node.total, valid, intact)
    };
    if !intact {
        state.focus = valid_focus.clone();
    }
    let focus_path = valid_focus;

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let width = ui.available_width();
        // Re-borrow the focus node inside the closure against the resolved
        // path (kept as an index path, not a borrow, to avoid holding a borrow
        // of `root` across the `state` mutation above).
        let (focus_node, _, _) = resolve_focus(root, &focus_path);
        let depth = max_depth(focus_node, width);
        let height = (depth as f32 + 1.0) * ROW_HEIGHT;

        let (area, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        let frames = layout_frames(focus_node, &focus_path, area);

        let painter = ui.painter_at(area);
        let hover_pos = response.hover_pos();
        let mut hovered: Option<&LaidOutFrame> = None;

        for frame in &frames {
            let is_hovered = hover_pos.is_some_and(|p| frame.rect.contains(p));
            if is_hovered {
                hovered = Some(frame);
            }
            let mut fill = frame_color(&frame.name);
            if is_hovered {
                fill = fill.gamma_multiply(1.25);
            }
            painter.rect_filled(frame.rect, 2.0, fill);
            painter.rect_stroke(
                frame.rect,
                2.0,
                Stroke::new(1.0, Color32::from_black_alpha(40)),
                egui::StrokeKind::Inside,
            );

            // Only label frames wide enough to fit at least a few glyphs;
            // narrower ones are still hoverable for their full name.
            if frame.rect.width() > 24.0 {
                let label = elide(&frame.name, frame.rect.width());
                painter.text(
                    pos2(frame.rect.left() + 4.0, frame.rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    FontId::monospace(11.0),
                    Color32::from_rgb(30, 20, 10),
                );
            }
        }

        if let Some(frame) = hovered {
            let root_total = root.total.max(1);
            let focus_total = focus_node_total.max(1);
            let name = frame.name.clone();
            let total = frame.total;
            let of_focus = 100.0 * total as f32 / focus_total as f32;
            let of_root = 100.0 * total as f32 / root_total as f32;
            response.clone().on_hover_ui_at_pointer(|ui| {
                ui.monospace(&name);
                ui.label(format!(
                    "{total} samples · {of_focus:.1}% of view · {of_root:.1}% of total"
                ));
            });
        }

        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(frame) = frames.iter().find(|f| f.rect.contains(pos))
        {
            if frame.path == focus_path {
                // Clicking the focused (top) frame zooms back out one level.
                state.focus.pop();
            } else {
                state.focus = frame.path.clone();
            }
        }
    });
}

/// Truncates `name` with a trailing ellipsis to roughly fit `width` points at
/// the 11pt monospace label size (~6.6 px/char), keeping the most specific
/// tail of a long `pkg/Class.method` readable rather than cutting it blindly.
fn elide(name: &str, width: f32) -> String {
    let max_chars = ((width - 8.0) / 6.6).floor().max(1.0) as usize;
    if name.chars().count() <= max_chars {
        return name.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let kept: String = name.chars().take(max_chars - 1).collect();
    format!("{kept}…")
}

#[cfg(test)]
#[path = "flame_graph_test.rs"]
mod flame_graph_test;
