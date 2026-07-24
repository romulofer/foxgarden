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
    paint_occurrence_highlights, paint_whitespace,
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

    /// `test_support::temp_document` takes `(name, contents)`; every one of
    /// this module's ~50 call sites was already written the other way
    /// around (`(contents, filename)`, matching how the fixture text reads
    /// as the "main" argument in a test body), so this keeps that order
    /// rather than touching all of them.
    fn open_fixture(contents: &str, filename: &str) -> (tempfile::TempDir, Document) {
        test_support::temp_document(filename, contents)
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
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
    }

    #[test]
    fn bracket_pair_highlighting_renders_without_panicking_when_cursor_is_beside_a_brace() {
        let (_dir, mut doc) = open_fixture("class Foo {\n}\n", "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let brace = doc.buffer.to_string().find('{').unwrap() + 1;

        // A collapsed selection right after the opening brace is exactly
        // the position `syntax::bracket_match` recognizes as "touching"
        // it — this is a smoke test for `paint_bracket_match` (painting
        // isn't otherwise assertable), the matching logic itself is
        // covered directly by `syntax::brackets`'s own tests.
        focused_frame_with_selection(&mut doc, &mut parser, brace..brace, vec![]);
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
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
    }

    #[test]
    fn occurrence_highlighting_does_not_panic_when_the_cursor_touches_a_word() {
        // A fresh widget's default (collapsed) cursor sits at char 0, which
        // touches "abc" — this should compute and paint every occurrence
        // ("abc" appears twice) without panicking.
        let (_dir, mut doc) = open_fixture("abc def abc", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        egui::__run_test_ui(|ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
    }

    #[test]
    fn plain_text_file_renders_without_a_parser_and_stays_free_of_diagnostics() {
        let (_dir, mut doc) = open_fixture("just some notes, no code here", "notes.txt");
        assert_eq!(doc.language, None);
        let mut parser: Option<IncrementalParser> = None;

        egui::__run_test_ui(|ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
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
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
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
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let first = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after first frame");

        // A second frame over the very same, unedited document — as if the
        // tab were simply redrawn, or switched away from and back to.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
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
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let first = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after first frame");

        doc.buffer = Rope::from_str("hello world");
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
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
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let first = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after first frame");

        // Same unedited content, but a different font size — the cached
        // galley was shaped at the old size, so it must not be reused.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 18.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let second = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after second frame");

        assert!(
            !Arc::ptr_eq(&first.galley, &second.galley),
            "a font size change must not reuse the previous frame's stale-sized galley"
        );
    }

    #[test]
    fn layout_cache_reshapes_after_a_word_wrap_toggle() {
        let (_dir, mut doc) = open_fixture("just some notes, no code here", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let cache_id = layout_cache_id(&doc);

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let first = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after first frame");

        // Same unedited content, but word wrap turned off — the cached
        // galley was shaped against the viewport's wrap width, so it must
        // not be reused once wrapping is disabled (which shapes against
        // `f32::INFINITY` instead).
        let no_wrap = ViewSettings { word_wrap: false, ..ViewSettings::default() };
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), no_wrap, None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let second = ctx
            .data(|d| d.get_temp::<CachedLayout>(cache_id))
            .expect("layout cache populated after second frame");

        assert!(
            !Arc::ptr_eq(&first.galley, &second.galley),
            "toggling word wrap off must not reuse the previous frame's wrapped galley"
        );
    }

    #[test]
    fn whitespace_and_indent_guides_render_without_panicking() {
        let (_dir, mut doc) = open_fixture("public class Hello {\n    int x = 1;\n}\n", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let view_settings =
            ViewSettings { word_wrap: true, show_whitespace: true, show_indent_guides: true };

        egui::__run_test_ui(|ui| {
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), view_settings, None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
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
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
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
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
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
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
    }

    /// Like `focused_frame_with_selection`, but returns the resulting
    /// cursor/selection range — needed for a purely cursor-moving
    /// interception like Home/Shift+Home, which leaves the buffer itself
    /// unchanged, so `doc.buffer` alone can't confirm anything moved.
    fn focused_frame_with_selection_returning_cursor(
        doc: &mut Document,
        parser: &mut Option<IncrementalParser>,
        selection: std::ops::Range<usize>,
        events: Vec<egui::Event>,
    ) -> CCursorRange {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });

        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(selection.start),
            CCursor::new(selection.end),
        )));
        state.store(&ctx, id);

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
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });

        egui::text_edit::TextEditState::load(&ctx, id)
            .and_then(|state| state.cursor.char_range())
            .expect("cursor range should be set after a focused frame")
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
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
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
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
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
            show(ui, doc, parser, EditorFont::Default, 14.0, indent_settings, ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
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
            show(ui, doc, parser, EditorFont::Default, 14.0, indent_settings, ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
    }

    /// Combines `focused_frame_with_indent_settings` (custom
    /// `IndentSettings`) and `focused_frame_with_selection` (an injected
    /// cursor/selection) — needed to exercise the Tab-with-no-selection
    /// live-template path under a non-default indent mode, which needs
    /// both: the cursor positioned right after a trigger word, and control
    /// over `use_tabs`.
    fn focused_frame_with_indent_settings_and_selection(
        doc: &mut Document,
        parser: &mut Option<IncrementalParser>,
        indent_settings: IndentSettings,
        selection: std::ops::Range<usize>,
        events: Vec<egui::Event>,
    ) {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, indent_settings, ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });

        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(selection.start),
            CCursor::new(selection.end),
        )));
        state.store(&ctx, id);

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
            show(ui, doc, parser, EditorFont::Default, 14.0, indent_settings, ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
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

    #[test]
    fn home_from_mid_line_goes_to_first_non_whitespace() {
        let (_dir, mut doc) = open_fixture("    foo", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        // Collapsed cursor (6..6) mid "foo".
        let range = focused_frame_with_selection_returning_cursor(&mut doc, &mut parser, 6..6, vec![key_event(egui::Key::Home)]);

        assert_eq!(range.primary.index.0, 4);
        assert!(range.is_empty(), "Home with no selection active must not create one");
        assert_eq!(doc.buffer.to_string(), "    foo", "Home must never change the buffer");
    }

    #[test]
    fn home_from_first_non_whitespace_goes_to_column_zero() {
        let (_dir, mut doc) = open_fixture("    foo", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        let range = focused_frame_with_selection_returning_cursor(&mut doc, &mut parser, 4..4, vec![key_event(egui::Key::Home)]);

        assert_eq!(range.primary.index.0, 0);
    }

    #[test]
    fn shift_home_extends_the_selection_instead_of_collapsing_it() {
        let (_dir, mut doc) = open_fixture("    foo", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        // Cursor at the end of "foo" (7), nothing selected yet.
        let range = focused_frame_with_selection_returning_cursor(
            &mut doc,
            &mut parser,
            7..7,
            vec![shift_key_event(egui::Key::Home)],
        );

        // Primary (the moving end) lands on first-non-whitespace; secondary
        // (the anchor) stays where Shift+Home started from.
        assert_eq!(range.primary.index.0, 4);
        assert_eq!(range.secondary.index.0, 7);
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

    fn alt_key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { alt: true, ..egui::Modifiers::NONE },
        }
    }

    fn alt_shift_key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { alt: true, shift: true, ..egui::Modifiers::NONE },
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

    #[test]
    fn tab_after_a_known_java_trigger_word_expands_the_live_template() {
        let (_dir, mut doc) = open_fixture("sout", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        // Collapsed cursor (4..4) right after "sout" — not a real
        // selection, just how `focused_frame_with_selection` positions a
        // bare cursor via the persisted `TextEditState` the Tab
        // interception reads.
        focused_frame_with_selection(&mut doc, &mut parser, 4..4, vec![key_event(egui::Key::Tab)]);

        assert_eq!(doc.buffer.to_string(), "System.out.println();");
    }

    #[test]
    fn tab_after_a_known_kotlin_trigger_word_expands_the_kotlin_template() {
        let (_dir, mut doc) = open_fixture("sout", "Hello.kt");
        let mut parser = parsed(Language::Kotlin, &doc.buffer.to_string());

        focused_frame_with_selection(&mut doc, &mut parser, 4..4, vec![key_event(egui::Key::Tab)]);

        assert_eq!(doc.buffer.to_string(), "println()");
    }

    #[test]
    fn tab_after_an_unknown_word_falls_through_to_normal_spaces_indentation() {
        let (_dir, mut doc) = open_fixture("xyz", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        focused_frame_with_selection(&mut doc, &mut parser, 3..3, vec![key_event(egui::Key::Tab)]);

        // No template matches "xyz" — default (spaces) indentation applies
        // instead, same as plain Tab anywhere else with no selection.
        assert_eq!(doc.buffer.to_string(), "xyz    ");
    }

    #[test]
    fn tab_after_a_trigger_word_expands_even_in_tabs_mode() {
        // Live-template expansion is orthogonal to the tabs-vs-spaces
        // setting — it must win even when `use_tabs` would otherwise leave
        // plain Tab un-intercepted.
        let (_dir, mut doc) = open_fixture("sout", "Hello.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let tabs_mode = IndentSettings { use_tabs: true, width: 4 };
        focused_frame_with_indent_settings_and_selection(
            &mut doc,
            &mut parser,
            tabs_mode,
            4..4,
            vec![key_event(egui::Key::Tab)],
        );

        assert_eq!(doc.buffer.to_string(), "System.out.println();");
    }

    #[test]
    fn alt_arrow_up_moves_the_current_line_up() {
        let (_dir, mut doc) = open_fixture("aaa\nbbb\nccc", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        // Cursor at column 0 of "bbb" (char 4).
        focused_frame_with_selection(&mut doc, &mut parser, 4..4, vec![alt_key_event(egui::Key::ArrowUp)]);

        assert_eq!(doc.buffer.to_string(), "bbb\naaa\nccc");
    }

    #[test]
    fn alt_arrow_down_moves_the_current_line_down() {
        let (_dir, mut doc) = open_fixture("aaa\nbbb\nccc", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        // Cursor at column 0 of "aaa" (char 0).
        focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![alt_key_event(egui::Key::ArrowDown)]);

        assert_eq!(doc.buffer.to_string(), "bbb\naaa\nccc");
    }

    #[test]
    fn alt_arrow_up_on_the_first_line_is_a_no_op() {
        let (_dir, mut doc) = open_fixture("aaa\nbbb", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![alt_key_event(egui::Key::ArrowUp)]);

        assert_eq!(doc.buffer.to_string(), "aaa\nbbb");
    }

    #[test]
    fn alt_shift_arrow_down_duplicates_the_line_and_lands_on_the_copy() {
        let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![alt_shift_key_event(egui::Key::ArrowDown)]);

        assert_eq!(doc.buffer.to_string(), "foo\nfoo\nbar");
    }

    #[test]
    fn alt_shift_arrow_up_duplicates_the_line_and_stays_on_the_original() {
        let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![alt_shift_key_event(egui::Key::ArrowUp)]);

        assert_eq!(doc.buffer.to_string(), "foo\nfoo\nbar");
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

    #[test]
    fn read_only_doc_ignores_typed_text() {
        let (_dir, mut doc) = open_fixture("foo", "notes.txt");
        doc.read_only = true;
        let mut parser: Option<IncrementalParser> = None;

        focused_frame(&mut doc, &mut parser, vec![egui::Event::Text("x".to_string())]);

        assert_eq!(doc.buffer.to_string(), "foo");
    }

    #[test]
    fn read_only_doc_ignores_backspace() {
        let (_dir, mut doc) = open_fixture("foo", "notes.txt");
        doc.read_only = true;
        let mut parser: Option<IncrementalParser> = None;

        focused_frame(&mut doc, &mut parser, vec![key_event(egui::Key::Backspace)]);

        assert_eq!(doc.buffer.to_string(), "foo");
    }

    #[test]
    fn read_only_doc_ignores_ctrl_j_join_lines() {
        let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
        doc.read_only = true;
        let mut parser: Option<IncrementalParser> = None;

        focused_frame(&mut doc, &mut parser, vec![command_key_event(egui::Key::J)]);

        assert_eq!(doc.buffer.to_string(), "foo\nbar");
    }

    #[test]
    fn read_only_doc_ignores_generate_request() {
        let (_dir, mut doc) = open_fixture("class Foo {\n    private int x;\n}\n", "Foo.java");
        doc.read_only = true;
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let original = doc.buffer.to_string();

        focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Both), &mut None, vec![]);

        assert_eq!(doc.buffer.to_string(), original);
    }

    #[test]
    fn is_mutating_event_blocks_typed_text_paste_and_cut() {
        assert!(is_mutating_event(&egui::Event::Text("a".to_string())));
        assert!(is_mutating_event(&egui::Event::Paste("a".to_string())));
        assert!(is_mutating_event(&egui::Event::Cut));
    }

    #[test]
    fn is_mutating_event_blocks_known_editing_shortcuts() {
        for key in [
            egui::Key::Backspace,
            egui::Key::Delete,
            egui::Key::Tab,
            egui::Key::Enter,
            egui::Key::J,
            egui::Key::G,
            egui::Key::Slash,
            egui::Key::U,
            egui::Key::L,
            egui::Key::Z,
            egui::Key::Y,
        ] {
            assert!(is_mutating_event(&key_event(key)), "{key:?} should be treated as mutating");
        }
        assert!(is_mutating_event(&alt_key_event(egui::Key::ArrowUp)));
        assert!(is_mutating_event(&alt_key_event(egui::Key::ArrowDown)));
    }

    #[test]
    fn is_mutating_event_allows_navigation_and_copy() {
        for key in [
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
            egui::Key::Home,
            egui::Key::End,
            egui::Key::PageUp,
            egui::Key::PageDown,
            egui::Key::Escape,
            egui::Key::D, // Ctrl+D occurrence select — not a mutation
            egui::Key::W, // Ctrl+W/Ctrl+Shift+W expand/shrink selection — not a mutation
        ] {
            assert!(!is_mutating_event(&key_event(key)), "{key:?} should not be treated as mutating");
        }
        assert!(!is_mutating_event(&egui::Event::Copy));
    }

    /// Runs one more frame on `ctx`/`id` (already focused and holding
    /// whatever selection the previous frame left persisted — unlike
    /// `focused_frame_with_selection`, this does *not* reset the selection
    /// first) with `event` as the only input, and returns the resulting
    /// persisted selection as a plain `Range<usize>`. Shared by the
    /// Ctrl+W/Ctrl+Shift+W tests below, which need to chain several presses
    /// in sequence — each reading the *previous* press's result as its own
    /// starting selection — rather than the single request/response shape
    /// every other helper in this file provides.
    fn run_frame_reading_selection(
        ctx: &egui::Context,
        id: egui::Id,
        doc: &mut Document,
        parser: &mut Option<IncrementalParser>,
        event: egui::Event,
    ) -> std::ops::Range<usize> {
        let modifiers = match &event {
            egui::Event::Key { modifiers, .. } => *modifiers,
            _ => egui::Modifiers::NONE,
        };
        let raw_input = egui::RawInput { events: vec![event], modifiers, ..Default::default() };
        let _ = ctx.run_ui(raw_input, |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let range = egui::text_edit::TextEditState::load(ctx, id)
            .and_then(|state| state.cursor.char_range())
            .expect("a selection should be persisted after the frame")
            .as_sorted_char_range();
        range.start.0..range.end.0
    }

    #[test]
    fn ctrl_w_expands_selection_by_syntax_node_and_ctrl_shift_w_shrinks_back() {
        let source = "class Foo {\n    void run() {\n        foo();\n    }\n}\n";
        let (_dir, mut doc) = open_fixture(source, "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
        let call_start = source.find("foo()").unwrap();

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(call_start), CCursor::new(call_start + 3))));
        state.store(&ctx, id);

        let after_first_expand =
            run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_key_event(egui::Key::W));
        assert_eq!(&source[after_first_expand.clone()], "foo()");

        let after_second_expand =
            run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_key_event(egui::Key::W));
        assert!(
            after_second_expand.start <= after_first_expand.start && after_second_expand.end >= after_first_expand.end,
            "each expand must grow the selection: {after_first_expand:?} -> {after_second_expand:?}"
        );
        assert_ne!(after_second_expand, after_first_expand);

        let after_shrink =
            run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_shift_key_event(egui::Key::W));
        assert_eq!(
            after_shrink, after_first_expand,
            "shrink must restore exactly what the second expand grew out of"
        );

        // Pure selection movement — the buffer itself must never change.
        assert_eq!(doc.buffer.to_string(), source);
    }

    #[test]
    fn ctrl_w_on_a_file_with_no_parsed_tree_is_a_no_op_that_still_consumes_the_keystroke() {
        // No language recognized for "notes.txt" — `parser` stays `None`
        // throughout, so there's no tree for `Ctrl+W` to act on. This must
        // not fall through to egui's own built-in `Ctrl+W` ("delete
        // previous word"): the buffer has to come out unchanged either way.
        let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        focused_frame_with_selection(&mut doc, &mut parser, 6..6, vec![command_key_event(egui::Key::W)]);

        assert_eq!(doc.buffer.to_string(), "hello world");
    }

    #[test]
    fn ctrl_w_still_expands_selection_on_a_read_only_java_file() {
        // Non-mutating navigation, same as Ctrl+D — must keep working even
        // when `doc.read_only` blocks every actual edit path.
        let source = "class Foo {\n    int x;\n}\n";
        let (_dir, mut doc) = open_fixture(source, "Foo.java");
        doc.read_only = true;
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let x_pos = source.find('x').unwrap();

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(x_pos), CCursor::new(x_pos))));
        state.store(&ctx, id);

        let expanded = run_frame_reading_selection(&ctx, id, &mut doc, &mut parser, command_key_event(egui::Key::W));

        assert!(!expanded.is_empty(), "Ctrl+W should still grow the selection on a read-only file");
        assert_eq!(doc.buffer.to_string(), source, "read-only must still block any actual edit");
    }

    fn alt_click_events(pos: egui::Pos2) -> Vec<egui::Event> {
        let modifiers = egui::Modifiers { alt: true, ..egui::Modifiers::NONE };
        vec![
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers },
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers },
        ]
    }

    #[test]
    fn alt_click_adds_a_bare_extra_cursor_without_moving_the_primary_one() {
        let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        // First frame: establishes the widget's real on-screen rect (read
        // back via `Context::read_response`, the same cache egui itself
        // uses) and parks the primary cursor at a known, deliberately
        // *not*-start-of-buffer position — so "the primary cursor didn't
        // move" below is actually testing something.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(6), CCursor::new(6))));
        state.store(&ctx, id);
        let widget_rect = ctx.read_response(id).expect("TextEdit response cached after a frame").rect;

        // Just inside the widget's own rect — exactly which character this
        // resolves to isn't asserted (that's `egui::Galley`'s own geometry,
        // not this feature's logic to re-verify); only that a click *inside
        // the widget* produces an extra bare caret without disturbing the
        // primary cursor is.
        let click_pos = widget_rect.left_top() + egui::vec2(2.0, 2.0);
        let events = alt_click_events(click_pos);
        let modifiers = egui::Modifiers { alt: true, ..egui::Modifiers::NONE };
        let raw_input = egui::RawInput { events, modifiers, ..Default::default() };
        let _ = ctx.run_ui(raw_input, |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });

        assert_eq!(doc.extra_selections.len(), 1, "Alt+Click should add exactly one extra selection");
        assert!(doc.extra_selections[0].is_empty(), "Alt+Click's extra selection should be a bare caret");

        let primary_after = egui::text_edit::TextEditState::load(&ctx, id)
            .and_then(|state| state.cursor.char_range())
            .unwrap();
        assert_eq!(
            (primary_after.primary.index.0, primary_after.secondary.index.0),
            (6, 6),
            "the primary cursor must stay exactly where it was before the Alt+Click"
        );
        assert_eq!(doc.buffer.to_string(), "hello world", "Alt+Click must never mutate the buffer");
    }

    #[test]
    fn alt_click_on_the_same_position_twice_does_not_duplicate_the_extra_cursor() {
        let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let widget_rect = ctx.read_response(id).expect("TextEdit response cached after a frame").rect;
        let click_pos = widget_rect.left_top() + egui::vec2(2.0, 2.0);
        let modifiers = egui::Modifiers { alt: true, ..egui::Modifiers::NONE };

        for _ in 0..2 {
            let raw_input = egui::RawInput { events: alt_click_events(click_pos), modifiers, ..Default::default() };
            let _ = ctx.run_ui(raw_input, |ui| {
                ui.memory_mut(|mem| mem.request_focus(id));
                show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
            });
        }

        assert_eq!(
            doc.extra_selections.len(),
            1,
            "clicking the identical position twice must not add a duplicate bare caret"
        );
    }

    #[test]
    fn ctrl_slash_comments_the_current_line() {
        let (_dir, mut doc) = open_fixture("foo();", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        // Ctrl+/ reads the *persisted* selection (see `show`'s comment on
        // this interception), so — like wrap-selection/Tab/Alt+Arrow — it
        // needs `focused_frame_with_selection`'s warm-up frame, not the
        // single dry frame `focused_frame` gives; a collapsed 0..0
        // selection is just "cursor at the start, nothing selected".
        focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![command_key_event(egui::Key::Slash)]);

        assert_eq!(doc.buffer.to_string(), "// foo();");
    }

    #[test]
    fn ctrl_slash_uncomments_an_already_commented_line() {
        let (_dir, mut doc) = open_fixture("// foo();", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        focused_frame_with_selection(&mut doc, &mut parser, 0..0, vec![command_key_event(egui::Key::Slash)]);

        assert_eq!(doc.buffer.to_string(), "foo();");
    }

    #[test]
    fn ctrl_slash_toggles_every_line_a_selection_touches() {
        let (_dir, mut doc) = open_fixture("foo\nbar", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        // All of "foo" and all of "bar" (chars 0..7).
        focused_frame_with_selection(&mut doc, &mut parser, 0..7, vec![command_key_event(egui::Key::Slash)]);

        assert_eq!(doc.buffer.to_string(), "// foo\n// bar");
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

    /// Like `focused_frame`, but exposes the `generate_request`/`last_error`
    /// parameters `Tools > Generate Getters/Setters` and `Ctrl+Shift+G`
    /// feed `show`, returning whatever ends up in `last_error` — used to
    /// verify every non-applicable case (wrong file type, no fields)
    /// surfaces visible feedback instead of a silent no-op, which is easy
    /// to mistake for "the shortcut doesn't work."
    fn focused_frame_with_generate_request(
        doc: &mut Document,
        parser: &mut Option<IncrementalParser>,
        generate_request: Option<AccessorKind>,
        generate_dialog: &mut Option<GenerateAccessorsDialog>,
        events: Vec<egui::Event>,
    ) -> Option<String> {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let modifiers = events
            .iter()
            .find_map(|e| match e {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let raw_input = egui::RawInput { events, modifiers, ..Default::default() };
        let mut last_error = None;
        let _ = ctx.run_ui(raw_input, |ui| {
            let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
            ui.memory_mut(|mem| mem.request_focus(id));
            show(
                ui,
                doc,
                parser,
                EditorFont::Default,
                14.0,
                IndentSettings::default(),
                ViewSettings::default(),
                generate_request,
                generate_dialog,
                None,
                &mut None,
                None,
                false,
                &mut None,
                None,
                false,
                false,
                &mut last_error,
                &mut Vec::new(),
                &mut None,
            );
        });
        last_error
    }

    /// Like `focused_frame_with_selection`, but also lets the caller drive
    /// `case_conversion_request` (the Tools menu path) and reads back
    /// `last_error` — needed because case conversion, like Ctrl+/, reads
    /// the *persisted* selection (see `show`'s comment on that
    /// interception), so it needs the same warm-up-frame treatment
    /// `focused_frame_with_selection` already gives wrap-selection/Tab.
    fn focused_frame_with_selection_and_case_request(
        doc: &mut Document,
        parser: &mut Option<IncrementalParser>,
        selection: std::ops::Range<usize>,
        case_conversion_request: Option<CaseConversion>,
        events: Vec<egui::Event>,
    ) -> Option<String> {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });

        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(selection.start),
            CCursor::new(selection.end),
        )));
        state.store(&ctx, id);

        let modifiers = events
            .iter()
            .find_map(|e| match e {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let raw_input = egui::RawInput { events, modifiers, ..Default::default() };
        let mut last_error = None;
        let _ = ctx.run_ui(raw_input, |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(
                ui,
                doc,
                parser,
                EditorFont::Default,
                14.0,
                IndentSettings::default(),
                ViewSettings::default(),
                None,
                &mut None,
                None,
                &mut None,
                None,
                false,
                &mut None,
                case_conversion_request,
                false,
                false,
                &mut last_error,
                &mut Vec::new(),
                &mut None,
            );
        });
        last_error
    }

    /// Like `focused_frame_with_selection_and_case_request`, but for
    /// `sort_lines_request`/`unique_lines_request` — same two-phase shape
    /// (a warm-up frame to persist the selection, then a second frame that
    /// actually drives the request), since sort/unique lines reads the
    /// *persisted* selection the same way case conversion does.
    fn focused_frame_with_selection_and_line_op_request(
        doc: &mut Document,
        parser: &mut Option<IncrementalParser>,
        selection: std::ops::Range<usize>,
        sort_lines_request: bool,
        unique_lines_request: bool,
    ) {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, doc, parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, None, false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });

        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(selection.start),
            CCursor::new(selection.end),
        )));
        state.store(&ctx, id);

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(
                ui,
                doc,
                parser,
                EditorFont::Default,
                14.0,
                IndentSettings::default(),
                ViewSettings::default(),
                None,
                &mut None,
                None,
                &mut None,
                None,
                false,
                &mut None,
                None,
                sort_lines_request,
                unique_lines_request,
                &mut None,
                &mut Vec::new(),
                &mut None,
            );
        });
    }

    #[test]
    fn sort_lines_request_sorts_the_selected_lines() {
        let (_dir, mut doc) = open_fixture("banana\napple\ncherry", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        focused_frame_with_selection_and_line_op_request(&mut doc, &mut parser, 0..19, true, false);

        assert_eq!(doc.buffer.to_string(), "apple\nbanana\ncherry");
    }

    #[test]
    fn unique_lines_request_dedupes_the_selected_lines() {
        let (_dir, mut doc) = open_fixture("foo\nbar\nfoo", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        focused_frame_with_selection_and_line_op_request(&mut doc, &mut parser, 0..11, false, true);

        assert_eq!(doc.buffer.to_string(), "foo\nbar");
    }

    #[test]
    fn read_only_doc_ignores_sort_lines_request() {
        let (_dir, mut doc) = open_fixture("banana\napple", "notes.txt");
        doc.read_only = true;
        let mut parser: Option<IncrementalParser> = None;

        focused_frame_with_selection_and_line_op_request(&mut doc, &mut parser, 0..12, true, false);

        assert_eq!(doc.buffer.to_string(), "banana\napple");
    }

    fn command_shift_u_event() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::U,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { command: true, shift: true, ..egui::Modifiers::NONE },
        }
    }

    fn command_shift_l_event() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::L,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { command: true, shift: true, ..egui::Modifiers::NONE },
        }
    }

    #[test]
    fn ctrl_shift_u_uppercases_the_selection() {
        let (_dir, mut doc) = open_fixture("foo bar baz", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        // "bar" is chars 4..7.
        let last_error =
            focused_frame_with_selection_and_case_request(&mut doc, &mut parser, 4..7, None, vec![command_shift_u_event()]);

        assert_eq!(last_error, None);
        assert_eq!(doc.buffer.to_string(), "foo BAR baz");
    }

    #[test]
    fn ctrl_shift_l_lowercases_the_selection() {
        let (_dir, mut doc) = open_fixture("foo BAR baz", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        let last_error =
            focused_frame_with_selection_and_case_request(&mut doc, &mut parser, 4..7, None, vec![command_shift_l_event()]);

        assert_eq!(last_error, None);
        assert_eq!(doc.buffer.to_string(), "foo bar baz");
    }

    #[test]
    fn tools_menu_convert_to_title_case() {
        let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;

        let last_error = focused_frame_with_selection_and_case_request(
            &mut doc,
            &mut parser,
            0..11,
            Some(CaseConversion::Title),
            vec![],
        );

        assert_eq!(last_error, None);
        assert_eq!(doc.buffer.to_string(), "Hello World");
    }

    #[test]
    fn case_conversion_with_no_selection_reports_why_instead_of_doing_nothing() {
        let (_dir, mut doc) = open_fixture("foo bar baz", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;
        let before = doc.buffer.to_string();

        // Collapsed selection (4..4) — cursor positioned, nothing selected.
        let last_error =
            focused_frame_with_selection_and_case_request(&mut doc, &mut parser, 4..4, None, vec![command_shift_u_event()]);

        assert_eq!(doc.buffer.to_string(), before);
        assert!(last_error.is_some_and(|msg| msg.contains("Select")));
    }

    #[test]
    fn ctrl_shift_g_is_a_no_op_for_kotlin_files_but_reports_why() {
        // Kotlin's `val`/`var` properties already are getters/setters;
        // generating explicit Java-shaped ones for them isn't idiomatic
        // (see `widget::show`'s comment on this shortcut), so the command
        // does nothing for a non-Java file — but must say so via
        // `last_error` rather than silently doing nothing.
        let (_dir, mut doc) = open_fixture("class Foo(val x: Int)\n", "Foo.kt");
        let mut parser = parsed(Language::Kotlin, &doc.buffer.to_string());
        let before = doc.buffer.to_string();

        let last_error = focused_frame_with_generate_request(
            &mut doc,
            &mut parser,
            None,
            &mut None,
            vec![command_shift_key_event(egui::Key::G)],
        );

        assert_eq!(doc.buffer.to_string(), before);
        assert!(last_error.is_some_and(|msg| msg.contains("Java")));
    }

    #[test]
    fn ctrl_shift_g_on_a_fieldless_class_is_a_no_op_but_reports_why() {
        let (_dir, mut doc) = open_fixture("public class Empty {\n}\n", "Empty.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let before = doc.buffer.to_string();

        let last_error = focused_frame_with_generate_request(
            &mut doc,
            &mut parser,
            None,
            &mut None,
            vec![command_shift_key_event(egui::Key::G)],
        );

        assert_eq!(doc.buffer.to_string(), before);
        assert!(last_error.is_some_and(|msg| msg.contains("fields")));
    }

    #[test]
    fn tools_menu_generate_getters_inserts_only_a_getter() {
        let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let last_error =
            focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Getters), &mut None, vec![]);

        assert_eq!(last_error, None);
        let text = doc.buffer.to_string();
        assert!(text.contains("getX"));
        assert!(!text.contains("setX"));
    }

    #[test]
    fn tools_menu_generate_setters_inserts_only_a_setter() {
        let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let last_error =
            focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Setters), &mut None, vec![]);

        assert_eq!(last_error, None);
        let text = doc.buffer.to_string();
        assert!(!text.contains("getX"));
        assert!(text.contains("setX"));
    }

    #[test]
    fn tools_menu_generate_setters_on_an_all_final_class_reports_why() {
        let (_dir, mut doc) = open_fixture("public class Foo {\n    private final int x;\n}\n", "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let before = doc.buffer.to_string();

        let last_error =
            focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Setters), &mut None, vec![]);

        assert_eq!(doc.buffer.to_string(), before);
        assert!(last_error.is_some_and(|msg| msg.contains("final")));
    }

    #[test]
    fn tools_menu_generate_getters_on_a_multi_class_file_opens_the_picker_instead_of_generating() {
        let (_dir, mut doc) =
            open_fixture("class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n", "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let before = doc.buffer.to_string();
        let mut generate_dialog = None;

        let last_error = focused_frame_with_generate_request(
            &mut doc,
            &mut parser,
            Some(AccessorKind::Getters),
            &mut generate_dialog,
            vec![],
        );

        assert_eq!(last_error, None);
        assert_eq!(doc.buffer.to_string(), before, "nothing should be inserted until the picker's Generate is clicked");
        let dialog = generate_dialog.expect("multiple eligible classes should open the picker");
        assert_eq!(dialog.classes().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["Foo", "Bar"]);
    }

    /// Like `focused_frame_with_generate_request`, but for
    /// `generate_method_request`/`generate_method_dialog` (Constructor/
    /// toString/equals+hashCode) instead of `generate_request`/
    /// `generate_dialog` (Getters/Setters).
    fn focused_frame_with_generate_method_request(
        doc: &mut Document,
        parser: &mut Option<IncrementalParser>,
        generate_method_request: Option<GenerateMethodKind>,
        generate_method_dialog: &mut Option<GenerateMethodDialog>,
    ) -> Option<String> {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let mut last_error = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
            ui.memory_mut(|mem| mem.request_focus(id));
            show(
                ui,
                doc,
                parser,
                EditorFont::Default,
                14.0,
                IndentSettings::default(),
                ViewSettings::default(),
                None,
                &mut None,
                generate_method_request,
                generate_method_dialog,
                None,
                false,
                &mut None,
                None,
                false,
                false,
                &mut last_error,
                &mut Vec::new(),
                &mut None,
            );
        });
        last_error
    }

    #[test]
    fn tools_menu_generate_constructor_inserts_immediately_for_a_single_eligible_class() {
        let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let last_error = focused_frame_with_generate_method_request(
            &mut doc,
            &mut parser,
            Some(GenerateMethodKind::Constructor),
            &mut None,
        );

        assert_eq!(last_error, None);
        assert!(doc.buffer.to_string().contains("public Foo(int x)"));
    }

    #[test]
    fn tools_menu_generate_to_string_inserts_immediately_for_a_single_eligible_class() {
        let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let last_error = focused_frame_with_generate_method_request(
            &mut doc,
            &mut parser,
            Some(GenerateMethodKind::ToString),
            &mut None,
        );

        assert_eq!(last_error, None);
        assert!(doc.buffer.to_string().contains("public String toString()"));
    }

    #[test]
    fn tools_menu_generate_equals_and_hash_code_inserts_both_together() {
        let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());

        let last_error = focused_frame_with_generate_method_request(
            &mut doc,
            &mut parser,
            Some(GenerateMethodKind::EqualsAndHashCode),
            &mut None,
        );

        assert_eq!(last_error, None);
        let text = doc.buffer.to_string();
        assert!(text.contains("public boolean equals(Object o)"));
        assert!(text.contains("public int hashCode()"));
    }

    #[test]
    fn generate_method_request_on_a_kotlin_file_reports_why() {
        let (_dir, mut doc) = open_fixture("class Foo(val x: Int)\n", "Foo.kt");
        let mut parser = parsed(Language::Kotlin, &doc.buffer.to_string());
        let before = doc.buffer.to_string();

        let last_error = focused_frame_with_generate_method_request(
            &mut doc,
            &mut parser,
            Some(GenerateMethodKind::Constructor),
            &mut None,
        );

        assert_eq!(doc.buffer.to_string(), before);
        assert!(last_error.is_some_and(|msg| msg.contains("Java")));
    }

    #[test]
    fn generate_method_request_on_a_multi_class_file_opens_the_picker_instead_of_generating() {
        let (_dir, mut doc) =
            open_fixture("class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n", "Foo.java");
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let before = doc.buffer.to_string();
        let mut generate_method_dialog = None;

        let last_error = focused_frame_with_generate_method_request(
            &mut doc,
            &mut parser,
            Some(GenerateMethodKind::ToString),
            &mut generate_method_dialog,
        );

        assert_eq!(last_error, None);
        assert_eq!(doc.buffer.to_string(), before, "nothing should be inserted until the picker's Generate is clicked");
        let dialog = generate_method_dialog.expect("multiple eligible classes should open the picker");
        assert_eq!(dialog.classes().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["Foo", "Bar"]);
    }

    #[test]
    fn read_only_doc_ignores_generate_method_request() {
        let (_dir, mut doc) = open_fixture("class Foo {\n    private int x;\n}\n", "Foo.java");
        doc.read_only = true;
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let before = doc.buffer.to_string();

        focused_frame_with_generate_method_request(
            &mut doc,
            &mut parser,
            Some(GenerateMethodKind::Constructor),
            &mut None,
        );

        assert_eq!(doc.buffer.to_string(), before);
    }

    #[test]
    fn override_method_finds_an_inherited_method_via_the_project_tree() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Base.java"),
            "public class Base {\n    public void run() {\n    }\n}\n",
        )
        .unwrap();
        let foo_path = dir.path().join("Foo.java");
        std::fs::write(&foo_path, "public class Foo extends Base {\n}\n").unwrap();

        let mut doc = Document::open(foo_path).unwrap();
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
        let cursor = doc.buffer.to_string().find('{').unwrap() + 1; // inside Foo's body

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, Some(&project), false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(cursor), CCursor::new(cursor))));
        state.store(&ctx, id);

        let mut override_method_dialog = None;
        let mut last_error = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, Some(&project), true, &mut override_method_dialog, None, false, false, &mut last_error, &mut Vec::new(), &mut None);
        });

        assert_eq!(last_error, None);
        let dialog = override_method_dialog.expect("Base.run() should be found as an overridable method");
        assert_eq!(dialog.methods().len(), 1);
        assert_eq!(dialog.methods()[0].name, "run");
    }

    #[test]
    fn override_method_excludes_a_method_the_current_class_already_overrides() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Base.java"),
            "public class Base {\n    public void run() {\n    }\n    public void stop() {\n    }\n}\n",
        )
        .unwrap();
        let foo_path = dir.path().join("Foo.java");
        let foo_source = "public class Foo extends Base {\n    public void run() {\n    }\n}\n";
        std::fs::write(&foo_path, foo_source).unwrap();

        let mut doc = Document::open(foo_path).unwrap();
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
        let cursor = foo_source.find('{').unwrap() + 1;

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, Some(&project), false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(cursor), CCursor::new(cursor))));
        state.store(&ctx, id);

        let mut override_method_dialog = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, Some(&project), true, &mut override_method_dialog, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });

        let dialog = override_method_dialog.expect("stop() should still be offered");
        assert_eq!(dialog.methods().len(), 1);
        assert_eq!(dialog.methods()[0].name, "stop");
    }

    #[test]
    fn override_method_on_a_superclass_not_found_in_the_project_reports_why() {
        let dir = tempfile::tempdir().unwrap();
        let foo_path = dir.path().join("Foo.java");
        let foo_source = "public class Foo extends SomeLibraryClass {\n}\n";
        std::fs::write(&foo_path, foo_source).unwrap();

        let mut doc = Document::open(foo_path).unwrap();
        let mut parser = parsed(Language::Java, &doc.buffer.to_string());
        let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
        let cursor = foo_source.find('{').unwrap() + 1;

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, Some(&project), false, &mut None, None, false, false, &mut None, &mut Vec::new(), &mut None);
        });
        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(cursor), CCursor::new(cursor))));
        state.store(&ctx, id);

        let mut override_method_dialog = None;
        let mut last_error = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.memory_mut(|mem| mem.request_focus(id));
            show(ui, &mut doc, &mut parser, EditorFont::Default, 14.0, IndentSettings::default(), ViewSettings::default(), None, &mut None, None, &mut None, Some(&project), true, &mut override_method_dialog, None, false, false, &mut last_error, &mut Vec::new(), &mut None);
        });

        assert!(override_method_dialog.is_none());
        assert!(last_error.is_some_and(|msg| msg.contains("this project")));
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

    #[test]
    fn queued_pending_input_is_drained_as_real_input_before_text_edit_runs() {
        // Proves the mechanism the right-click menu's Undo/Redo/Select All
        // items depend on: a synthetic event pushed into `pending_input`
        // (exactly as those menu items do — see `synthetic_shortcut`) is
        // drained into real input *before* `TextEdit::show()` runs, so
        // egui's own handling for it fires as if the user had actually
        // pressed the key. Undo is the proof here specifically because
        // it's the one whose entire reason for existing behind this queue
        // is that it's driven by egui's own private per-widget undo
        // history — nothing about it is reimplemented on our side, so
        // watching it actually revert text proves the queued event reached
        // real egui event handling, not just our own code.
        //
        // Three frames, with `time` advanced past `Undoer`'s stable-time
        // window (1s) between the second and third: frame 1 (idle) seeds
        // the initial undo point; frame 2 types a character via a real
        // `Event::Text`, the same path any keystroke takes; frame 3, a
        // full second later, both lets that edit's undo point actually
        // commit *and* queues Ctrl+Z via `pending_input`.
        let (_dir, mut doc) = open_fixture("hello world", "notes.txt");
        let mut parser: Option<IncrementalParser> = None;
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new(doc.path.to_string_lossy().into_owned());

        let run_frame = |doc: &mut Document,
                          parser: &mut Option<IncrementalParser>,
                          time: f64,
                          events: Vec<egui::Event>,
                          pending_input: &mut Vec<egui::Event>| {
            let raw_input = egui::RawInput { events, time: Some(time), ..Default::default() };
            let _ = ctx.run_ui(raw_input, |ui| {
                ui.memory_mut(|mem| mem.request_focus(id));
                show(
                    ui,
                    doc,
                    parser,
                    EditorFont::Default,
                    14.0,
                    IndentSettings::default(),
                    ViewSettings::default(),
                    None,
                    &mut None,
                    None,
                    &mut None,
                    None,
                    false,
                    &mut None,
                    None,
                    false,
                    false,
                    &mut None,
                    pending_input,
                    &mut None,
                );
            });
        };

        run_frame(&mut doc, &mut parser, 0.0, vec![], &mut Vec::new());
        run_frame(&mut doc, &mut parser, 0.0, vec![egui::Event::Text("X".to_string())], &mut Vec::new());
        assert_eq!(doc.buffer.to_string(), "Xhello world");

        let mut pending_input = vec![synthetic_shortcut(egui::Key::Z, false)];
        run_frame(&mut doc, &mut parser, 1.5, vec![], &mut pending_input);

        assert!(pending_input.is_empty(), "the queue should be drained once used");
        assert_eq!(doc.buffer.to_string(), "hello world", "the queued Ctrl+Z should have reverted the typed character");
    }
}
