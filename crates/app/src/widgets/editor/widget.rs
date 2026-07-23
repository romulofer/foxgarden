use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Event, FontId, Galley, Key};
use fg_core::{Document, Language};
use ropey::Rope;
use syntax::IncrementalParser;

use super::auto_edit::{
    apply_auto_indent, apply_auto_pair, char_to_byte, indent_selected_lines, is_pairable, join_lines, wrap_selection,
};
use super::codegen::{generate_accessors, insert_generated};
use super::multi_cursor::{self, MultiEditOp};
use super::painting::{paint_diagnostics, paint_extra_selections, paint_line_numbers};
use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::style::theme;

/// Identifies what a laid-out galley depends on: the buffer's exact
/// contents, which language (if any) is highlighting it, the color theme,
/// the wrap width, and the font size. Two frames with an equal key produce
/// an identical `Galley`, so a match means the cached one from
/// `CachedLayout` can be reused outright.
#[derive(Clone, Copy, PartialEq)]
struct LayoutCacheKey {
    content_hash: u64,
    language: Option<Language>,
    dark_mode: bool,
    wrap_width_bits: u32,
    font_size_bits: u32,
}

#[derive(Clone)]
struct CachedLayout {
    key: LayoutCacheKey,
    galley: Arc<Galley>,
}

/// Horizontal breathing room on each side of the line-number gutter's
/// digits, so they don't crowd the window edge or the text they precede.
const GUTTER_PADDING: f32 = 8.0;

