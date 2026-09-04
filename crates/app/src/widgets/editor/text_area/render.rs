//! Read-only virtualized render path (PLAN.md Phase 2b): given a buffer and a
//! font, allocate a content-sized rect so the enclosing `ScrollArea`'s
//! scrollbar is correct, then shape and paint **only the rows the viewport
//! actually touches**. No caret, selection, or input yet — those are 2d–2f;
//! this is the piece that proves virtualization renders the right pixels.
//!
//! Lines are shaped as plain single-color text for this milestone; per-token
//! syntax highlighting slots in with the overlay/`char_rect` work (2c), where
//! the visible-range span slicing lives.

use std::ops::Range;
use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{Color32, FontId, Galley, Sense, TextFormat};
use ropey::Rope;

pub use super::cache::ContentKey;
use super::cache::hash_hidden;
use super::{FoldMap, content_height, prefix_rows, visible_lines, visible_rows};

/// One already-resolved syntax-highlighting span: a **byte** range into the
/// buffer's full text, and the theme color to paint it. Deliberately just
/// `(Range<usize>, Color32)` rather than a tree-sitter/`syntax::Scope`
/// dependency — `text_area` stays decoupled from parsing and theming;
/// `widget.rs` computes these once per frame from `syntax::highlight_spans`
/// combined with `theme::color_for_scope`, the same way its own `layouter`
/// closure already does for the `egui::TextEdit` path today, and hands the
/// result down. An empty slice means "no highlighting" (a non-Java/Kotlin
/// file, or no parsed tree yet) — every byte then falls through to the
/// caller's single fallback color, same as before this existed.
#[derive(Clone)]
pub struct HighlightSpan {
    pub range: Range<usize>,
    pub color: Color32,
}

/// What a virtualized editor frame produces for the overlay/interception code
/// to read — the hand-built analogue of `egui::text_edit::TextEditOutput`.
/// Grows a caret/selection field in 2d; for now it carries the geometry and
/// the per-visible-row galleys the overlays will position against.
#[derive(Clone)]
pub struct TextAreaOutput {
    pub response: egui::Response,
    /// Uniform height of one **visual row**, in points — with word-wrap on,
    /// a logical line can span several of these; see `row_offsets`.
    pub row_height: f32,
    /// Screen position of logical line 0, column 0 — already scroll-adjusted,
    /// so `content_origin.y + row_offset * row_height` is a row's screen y.
    pub content_origin: egui::Pos2,
    /// The visual-row range shaped and painted this frame.
    pub visible_rows: Range<usize>,
    /// `(logical line index, shaped galley)` for each visible **logical
    /// line** touched this frame, in ascending order — one entry per line,
    /// *not* one per visual row (with word-wrap on, one line's galley
    /// already contains all of its own wrapped rows internally, the same
    /// way `egui::Galley` always has; see `row_offsets`).
    pub row_galleys: Vec<(usize, Arc<Galley>)>,
    /// Index-aligned with `row_galleys`: the visual row each entry's galley
    /// starts painting at. `visible_rows.start + i` in the no-wrap case
    /// (every line is exactly one row), but diverges once a wrapped line
    /// upstream of a given entry has pushed everything after it further
    /// down than a flat per-*line* count would predict.
    pub row_offsets: Vec<usize>,
}

impl TextAreaOutput {
    /// Screen rect of the character at `char_offset` (a thin caret-width box
    /// one row tall), or `None` if that offset's line isn't shaped this frame
    /// (scrolled off, or hidden inside a fold). The virtualized replacement for
    /// the overlays' `galley.pos_from_cursor(CCursor::new(char))` idiom — an
    /// overlay maps each decoration's char offset through this and simply skips
    /// the `None`s, since there's nothing visible to decorate off-screen.
    /// Correct for a char on any of a wrapped line's rows: `galley.
    /// pos_from_cursor` already resolves a column to the right *sub*-row
    /// within that line's own multi-row galley and returns a rect relative
    /// to the galley's top — adding that to the line's own block-start
    /// screen position (`row_offsets[idx] * row_height`) lands on the
    /// correct absolute row without this function needing to know which
    /// sub-row it was.
    pub fn char_rect(&self, buffer: &Rope, char_offset: usize) -> Option<egui::Rect> {
        let clamped = char_offset.min(buffer.len_chars());
        let line = buffer.char_to_line(clamped);
        let col = clamped - buffer.line_to_char(line);
        // `row_galleys` is always kept in ascending-line order (see its own
        // doc comment), so a binary search (SPEC.md §8) finds the same entry
        // a linear `position` scan would, in O(log rows) instead of O(rows)
        // — called several times per frame (diagnostics, occurrence/bracket
        // highlights, caret, IME range), each over however many rows are
        // currently visible.
        let idx = self.row_galleys.binary_search_by_key(&line, |(l, _)| *l).ok()?;
        let galley = &self.row_galleys[idx].1;
        let block_top = self.content_origin.y + self.row_offsets[idx] as f32 * self.row_height;
        let local = galley.pos_from_cursor(egui::text::CCursor::new(col));
        let x = self.content_origin.x + local.left();
        let y = block_top + local.top();
        Some(egui::Rect::from_min_size(
            egui::pos2(x, y),
            egui::vec2(1.0, self.row_height),
        ))
    }
}

