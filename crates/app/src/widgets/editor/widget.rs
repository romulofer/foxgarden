use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Event, FontId, Galley, Key};
use fg_core::{Document, Language, Project};
use ropey::Rope;
use syntax::IncrementalParser;

use super::auto_edit::{
    apply_auto_indent, apply_auto_pair, convert_selection_case, duplicate_line, indent_selected_lines, is_pairable,
    join_lines, move_line_down, move_line_up, smart_home_target, sort_lines, toggle_line_comments, unique_lines,
    wrap_selection, CaseConversion,
};
use super::codegen::{
    self, generate_accessors, insert_at_class_end, AccessorKind, GenerateAccessorsDialog, GenerateMethodDialog,
    GenerateMethodKind, OverrideMethodDialog,
};
use super::context_menu;
#[cfg(test)]
use super::context_menu::synthetic_shortcut;
use super::multi_cursor::{self, MultiEditOp};
use super::painting::{
    paint_bracket_match, paint_diagnostics, paint_extra_selections, paint_indent_guides, paint_line_numbers,
    paint_occurrence_highlights, paint_sticky_scroll, paint_whitespace,
};
use super::templates::{self, expand, find_template, word_before_cursor};
use super::text_offset::{byte_to_char, char_to_byte};
use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::style::theme;
use crate::style::view::ViewSettings;

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

/// Persisted (via `egui::Context`'s per-frame-surviving temp storage, same
/// mechanism `CachedLayout` above uses) across `Ctrl+W`/`Ctrl+Shift+W`
/// presses: `history` is every selection expand has grown *from*, most
/// recent last, so shrink can restore exactly the one expand just grew out
/// of rather than independently recomputing a smaller node (which could
/// land on a different, same-size candidate than where the user just was).
/// `last_applied` is whichever selection expand/shrink itself last set —
/// compared against the actual current selection each press to detect an
/// unrelated change in between (a click, an edit, arrow-key navigation)
/// that should invalidate `history` rather than let a stale shrink target
/// silently apply.
#[derive(Clone, Default)]
struct SelectionExpandState {
    history: Vec<Range<usize>>,
    last_applied: Option<Range<usize>>,
}

/// Horizontal breathing room on each side of the line-number gutter's
/// digits, so they don't crowd the window edge or the text they precede.
const GUTTER_PADDING: f32 = 8.0;

/// Most enclosing scopes sticky scroll pins at once — matches VSCode's default
/// `stickyScroll.maxLineCount`. Deep nesting keeps the *outermost* scopes,
/// since losing the outer context (which class/method am I in) is more
/// disorienting than losing an inner block header.
const STICKY_MAX_DEPTH: usize = 5;

/// Given the enclosing-scope header line indices (outermost-first, as
/// `syntax::enclosing_scope_starts` returns them once mapped from bytes to
/// lines) and the current top visible line, picks which headers sticky scroll
/// actually pins: only those **strictly above** the viewport top — a scope
/// whose own header is still on screen doesn't need pinning — capped at
/// `max_depth`, keeping the outermost (the front of the list). Pure and
/// tree-free so it's unit-testable without a frame, the same split
/// `selection.rs` uses to test its Range bookkeeping apart from the tree walk.
fn sticky_headers_to_pin(scope_lines: &[usize], top_line: usize, max_depth: usize) -> Vec<usize> {
    scope_lines.iter().copied().filter(|&line| line < top_line).take(max_depth).collect()
}

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
pub(super) fn apply_edit(doc: &mut Document, parser: &mut Option<IncrementalParser>, old_text: &str, new_text: &str) {
    doc.buffer = Rope::from_str(new_text);
    if let Some(parser) = parser.as_mut() {
        let edit = syntax::diff_edit(old_text, new_text);
        parser.reparse(new_text, edit);
        doc.diagnostics = syntax::syntax_errors(parser.tree().expect("just reparsed"));
    }
}

/// Whether `event` could mutate the buffer if left in the queue — either via
/// egui's own built-in `TextEdit` editing (typed text, paste, cut,
/// `Backspace`/`Delete`/`Tab`/`Enter`, the emacs-style `Ctrl+H`/`K`/`U`
/// deletions, undo/redo on `Z`/`Y`) or via one of this file's own
/// shortcut-triggered edits (`Ctrl+J` join, `Ctrl+Shift+G` generate,
/// `Ctrl+Shift+U`/`L` case conversion, `Ctrl+/` comment toggle, `Alt+↑`/`↓`
/// move/duplicate line). Deliberately an allowlist of *keys* that get
/// blocked outright rather than an attempt to replicate every modifier
/// condition each mutating path actually checks — simpler to keep correct
/// as new shortcuts are added, and none of these keys are needed for
/// read-only navigation (arrows, Home/End, PageUp/Down, Escape, and
/// pointer/scroll events are untouched, since they're not `Event::Key` at
/// all).
///
/// `Key::W` is deliberately *not* in this list even though bare `Ctrl+W` is
/// also one of egui's built-in emacs-style deletions (`check_for_mutating_
/// key_press`'s "delete previous word") — `show`'s own `Ctrl+W`/`Ctrl+Shift+
/// W` semantic-selection expand/shrink always consumes that event itself
/// before `TextEdit::show()` ever sees it (regardless of `doc.read_only`,
/// the same as `Ctrl+D` staying available on a read-only file), so it never
/// reaches egui's mutating handling to strip in the first place.
fn is_mutating_event(event: &Event) -> bool {
    match event {
        Event::Text(_) | Event::Paste(_) | Event::Cut => true,
        Event::Key { key: Key::ArrowUp | Key::ArrowDown, pressed: true, modifiers, .. } if modifiers.alt => true,
        Event::Key { key, pressed: true, .. } => matches!(
            key,
            Key::Backspace
                | Key::Delete
                | Key::Tab
                | Key::Enter
                | Key::H
                | Key::K
                | Key::U
                | Key::L
                | Key::Y
                | Key::Z
                | Key::J
                | Key::G
                | Key::Slash
        ),
        _ => false,
    }
}