fn hash_source(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

fn plain_format(font_id: FontId, dark_mode: bool) -> TextFormat {
    TextFormat {
        font_id,
        color: theme::default_text(dark_mode),
        ..Default::default()
    }
}

fn scope_format(font_id: FontId, scope: syntax::Scope, dark_mode: bool) -> TextFormat {
    TextFormat {
        font_id,
        color: theme::color_for_scope(scope, dark_mode),
        ..Default::default()
    }
}

/// Commits `new_text` as `doc`'s buffer and brings `parser` (if any) back in
/// sync with it via the incremental diff-and-reparse path: compute the edit
/// between `old_text` and `new_text`, reparse, refresh diagnostics from the
/// result. Every path in `show` that produces a new full-text snapshot from
/// an old one — multi-cursor edits, plain typing, `Ctrl+J`, wrap-selection —
/// needs this exact sequence, so it's centralized here rather than repeated
/// at each call site (repetition that's exactly how a future edit path
/// could forget the reparse step and silently drift out of sync, the same
/// bug `save_tab` in `panels::tabs` had before it started reusing this
/// pattern too).
fn apply_edit(doc: &mut Document, parser: &mut Option<IncrementalParser>, old_text: &str, new_text: &str) {
    doc.buffer = Rope::from_str(new_text);
    if let Some(parser) = parser.as_mut() {
        let edit = syntax::diff_edit(old_text, new_text);
        parser.reparse(new_text, edit);
        doc.diagnostics = syntax::syntax_errors(parser.tree().expect("just reparsed"));
    }
}

/// Renders `doc`'s buffer as an editable text area, keeping `parser`'s
/// incremental tree in sync with edits (SPEC.md sections 5.4 and 5.5).
/// `parser` is `None` for files with no recognized language (anything other
/// than `.java`/`.kt`) — such files still open and edit normally, they just
/// get plain rendering and no diagnostics; auto-pair/auto-indent/multi-cursor
/// are language-agnostic and keep working regardless.
pub fn show(
    ui: &mut egui::Ui,
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    editor_font: EditorFont,
    font_size: f32,
    indent_settings: IndentSettings,
) {
    // Mutable: the wrap-selection interception below may replace both with
    // an already-edited version *before* `TextEdit::show()` ever runs, so
    // everything downstream (the layouter's highlighting, the
    // `response.changed()` diff) sees the post-wrap text as its baseline
    // rather than redoing (or fighting with) the edit egui would otherwise
    // apply on its own.
    let mut old_text = doc.buffer.to_string();
    let mut text = old_text.clone();
    // A stable id (rather than the default position-based auto id) keeps
    // this widget's identity — and thus its cursor/selection state — tied
    // to the document, not to where `show` happens to be called from in the
    // ui tree; it also lets tests request focus deterministically. Set via
    // `.id(widget_id)` below, not `.id_salt(id_salt)`: the latter still
    // combines the salt with whichever `Ui` calls `.show()`
    // (`ui.make_persistent_id`), so it silently changes if `show`'s
    // internals end up calling the builder through a different nested
    // `Ui` than before (as happened when the line-number gutter moved the
    // `TextEdit` inside a `ui.horizontal` one level deeper) — the opposite
    // of the position-independence this comment claims. `Id::new` is a
    // pure hash of the salt with no `Ui` involved, so it actually holds.
    let id_salt = doc.path.to_string_lossy().into_owned();
    let widget_id = egui::Id::new(&id_salt);
    // A galley is expensive to shape (font lookups, kerning, glyph layout)
    // but egui's own galley cache (`Fonts`) is flushed of anything not
    // touched *this* frame, every frame — and only the active tab's editor
    // renders each frame. So switching back to a tab that was merely
    // sitting open (not the active one for a few frames) would otherwise
    // force a full reshape of its entire buffer, no matter how big it is.
    // This cache lives in `egui::Context`'s persistent temp storage instead
    // (keyed off this document's `id_salt`), which isn't subject to that
    // per-frame flush — it survives tab switches and is only replaced when
    // `LayoutCacheKey` actually changes, i.e. the buffer, language, theme,
    // or wrap width changed since it was last shaped.
    let layout_cache_id = egui::Id::new(("editor_layout_cache", id_salt.as_str()));

    let multi_cursor_active_at_start = !doc.extra_selections.is_empty();

    // `TextEditState::store` takes `self` by value, so it can only be
    // called once per frame — every path below that wants to override
    // egui's own post-edit cursor/selection just records the target range
    // here, and a single `set_char_range` + `store` happens at the very
    // end.
    let mut manual_cursor_range: Option<CCursorRange> = None;

    // While extra (Ctrl+D) cursors are active, an editing keystroke must
    // land at every active cursor at once, not just the one egui's own
    // single-cursor `TextEdit` logic would edit alone. This reads the
    // *persisted* selection and applies the edit manually *before*
    // `TextEdit::show()` runs — the same pre-apply timing wrap-selection and
    // the indent interception below use — so this frame already renders the
    // fully multi-edited result instead of a stale one needing a follow-up
    // `request_repaint()`. (A previous version of this function applied the
    // edit *after* `show()`, off `output.cursor_range`, and ate a stale
    // frame; see the resolved entry in `TECHNICAL_DEBT.md` for why that was
    // debt worth fixing rather than a style difference from wrap-selection.)
    // `doc.extra_selections` only ever becomes non-empty via a prior
    // `Ctrl+D` frame, so by the time `multi_cursor_active_at_start` is true
    // here, a persisted `TextEditState` from that prior frame is always
    // expected to exist.
    if multi_cursor_active_at_start {
        let primary_range = egui::text_edit::TextEditState::load(ui.ctx(), widget_id)
            .and_then(|state| state.cursor.char_range())
            .map(|range| range.as_sorted_char_range());

        if let Some(primary_range) = primary_range {
            let intercepted_events = ui.input_mut(|i| {
                let matched: Vec<Event> = i.events.iter().filter(|e| is_multi_edit_event(e)).cloned().collect();
                i.events.retain(|e| !is_multi_edit_event(e));
                matched
            });

            if !intercepted_events.is_empty() {
                let op = multi_edit_op_from_events(&intercepted_events);
                let mut selections = Vec::with_capacity(1 + doc.extra_selections.len());
                selections.push(primary_range.start.0..primary_range.end.0);
                selections.extend(doc.extra_selections.iter().cloned());

                let (new_text, new_cursors) = multi_cursor::apply_multi_edit(&old_text, &selections, &op);

                apply_edit(doc, parser, &old_text, &new_text);
                manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursors[0])));
                doc.extra_selections = new_cursors[1..].iter().map(|&c| c..c).collect();
                old_text = new_text.clone();
                text = new_text;
            }
        }
    }

    // Typing a bracket/quote while a selection is active should wrap the
    // selected text in it, not replace it the way egui's `TextEdit` does by
    // default. That default has already happened by the time a post-edit
    // diff could see it (the same technique `apply_auto_pair`/
    // `apply_auto_indent` use below) — the selected text egui deleted is
    // just gone from `text` at that point. So this reads the *persisted*
    // selection from before this frame's `TextEdit::show()` call instead,
    // and — if it's non-empty and the next queued keystroke is a single
    // pairable character — intercepts that event and applies the wrap
    // manually, the same "pull it out of the queue before `TextEdit` sees
    // it" approach the multi-cursor interception above uses. Skipped
    // whenever multi-cursor is active: wrapping is a single-selection
    // concept, and the primary selection's meaning while `Ctrl+D` extras
    // exist is already spoken for by the multi-edit path above.
    if !multi_cursor_active_at_start {
        // Cheap check first: only load the persisted `TextEditState` (a
        // `ctx.data` mutex lock + hashmap probe + struct clone, paid again
        // moments later by `TextEdit::show()`'s own internal load of the
        // exact same state) on the frames where there's actually a
        // candidate keystroke queued — not on every idle/mouse-only/
        // arrow-key frame the editor is visible.
        let has_candidate_keystroke = ui.input(|i| i.events.iter().any(|e| single_pairable_char(e).is_some()));

        if has_candidate_keystroke {
            let prior_selection = egui::text_edit::TextEditState::load(ui.ctx(), widget_id)
                .and_then(|state| state.cursor.char_range())
                .map(|range| range.as_sorted_char_range())
                .filter(|range| !range.is_empty());

            if let Some(range) = prior_selection {
                let opener = take_event(ui, |e| single_pairable_char(e).is_some())
                    .map(|e| single_pairable_char(&e).expect("matched above"));

                if let Some(opener) = opener {
                    if let Some((wrapped, sel_start, sel_end)) =
                        wrap_selection(&old_text, range.start.0, range.end.0, opener)
                    {
                        apply_edit(doc, parser, &old_text, &wrapped);
                        manual_cursor_range =
                            Some(CCursorRange::two(CCursor::new(sel_start), CCursor::new(sel_end)));
                        old_text = wrapped.clone();
                        text = wrapped;
                    }
                }
            }
        }
    }

    // Tab/Shift+Tab while a selection is active should indent/dedent every
    // line the selection touches, not replace the selection the way egui's
    // `TextEdit` does by default (see `indent_selected_lines`'s doc comment:
    // egui deletes the *entire* selection first, then for Shift+Tab dedents
    // only the single line the resulting cursor lands on). Same
    // pull-it-out-of-the-queue-before-`TextEdit`-sees-it approach as the
    // wrap-selection interception above, and skipped for the same reason
    // whenever multi-cursor is active.
    //
    // Plain Tab with *no* selection is handled here too, in "spaces" mode:
    // egui's own `TextEdit` (a `.code_editor()`, which sets `lock_focus`)
    // inserts a literal `\t` for a bare Tab keypress, which would silently
    // ignore an `indent_settings.use_tabs == false` choice. Skipped when
    // `use_tabs` is set — a literal tab already *is* that setting's unit,
    // so egui's default is exactly right and needs no interception — and
    // for Shift+Tab, which egui's own no-selection handling is left to
    // decide, same as before this feature existed.
    if !multi_cursor_active_at_start {
        let tab_pressed =
            ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Key { key: Key::Tab, pressed: true, .. })));

        if tab_pressed {
            let prior_selection = egui::text_edit::TextEditState::load(ui.ctx(), widget_id)
                .and_then(|state| state.cursor.char_range())
                .map(|range| range.as_sorted_char_range());

            if let Some(range) = prior_selection {
                if !range.is_empty() {
                    let removed = take_event(ui, |e| matches!(e, Event::Key { key: Key::Tab, pressed: true, .. }));

                    if removed.is_some() {
                        let dedent = ui.input(|i| i.modifiers.shift);
                        let (indented, sel_start, sel_end) =
                            indent_selected_lines(&old_text, range.start.0, range.end.0, dedent, indent_settings);
                        apply_edit(doc, parser, &old_text, &indented);
                        manual_cursor_range =
                            Some(CCursorRange::two(CCursor::new(sel_start), CCursor::new(sel_end)));
                        old_text = indented.clone();
                        text = indented;
                    }
                } else if !indent_settings.use_tabs && !ui.input(|i| i.modifiers.shift) {
                    let removed = take_event(ui, |e| matches!(e, Event::Key { key: Key::Tab, pressed: true, .. }));

                    if removed.is_some() {
                        let unit = indent_settings.unit();
                        let byte = char_to_byte(&old_text, range.start.0);
                        let inserted = format!("{}{unit}{}", &old_text[..byte], &old_text[byte..]);
                        apply_edit(doc, parser, &old_text, &inserted);
                        let new_cursor = range.start.0 + unit.chars().count();
                        manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
                        old_text = inserted.clone();
                        text = inserted;
                    }
                }
            }
        }
    }

    // Sized to the widest line number the buffer currently has, so a
    // 9-line file gets a narrow gutter and a 10,000-line one gets a wider
    // one rather than every file paying for a fixed worst-case width.
    let gutter_font_id = FontId::new(font_size, editor_font.family());
    let digit_width = ui.fonts_mut(|f| f.glyph_width(&gutter_font_id, '0'));
    let line_count = doc.buffer.len_lines().max(1);
    let gutter_width = digit_width * line_count.to_string().len() as f32 + GUTTER_PADDING * 2.0;

    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let source = buf.as_str();
        let dark_mode = ui.visuals().dark_mode;
        // Matches the rounding epaint's own galley cache applies to
        // `wrap.max_width` before hashing it — without this, float jitter
        // from upstream layout rounding would make the key flap between
        // frames and defeat the cache.
        let wrap_width = wrap_width.round();

        let tree_and_language = parser.as_ref().and_then(|p| p.tree().map(|tree| (tree, p.language())));
        let key = LayoutCacheKey {
            content_hash: hash_source(source),
            language: tree_and_language.as_ref().map(|(_, language)| *language),
            dark_mode,
            wrap_width_bits: wrap_width.to_bits(),
            font_size_bits: font_size.to_bits(),
        };

        if let Some(cached) = ui.ctx().data(|d| d.get_temp::<CachedLayout>(layout_cache_id)) {
            if cached.key == key {
                return cached.galley;
            }
        }

        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap_width;

        let font_id = FontId::new(font_size, editor_font.family());

        if let Some((tree, language)) = tree_and_language {
            let spans = syntax::highlight_spans(tree, &old_text, language);
            let mut cursor = 0usize;
            for (range, scope) in spans {
                if range.start > range.end
                    || range.end > source.len()
                    || !source.is_char_boundary(range.start)
                    || !source.is_char_boundary(range.end)
                    || range.start < cursor
                {
                    continue;
                }
                if range.start > cursor {
                    job.append(&source[cursor..range.start], 0.0, plain_format(font_id.clone(), dark_mode));
                }
                job.append(
                    &source[range.start..range.end],
                    0.0,
                    scope_format(font_id.clone(), scope, dark_mode),
                );
                cursor = range.end;
            }
            if cursor < source.len() {
                job.append(&source[cursor..], 0.0, plain_format(font_id.clone(), dark_mode));
            }
        } else if !source.is_empty() {
            job.append(source, 0.0, plain_format(font_id, dark_mode));
        }

        let galley = ui.fonts_mut(|f| f.layout_job(job));
        ui.ctx().data_mut(|d| {
            d.insert_temp(layout_cache_id, CachedLayout { key, galley: galley.clone() })
        });
        galley
    };

    // The gutter and the text field are laid out side by side, in that
    // order, inside one `horizontal` — that's what shifts the `TextEdit`
    // right to make room, and what gives `paint_line_numbers` (called once
    // `output` is available, alongside the other overlay painting below)
    // the gutter's left edge to right-align digits against. Both live
    // inside the *same* `ScrollArea` call site (`panels::tabs::show`), so
    // they scroll together as one unit rather than independently.
    let (mut output, gutter_left) = ui
        .horizontal(|ui| {
            let gutter_left = ui.cursor().left();
            ui.add_space(gutter_width);
            let output = egui::TextEdit::multiline(&mut text)
                .id(widget_id)
                .code_editor()
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter)
                .show(ui);
            (output, gutter_left)
        })
        .inner;

    if output.response.changed() {
        if multi_cursor_active_at_start {
            // A mutating event that wasn't applied by the multi-cursor block
            // above — Tab, undo/redo, an IME commit, or (in principle, never
            // observed in practice — see that block's comment) no persisted
            // selection yet to apply against — reached egui's own
            // single-cursor logic and edited the primary cursor alone.
            // `doc.extra_selections` is now stale relative to `text`, so
            // rather than paint/edit at wrong offsets next frame, treat
            // this as an implicit collapse back to single-cursor mode.
            doc.extra_selections.clear();
        }

        let cursor_char = output.cursor_range.map(|r| r.primary.index.0);
        let (text_after_indent, indent_cursor) = apply_auto_indent(&old_text, &text, cursor_char, indent_settings);
        let corrected = if indent_cursor.is_some() {
            manual_cursor_range = indent_cursor.map(|c| CCursorRange::one(CCursor::new(c)));
            text_after_indent
        } else {
            apply_auto_pair(&old_text, &text, cursor_char)
        };

        apply_edit(doc, parser, &old_text, &corrected);
    }

    let modifiers = ui.input(|i| i.modifiers);
    let ctrl_d_pressed = ui.input(|i| i.key_pressed(Key::D)) && modifiers.command;
    if ctrl_d_pressed && let Some(primary_range) = output.cursor_range {
        if primary_range.is_empty() {
            let word = multi_cursor::word_range_at(&doc.buffer.to_string(), primary_range.primary.index.0);
            if !word.is_empty() {
                manual_cursor_range = Some(CCursorRange::two(CCursor::new(word.start), CCursor::new(word.end)));
            }
        } else {
            let text_now = doc.buffer.to_string();
            let sorted = primary_range.as_sorted_char_range();
            let needle_range = sorted.start.0..sorted.end.0;
            let needle = text_now[char_to_byte(&text_now, needle_range.start)..char_to_byte(&text_now, needle_range.end)]
                .to_string();

            let mut claimed = doc.extra_selections.clone();
            claimed.push(needle_range.clone());

            let case_sensitive = modifiers.shift;
            if let Some(found) = multi_cursor::find_next_unclaimed_occurrence(
                &text_now,
                &needle,
                needle_range.end,
                &claimed,
                case_sensitive,
            ) {
                doc.extra_selections.push(needle_range);
                manual_cursor_range = Some(CCursorRange::two(CCursor::new(found.start), CCursor::new(found.end)));
            }
        }
    }

    let ctrl_j_pressed = ui.input(|i| i.key_pressed(Key::J)) && modifiers.command;
    if ctrl_j_pressed && let Some(primary_range) = output.cursor_range {
        let text_now = doc.buffer.to_string();
        if let Some((joined, new_cursor)) = join_lines(&text_now, primary_range.primary.index.0) {
            apply_edit(doc, parser, &text_now, &joined);
            manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
        }
    }

    // Ctrl+Shift+G: generate getters/setters for the enclosing Java class's
    // fields at the cursor. Java-only — Kotlin's `val`/`var` properties
    // already *are* getters/setters, so generating explicit ones for them
    // isn't the idiomatic move a Java accessor-boilerplate command is.
    let generate_accessors_pressed = ui.input(|i| i.key_pressed(Key::G)) && modifiers.command && modifiers.shift;
    if generate_accessors_pressed
        && doc.language == Some(Language::Java)
        && let Some(primary_range) = output.cursor_range
        && let Some(tree) = parser.as_ref().and_then(|p| p.tree())
    {
        let text_now = doc.buffer.to_string();
        let cursor_char = primary_range.primary.index.0;
        let cursor_byte = char_to_byte(&text_now, cursor_char);
        let fields = syntax::java_fields_in_enclosing_class(tree, &text_now, cursor_byte);
        if !fields.is_empty() {
            let generated = generate_accessors(&fields, &indent_settings.unit());
            let (inserted, new_cursor) = insert_generated(&text_now, cursor_char, &generated);
            apply_edit(doc, parser, &text_now, &inserted);
            manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
        }
    }

    if !doc.extra_selections.is_empty() && !ctrl_d_pressed {
        let should_collapse = ui.input(|i| i.events.iter().any(is_multi_cursor_collapse_event));
        if should_collapse {
            doc.extra_selections.clear();
        }
    }

    paint_diagnostics(ui, &output, &text, &doc.diagnostics);
    paint_extra_selections(ui, &output, &doc.extra_selections);
    paint_line_numbers(ui, &output, gutter_left + gutter_width - GUTTER_PADDING, gutter_font_id, ui.visuals().dark_mode);

    // Auto-indent inserts content *before* where egui placed the cursor
    // (unlike auto-pair, which only ever inserts after it), so the cursor
    // needs to be pushed forward past the inserted indentation manually.
    // The multi-cursor paths above reuse the same mechanism to land the
    // primary cursor after a multi-edit or a Ctrl+D word/occurrence jump.
    if let Some(range) = manual_cursor_range {
        output.state.cursor.set_char_range(Some(range));
        let id = output.response.id;
        output.state.store(ui.ctx(), id);
    }
}