/// Paints `buffer` read-only into the current (scroll-area) `ui`, virtualized:
/// only rows inside `ui.clip_rect()` are shaped. `hidden` is the fold set
/// (empty in Phase 2 — no folding yet). Unused in production since the swap
/// to `shell::show` (PLAN 2h) — kept for a future read-only viewer (e.g. a
/// diff/preview pane) that wants virtualized rendering without a caret;
/// exercised directly by `text_area/tests.rs` today, hence the `cfg_attr`
/// rather than a plain `#[expect]` (which would misfire as "unfulfilled" in
/// test builds, where this genuinely is used).
#[cfg_attr(not(test), expect(dead_code, reason = "future read-only-viewer API surface"))]
pub fn show_readonly(
    ui: &mut egui::Ui,
    id: egui::Id,
    buffer: &Rope,
    font_id: FontId,
    text_color: Color32,
    hidden: &[Range<usize>],
) -> TextAreaOutput {
    let out = layout_visible(ui, id, buffer, font_id, hidden, &[]);
    paint_rows(ui, &out, text_color);
    out
}

/// Allocates the widget's rect/response (sized to the buffer's full virtual
/// extent, so the enclosing `ScrollArea` and click/drag sensing behave
/// exactly as `show_readonly` always did) and shapes the currently-visible
/// rows' galleys, **without painting them**.
///
/// Split out from `show_readonly` so the interactive shell (`shell.rs`) can
/// shape once against the pre-edit buffer (to resolve a pointer event against
/// what's actually on screen this frame) and, only on a frame where an edit
/// actually changed the text, re-shape+paint the *post*-edit buffer — instead
/// of always painting stale, pre-edit galleys for one frame after every
/// keystroke the way a naive single-pass `show_readonly` call would.
/// `allocate_exact_size` must run exactly once per frame (calling it twice
/// would double-reserve layout space), which is why shaping and painting are
/// separate steps here rather than two full `show_readonly` calls.
pub(super) fn layout_visible(
    ui: &mut egui::Ui,
    id: egui::Id,
    buffer: &Rope,
    font_id: FontId,
    hidden: &[Range<usize>],
    spans: &[HighlightSpan],
) -> TextAreaOutput {
    let row_height = ui.fonts_mut(|f| f.row_height(&font_id));
    let total_lines = buffer.len_lines().max(1);
    let map = FoldMap::new(hidden);
    let total_rows = map.visual_count(total_lines);

    // Reserve the full virtual extent so the ScrollArea scrolls the whole
    // buffer even though only a slice is painted. Width is the viewport width
    // for now (real max-line-width horizontal scroll is a later refinement);
    // `desired_width`-style infinite growth isn't needed since we paint
    // absolutely, not via layout.
    let width = ui.available_width();
    // `ui.interact` with the caller's own stable `id` (rather than
    // `allocate_exact_size`'s auto-generated one) so the response is
    // reachable via `Context::read_response(id)` the same way `egui::
    // TextEdit::multiline(..).id(widget_id)` used to make it — callers
    // (Alt+Click's rect lookup, tests) depend on that exact identity.
    let (_, rect) = ui.allocate_space(egui::vec2(width, content_height(total_rows, row_height)));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let content_origin = rect.min;

    // How far the content's top has scrolled above the viewport's top.
    let clip = ui.clip_rect();
    let scroll_y = (clip.top() - rect.top()).max(0.0);
    let rows = visible_rows(scroll_y, clip.height(), row_height, total_rows);
    let row_galleys = shape_range(ui, buffer, rows.clone(), hidden, &font_id, spans);
    // No-wrap fast path: every visible logical line is exactly one visual
    // row, so its offset is trivially its position in `rows` — no wrapped
    // line ever pushes a later one further down than that.
    let row_offsets = rows.clone().collect();

    TextAreaOutput {
        response,
        row_height,
        content_origin,
        visible_rows: rows,
        row_galleys,
        row_offsets,
    }
}