/// Drops every mutating event (`is_mutating_event`) from this frame's queue
/// — `doc.read_only`'s entire enforcement mechanism, see `show`'s call site.
fn strip_mutating_events(ui: &egui::Ui) {
    ui.input_mut(|i| i.events.retain(|e| !is_mutating_event(e)));
}

/// Renders `doc`'s buffer as an editable text area, keeping `parser`'s
/// incremental tree in sync with edits (SPEC.md sections 5.4 and 5.5).
/// `parser` is `None` for files with no recognized language (anything other
/// than `.java`/`.kt`) — such files still open and edit normally, they just
/// get plain rendering and no diagnostics; auto-pair/auto-indent/multi-cursor
/// are language-agnostic and keep working regardless.
#[expect(clippy::too_many_arguments, reason = "each parameter is independently threaded editor-frame state, not a bundle waiting to be a struct — see TECHNICAL_DEBT.md #5 (the 'splitting widget.rs further' entry) for why bundling into a struct isn't a clear win here, and context_menu::show_context_menu's own allowance for the same shape")]
pub fn show(
    ui: &mut egui::Ui,
    doc: &mut Document,
    parser: &mut Option<IncrementalParser>,
    editor_font: EditorFont,
    font_size: f32,
    indent_settings: IndentSettings,
    view_settings: ViewSettings,
    generate_request: Option<AccessorKind>,
    generate_dialog: &mut Option<GenerateAccessorsDialog>,
    generate_method_request: Option<GenerateMethodKind>,
    generate_method_dialog: &mut Option<GenerateMethodDialog>,
    project: Option<&Project>,
    override_method_request: bool,
    override_method_dialog: &mut Option<OverrideMethodDialog>,
    case_conversion_request: Option<CaseConversion>,
    sort_lines_request: bool,
    unique_lines_request: bool,
    last_error: &mut Option<String>,
    pending_input: &mut Vec<Event>,
    cached_clipboard_text: &mut Option<String>,
) {
    // Undo/Redo/Select All from the right-click menu (below) can't be
    // driven directly — they're handled entirely *inside* egui's own
    // `TextEdit::show()`, in response to real input events, and that
    // widget's undo history is private state with no other API to trigger
    // it. A click queues the matching key event into `pending_input`
    // instead; since a fresh `InputState` is rebuilt from the platform's
    // raw input every frame (nothing pushed into it survives on its own),
    // draining the queue back in *before* `TextEdit::show()` runs below —
    // right here, at the top of the very next frame after the click — is
    // what makes the queued event arrive as genuine input for that frame,
    // one frame later than a click on anything else in the menu. That's
    // imperceptible, and it means Ctrl+Z and the menu item share the exact
    // same undo stack instead of risking two that quietly disagree.
    if !pending_input.is_empty() {
        let events = std::mem::take(pending_input);
        ui.ctx().input_mut(|i| i.events.extend(events));
    }

    // `doc.read_only` blocks every edit path below in one place, rather than
    // adding an individual guard at each of them: stripping the mutating
    // events out of this frame's queue up front means every one of this
    // file's own shortcut interceptions (which all key off specific events
    // still being present in that queue) and `TextEdit::show()`'s own
    // built-in editing both simply find nothing to act on. Read-only
    // navigation — arrow keys, Home/End, PageUp/Down, Escape, click-to-
    // position, drag-to-select, scrolling, Copy, Ctrl+D occurrence select —
    // is untouched, since none of those events are in the blocked set.
    // Deliberately not done via `TextEdit::interactive(false)`: that egui
    // builtin also disables selection entirely ("you cannot interact with
    // the text (neither edit or select it)"), which would break exactly the
    // read-only browsing this feature is meant to still allow.
    if doc.read_only {
        strip_mutating_events(ui);
    }
    let generate_request = if doc.read_only { None } else { generate_request };
    let generate_method_request = if doc.read_only { None } else { generate_method_request };
    let override_method_request = !doc.read_only && override_method_request;
    let case_conversion_request = if doc.read_only { None } else { case_conversion_request };

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

                if let Some(opener) = opener
                    && let Some((wrapped, sel_start, sel_end)) =
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

    // Tab/Shift+Tab while a selection is active should indent/dedent every
    // line the selection touches, not replace the selection the way egui's
    // `TextEdit` does by default (see `indent_selected_lines`'s doc comment:
    // egui deletes the *entire* selection first, then for Shift+Tab dedents
    // only the single line the resulting cursor lands on). Same
    // pull-it-out-of-the-queue-before-`TextEdit`-sees-it approach as the
    // wrap-selection interception above, and skipped for the same reason
    // whenever multi-cursor is active.
    //
    // Plain Tab with *no* selection is handled here too, for two things:
    // live-template expansion (a trigger word like "sout" completing into
    // its snippet), and — in "spaces" mode — indentation. egui's own
    // `TextEdit` (a `.code_editor()`, which sets `lock_focus`) inserts a
    // literal `\t` for a bare Tab keypress, which would silently ignore
    // both a matched template and an `indent_settings.use_tabs == false`
    // choice. A template match is checked (and, if found, wins) regardless
    // of `use_tabs` — expansion is orthogonal to indentation style — so
    // only the *indentation* half of this is skipped once `use_tabs` is
    // set (a literal tab already *is* that setting's unit, so egui's
    // default is exactly right there and needs no interception). Shift+Tab
    // is left to egui's own no-selection handling either way, same as
    // before either feature existed.
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
                } else if !ui.input(|i| i.modifiers.shift) {
                    let word_range = word_before_cursor(&old_text, range.start.0);
                    let word_start_byte = char_to_byte(&old_text, word_range.start);
                    let word_end_byte = char_to_byte(&old_text, word_range.end);
                    let templates = match doc.language {
                        Some(Language::Java) => templates::JAVA_TEMPLATES,
                        Some(Language::Kotlin) => templates::KOTLIN_TEMPLATES,
                        _ => &[],
                    };
                    let template_body = find_template(templates, &old_text[word_start_byte..word_end_byte]);

                    if template_body.is_some() || !indent_settings.use_tabs {
                        let removed =
                            take_event(ui, |e| matches!(e, Event::Key { key: Key::Tab, pressed: true, .. }));

                        if removed.is_some() {
                            let (new_full_text, new_cursor) = if let Some(body) = template_body {
                                expand(&old_text, word_range, body)
                            } else {
                                let unit = indent_settings.unit();
                                let byte = char_to_byte(&old_text, range.start.0);
                                let inserted = format!("{}{unit}{}", &old_text[..byte], &old_text[byte..]);
                                (inserted, range.start.0 + unit.chars().count())
                            };
                            apply_edit(doc, parser, &old_text, &new_full_text);
                            manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
                            old_text = new_full_text.clone();
                            text = new_full_text;
                        }
                    }
                }
            }
        }
    }

    // Alt+ArrowUp/Down move the current line up/down; Alt+Shift+ArrowUp/
    // Down duplicate it (inserted below, cursor landing on the original
    // for Up or the duplicate for Down — matching a widely-used editor
    // convention). Pre-apply, same interception shape as Tab above: egui's
    // own `TextEdit` would otherwise also move/extend-select the cursor
    // via its native arrow-key handling this same frame, competing with
    // this transform.
    if !multi_cursor_active_at_start {
        let alt_arrow = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Key { key: key @ (Key::ArrowUp | Key::ArrowDown), pressed: true, modifiers, .. }
                    if modifiers.alt =>
                {
                    Some((*key, modifiers.shift))
                }
                _ => None,
            })
        });

        if let Some((key, shift)) = alt_arrow {
            let prior_cursor = egui::text_edit::TextEditState::load(ui.ctx(), widget_id)
                .and_then(|state| state.cursor.char_range())
                .map(|range| range.primary.index.0);

            if let Some(cursor_char) = prior_cursor {
                let removed = take_event(ui, |e| {
                    matches!(e, Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.alt && modifiers.shift == shift)
                });

                if removed.is_some() {
                    let outcome = match (key, shift) {
                        (Key::ArrowUp, false) => move_line_up(&old_text, cursor_char),
                        (Key::ArrowDown, false) => move_line_down(&old_text, cursor_char),
                        (Key::ArrowUp, true) => {
                            let (duplicated, _) = duplicate_line(&old_text, cursor_char);
                            Some((duplicated, cursor_char))
                        }
                        (Key::ArrowDown, true) => {
                            let (duplicated, cursor_on_duplicate) = duplicate_line(&old_text, cursor_char);
                            Some((duplicated, cursor_on_duplicate))
                        }
                        _ => None,
                    };
                    if let Some((new_full_text, new_cursor)) = outcome {
                        apply_edit(doc, parser, &old_text, &new_full_text);
                        manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
                        old_text = new_full_text.clone();
                        text = new_full_text;
                    }
                }
            }
        }
    }

    // Home/Shift+Home: "smart home" — jump to the line's first non-
    // whitespace character, or column 0 if the cursor is already there
    // (see `smart_home_target`'s doc comment for the exact toggle rule).
    // Pre-apply, same interception shape as Tab/Alt+Arrow above: egui's own
    // `TextEdit` would otherwise move the cursor to column 0 unconditionally
    // via its native Home handling, which this needs to override. Doesn't
    // call `apply_edit` — Home never changes the buffer, only where the
    // cursor points into it.
    if !multi_cursor_active_at_start {
        let home_pressed =
            ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Key { key: Key::Home, pressed: true, .. })));

        if home_pressed {
            let prior_range =
                egui::text_edit::TextEditState::load(ui.ctx(), widget_id).and_then(|state| state.cursor.char_range());

            if let Some(range) = prior_range {
                let shift = ui.input(|i| i.modifiers.shift);
                let removed = take_event(ui, |e| matches!(e, Event::Key { key: Key::Home, pressed: true, .. }));

                if removed.is_some() {
                    let target = smart_home_target(&old_text, range.primary.index.0);
                    manual_cursor_range = Some(if shift {
                        CCursorRange { primary: CCursor::new(target), secondary: range.secondary, h_pos: None }
                    } else {
                        CCursorRange::one(CCursor::new(target))
                    });
                }
            }
        }
    }

    // Ctrl+/: toggle `//` line comments on every line the selection
    // touches (or just the cursor's line, for a collapsed selection).
    // Pre-apply, same interception shape as Tab/Alt+Arrow above — and for
    // an extra reason beyond "egui's own handling would otherwise compete
    // with it": reading the *persisted* selection from before this frame's
    // `TextEdit::show()` call is what makes an externally set/dragged
    // selection actually usable here. `output.cursor_range` (the post-show
    // value simpler shortcuts like Ctrl+D/Ctrl+J read) doesn't reliably
    // agree with it — the same class of caveat `focused_frame_with_
    // selection`'s doc comment already documents for a different case.
    if !multi_cursor_active_at_start {
        let ctrl_slash_pressed = ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, Event::Key { key: Key::Slash, pressed: true, modifiers, .. } if modifiers.command))
        });

        if ctrl_slash_pressed {
            let prior_selection = egui::text_edit::TextEditState::load(ui.ctx(), widget_id)
                .and_then(|state| state.cursor.char_range())
                .map(|range| range.as_sorted_char_range());

            if let Some(range) = prior_selection {
                let removed = take_event(ui, |e| {
                    matches!(e, Event::Key { key: Key::Slash, pressed: true, modifiers, .. } if modifiers.command)
                });

                if removed.is_some() {
                    let (toggled, sel_start, sel_end) = toggle_line_comments(&old_text, range.start.0, range.end.0);
                    apply_edit(doc, parser, &old_text, &toggled);
                    manual_cursor_range = Some(CCursorRange::two(CCursor::new(sel_start), CCursor::new(sel_end)));
                    old_text = toggled.clone();
                    text = toggled;
                }
            }
        }
    }

    // Ctrl+Shift+U/L (uppercase/lowercase — the two most commonly wanted)
    // or a Tools menu click (`case_conversion_request`, which also offers
    // Title Case) convert the selected text's case. Same pre-apply,
    // persisted-selection-reading shape as Ctrl+/ above, and for the same
    // reason. Unlike every other transform in this file, there's no
    // sensible fallback to "the cursor's line" — case conversion needs an
    // actual selection — so a request with nothing (or an empty range)
    // selected reports that through `last_error` instead of silently
    // doing nothing.
    if !multi_cursor_active_at_start {
        let command_shift_held = ui.input(|i| i.modifiers.command && i.modifiers.shift);
        let keyboard_case_request = if command_shift_held && ui.input(|i| i.key_pressed(Key::U)) {
            Some(CaseConversion::Upper)
        } else if command_shift_held && ui.input(|i| i.key_pressed(Key::L)) {
            Some(CaseConversion::Lower)
        } else {
            None
        };

        if let Some(case) = case_conversion_request.or(keyboard_case_request) {
            let prior_selection = egui::text_edit::TextEditState::load(ui.ctx(), widget_id)
                .and_then(|state| state.cursor.char_range())
                .map(|range| range.as_sorted_char_range());

            match prior_selection {
                Some(range) if !range.is_empty() => {
                    if let Some(key) = keyboard_case_request.map(|_| if case == CaseConversion::Upper { Key::U } else { Key::L }) {
                        take_event(ui, |e| {
                            matches!(e, Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.command && modifiers.shift)
                        });
                    }
                    if let Some((converted, sel_start, sel_end)) =
                        convert_selection_case(&old_text, range.start.0, range.end.0, case)
                    {
                        apply_edit(doc, parser, &old_text, &converted);
                        manual_cursor_range = Some(CCursorRange::two(CCursor::new(sel_start), CCursor::new(sel_end)));
                        old_text = converted.clone();
                        text = converted;
                    }
                }
                _ => {
                    *last_error = Some("Select some text first, then try again.".to_string());
                }
            }
        }
    }

    // A Tools menu click (`sort_lines_request`/`unique_lines_request`) sorts
    // or dedupes the lines the selection touches (or just the cursor's
    // line, for a collapsed selection — trivially a no-op for both, unlike
    // case conversion above, so unlike that block there's no "select
    // something first" error case to report). Same pre-apply,
    // persisted-selection-reading shape as Ctrl+/ and case conversion
    // above, and for the same reason. Neither request is keyboard-driven
    // (no shortcut, Tools menu only, like Generate Getters/Setters), so
    // `doc.read_only` needs an explicit check here — the raw-event
    // stripping above only blocks *keyboard* paths, and these two are
    // plain `bool` parameters instead.
    if !multi_cursor_active_at_start && !doc.read_only && (sort_lines_request || unique_lines_request) {
        let prior_selection = egui::text_edit::TextEditState::load(ui.ctx(), widget_id)
            .and_then(|state| state.cursor.char_range())
            .map(|range| range.as_sorted_char_range());

        if let Some(range) = prior_selection {
            let (transformed, sel_start, sel_end) = if sort_lines_request {
                sort_lines(&old_text, range.start.0, range.end.0)
            } else {
                unique_lines(&old_text, range.start.0, range.end.0)
            };
            apply_edit(doc, parser, &old_text, &transformed);
            manual_cursor_range = Some(CCursorRange::two(CCursor::new(sel_start), CCursor::new(sel_end)));
            old_text = transformed.clone();
            text = transformed;
        }
    }

    // Ctrl+W/Ctrl+Shift+W: expand/shrink the selection by syntax node
    // ("semantic selection"). Intercepted before `TextEdit::show()` runs —
    // same pre-apply shape as Ctrl+/ above — even though neither mutates
    // the buffer: bare Ctrl+W is otherwise egui's own emacs-style "delete
    // previous word" (`check_for_mutating_key_press`'s `Key::W` arm;
    // `Ctrl+Backspace` already covers deleting the previous word
    // redundantly), so this needs to consume the event itself to keep it
    // from falling through to that unrelated, destructive default.
    // Critically, the event is consumed *unconditionally* on every
    // `Ctrl+W`/`Ctrl+Shift+W` press, not just when a tree exists to act on
    // — `is_mutating_event` deliberately excludes `Key::W` from what
    // `doc.read_only` strips (see that function's doc comment), on exactly
    // this assumption: if a file with no parsed tree (no Java/Kotlin) let
    // the event fall through un-consumed here, it would still reach egui's
    // own mutating handling below and silently break read-only protection
    // for that file. So this only *acts* on the selection when a tree
    // exists (a no-op otherwise, same as bracket-pair highlighting above),
    // but always eats the keystroke either way. Non-mutating, so unlike
    // sort/unique lines above this isn't gated on `doc.read_only` beyond
    // that: it's pure navigation, the same as Ctrl+D staying available on
    // a read-only file.
    if !multi_cursor_active_at_start {
        let modifiers = ui.input(|i| i.modifiers);
        let w_pressed = ui.input(|i| i.key_pressed(Key::W)) && modifiers.command;

        if w_pressed {
            let shrink = modifiers.shift;
            take_event(ui, |e| {
                matches!(e, Event::Key { key: Key::W, pressed: true, modifiers, .. } if modifiers.command)
            });

            if let Some(tree) = parser.as_ref().and_then(|p| p.tree())
                && let Some(prior_range) = egui::text_edit::TextEditState::load(ui.ctx(), widget_id)
                .and_then(|state| state.cursor.char_range())
                .map(|range| range.as_sorted_char_range())
            {
                let history_id = egui::Id::new(("selection_expand_history", id_salt.as_str()));
                let mut expand_state = ui
                    .ctx()
                    .data(|d| d.get_temp::<SelectionExpandState>(history_id))
                    .unwrap_or_default();

                let current = prior_range.start.0..prior_range.end.0;
                if expand_state.last_applied.as_ref() != Some(&current) {
                    expand_state.history.clear();
                }

                let new_range = if shrink {
                    expand_state.history.pop()
                } else {
                    // A collapsed cursor starts from the word touching it
                    // (`Ctrl+D`'s own starting point) rather than jumping
                    // straight to the smallest AST node — a bare keyword/
                    // punctuation token under the cursor would otherwise
                    // make the very first press jump much further than
                    // "select the word here."
                    let probe = if current.is_empty() {
                        let word = multi_cursor::word_range_at(&old_text, current.start);
                        if word.is_empty() { current.clone() } else { word }
                    } else {
                        current.clone()
                    };
                    let probe_start = char_to_byte(&old_text, probe.start);
                    let probe_end = char_to_byte(&old_text, probe.end);
                    syntax::expand_selection(tree, probe_start, probe_end)
                        .map(|r| byte_to_char(&old_text, r.start)..byte_to_char(&old_text, r.end))
                };

                if let Some(new_range) = new_range {
                    if !shrink {
                        expand_state.history.push(current);
                    }
                    expand_state.last_applied = Some(new_range.clone());
                    manual_cursor_range =
                        Some(CCursorRange::two(CCursor::new(new_range.start), CCursor::new(new_range.end)));
                }

                ui.ctx().data_mut(|d| d.insert_temp(history_id, expand_state));
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
        // frames and defeat the cache. `ViewSettings::word_wrap` off means
        // "don't wrap at all," folded into this same value (rather than a
        // separate `LayoutCacheKey` field) so the existing
        // `wrap_width_bits` key naturally invalidates the cache on toggle —
        // `f32::INFINITY`'s bit pattern differs from any finite width, so a
        // stale wrapped galley can never be mistaken for the unwrapped one.
        let wrap_width = if view_settings.word_wrap { wrap_width.round() } else { f32::INFINITY };

        let tree_and_language = parser.as_ref().and_then(|p| p.tree().map(|tree| (tree, p.language())));
        let key = LayoutCacheKey {
            content_hash: hash_source(source),
            language: tree_and_language.as_ref().map(|(_, language)| *language),
            dark_mode,
            wrap_width_bits: wrap_width.to_bits(),
            font_size_bits: font_size.to_bits(),
        };

        if let Some(cached) = ui.ctx().data(|d| d.get_temp::<CachedLayout>(layout_cache_id))
            && cached.key == key {
                return cached.galley;
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

    // Alt+Click adds a bare secondary cursor at the click position without
    // disturbing the primary one — captured here, *before* `TextEdit::
    // show()` runs, since that call's own internal click handling
    // unconditionally moves the primary cursor to wherever was just
    // clicked (Alt held or not — it doesn't know the difference). Holding
    // onto the primary selection as it stood right before that happens is
    // what lets the block below restore it afterward. `i.pointer.
    // primary_clicked()` is the cheap pre-check (mirrors `wrap_selection`'s
    // `has_candidate_keystroke` above: don't pay for a `TextEditState`
    // load on every idle/non-click frame) — the actual click position
    // comes from `output.response.interact_pointer_pos()` once `output`
    // exists, below.
    let alt_click_prior_primary = if ui.input(|i| i.modifiers.alt && i.pointer.primary_clicked()) {
        egui::text_edit::TextEditState::load(ui.ctx(), widget_id).and_then(|state| state.cursor.char_range())
    } else {
        None
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

    // Completes the Alt+Click interception begun above `output`: place a
    // bare secondary cursor at the click position, then restore the
    // primary cursor to `alt_click_prior_primary` (undoing the move
    // `TextEdit::show()`'s own click handling just made) — same
    // `manual_cursor_range` override mechanism every other post-`output`
    // cursor adjustment in this file uses. `interact_pointer_pos()` is
    // `None` if `alt_click_prior_primary` was captured but the click
    // actually landed outside the `TextEdit`'s own bounds (the gutter,
    // say), which correctly no-ops this rather than adding a caret at a
    // stale/wrong position.
    if let Some(prior_primary) = alt_click_prior_primary
        && let Some(click_pos) = output.response.interact_pointer_pos()
    {
        let local = click_pos - output.galley_pos;
        let click_char = output.galley.cursor_from_pos(local).index.0;

        if !doc.extra_selections.contains(&(click_char..click_char)) {
            doc.extra_selections.push(click_char..click_char);
        }
        manual_cursor_range = Some(prior_primary);

        // Must be removed from the queue, not just acted on: the
        // multi-cursor collapse check further below
        // (`is_multi_cursor_collapse_event`) treats *any* primary-button
        // click as "user clicked somewhere, drop back to single-cursor
        // mode" — if this frame's click event were left in the queue, that
        // check would immediately wipe the very extra selection just
        // pushed above, in this same frame.
        take_event(ui, |e| {
            matches!(e, Event::PointerButton { pressed: true, button: egui::PointerButton::Primary, .. })
        });
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


    // Ctrl+Shift+G (always `Both`) or a Tools menu click (`generate_request`,
    // already narrowed to `Getters`/`Setters`/`Both`) generate accessors for
    // the file's classes. Java-only — Kotlin's `val`/`var` properties
    // already *are* getters/setters, so generating explicit ones for them
    // isn't the idiomatic move a Java accessor-boilerplate command is.
    // Every non-applicable case sets `last_error` instead of silently doing
    // nothing — a request that visibly changes nothing (wrong file type, no
    // matching fields) is easy to mistake for "the shortcut doesn't work"
    // otherwise. A single eligible class generates immediately for every
    // one of its fields; more than one opens `generate_dialog` so the user
    // picks which class (then which fields) — see
    // `codegen::GenerateAccessorsDialog`.
    let keyboard_requested_accessors =
        (ui.input(|i| i.key_pressed(Key::G)) && modifiers.command && modifiers.shift).then_some(AccessorKind::Both);
    if let Some(kind) = generate_request.or(keyboard_requested_accessors) {
        if doc.language != Some(Language::Java) {
            *last_error = Some("Generate getters/setters only works for Java files.".to_string());
        } else if let Some(tree) = parser.as_ref().and_then(|p| p.tree()) {
            let text_now = doc.buffer.to_string();
            let classes = syntax::java_classes_with_fields(tree, &text_now);
            match classes.len() {
                0 => *last_error = Some("No class fields found in this file.".to_string()),
                1 => {
                    let generated = generate_accessors(&classes[0].fields, &indent_settings.unit(), kind);
                    if generated.is_empty() {
                        // Only reachable for `AccessorKind::Setters` when
                        // every field found is `final`.
                        *last_error = Some("Nothing to generate: every field here is final.".to_string());
                    } else {
                        let (inserted, new_cursor) = insert_at_class_end(&text_now, classes[0].insertion_byte, &generated);
                        apply_edit(doc, parser, &text_now, &inserted);
                        manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
                    }
                }
                _ => *generate_dialog = Some(GenerateAccessorsDialog::new(classes, kind)),
            }
        } else {
            *last_error = Some("Couldn't generate accessors: no syntax tree available yet.".to_string());
        }
    }

    // Gated on `generate_dialog` already being open, not just deferred to
    // `codegen::show_generate_accessors_dialog`'s own internal check —
    // every other frame (the overwhelming majority: the dialog is only
    // open right after a multi-class Generate request) would otherwise pay
    // for cloning the whole buffer into `doc_text_for_dialog` just to
    // immediately discover there was nothing to show.
    if generate_dialog.is_some() {
        let doc_text_for_dialog = doc.buffer.to_string();
        if let Some(outcome) =
            codegen::show_generate_accessors_dialog(ui, generate_dialog, &doc_text_for_dialog, &indent_settings.unit())
        {
            match outcome {
                Ok((inserted, new_cursor)) => {
                    apply_edit(doc, parser, &doc_text_for_dialog, &inserted);
                    manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
                }
                Err(message) => *last_error = Some(message),
            }
        }
    }

    // A Tools menu click (`generate_method_request` — Constructor/
    // toString/equals+hashCode; no keyboard shortcut, same as Generate
    // Getters/Setters alone) generates a whole-method-body template for
    // the file's classes — same Java-only gating and single-vs-multi-class
    // picker shape as accessor generation above, just never hitting the
    // "nothing to generate" case (`codegen::generate_method`'s three
    // templates are all valid Java even with zero fields selected).
    if let Some(kind) = generate_method_request {
        if doc.language != Some(Language::Java) {
            *last_error = Some("Generate Constructor/toString/equals() only works for Java files.".to_string());
        } else if let Some(tree) = parser.as_ref().and_then(|p| p.tree()) {
            let text_now = doc.buffer.to_string();
            let classes = syntax::java_classes_with_fields(tree, &text_now);
            match classes.len() {
                0 => *last_error = Some("No class fields found in this file.".to_string()),
                1 => {
                    let generated =
                        codegen::generate_method(&classes[0].name, &classes[0].fields, &indent_settings.unit(), kind);
                    let (inserted, new_cursor) = insert_at_class_end(&text_now, classes[0].insertion_byte, &generated);
                    apply_edit(doc, parser, &text_now, &inserted);
                    manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
                }
                _ => *generate_method_dialog = Some(GenerateMethodDialog::new(classes, kind)),
            }
        } else {
            *last_error = Some("Couldn't generate: no syntax tree available yet.".to_string());
        }
    }

    if generate_method_dialog.is_some() {
        let doc_text_for_dialog = doc.buffer.to_string();
        if let Some((inserted, new_cursor)) = codegen::show_generate_method_dialog(
            ui,
            generate_method_dialog,
            &doc_text_for_dialog,
            &indent_settings.unit(),
        ) {
            apply_edit(doc, parser, &doc_text_for_dialog, &inserted);
            manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
        }
    }

    // A Tools menu click (`override_method_request` — no keyboard shortcut,
    // same as the other Tools-only generation requests) looks up the
    // superclass/interface of whichever class the cursor sits in
    // (`syntax::enclosing_class` + `syntax::superclass_name`), finds *that*
    // type's source file elsewhere in the open project
    // (`codegen::find_java_file_by_stem` — in-project supertypes only, a
    // JDK/library type has no file to find and correctly falls through to
    // the last error case), and offers its not-already-overridden methods
    // in a picker. Every non-applicable step reports specifically why
    // through `last_error`, same reasoning as accessor/method generation
    // above: a silent no-op is easy to mistake for "the shortcut doesn't
    // work."
    if override_method_request {
        if doc.language != Some(Language::Java) {
            *last_error = Some("Override Method only works for Java files.".to_string());
        } else if let Some(tree) = parser.as_ref().and_then(|p| p.tree()) {
            let text_now = doc.buffer.to_string();
            let cursor_byte = output.cursor_range.map(|r| char_to_byte(&text_now, r.primary.index.0));
            let enclosing = cursor_byte.and_then(|c| syntax::enclosing_class(tree, &text_now, c));

            match enclosing {
                None => *last_error = Some("Place the cursor inside a class to override a method.".to_string()),
                Some((class_name, insertion_byte)) => match syntax::superclass_name(tree, &text_now, &class_name) {
                    None => {
                        *last_error =
                            Some(format!("{class_name} has no superclass or interface to override methods from."));
                    }
                    Some(super_name) => {
                        let super_path = project.and_then(|p| codegen::find_java_file_by_stem(&p.tree, &super_name));
                        match super_path {
                            None => {
                                *last_error = Some(format!(
                                    "Override Method only looks up superclasses in this project (couldn't find {super_name}.java)."
                                ));
                            }
                            Some(super_path) => match std::fs::read_to_string(&super_path) {
                                Err(err) => {
                                    *last_error = Some(format!("failed to read {}: {err}", super_path.display()));
                                }
                                Ok(super_source) => {
                                    let mut super_parser = IncrementalParser::new(Language::Java);
                                    let super_tree = super_parser.parse(&super_source);
                                    let inherited = syntax::methods_in_type(super_tree, &super_source, &super_name);
                                    let already_here = syntax::methods_in_type(tree, &text_now, &class_name);
                                    let candidates: Vec<_> = inherited
                                        .into_iter()
                                        .filter(|m| {
                                            !already_here
                                                .iter()
                                                .any(|existing| existing.name == m.name && existing.params.len() == m.params.len())
                                        })
                                        .collect();

                                    if candidates.is_empty() {
                                        *last_error = Some(format!(
                                            "No overridable methods found on {super_name} (or they're all already overridden)."
                                        ));
                                    } else {
                                        *override_method_dialog = Some(OverrideMethodDialog::new(candidates, insertion_byte));
                                    }
                                }
                            },
                        }
                    }
                },
            }
        } else {
            *last_error = Some("Couldn't find overridable methods: no syntax tree available yet.".to_string());
        }
    }

    if override_method_dialog.is_some() {
        let doc_text_for_dialog = doc.buffer.to_string();
        if let Some(outcome) =
            codegen::show_override_method_dialog(ui, override_method_dialog, &doc_text_for_dialog, &indent_settings.unit())
        {
            match outcome {
                Ok((inserted, new_cursor)) => {
                    apply_edit(doc, parser, &doc_text_for_dialog, &inserted);
                    manual_cursor_range = Some(CCursorRange::one(CCursor::new(new_cursor)));
                }
                Err(message) => *last_error = Some(message),
            }
        }
    }

    // Right-click context menu: see `context_menu::show_context_menu`.
    // `output.cursor_range` is copied out before calling it so that
    // function doesn't need to borrow `output` at all (it already has its
    // own long-lived mutable borrow of `output.state` at the very end of
    // this function) — `CCursorRange` is `Copy`, so this costs nothing.
    context_menu::show_context_menu(
        &output.response,
        widget_id,
        doc,
        parser,
        output.cursor_range,
        &mut text,
        &mut old_text,
        &mut manual_cursor_range,
        pending_input,
        last_error,
        cached_clipboard_text,
    );

    if !doc.extra_selections.is_empty() && !ctrl_d_pressed {
        let should_collapse = ui.input(|i| i.events.iter().any(is_multi_cursor_collapse_event));
        if should_collapse {
            doc.extra_selections.clear();
        }
    }

    // Passive, read-only highlight of every occurrence of the word under
    // (or touching) the cursor — distinct from `Ctrl+D`'s *active*
    // multi-cursor editing, so it only applies with a collapsed cursor and
    // no multi-cursor selections active, to avoid competing visually with
    // either. Uses `output.cursor_range` (post-show), the same source
    // Ctrl+D/Ctrl+J already read for real, non-test usage — this is purely
    // a display concern, not an edit, so there's no interception timing to
    // get right here.
    if doc.extra_selections.is_empty()
        && let Some(primary_range) = output.cursor_range
        && primary_range.is_empty()
    {
        let word_range = multi_cursor::word_range_at(&text, primary_range.primary.index.0);
        if !word_range.is_empty() {
            let word = &text[char_to_byte(&text, word_range.start)..char_to_byte(&text, word_range.end)];
            let occurrences = multi_cursor::find_all_occurrences(&text, word, true);
            paint_occurrence_highlights(ui, &output, &occurrences);
        }
    }

    // Bracket-pair highlighting: same collapsed-cursor gate as the
    // occurrence highlight above (a selection means there's no single
    // cursor position to check bracket-adjacency against), plus requiring
    // a parsed tree — brackets are a tree-sitter-derived concept, so a file
    // with no recognized language (no `parser`) just skips this, same as
    // every other tree-driven feature in this file.
    if doc.extra_selections.is_empty()
        && let Some(primary_range) = output.cursor_range
        && primary_range.is_empty()
        && let Some(tree) = parser.as_ref().and_then(|p| p.tree())
    {
        let cursor_byte = char_to_byte(&text, primary_range.primary.index.0);
        if let Some(pair) = syntax::bracket_match(tree, &text, cursor_byte) {
            paint_bracket_match(ui, &output, &text, pair);
        }
    }

    if view_settings.show_indent_guides {
        paint_indent_guides(ui, &output, &text, indent_settings);
    }
    if view_settings.show_whitespace {
        paint_whitespace(ui, &output, &text);
    }

    paint_diagnostics(ui, &output, &text, &doc.diagnostics);
    paint_extra_selections(ui, &output, &doc.extra_selections);
    paint_line_numbers(ui, &output, gutter_left + gutter_width - GUTTER_PADDING, gutter_font_id, ui.visuals().dark_mode);

    // Sticky scroll: pin the enclosing class/method header line(s) at the top
    // of the viewport while their body scrolls under them. Painted last so it
    // occludes everything else at the top edge (that's the point — the band
    // is opaque). Tree-driven (Java only today), so a no-parser file skips it
    // like every other tree feature.
    //
    // The top visible *logical* line is found by hit-testing the galley at the
    // viewport's top edge (`cursor_from_pos(..).index.0`, the same inverse of
    // `pos_from_cursor` the Alt+Click path uses) and mapping that char to its
    // line — not by counting galley rows. That distinction matters with
    // word-wrap on (the default): a wrapped line spans several rows but is one
    // logical line, so a raw row count would drift and pin the wrong header,
    // where the galley's own hit-testing stays wrap-correct. The enclosing
    // scopes come from the reparsed tree via `doc.buffer`, whose line indices
    // match the displayed text's (auto-indent/auto-pair only ever add
    // characters, never lines, so the two agree even if they differ by a few
    // whitespace bytes on the exact frame of such an edit).
    if view_settings.show_sticky_scroll
        && let Some((tree, language)) = parser.as_ref().and_then(|p| p.tree().map(|t| (t, p.language())))
    {
        let local_top = (ui.clip_rect().top() - output.galley_pos.y).max(0.0);
        let top_char = output.galley.cursor_from_pos(egui::vec2(0.0, local_top)).index.0;
        let line_count = doc.buffer.len_lines();
        let top_line = doc.buffer.char_to_line(top_char.min(doc.buffer.len_chars()));
        if top_line > 0 && line_count > 0 {
            let top_byte = doc.buffer.line_to_byte(top_line.min(line_count - 1));
            let scope_lines: Vec<usize> = syntax::enclosing_scope_starts(tree, top_byte, language)
                .into_iter()
                .map(|byte| doc.buffer.byte_to_line(byte))
                .collect();
            let pin_lines = sticky_headers_to_pin(&scope_lines, top_line, STICKY_MAX_DEPTH);
            if !pin_lines.is_empty() {
                let headers: Vec<String> =
                    pin_lines.iter().map(|&line| doc.buffer.line(line).chars().collect()).collect();
                let sticky_font = FontId::new(font_size, editor_font.family());
                paint_sticky_scroll(ui, &output, &headers, sticky_font, ui.visuals().dark_mode);
            }
        }
    }

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
mod tests;