/// Removes and returns the first event in this frame's input queue matching
/// `predicate`, if any. The shared "pull one event out of the queue before
/// `TextEdit::show()` sees it" primitive behind wrap-selection's and the
/// Tab/Shift+Tab indent interception's single-event removal (see
/// `TECHNICAL_DEBT.md`'s formerly-open "two interception mechanisms" entry)
/// — factored out once a third call site needed the identical shape, rather
/// than let each new feature reinvent its own `position` + `remove`.
fn take_event(ui: &egui::Ui, predicate: impl Fn(&Event) -> bool) -> Option<Event> {
    ui.input_mut(|i| {
        let index = i.events.iter().position(predicate)?;
        Some(i.events.remove(index))
    })
}

/// The single character `event` types, if it's exactly one character *and*
/// one this editor auto-pairs — the condition that makes it a candidate for
/// wrap-selection to intercept. Shared between the cheap "is there anything
/// worth loading `TextEditState` for" check and the actual removal, so the
/// two can't drift apart on what counts as a match.
fn single_pairable_char(event: &Event) -> Option<char> {
    let Event::Text(s) = event else { return None };
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if is_pairable(c) => Some(c),
        _ => None,
    }
}

fn is_multi_edit_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Text(_)
            | Event::Paste(_)
            | Event::Key {
                key: Key::Backspace | Key::Delete | Key::Enter,
                pressed: true,
                ..
            }
    )
}