/// Shapes just `rows` (a **visual** row range) against `buffer`, with no
/// allocation and no painting — the piece `layout_visible` uses for its own
/// first shaping pass, and that the shell (`shell.rs`) calls a second time,
/// against the post-edit buffer but the *same* row range, to repaint
/// same-frame after a keystroke without allocating layout space twice.
pub(super) fn shape_range(
    ui: &egui::Ui,
    buffer: &Rope,
    rows: Range<usize>,
    hidden: &[Range<usize>],
    font_id: &FontId,
    spans: &[HighlightSpan],
) -> Vec<(usize, Arc<Galley>)> {
    let map = FoldMap::new(hidden);
    rows.map(|visual_row| {
        let logical = map.to_logical(visual_row);
        (logical, shape_line(ui, buffer, logical, font_id, spans, f32::INFINITY))
    })
    .collect()
}

/// The word-wrap analogue of `shape_range`: shapes every logical line in
/// `lines` (not a visual-row range — with wrap on, one line can be several
/// rows, so the caller works in line space via `prefix_rows`/`visible_lines`
/// instead) against `wrap_width`. Lines inside `hidden` are skipped (`0`
/// rows, nothing to shape) rather than shaped and discarded.
pub(super) fn shape_line_range(
    ui: &egui::Ui,
    buffer: &Rope,
    lines: Range<usize>,
    hidden: &[Range<usize>],
    font_id: &FontId,
    spans: &[HighlightSpan],
    wrap_width: f32,
) -> Vec<(usize, Arc<Galley>)> {
    lines
        .filter(|line| !hidden.iter().any(|h| h.contains(line)))
        .map(|line| (line, shape_line(ui, buffer, line, font_id, spans, wrap_width)))
        .collect()
}

/// Paints the galleys `layout_visible` already shaped — the other half of
/// `show_readonly`'s split, so the shell can defer painting until it knows
/// which buffer version (pre- or post-edit) should actually appear on
/// screen this frame.
pub(super) fn paint_rows(ui: &egui::Ui, out: &TextAreaOutput, text_color: Color32) {
    let painter = ui.painter();
    for (i, (_, galley)) in out.row_galleys.iter().enumerate() {
        let pos = egui::pos2(
            out.content_origin.x,
            out.content_origin.y + out.row_offsets[i] as f32 * out.row_height,
        );
        painter.galley(pos, galley.clone(), text_color);
    }
}

/// Shapes one logical line (its trailing newline stripped, so it stays a
/// single row unless `wrap_width` is finite and the line is long enough to
/// wrap) into a galley, one section per `spans` entry that touches the line
/// (clipped to it and converted from global to line-local byte offsets)
/// plus `Color32::PLACEHOLDER` sections filling every gap — so a line with
/// no matching spans at all (plain text, or a language-less file) still
/// shapes exactly as it did before highlighting existed: one placeholder-
/// colored section, resolved to the caller's fallback color at paint time.
/// `wrap_width` is `f32::INFINITY` for the no-wrap path (PLAN.md Phase 2/3),
/// or the viewport width for Phase 4's word-wrap — `egui::Galley` already
/// lays a wrapped job out as several internal rows on its own, which is
/// what makes `galley.rows.len()` (`line_row_count`) and `galley.pos_from_
/// cursor`'s existing multi-row-aware hit-testing (`char_rect`) both work
/// here for free, with no separate "which sub-row" bookkeeping of our own.
fn shape_line(
    ui: &egui::Ui,
    buffer: &Rope,
    logical: usize,
    font_id: &FontId,
    spans: &[HighlightSpan],
    wrap_width: f32,
) -> Arc<Galley> {
    let line_idx = logical.min(buffer.len_lines().saturating_sub(1));
    let raw = buffer.line(line_idx);
    let text: String = raw.chars().filter(|&c| c != '\n' && c != '\r').collect();
    let line_start_byte = buffer.line_to_byte(line_idx);
    let line_len_bytes = text.len();
    let line_end_byte = line_start_byte + line_len_bytes;

    let format = |color: Color32| TextFormat {
        font_id: font_id.clone(),
        color,
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;

    let mut cursor = 0usize; // local byte offset into `text`
    for span in spans {
        if span.range.end <= line_start_byte || span.range.start >= line_end_byte {
            continue; // doesn't touch this line
        }
        let local_start = span
            .range
            .start
            .saturating_sub(line_start_byte)
            .clamp(cursor, line_len_bytes);
        let local_end = span.range.end.saturating_sub(line_start_byte).min(line_len_bytes);
        if local_start >= local_end || !text.is_char_boundary(local_start) || !text.is_char_boundary(local_end) {
            continue;
        }
        if local_start > cursor {
            job.append(&text[cursor..local_start], 0.0, format(Color32::PLACEHOLDER));
        }
        job.append(&text[local_start..local_end], 0.0, format(span.color));
        cursor = local_end;
    }
    if cursor < text.len() || job.sections.is_empty() {
        job.append(&text[cursor..], 0.0, format(Color32::PLACEHOLDER));
    }

    ui.fonts_mut(|f| f.layout_job(job))
}

/// PLAN.md Phase 4: word-wrap's own `layout_visible` — same contract
/// (allocate once, shape only what's currently visible, don't paint), but
/// working in *logical line* space via `text_area::{prefix_rows,
/// visible_lines}` instead of a flat visual-row count, since a wrapped line
/// can be more than one row.
pub(super) fn layout_visible_wrapped(
    ui: &mut egui::Ui,
    id: egui::Id,
    buffer: &Rope,
    content: ContentKey,
    font_id: FontId,
    hidden: &[Range<usize>],
    spans: &[HighlightSpan],
) -> TextAreaOutput {
    let row_height = ui.fonts_mut(|f| f.row_height(&font_id));
    let total_lines = buffer.len_lines().max(1);
    let width = ui.available_width();

    let counts = cached_row_counts(ui, id, content, &font_id, width, hidden, total_lines);
    let prefix = prefix_rows(&counts);
    let total_rows = *prefix.last().expect("prefix_rows always returns at least one entry");

    // Same stable-`id` reasoning as `layout_visible`'s own allocate/interact
    // split (see its doc comment).
    let (_, rect) = ui.allocate_space(egui::vec2(width, content_height(total_rows, row_height)));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let content_origin = rect.min;

    let clip = ui.clip_rect();
    let scroll_y = (clip.top() - rect.top()).max(0.0);
    let lines = visible_lines(scroll_y, clip.height(), row_height, &prefix);
    let row_galleys = shape_line_range(ui, buffer, lines.clone(), hidden, &font_id, spans, width);
    // Derived *from* `row_galleys` (not independently re-filtered over
    // `lines`) so the two can never drift out of index-alignment with each
    // other — every entry's offset is just its own line's prefix-sum value.
    let row_offsets: Vec<usize> = row_galleys.iter().map(|(line, _)| prefix[*line]).collect();
    let visible_rows =
        prefix.get(lines.start).copied().unwrap_or(0)..prefix.get(lines.end).copied().unwrap_or(total_rows);

    // Feed back what shaping-for-paint just learned for real, so a later
    // frame at the same cache key (pure scroll, no edit) doesn't have to
    // guess `default_row_counts`' baseline for a line already visited.
    let key = RowCountsKey {
        content,
        hidden_hash: hash_hidden(hidden),
        wrap_width_bits: width.to_bits(),
        font_size_bits: font_id.size.to_bits(),
    };
    record_shaped_rows(ui, id, &key, counts, &row_galleys);

    TextAreaOutput {
        response,
        row_height,
        content_origin,
        visible_rows,
        row_galleys,
        row_offsets,
    }
}

#[derive(Clone, PartialEq)]
struct RowCountsKey {
    content: ContentKey,
    hidden_hash: u64,
    wrap_width_bits: u32,
    font_size_bits: u32,
}

#[derive(Clone)]
struct CachedRowCounts {
    key: RowCountsKey,
    counts: Arc<Vec<usize>>,
}

/// `1` row for every non-hidden logical line, `0` for one inside `hidden` —
/// the cheap baseline `cached_row_counts` starts from on a miss instead of
/// actually shaping anything. Pure and `ui`/font-independent so it stays
/// unit-testable on its own (Track 19 Phase 1).
pub(super) fn default_row_counts(total_lines: usize, hidden: &[Range<usize>]) -> Vec<usize> {
    let mut counts = vec![1usize; total_lines];
    for range in hidden {
        for i in range.clone().filter(|i| *i < total_lines) {
            counts[i] = 0;
        }
    }
    counts
}

/// How many visual rows each logical line in `buffer` is currently believed
/// to occupy at `wrap_width` (`0` for a line inside `hidden`) — needed up
/// front to build `layout_visible_wrapped`'s prefix sum. Cached in `ctx.data`
/// keyed by `id` plus a hash of everything that can invalidate it (buffer
/// content, the fold set, wrap width, font size), so a frame where none of
/// those changed — by far the common case: idle repaints, pure scrolling,
/// an edit to a *different* tab's document — reuses last frame's table.
///
/// Track 19 Phase 1: a cache **miss** no longer shapes every line to learn
/// its real row count (that was the whole-buffer-pass-per-edit cost PLAN.md
/// flagged). It instead seeds the table with `default_row_counts`' "1 row
/// per line" guess — real values are filled in lazily by `record_shaped_
/// rows`, called from `layout_visible_wrapped` right after it shapes the
/// *visible* slice for painting anyway, so no line is ever shaped just to
/// learn its row count. A scroll-only sequence of frames (same cache key)
/// keeps whatever's already been corrected and layers in whatever's newly
/// visible, converging toward exact for whatever's been scrolled through;
/// an edit resets to the cheap baseline (see `record_shaped_rows` and
/// PLAN.md's Track 19 Phase 1 entry for the accepted scrollbar/jump-target
/// approximation this trades for).
pub(super) fn cached_row_counts(
    ui: &egui::Ui,
    id: egui::Id,
    content: ContentKey,
    font_id: &FontId,
    wrap_width: f32,
    hidden: &[Range<usize>],
    total_lines: usize,
) -> Arc<Vec<usize>> {
    let cache_id = egui::Id::new(("text_area_row_counts", id));
    let key = RowCountsKey {
        content,
        hidden_hash: hash_hidden(hidden),
        wrap_width_bits: wrap_width.to_bits(),
        font_size_bits: font_id.size.to_bits(),
    };

    if let Some(cached) = ui.ctx().data(|d| d.get_temp::<CachedRowCounts>(cache_id))
        && cached.key == key
    {
        return cached.counts;
    }

    let counts = Arc::new(default_row_counts(total_lines, hidden));
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            cache_id,
            CachedRowCounts {
                key,
                counts: counts.clone(),
            },
        )
    });
    counts
}