fn multi_edit_op_from_events(events: &[Event]) -> MultiEditOp {
    let mut inserted = String::new();
    for event in events {
        match event {
            Event::Text(s) => inserted.push_str(s),
            Event::Paste(s) => inserted.push_str(s),
            Event::Key { key: Key::Enter, .. } => inserted.push('\n'),
            Event::Key { key: Key::Backspace, .. } => return MultiEditOp::Backspace,
            Event::Key { key: Key::Delete, .. } => return MultiEditOp::Delete,
            _ => {}
        }
    }
    MultiEditOp::Insert(inserted)
}

fn is_multi_cursor_collapse_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Key {
            key: Key::ArrowLeft
                | Key::ArrowRight
                | Key::ArrowUp
                | Key::ArrowDown
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown
                | Key::Escape,
            pressed: true,
            ..
        } | Event::PointerButton {
            pressed: true,
            button: egui::PointerButton::Primary,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fg_core::Language;

    fn open_fixture(contents: &str, filename: &str) -> (tempfile::TempDir, Document) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(filename);
        std::fs::write(&path, contents).unwrap();
        let doc = Document::open(path).unwrap();
        (dir, doc)
    }

    /// Builds a freshly parsed `Some(IncrementalParser)`, matching what
    /// `panels::tabs::open_parser_for` produces for any file with a
    /// recognized language — `show`'s tests always exercise the "has a
    /// language" path unless a test says otherwise.
    fn parsed(language: Language, source: &str) -> Option<IncrementalParser> {
        let mut parser = IncrementalParser::new(language);
        parser.parse(source);
        Some(parser)
    }

    #[test]
    fn renders_highlighted_valid_file_without_panicking() {
        let (_dir, mut doc) = open_fixture(
            "public class Hello {\n    // greeting\n    String greet() { return \"hi\"; }\n}\n",
            "Hello.java",
        );
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        egui::__run_test_ui(|ui| {
            // egui::__run_test_ui uses an empty FontDefinitions with no
            // registered families beyond the built-in Monospace/Proportional
            // ones, so EditorFont::JetBrainsMono (a custom Name() family
            // only registered by fonts::install in real main()) would panic
            // here with "is not bound to any fonts". Use the built-in family
            // instead — this test exercises the widget's rendering logic,
            // not font registration.
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
    }

    #[test]
    fn renders_squiggles_for_real_syntax_error_without_panicking() {
        let (_dir, mut doc) = open_fixture(
            "public class Hello {\n    public String greet() {\n        return \"Hello!\";\n    }\n",
            "Broken.java",
        );
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        doc.diagnostics = syntax::syntax_errors(parser.as_ref().unwrap().tree().unwrap());
        assert!(
            !doc.diagnostics.is_empty(),
            "fixture should contain a deliberate syntax error"
        );

        egui::__run_test_ui(|ui| {
            // EditorFont::Default, not JetBrainsMono: see comment on the
            // first test in this file for why.
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
    }

    #[test]
    fn plain_text_file_renders_without_a_parser_and_stays_free_of_diagnostics() {
        let (_dir, mut doc) = open_fixture("just some notes, no code here", "notes.txt");
        assert_eq!(doc.language, None);
        let mut parser: Option<IncrementalParser> = None;

        egui::__run_test_ui(|ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default());
        });

        assert!(doc.diagnostics.is_empty());
    }

    #[test]
    fn auto_pair_still_works_for_a_plain_text_file_with_no_parser() {
        let (_dir, mut doc) = open_fixture("", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        focused_frame(&mut doc, &mut parser, vec![egui::Event::Text("{".to_string())]);

        assert_eq!(doc.buffer.to_string(), "{}");
    }

    #[test]
    fn simulated_edit_updates_diagnostics_and_dirty_state() {
        let (_dir, mut doc) = open_fixture("public class Hello {}\n", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        assert!(!doc.is_dirty());

        // Directly exercise the same edit -> reparse -> diagnostics path
        // that `show`'s `response.changed()` branch runs, without needing a
        // simulated keystroke through egui's input queue.
        let old_text = doc.buffer.to_string();
        let new_text = "public class Hello {\n".to_string(); // drop the closing brace
        let edit = syntax::diff_edit(&old_text, &new_text);
        let inner_parser = parser.as_mut().unwrap();
        inner_parser.reparse(&new_text, edit);
        doc.buffer = Rope::from_str(&new_text);
        doc.diagnostics = syntax::syntax_errors(inner_parser.tree().unwrap());

        assert!(doc.is_dirty());
        assert!(!doc.diagnostics.is_empty());

        egui::__run_test_ui(|ui| {
            // EditorFont::Default, not JetBrainsMono: see comment on the
            // first test in this file for why.
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
    }

    /// Computes the same `egui::Id` `show`'s layouter uses internally to key
    /// its persistent galley cache, so a test can look the cached entry up
    /// directly from `ctx.data()` without `show` needing to expose it.
    fn layout_cache_id(doc: &Document) -> egui::Id {
        let id_salt = doc.path.to_string_lossy().into_owned();
        egui::Id::new(("editor_layout_cache", id_salt.as_str()))
    }

    #[test]
    fn layout_cache_reuses_galley_across_unchanged_frames() {
        let (_dir, mut doc) = open_fixture("just some notes, no code here", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let cache_id = layout_cache_id(&doc);

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
        let first = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after first frame");

        // A second frame over the very same, unedited document — as if the
        // tab were simply redrawn, or switched away from and back to.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
        let second = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after second frame");

        assert!(
            Arc::ptr_eq(&first.galley, &second.galley),
            "unchanged content across frames should reuse the same shaped galley instead of reshaping it"
        );
    }

    #[test]
    fn layout_cache_reshapes_after_an_edit() {
        let (_dir, mut doc) = open_fixture("hello", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let cache_id = layout_cache_id(&doc);

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
        let first = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after first frame");

        doc.buffer = Rope::from_str("hello world");
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
        let second = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after second frame");

        assert!(
            !Arc::ptr_eq(&first.galley, &second.galley),
            "an edited buffer must not reuse the previous frame's stale galley"
        );
    }

    #[test]
    fn layout_cache_reshapes_after_a_font_size_change() {
        let (_dir, mut doc) = open_fixture("just some notes, no code here", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let cache_id = layout_cache_id(&doc);

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
        let first = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after first frame");

        // Same unedited content, but a different font size — the cached
        // galley was shaped at the old size, so it must not be reused.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 18.0, IndentSettings::default());
        });
        let second = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after second frame");

        assert!(
            !Arc::ptr_eq(&first.galley, &second.galley),
            "a font size change must not reuse the previous frame's stale-sized galley"
        );
    }

    // The tests below drive `show` through a real, reused `egui::Context`
    // (rather than the fire-and-forget `egui::__run_test_ui` used above),
    // since they need to simulate focused keyboard events: `show`'s
    // multi-cursor branches only run once `output.cursor_range` is `Some`,
    // which egui only produces for a widget that currently has keyboard
    // focus. `show`'s `.id_salt(doc.path...)` makes the widget's id
    // reproducible outside of `show` itself, so a test can request focus on
    // exactly that id before calling `show`.
    fn focused_frame(doc: &mut Document, parser: &mut Option<IncrementalParser>, events: Vec<egui::Event>) {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        // `RawInput::modifiers` ("which modifier keys are down at the start
        // of the frame") is a separate top-level field from each
        // `Event::Key`'s own `modifiers` — it's what `ui.input(|i|
        // i.modifiers)` actually reads (e.g. `Ctrl+J`'s `modifiers.command`
        // check), not the per-event field. Left at its `..Default::default()`
        // value (`NONE`), a simulated `Ctrl+<key>` event would carry the
        // right modifiers on the event itself but still read as unmodified —
        // so derive it from whichever `Key` event carries it.
        let modifiers = events
            .iter()
            .find_map(|e| match e {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let raw_input = egui::RawInput { events, modifiers, ..Default::default() };
        let _ = ctx.run_ui(raw_input, |ui| {
            // `show` sets the widget's id via `.id(egui::Id::new(id_salt))`
            // — a pure hash of the path string, independent of which `Ui`
            // ends up calling `.show()` — so replicate that exact
            // computation here or the id won't match and `request_focus`
            // will target nothing.
            let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
    }

    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Like `focused_frame`, but for exercising wrap-selection: `show`'s
    /// interception reads the *persisted* selection from before its own
    /// frame runs (see `show`'s wrap-selection block), so this pre-stores
    /// one — as if some earlier, unmodeled frame were where the user
    /// actually dragged/clicked to create it — before driving the frame
    /// under test.
    ///
    /// Runs one plain, unfocused-to-focused warm-up frame before injecting
    /// the selection: egui's `TextEdit` doesn't reliably honor a selection
    /// that was only ever set via `TextEditState::store` without the
    /// widget having actually lived through a real frame first — the
    /// same-frame "just gained focus" transition doesn't trust it, so a
    /// character typed on that very first frame lands at a default cursor
    /// position instead of replacing the injected selection. A real
    /// drag-selection always happens on a frame *after* the widget already
    /// has focus, so this just makes the test match that.
    fn focused_frame_with_selection(
        doc: &mut Document,
        parser: &mut Option<IncrementalParser>,
        selection: std::ops::Range<usize>,
        events: Vec<egui::Event>,
    ) {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default());
        });

        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(selection.start),
            CCursor::new(selection.end),
        )));
        state.store(&ctx, id);

        // Same derivation as `focused_frame`: `RawInput::modifiers` (what
        // `ui.input(|i| i.modifiers)` actually reads) is separate from each
        // `Event::Key`'s own `modifiers` field, so a simulated Shift+Tab
        // needs it pulled up to the top level or `show`'s
        // `ui.input(|i| i.modifiers.shift)` check would read unmodified.
        let modifiers = events
            .iter()
            .find_map(|e| match e {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let raw_input = egui::RawInput { events, modifiers, ..Default::default() };
        let _ = ctx.run_ui(raw_input, |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
    }

    /// Like `focused_frame`, but for exercising multi-cursor edits: `show`
    /// reads the primary cursor's *persisted* `TextEditState` to apply a
    /// multi-cursor edit before its own `TextEdit::show()` call runs (see
    /// `show`'s multi-cursor block), so — same reasoning as
    /// `focused_frame_with_selection` above — this runs one warm-up frame
    /// first to establish that persisted state before driving the frame
    /// under test. This isn't just a test-harness nicety: in real usage
    /// `doc.extra_selections` can only ever become non-empty via an earlier
    /// `Ctrl+D` frame, so a widget with multi-cursor active has necessarily
    /// already lived through at least one prior frame — a single dry frame
    /// with `extra_selections` pre-seeded, as `focused_frame` alone would
    /// give it, is a scenario that can't happen outside a test.
    fn focused_frame_with_extra_selections(
        doc: &mut Document,
        parser: &mut Option<IncrementalParser>,
        extra_selections: Vec<std::ops::Range<usize>>,
        events: Vec<egui::Event>,
    ) {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default());
        });

        doc.extra_selections = extra_selections;

        let modifiers = events
            .iter()
            .find_map(|e| match e {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let raw_input = egui::RawInput { events, modifiers, ..Default::default() };
        let _ = ctx.run_ui(raw_input, |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default());
        });
    }

    /// Like `focused_frame`, but runs a warm-up frame first and lets the
    /// caller choose `IndentSettings` — needed to exercise the plain-
    /// Tab-with-no-selection interception, which (like wrap-selection and
    /// multi-cursor) reads the *persisted* selection from before
    /// `TextEdit::show()` runs this frame, so a single dry frame with no
    /// prior state can't reach it. See `focused_frame_with_selection`'s doc
    /// comment for why a real warm-up frame, not just a stored
    /// `TextEditState`, is what's needed.
    fn focused_frame_with_indent_settings(
        doc: &mut Document,
        parser: &mut Option<IncrementalParser>,
        indent_settings: IndentSettings,
        events: Vec<egui::Event>,
    ) {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, indent_settings);
        });

        let modifiers = events
            .iter()
            .find_map(|e| match e {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let raw_input = egui::RawInput { events, modifiers, ..Default::default() };
        let _ = ctx.run_ui(raw_input, |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, indent_settings);
        });
    }

    #[test]
    fn typing_a_bracket_over_a_selection_wraps_it_instead_of_replacing_it() {
        let (_dir, mut doc) = open_fixture("foo bar baz", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        // "bar" is chars 4..7.
        focused_frame_with_selection(&mut doc, &mut parser, 4..7, vec![egui::Event::Text("(".to_string())]);

        assert_eq!(doc.buffer.to_string(), "foo (bar) baz");
    }

    #[test]
    fn typing_an_angle_bracket_over_a_selection_wraps_it_too() {
        let (_dir, mut doc) = open_fixture("List Item", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        // "Item" is chars 5..9.
        focused_frame_with_selection(&mut doc, &mut parser, 5..9, vec![egui::Event::Text("<".to_string())]);

        assert_eq!(doc.buffer.to_string(), "List <Item>");
    }

    fn shift_key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        }
    }

    #[test]
    fn shift_tab_dedents_every_line_a_multi_line_selection_touches() {
        let (_dir, mut doc) = open_fixture("    foo\n    bar\nbaz", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        // Selects all of "foo" and all of "bar" (chars 4..15), leaving
        // "baz" untouched.
        focused_frame_with_selection(&mut doc, &mut parser, 4..15, vec![shift_key_event(egui::Key::Tab)]);

        assert_eq!(doc.buffer.to_string(), "foo\nbar\nbaz");
    }

    #[test]
    fn shift_tab_over_a_selection_does_not_delete_the_selected_text() {
        // Regression test for the bug this feature fixes: egui's own
        // Shift+Tab deletes the entire selection before dedenting, so a
        // multi-line selection lost all its text, not just its leading
        // whitespace.
        let (_dir, mut doc) = open_fixture("    foo\n    bar", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        // "oo\n    ba" (chars 5..14) — a selection that starts and ends
        // mid-line, not on either line's boundary. Dedent still strips each
        // touched *line's* leading whitespace in full (not just whatever
        // fell inside the selection), same as every other editor's
        // block-dedent — so both lines lose their 4-space indent, and none
        // of "foo"/"bar" is lost the way egui's own delete-then-dedent
        // default would lose it.
        focused_frame_with_selection(&mut doc, &mut parser, 5..14, vec![shift_key_event(egui::Key::Tab)]);

        assert_eq!(doc.buffer.to_string(), "foo\nbar");
    }

    #[test]
    fn tab_over_a_selection_indents_every_line_instead_of_replacing_it() {
        let (_dir, mut doc) = open_fixture("foo\nbar", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        // All of "foo" and all of "bar" (chars 0..7).
        focused_frame_with_selection(&mut doc, &mut parser, 0..7, vec![key_event(egui::Key::Tab)]);

        assert_eq!(doc.buffer.to_string(), "    foo\n    bar");
    }

    #[test]
    fn tab_with_no_selection_inserts_a_literal_tab_in_tabs_mode() {
        // Guards the un-intercepted path: with `use_tabs: true`, a literal
        // tab already *is* the configured indent unit, so plain Tab with a
        // collapsed cursor (no selection) must keep falling through to
        // egui's own behavior rather than being intercepted.
        let (_dir, mut doc) = open_fixture("abc", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let tabs_mode = IndentSettings { use_tabs: true, width: 4 };
        focused_frame_with_indent_settings(&mut doc, &mut parser, tabs_mode, vec![key_event(egui::Key::Tab)]);

        assert_eq!(doc.buffer.to_string(), "\tabc");
    }

    #[test]
    fn tab_with_no_selection_inserts_spaces_in_spaces_mode() {
        // In "spaces" mode (the default), plain Tab with a collapsed cursor
        // must insert `width` spaces instead of the literal tab egui's own
        // `.code_editor()` handling would otherwise insert.
        let (_dir, mut doc) = open_fixture("abc", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let spaces_mode = IndentSettings { use_tabs: false, width: 4 };
        focused_frame_with_indent_settings(&mut doc, &mut parser, spaces_mode, vec![key_event(egui::Key::Tab)]);

        assert_eq!(doc.buffer.to_string(), "    abc");
    }

    #[test]
    fn tab_with_no_selection_respects_a_configured_width() {
        let (_dir, mut doc) = open_fixture("abc", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let two_space_mode = IndentSettings { use_tabs: false, width: 2 };
        focused_frame_with_indent_settings(&mut doc, &mut parser, two_space_mode, vec![key_event(egui::Key::Tab)]);

        assert_eq!(doc.buffer.to_string(), "  abc");
    }

    #[test]
    fn shift_tab_with_no_selection_is_left_to_egui_regardless_of_indent_mode() {
        // The new spaces-mode interception only ever fires for plain Tab —
        // Shift+Tab with no selection is (and remains) egui's own no-
        // selection dedent handling, untouched by `indent_settings`.
        let (_dir, mut doc) = open_fixture("    abc", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let spaces_mode = IndentSettings { use_tabs: false, width: 4 };
        focused_frame_with_indent_settings(&mut doc, &mut parser, spaces_mode, vec![shift_key_event(egui::Key::Tab)]);

        // Whatever egui's own no-selection Shift+Tab does, the buffer must
        // not have grown by a spaces-mode insertion — the interception must
        // not have fired.
        assert!(doc.buffer.to_string().len() <= "    abc".len());
    }

    fn command_key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }
    }

    #[test]
    fn ctrl_j_joins_the_current_line_with_the_next_one() {
        let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        // A fresh widget's default cursor sits somewhere on the first line
        // ("foo") — `join_lines` only cares which line the cursor is on,
        // not its exact column (see `join_lines_uses_cursor_position_
        // regardless_of_column_within_the_line` in `auto_edit.rs`).
        focused_frame(&mut doc, &mut parser, vec![command_key_event(egui::Key::J)]);

        assert_eq!(doc.buffer.to_string(), "foo bar");
    }

    #[test]
    fn ctrl_j_on_the_last_line_is_a_no_op() {
        let (_dir, mut doc) = open_fixture("foo", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        focused_frame(&mut doc, &mut parser, vec![command_key_event(egui::Key::J)]);

        assert_eq!(doc.buffer.to_string(), "foo");
    }

    fn command_shift_key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { command: true, shift: true, ..egui::Modifiers::NONE },
        }
    }

    #[test]
    fn ctrl_shift_g_generates_getter_and_setter_at_the_cursor() {
        let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        // A fresh widget's default cursor sits at char 0, which is still
        // "inside" the class_declaration spanning the whole file — see
        // `syntax::java_fields_in_enclosing_class`'s inclusive containment
        // check.
        focused_frame(&mut doc, &mut parser, vec![command_shift_key_event(egui::Key::G)]);

        let text = doc.buffer.to_string();
        assert!(text.contains("public int getX() {\n        return this.x;\n    }"));
        assert!(text.contains("public void setX(int x) {\n        this.x = x;\n    }"));
    }

    #[test]
    fn ctrl_shift_g_is_a_no_op_for_kotlin_files() {
        // Kotlin's `val`/`var` properties already are getters/setters;
        // generating explicit Java-shaped ones for them isn't idiomatic
        // (see `widget::show`'s comment on this shortcut), so the command
        // does nothing for a non-Java file.
        let (_dir, mut doc) = open_fixture("class Foo(val x: Int)\n", "Foo.kt");
        let mut parser = parsed(Language::Kotlin, &doc.buffer.to_string());
        let before = doc.buffer.to_string();

        focused_frame(&mut doc, &mut parser, vec![command_shift_key_event(egui::Key::G)]);

        assert_eq!(doc.buffer.to_string(), before);
    }

    #[test]
    fn ctrl_shift_g_is_a_no_op_when_the_class_has_no_instance_fields() {
        let (_dir, mut doc) = open_fixture("public class Empty {\n}\n", "Empty.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let before = doc.buffer.to_string();

        focused_frame(&mut doc, &mut parser, vec![command_shift_key_event(egui::Key::G)]);

        assert_eq!(doc.buffer.to_string(), before);
    }

    #[test]
    fn realistic_paste_reparses_and_highlights_correctly() {
        // A realistic paste: inserting a whole new, syntactically valid
        // method into a class body at a clean line boundary.
        let old_text = "public class Hello {\n}\n";
        let pasted = "    public String greet() {\n        return \"hi\";\n    }\n";
        let insert_at = old_text.find('}').unwrap();
        let mut new_text = old_text.to_string();
        new_text.insert_str(insert_at, pasted);

        let mut parser = IncrementalParser::new(Language::Java);
        parser.parse(old_text);
        let edit = syntax::diff_edit(old_text, &new_text);
        parser.reparse(&new_text, edit);

        let tree = parser.tree().unwrap();
        let spans = syntax::highlight_spans(tree, &new_text, Language::Java);

        let has_scope_over = |needle: &str, scope: syntax::Scope| {
            let start = new_text.find(needle).unwrap();
            let end = start + needle.len();
            spans
                .iter()
                .any(|(range, s)| *s == scope && range.start <= start && range.end >= end)
        };
        assert!(has_scope_over("class", syntax::Scope::Keyword));
        assert!(has_scope_over("greet", syntax::Scope::Function));
        assert!(has_scope_over("return", syntax::Scope::Keyword));
        assert!(has_scope_over("\"hi\"", syntax::Scope::String));

        // Cross-check against a from-scratch full parse of the same final
        // text: if incremental reparse after this paste produced the same
        // tree a fresh parse would, the highlighting can't be stale.
        let mut full_parser = IncrementalParser::new(Language::Java);
        full_parser.parse(&new_text);
        let full_spans = syntax::highlight_spans(full_parser.tree().unwrap(), &new_text, Language::Java);
        assert_eq!(spans, full_spans);
    }

    #[test]
    fn multi_cursor_typed_edit_applies_at_every_active_cursor() {
        let (_dir, mut doc) = open_fixture("abcde", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        // Primary cursor starts at 0 (a fresh widget's default cursor, from
        // the warm-up frame `focused_frame_with_extra_selections` runs
        // before setting these); the two extras sit at char indices 2 and 4.
        focused_frame_with_extra_selections(
            &mut doc,
            &mut parser,
            vec![2..2, 4..4],
            vec![egui::Event::Text("Y".to_string())],
        );

        assert_eq!(doc.buffer.to_string(), "YabYcdYe");
        assert_eq!(doc.extra_selections, vec![4..4, 7..7]);
    }

    #[test]
    fn arrow_key_collapses_extra_selections() {
        let (_dir, mut doc) = open_fixture("abcde", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        doc.extra_selections = vec![2..2, 4..4];

        focused_frame(&mut doc, &mut parser, vec![key_event(egui::Key::ArrowLeft)]);

        assert!(doc.extra_selections.is_empty());
        // Arrow keys just navigate — the buffer itself is untouched.
        assert_eq!(doc.buffer.to_string(), "abcde");
    }

    #[test]
    fn non_intercepted_mutating_key_collapses_extra_selections_via_safety_net() {
        let (_dir, mut doc) = open_fixture("abc", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        // a genuine one-caret Vec<Range<usize>>, not a range of a Vec
        #[allow(clippy::single_range_in_vec_init)]
        let one_caret = vec![1..1];
        doc.extra_selections = one_caret;

        // Tab isn't in the intercepted-event set, so it reaches egui's own
        // single-cursor logic (code editors call `.lock_focus(true)`, which
        // makes Tab insert a literal tab character instead of moving focus)
        // and edits the primary cursor alone — the safety net must then
        // notice `extra_selections` is now stale and clear it.
        focused_frame(&mut doc, &mut parser, vec![key_event(egui::Key::Tab)]);

        assert!(doc.extra_selections.is_empty());
        assert_eq!(doc.buffer.to_string(), "\tabc");
    }
}