/// Writes real, just-shaped row counts (`galley.rows.len()`, learned as a
/// side effect of shaping `shaped` for painting — never a second shaping
/// pass of its own) back into the cached table `cached_row_counts` reads,
/// for whichever lines `layout_visible_wrapped` actually shaped this frame.
/// A no-op past the initial index reads when every visible line's cached
/// count already matches (steady-state scrolling through an
/// already-corrected region, or an idle repaint): no allocation, no
/// `ctx().data_mut()` write-lock. `Arc::make_mut` clones the table at most
/// once per call, only when a correction is actually needed.
fn record_shaped_rows(
    ui: &egui::Ui,
    id: egui::Id,
    key: &RowCountsKey,
    counts: Arc<Vec<usize>>,
    shaped: &[(usize, Arc<Galley>)],
) {
    let cache_id = egui::Id::new(("text_area_row_counts", id));
    let mut counts = counts;
    let mut changed = false;
    for (line, galley) in shaped {
        let real = galley.rows.len().max(1);
        if counts.get(*line).copied() != Some(real) {
            changed = true;
            Arc::make_mut(&mut counts)[*line] = real;
        }
    }
    if changed {
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                cache_id,
                CachedRowCounts {
                    key: key.clone(),
                    counts,
                },
            )
        });
    }
}
