use fg_i18n::{msg, t};
use std::ops::Range;
use std::sync::Arc;

use egui::{Event, FontId, Key};
use fg_core::{Diagnostic, Document, Language, Project};
use ropey::Rope;
use syntax::{IncrementalParser, Scope, Tree};

use super::auto_edit::{
    CaseConversion, apply_auto_indent, apply_auto_pair, apply_auto_pair_delete, convert_selection_case,
    current_line_range, duplicate_line, indent_selected_lines, is_pairable, join_lines, move_line_down, move_line_up,
    smart_home_target, sort_lines, toggle_line_comments, unique_lines, wrap_selection,
};
use super::codegen::{
    self, AccessorKind, GenerateAccessorsDialog, GenerateMethodDialog, GenerateMethodKind, OverrideMethodDialog,
    generate_accessors, insert_at_class_end,
};
use super::completion::{CompletionItem, CompletionKind, CompletionState, insert_completion};
use super::context_menu;
#[cfg(test)]
use super::context_menu::synthetic_shortcut;
use super::diff_gutter;
use super::folding;
use super::hover::HoverState;
use super::multi_cursor::{self, MultiEditOp};
use super::peek::PeekState;
use super::references::FindReferencesState;
use super::rename::RenameBox;
use super::painting::{
    paint_blame_annotation, paint_bracket_match, paint_diagnostics, paint_extra_selections, paint_indent_guides,
    paint_line_numbers, paint_occurrence_highlights, paint_sticky_scroll, paint_whitespace,
};
use super::spring_annotation_completion;
use super::spring_config_completion;
use super::templates::{self, UserTemplates, expand, find_expansion, word_before_cursor};
use super::text_area::{self, Caret, HighlightSpan};
use super::text_offset::{byte_to_char, char_to_byte};
use crate::goto_definition::GotoDefinitionState;
use crate::style::fonts::EditorFont;
use crate::style::indent::IndentSettings;
use crate::style::theme;
use crate::style::view::ViewSettings;

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
    scope_lines
        .iter()
        .copied()
        .filter(|&line| line < top_line)
        .take(max_depth)
        .collect()
}

#[derive(Clone, PartialEq)]
struct HighlightSpansKey {
    content_hash: u64,
    dark_mode: bool,
}

#[derive(Clone)]
struct CachedHighlightSpans {
    key: HighlightSpansKey,
    spans: Arc<Vec<HighlightSpan>>,
}

/// Cache-checking wrapper (PLAN.md Phase 3 / SPEC.md §4) around
/// `compute_highlight_spans`, keyed on `(hash_rope_content(&doc.buffer),
/// dark_mode)` — spans are theme-dependent, so a dark/light toggle with no
/// text change still needs a recompute. Mirrors `text_area/render.rs`'s
/// `cached_row_counts` exact key/store/invalidate shape: without this, the
/// full tree-sitter query re-ran every single frame, idle cursor-blink
/// frames included, even though the result only ever changes on an actual
/// edit or theme switch.
fn highlight_spans_for(
    ui: &egui::Ui,
    widget_id: egui::Id,
    parser: Option<&IncrementalParser>,
    buffer: &Rope,
    source: &str,
    dark_mode: bool,
) -> Arc<Vec<HighlightSpan>> {
    let cache_id = egui::Id::new(("widget_highlight_spans", widget_id));
    let key = HighlightSpansKey {
        content_hash: text_area::hash_rope_content(buffer),
        dark_mode,
    };

    if let Some(cached) = ui.ctx().data(|d| d.get_temp::<CachedHighlightSpans>(cache_id))
        && cached.key == key
    {
        return cached.spans;
    }

    let spans = Arc::new(compute_highlight_spans(parser, source, dark_mode));
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            cache_id,
            CachedHighlightSpans {
                key,
                spans: spans.clone(),
            },
        )
    });
    spans
}

/// Resolves the syntax-highlighting spans for the widget's shape pass —
/// `syntax::highlight_spans` over the reparsed tree, each `Scope` resolved
/// to a concrete `Color32` via `theme::color_for_scope` right here (rather
/// than inside `text_area`, which stays decoupled from both tree-sitter and
/// theming — see `text_area::HighlightSpan`'s doc comment). Empty for a
/// language-less file or one with no tree yet, which `text_area` already
/// treats as "no highlighting, everything falls through to the plain text
/// color."
fn compute_highlight_spans(parser: Option<&IncrementalParser>, source: &str, dark_mode: bool) -> Vec<HighlightSpan> {
    let Some((tree, language)) = parser.and_then(|p| p.tree().map(|t| (t, p.language()))) else {
        return Vec::new();
    };
    syntax::highlight_spans(tree, source, language)
        .into_iter()
        .map(|(range, scope)| HighlightSpan {
            range,
            color: theme::color_for_scope(scope, dark_mode),
        })
        .collect()
}

#[derive(Clone)]
struct CachedFolds {
    content_hash: u64,
    folds: Arc<Vec<syntax::FoldRange>>,
}

/// Cache-checking wrapper (PLAN.md Phase 3 / SPEC.md §5) around
/// `syntax::foldable_ranges`, keyed on `hash_rope_content(&doc.buffer)`
/// alone — folding isn't theme-dependent, unlike `highlight_spans_for`'s
/// cache. Sits *upstream* of `folding::hidden_ranges`, which further narrows
/// this against the per-tab `folded_lines` set every frame — that narrowing
/// stays uncached (small-set filtering, not a tree walk, so it was never the
/// expensive part).
fn foldable_ranges_for(
    ui: &egui::Ui,
    widget_id: egui::Id,
    parser: Option<&IncrementalParser>,
    buffer: &Rope,
    source: &str,
) -> Arc<Vec<syntax::FoldRange>> {
    let cache_id = egui::Id::new(("widget_foldable_ranges", widget_id));
    let content_hash = text_area::hash_rope_content(buffer);

    if let Some(cached) = ui.ctx().data(|d| d.get_temp::<CachedFolds>(cache_id))
        && cached.content_hash == content_hash
    {
        return cached.folds;
    }

    let folds = parser
        .and_then(|p| p.tree().map(|tree| syntax::foldable_ranges(tree, source, p.language())))
        .unwrap_or_default();
    let folds = Arc::new(folds);
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            cache_id,
            CachedFolds {
                content_hash,
                folds: folds.clone(),
            },
        )
    });
    folds
}

#[derive(Clone, PartialEq)]
struct OccurrenceHighlightKey {
    word: String,
    content_hash: u64,
}

#[derive(Clone)]
struct CachedOccurrenceHighlights {
    key: OccurrenceHighlightKey,
    occurrences: Arc<Vec<Range<usize>>>,
}

/// Cache-checking wrapper (PLAN.md Phase 4 / SPEC.md §6) around
/// `multi_cursor::find_all_occurrences`, keyed on `(word, hash_rope_content(
/// &doc.buffer))` — a tighter, per-word key rather than the whole-buffer
/// cache shape §4/§5 use, since the occurrence set only ever needs
/// recomputing when either the touched word or the buffer itself changes,
/// not on every frame the cursor merely sits still on the same word (idle
/// blink frames included). `word_range_at` still runs every frame to learn
/// `word` in the first place — cheap relative to `find_all_occurrences`'
/// whole-buffer multi-match scan, and unavoidable without already knowing
/// whether the cursor moved, which is exactly what this cache is for.
fn occurrences_for(
    ui: &egui::Ui,
    widget_id: egui::Id,
    buffer: &Rope,
    text: &str,
    word: &str,
) -> Arc<Vec<Range<usize>>> {
    let cache_id = egui::Id::new(("widget_occurrence_highlights", widget_id));
    let key = OccurrenceHighlightKey {
        word: word.to_owned(),
        content_hash: text_area::hash_rope_content(buffer),
    };

    if let Some(cached) = ui.ctx().data(|d| d.get_temp::<CachedOccurrenceHighlights>(cache_id))
        && cached.key == key
    {
        return cached.occurrences;
    }

    let occurrences = Arc::new(multi_cursor::find_all_occurrences(text, word, true));
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            cache_id,
            CachedOccurrenceHighlights {
                key,
                occurrences: occurrences.clone(),
            },
        )
    });
    occurrences
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
    doc.lsp_version += 1;
    doc.lsp_sync_pending = true;
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
        Event::Key {
            key: Key::ArrowUp | Key::ArrowDown,
            pressed: true,
            modifiers,
            ..
        } if modifiers.alt => true,
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
#[expect(
    clippy::too_many_arguments,
    reason = "each parameter is independently threaded editor-frame state, not a bundle waiting to be a struct — see TECHNICAL_DEBT.md #5 (the 'splitting widget.rs further' entry) for why bundling into a struct isn't a clear win here, and context_menu::show_context_menu's own allowance for the same shape"
)]
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
    completion: &mut Option<CompletionState>,
    hover: &mut HoverState,
    goto_definition: &mut GotoDefinitionState,
    peek: &mut PeekState,
    case_conversion_request: Option<CaseConversion>,
    sort_lines_request: bool,
    unique_lines_request: bool,
    fold_all_request: bool,
    expand_all_request: bool,
    last_error: &mut Option<String>,
    pending_input: &mut Vec<Event>,
    cached_clipboard_text: &mut Option<String>,
    custom_templates: &UserTemplates,
    spring_config: &mut crate::panels::spring_config::SpringConfigState,
    lsp: &mut crate::lsp_state::LspState,
    find_references: &mut FindReferencesState,
    rename_box: &mut RenameBox,
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
        // A read-only file can't accept the popup's own insertion anyway
        // — same "no point offering an edit this file can't take" reasoning
        // `generate_request`/`override_method_request` already apply below.
        *completion = None;
    }
    let generate_request = if doc.read_only { None } else { generate_request };
    let generate_method_request = if doc.read_only { None } else { generate_method_request };
    let override_method_request = !doc.read_only && override_method_request;
    let case_conversion_request = if doc.read_only { None } else { case_conversion_request };

    // Mutable: the wrap-selection interception below may replace it with an
    // already-edited version *before* `TextEdit::show()` ever runs, so
    // everything downstream (the layouter's highlighting, the
    // `response.changed()` diff) sees the post-wrap text as its baseline
    // rather than redoing (or fighting with) the edit egui would otherwise
    // apply on its own. Every interception below that produces a new text
    // reassigns this same binding (never a separate copy — SPEC.md §2 found
    // two `String`s kept in permanent lockstep here, one of them dead
    // weight), so it's always this frame's current buffer content by the
    // time anything downstream reads it.
    let mut old_text = doc.buffer.to_string();
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

    let multi_cursor_active_at_start = !doc.extra_selections.is_empty();

    // `text_area::set_caret` only takes effect on the *next* `show`, so
    // every path below that wants to override the widget's post-frame
    // caret/selection just records the target here, and a single
    // `set_caret` call happens at the very end — the same "collect, apply
    // once" shape `TextEditState::store`'s once-per-frame constraint used
    // to force.
    let mut manual_caret: Option<Caret> = None;

    // `Ctrl+Space` force-opens word-completion regardless of how many
    // identifier characters are already typed (`SPEC.md` §1) — for
    // suggestions after just one character, or after moving the cursor back
    // into an existing word. Anchored at `word_before_cursor`'s *start*,
    // not the raw cursor position, so whatever's already been typed of the
    // current word still acts as the initial filter prefix instead of every
    // candidate showing unfiltered.
    if !multi_cursor_active_at_start && !doc.read_only && completion.is_none() {
        let ctrl_space_pressed = ui.input(|i| i.key_pressed(Key::Space) && i.modifiers.command);
        if ctrl_space_pressed && let Some(cursor_char) = text_area::peek_caret(ui.ctx(), widget_id).map(|c| c.primary) {
            let word_range = word_before_cursor(&old_text, cursor_char);
            let anchor_byte = char_to_byte(&old_text, word_range.start);
            let current_run = &old_text[anchor_byte..char_to_byte(&old_text, word_range.end)];
            let candidates = word_completion_candidates(&old_text, current_run, doc.language, custom_templates);
            *completion = Some(CompletionState::open(anchor_byte, candidates));
        }
    }

    // Completion popup lifecycle (`SPEC.md` §0): while open, Enter/Tab/
    // ArrowUp/ArrowDown/Escape must be consumed here, before `text_area::
    // show_interactive` runs (and before every other Tab-consuming block
    // below it, in particular the live-template interception at
    // `widget.rs:586-610`-ish) — same "intercept before the widget's own
    // default" shape that block already establishes for Tab specifically,
    // just running earlier so this popup always wins the same keystroke.
    if !multi_cursor_active_at_start
        && let Some(state) = completion.as_mut()
        && let Some(cursor_char) = text_area::peek_caret(ui.ctx(), widget_id).map(|c| c.primary)
    {
        let cursor_byte = char_to_byte(&old_text, cursor_char);

        // Cursor backed up before the anchor (backspacing past the `.`/
        // word-start that opened the popup, or a left-arrow) — the popup's
        // own filter slice would go out of range, so this closes it rather
        // than let anything below try to read `old_text[anchor..cursor]`
        // with `cursor < anchor`. A cursor that's moved *forward* off the
        // anchor's line entirely (e.g. an unrelated click) is a known,
        // narrow gap this simple check doesn't catch — see `SPEC.md` §0's
        // own "moved off the thing being edited" close rule, which this
        // approximates rather than fully implements.
        let closed_by_anchor_or_escape = cursor_byte < state.anchor_byte()
            || take_event(ui, |e| {
                matches!(
                    e,
                    Event::Key {
                        key: Key::Escape,
                        pressed: true,
                        ..
                    }
                )
            })
            .is_some();

        if closed_by_anchor_or_escape {
            *completion = None;
        } else {
            let arrow_key = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    Event::Key {
                        key: key @ (Key::ArrowUp | Key::ArrowDown),
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.is_none() => Some(*key),
                    _ => None,
                })
            });

            if let Some(key) = arrow_key {
                let removed = take_event(
                    ui,
                    |e| matches!(e, Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.is_none()),
                );
                if removed.is_some() {
                    let visible_len = state.visible(&old_text, cursor_byte).len();
                    state.move_selection(if key == Key::ArrowDown { 1 } else { -1 }, visible_len);
                }
            } else {
                let accept_key = ui.input(|i| {
                    i.events.iter().find_map(|e| match e {
                        Event::Key {
                            key: key @ (Key::Enter | Key::Tab),
                            pressed: true,
                            modifiers,
                            ..
                        } if modifiers.is_none() => Some(*key),
                        _ => None,
                    })
                });

                if let Some(key) = accept_key {
                    let anchor_byte = state.anchor_byte();
                    let selected_item = state
                        .visible(&old_text, cursor_byte)
                        .get(state.selected())
                        .map(|item| (*item).clone());

                    if let Some(item) = selected_item {
                        take_event(
                            ui,
                            |e| matches!(e, Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.is_none()),
                        );
                        let anchor_char = byte_to_char(&old_text, anchor_byte);
                        let cursor_char = byte_to_char(&old_text, cursor_byte);

                        // `Template` candidates don't go through
                        // `insert_completion` at all — they call
                        // `templates::expand` directly, exactly like the
                        // Tab-trigger path below, since that function
                        // already handles multi-line bodies and the
                        // `${cursor}` marker (`SPEC.md` §1/§5). A trigger
                        // whose body has since disappeared (a custom
                        // template edited away mid-session) is a silent
                        // no-op rather than an error.
                        let expansion = if item.kind == CompletionKind::Template {
                            let (language_templates, language_custom) =
                                language_template_tables(doc.language, custom_templates);
                            find_expansion(
                                &[language_custom, &custom_templates.global],
                                &[language_templates, templates::GLOBAL_TEMPLATES],
                                &item.label,
                            )
                            .map(|body| expand(&old_text, anchor_char..cursor_char, body))
                        } else if item.kind == CompletionKind::Annotation {
                            // Accepting a Spring annotation also inserts a
                            // matching `import`, if the file doesn't
                            // already have one — computed against the
                            // *pre*-completion `old_text`/`tree` (see
                            // `apply_with_import`'s own doc comment for why
                            // that's the safe coordinate space to splice
                            // against).
                            let (completed_text, cursor_after_label) =
                                insert_completion(&old_text, anchor_char, cursor_char, &item);
                            match (doc.language, parser.as_ref().and_then(|p| p.tree())) {
                                (Some(language), Some(tree)) => Some(spring_annotation_completion::apply_with_import(
                                    &old_text,
                                    tree,
                                    language,
                                    &item.label,
                                    anchor_byte,
                                    completed_text,
                                    cursor_after_label,
                                )),
                                _ => Some((completed_text, cursor_after_label)),
                            }
                        } else {
                            Some(insert_completion(&old_text, anchor_char, cursor_char, &item))
                        };

                        if let Some((new_text, new_cursor_char)) = expansion {
                            apply_edit(doc, parser, &old_text, &new_text);
                            manual_caret = Some(Caret::at(new_cursor_char));
                            old_text = new_text;
                        }
                    }
                    // Either way — accepted, or nothing left to accept
                    // (the filtered list emptied out since the last
                    // frame's paint) — Enter/Tab is done with the popup.
                    *completion = None;
                }
            }
        }
    }

    // While extra (Ctrl+D) cursors are active, an editing keystroke must
    // land at every active cursor at once, not just the one the widget's own
    // single-cursor editing would edit alone. This reads the *persisted*
    // selection and applies the edit manually *before* the widget's own
    // `show` runs — the same pre-apply timing wrap-selection and the indent
    // interception below use — so this frame already renders the fully
    // multi-edited result instead of a stale one needing a follow-up
    // `request_repaint()`. (A previous version of this function applied the
    // edit *after* `show()`, off the post-frame cursor, and ate a stale
    // frame; see the resolved entry in `TECHNICAL_DEBT.md` for why that was
    // debt worth fixing rather than a style difference from wrap-selection.)
    // `doc.extra_selections` only ever becomes non-empty via a prior
    // `Ctrl+D` frame, so by the time `multi_cursor_active_at_start` is true
    // here, a persisted caret from that prior frame is always expected to
    // exist.
    if multi_cursor_active_at_start {
        let primary_range = text_area::peek_caret(ui.ctx(), widget_id).map(|c| c.range());

        if let Some(primary_range) = primary_range {
            let intercepted_events = ui.input_mut(|i| {
                let matched: Vec<Event> = i.events.iter().filter(|e| is_multi_edit_event(e)).cloned().collect();
                i.events.retain(|e| !is_multi_edit_event(e));
                matched
            });

            if !intercepted_events.is_empty() {
                let op = multi_edit_op_from_events(&intercepted_events);
                let mut selections = Vec::with_capacity(1 + doc.extra_selections.len());
                selections.push(primary_range.start..primary_range.end);
                selections.extend(doc.extra_selections.iter().cloned());

                let (new_text, new_cursors) = multi_cursor::apply_multi_edit(&old_text, &selections, &op);

                apply_edit(doc, parser, &old_text, &new_text);
                manual_caret = Some(Caret::at(new_cursors[0]));
                doc.extra_selections = new_cursors[1..].iter().map(|&c| c..c).collect();
                old_text = new_text;
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
        // Cheap check first: only load the persisted caret (a `ctx.data`
        // mutex lock + hashmap probe + struct clone) on the frames where
        // there's actually a candidate keystroke queued — not on every
        // idle/mouse-only/arrow-key frame the editor is visible.
        let has_candidate_keystroke = ui.input(|i| i.events.iter().any(|e| single_pairable_char(e).is_some()));

        if has_candidate_keystroke {
            let prior_selection = text_area::peek_caret(ui.ctx(), widget_id)
                .map(|c| c.range())
                .filter(|range| !range.is_empty());

            if let Some(range) = prior_selection {
                let opener = take_event(ui, |e| single_pairable_char(e).is_some())
                    .map(|e| single_pairable_char(&e).expect("matched above"));

                if let Some(opener) = opener
                    && let Some((wrapped, sel_start, sel_end)) =
                        wrap_selection(&old_text, range.start, range.end, opener)
                {
                    apply_edit(doc, parser, &old_text, &wrapped);
                    manual_caret = Some(Caret {
                        primary: sel_end,
                        anchor: sel_start,
                    });
                    old_text = wrapped;
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
    // with no selection dedents just the current line (below), via
    // `indent_selected_lines`'s existing collapsed-range support — this
    // used to be left to "egui's own no-selection handling," but egui has
    // none, so it was silently a no-op until fixed here.
    if !multi_cursor_active_at_start {
        let tab_pressed = ui.input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    Event::Key {
                        key: Key::Tab,
                        pressed: true,
                        ..
                    }
                )
            })
        });

        if tab_pressed {
            let prior_selection = text_area::peek_caret(ui.ctx(), widget_id).map(|c| c.range());

            if let Some(range) = prior_selection {
                if !range.is_empty() {
                    let removed = take_event(ui, |e| {
                        matches!(
                            e,
                            Event::Key {
                                key: Key::Tab,
                                pressed: true,
                                ..
                            }
                        )
                    });

                    if removed.is_some() {
                        let dedent = ui.input(|i| i.modifiers.shift);
                        let (indented, sel_start, sel_end) =
                            indent_selected_lines(&old_text, range.start, range.end, dedent, indent_settings);
                        apply_edit(doc, parser, &old_text, &indented);
                        manual_caret = Some(Caret {
                            primary: sel_end,
                            anchor: sel_start,
                        });
                        old_text = indented;
                    }
                } else if ui.input(|i| i.modifiers.shift) {
                    let removed = take_event(ui, |e| {
                        matches!(
                            e,
                            Event::Key {
                                key: Key::Tab,
                                pressed: true,
                                ..
                            }
                        )
                    });

                    if removed.is_some() {
                        let (dedented, _, new_cursor) =
                            indent_selected_lines(&old_text, range.start, range.start, true, indent_settings);
                        apply_edit(doc, parser, &old_text, &dedented);
                        manual_caret = Some(Caret::at(new_cursor));
                        old_text = dedented;
                    }
                } else {
                    let word_range = word_before_cursor(&old_text, range.start);
                    let word_start_byte = char_to_byte(&old_text, word_range.start);
                    let word_end_byte = char_to_byte(&old_text, word_range.end);
                    let (language_templates, language_custom) =
                        language_template_tables(doc.language, custom_templates);
                    // `templates::GLOBAL_TEMPLATES`/`custom_templates.global` are
                    // included unconditionally, even for a file with no
                    // recognized language at all — a global trigger like `pipe`
                    // expands the same way everywhere, language-specific tables
                    // included or not.
                    let template_body = find_expansion(
                        &[language_custom, &custom_templates.global],
                        &[language_templates, templates::GLOBAL_TEMPLATES],
                        &old_text[word_start_byte..word_end_byte],
                    );

                    if template_body.is_some() || !indent_settings.use_tabs {
                        let removed = take_event(ui, |e| {
                            matches!(
                                e,
                                Event::Key {
                                    key: Key::Tab,
                                    pressed: true,
                                    ..
                                }
                            )
                        });

                        if removed.is_some() {
                            let (new_full_text, new_cursor) = if let Some(body) = template_body {
                                expand(&old_text, word_range, body)
                            } else {
                                let unit = indent_settings.unit();
                                let byte = char_to_byte(&old_text, range.start);
                                let inserted = format!("{}{unit}{}", &old_text[..byte], &old_text[byte..]);
                                (inserted, range.start + unit.chars().count())
                            };
                            apply_edit(doc, parser, &old_text, &new_full_text);
                            manual_caret = Some(Caret::at(new_cursor));
                            old_text = new_full_text;
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
                Event::Key {
                    key: key @ (Key::ArrowUp | Key::ArrowDown),
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.alt => Some((*key, modifiers.shift)),
                _ => None,
            })
        });

        if let Some((key, shift)) = alt_arrow {
            let prior_cursor = text_area::peek_caret(ui.ctx(), widget_id).map(|c| c.primary);

            if let Some(cursor_char) = prior_cursor {
                let removed = take_event(
                    ui,
                    |e| matches!(e, Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.alt && modifiers.shift == shift),
                );

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
                        manual_caret = Some(Caret::at(new_cursor));
                        old_text = new_full_text;
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
    // cursor points into it. Deliberately skipped for Ctrl+Home
    // (`modifiers.command`): that's "go to the very start of the document,"
    // not "go to this line's start," and must fall through untouched to
    // `text_area::shell`'s own `Key::Home` handling, which is what actually
    // knows how to do that.
    if !multi_cursor_active_at_start && !ui.input(|i| i.modifiers.command) {
        let home_pressed = ui.input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    Event::Key {
                        key: Key::Home,
                        pressed: true,
                        ..
                    }
                )
            })
        });

        if home_pressed {
            let prior_caret = text_area::peek_caret(ui.ctx(), widget_id);

            if let Some(caret) = prior_caret {
                let shift = ui.input(|i| i.modifiers.shift);
                let removed = take_event(ui, |e| {
                    matches!(
                        e,
                        Event::Key {
                            key: Key::Home,
                            pressed: true,
                            ..
                        }
                    )
                });

                if removed.is_some() {
                    let target = smart_home_target(&old_text, caret.primary);
                    manual_caret = Some(if shift {
                        Caret {
                            primary: target,
                            anchor: caret.anchor,
                        }
                    } else {
                        Caret::at(target)
                    });
                }
            }
        }
    }

    // Ctrl+/: toggle `//` line comments on every line the selection
    // touches (or just the cursor's line, for a collapsed selection).
    // Pre-apply, same interception shape as Tab/Alt+Arrow above — and for
    // an extra reason beyond "the widget's own handling would otherwise
    // compete with it": reading the *persisted* selection from before this
    // frame's `show` call is what makes an externally set/dragged selection
    // actually usable here. `shell_out.caret` (the post-frame value simpler
    // shortcuts like Ctrl+D/Ctrl+J read) doesn't reliably agree with it —
    // the same class of caveat `focused_frame_with_selection`'s doc comment
    // already documents for a different case.
    if !multi_cursor_active_at_start {
        let ctrl_slash_pressed = ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, Event::Key { key: Key::Slash, pressed: true, modifiers, .. } if modifiers.command))
        });

        if ctrl_slash_pressed {
            let prior_selection = text_area::peek_caret(ui.ctx(), widget_id).map(|c| c.range());

            if let Some(range) = prior_selection {
                let removed = take_event(
                    ui,
                    |e| matches!(e, Event::Key { key: Key::Slash, pressed: true, modifiers, .. } if modifiers.command),
                );

                if removed.is_some() {
                    let (toggled, sel_start, sel_end) = toggle_line_comments(&old_text, range.start, range.end);
                    apply_edit(doc, parser, &old_text, &toggled);
                    manual_caret = Some(Caret {
                        primary: sel_end,
                        anchor: sel_start,
                    });
                    old_text = toggled;
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
            let prior_selection = text_area::peek_caret(ui.ctx(), widget_id).map(|c| c.range());

            match prior_selection {
                Some(range) if !range.is_empty() => {
                    if let Some(key) =
                        keyboard_case_request.map(|_| if case == CaseConversion::Upper { Key::U } else { Key::L })
                    {
                        take_event(
                            ui,
                            |e| matches!(e, Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.command && modifiers.shift),
                        );
                    }
                    if let Some((converted, sel_start, sel_end)) =
                        convert_selection_case(&old_text, range.start, range.end, case)
                    {
                        apply_edit(doc, parser, &old_text, &converted);
                        manual_caret = Some(Caret {
                            primary: sel_end,
                            anchor: sel_start,
                        });
                        old_text = converted;
                    }
                }
                _ => {
                    *last_error = Some(t().errors.select_text_first.to_string());
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
        let prior_selection = text_area::peek_caret(ui.ctx(), widget_id).map(|c| c.range());

        if let Some(range) = prior_selection {
            let (transformed, sel_start, sel_end) = if sort_lines_request {
                sort_lines(&old_text, range.start, range.end)
            } else {
                unique_lines(&old_text, range.start, range.end)
            };
            apply_edit(doc, parser, &old_text, &transformed);
            manual_caret = Some(Caret {
                primary: sel_end,
                anchor: sel_start,
            });
            old_text = transformed;
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
            take_event(
                ui,
                |e| matches!(e, Event::Key { key: Key::W, pressed: true, modifiers, .. } if modifiers.command),
            );

            if let Some(tree) = parser.as_ref().and_then(|p| p.tree())
                && let Some(prior_range) = text_area::peek_caret(ui.ctx(), widget_id).map(|c| c.range())
            {
                let history_id = egui::Id::new(("selection_expand_history", id_salt.as_str()));
                let mut expand_state = ui
                    .ctx()
                    .data(|d| d.get_temp::<SelectionExpandState>(history_id))
                    .unwrap_or_default();

                let current = prior_range;
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
                    manual_caret = Some(Caret {
                        primary: new_range.end,
                        anchor: new_range.start,
                    });
                }

                ui.ctx().data_mut(|d| d.insert_temp(history_id, expand_state));
            }
        }
    }

    // Code folding (PLAN.md Phase 3): recomputed fresh every frame from the
    // live tree — cheap, and it means an edit that changes the tree's shape
    // needs no separate reconciliation pass, see `Document::folded_lines`'s
    // doc comment — then narrowed to just the currently-collapsed ones as
    // the line-space hidden-range list `text_area`'s `FoldMap` skips.
    let folds = foldable_ranges_for(ui, widget_id, parser.as_ref(), &doc.buffer, &old_text);
    if fold_all_request {
        folding::fold_all(&folds, &mut doc.folded_lines);
    }
    if expand_all_request {
        folding::expand_all(&mut doc.folded_lines);
    }
    let hidden = folding::hidden_ranges(&doc.buffer, &folds, &doc.folded_lines);

    // Sized to the widest line number the buffer currently has, so a
    // 9-line file gets a narrow gutter and a 10,000-line one gets a wider
    // one rather than every file paying for a fixed worst-case width. An
    // extra `FOLD_GUTTER_WIDTH` is reserved only when the file actually has
    // foldable regions, so a plain-text file's gutter stays exactly as
    // narrow as before this feature existed.
    let gutter_font_id = FontId::new(font_size, editor_font.family());
    let digit_width = ui.fonts_mut(|f| f.glyph_width(&gutter_font_id, '0'));
    let line_count = doc.buffer.len_lines().max(1);
    let fold_gutter_width = if folds.is_empty() {
        0.0
    } else {
        folding::FOLD_GUTTER_WIDTH
    };
    // Same "only reserve it when there's something to show" rule as the
    // fold column above — a file with no diff hunks (untracked, unchanged,
    // or no project open at all) keeps exactly the gutter width it had
    // before Track 9 Phase 1 existed.
    let diff_gutter_width = if doc.diff_hunks.is_empty() {
        0.0
    } else {
        diff_gutter::DIFF_GUTTER_WIDTH
    };
    let gutter_width =
        digit_width * line_count.to_string().len() as f32 + GUTTER_PADDING * 2.0 + fold_gutter_width + diff_gutter_width;

    // `text_area` shapes with real per-token colors (`HighlightSpan`) instead
    // of egui's own `layouter` closure — resolved here (against `old_text`'s
    // *not-yet-reparsed-this-frame* tree, the same timing the old layouter
    // read `parser` at, since it ran inside the same `TextEdit::show()` call
    // that any of this frame's own edits land in too) rather than inside
    // `text_area`, which stays decoupled from `syntax`/`theme` on purpose.
    let dark_mode = ui.visuals().dark_mode;
    let spans = highlight_spans_for(ui, widget_id, parser.as_ref(), &doc.buffer, &old_text, dark_mode);
    let font_id = FontId::new(font_size, editor_font.family());
    let text_color = theme::default_text(dark_mode);

    // Alt+Click adds a bare secondary cursor at the click position without
    // disturbing the primary one — captured here, *before* the widget's own
    // `show` runs, since that call's own internal click handling
    // unconditionally moves the primary cursor to wherever was just
    // clicked (Alt held or not — it doesn't know the difference). Holding
    // onto the primary selection as it stood right before that happens is
    // what lets the block below restore it afterward. `i.pointer.
    // primary_clicked()` is the cheap pre-check (mirrors `wrap_selection`'s
    // `has_candidate_keystroke` above: don't pay for a persisted-caret load
    // on every idle/non-click frame) — the actual click position comes from
    // `shell_out.base.response.interact_pointer_pos()` once it exists, below.
    let alt_click_prior_primary = if ui.input(|i| i.modifiers.alt && i.pointer.primary_clicked()) {
        text_area::peek_caret(ui.ctx(), widget_id)
    } else {
        None
    };

    // The gutter and the text field are laid out side by side, in that
    // order, inside one `horizontal` — that's what shifts the editor right
    // to make room, and what gives `paint_line_numbers` (called once
    // `shell_out` is available, alongside the other overlay painting below)
    // the gutter's left edge to right-align digits against. Both live
    // inside the *same* `ScrollArea` call site (`panels::tabs::show`), so
    // they scroll together as one unit rather than independently.
    let horizontal_response = ui.horizontal(|ui| {
        let gutter_left = ui.cursor().left();
        ui.add_space(gutter_width);
        let out = text_area::show_interactive(
            ui,
            widget_id,
            &doc.buffer,
            &old_text,
            font_id,
            text_color,
            doc.read_only,
            &spans,
            &hidden,
            view_settings.word_wrap,
            view_settings.cursor_blink,
        );
        (out, gutter_left)
    });
    // The bounding rect of the whole gutter+text row, captured before
    // `shell_out` is destructured further below — used to redraw the
    // focus-aware border `egui::TextEdit` painted around itself for free,
    // lost when this widget replaced it (PLAN.md 2g/2h) with the
    // virtualized `text_area::show_interactive` shell, which paints only
    // rows/caret and never a frame.
    let editor_rect = horizontal_response.response.rect;
    let (mut shell_out, gutter_left) = horizontal_response.inner;

    let text_changed_this_frame = shell_out.new_text.is_some();
    if let Some(raw_new_text) = shell_out.new_text.take() {
        if multi_cursor_active_at_start {
            // A mutating event that wasn't applied by the multi-cursor block
            // above — Tab, undo/redo, an IME commit, or (in principle, never
            // observed in practice — see that block's comment) no persisted
            // selection yet to apply against — reached the widget's own
            // single-cursor editing. `doc.extra_selections` is now stale
            // relative to `text`, so rather than paint/edit at wrong offsets
            // next frame, treat this as an implicit collapse back to
            // single-cursor mode.
            doc.extra_selections.clear();
        }

        let cursor_char = shell_out.caret.map(|c| c.primary);
        let (text_after_indent, indent_cursor) =
            apply_auto_indent(&old_text, &raw_new_text, cursor_char, indent_settings);
        let corrected = if indent_cursor.is_some() {
            manual_caret = indent_cursor.map(Caret::at);
            text_after_indent.into_owned()
        } else {
            let paired = apply_auto_pair(&old_text, &raw_new_text, cursor_char);
            apply_auto_pair_delete(&old_text, &paired, cursor_char)
        };

        apply_edit(doc, parser, &old_text, &corrected);
        old_text = corrected;
    }

    // Close a completion popup whose filtered list has just gone empty,
    // *before* the word-/dot-completion triggers below (not only at paint
    // time, the old timing) — otherwise a stale-but-not-yet-closed popup
    // blocks the very trigger that should fire on the same keystroke: e.g.
    // finishing "super" then typing "." both empties the old popup and
    // should open dot-completion; the old timing missed that, requiring an
    // erase-and-retype to get a clean frame.
    //
    // `has_pending_lsp` (`PLAN.md` Track 20 Phase 5) guards this the same
    // way it guards the later paint-time check below: a dot-completion
    // popup that opened with zero local candidates, waiting on a real
    // language server's own reply, is empty on every frame until that
    // reply lands — without this guard, *this* check (which runs on every
    // frame, not just the one the trigger fired on) would close it one
    // frame after it opened, before `poll_lsp` (called much later, near
    // painting) ever got a chance to merge the response in.
    if let Some(state) = completion.as_ref()
        && !state.has_pending_lsp()
    {
        let cursor_byte = shell_out.caret.map(|c| char_to_byte(&old_text, c.primary));
        if !cursor_byte.is_some_and(|b| !state.visible(&old_text, b).is_empty()) {
            *completion = None;
        }
    }

    // Word-completion's own trigger (`SPEC.md` §1b): once a just-typed
    // identifier character extends the run ending at the cursor to 2+
    // characters, open the popup with the real candidate source
    // (`word_completion_candidates`) — `Ctrl+Space` above covers the
    // shorter-run/force-open case; this is the always-on fallback. Checked
    // here, after the edit above lands, so `old_text`/`shell_out.caret`
    // reflect this frame's actual insertion rather than last frame's (a
    // multi-cursor edit's own trigger isn't handled — same "single-cursor
    // only" scope every other post-typing feature in this file already
    // has). `text_changed_this_frame` (captured before `.take()` above)
    // rules out a frame with no edit at all — e.g. the popup already open,
    // consuming its own Enter/Tab this same frame — that wouldn't have a
    // real character to react to here regardless.
    let is_spring_config_language = matches!(doc.language, Some(Language::Properties) | Some(Language::Yaml));

    if !multi_cursor_active_at_start
        && !doc.read_only
        && completion.is_none()
        && text_changed_this_frame
        && !is_spring_config_language
    {
        let typed_identifier_char = ui.input(|i| {
            i.events.iter().any(|e| {
                matches!(e, Event::Text(s) if s.chars().count() == 1
                    && s.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_'))
            })
        });

        if typed_identifier_char && let Some(cursor_char) = shell_out.caret.map(|c| c.primary) {
            let word_range = word_before_cursor(&old_text, cursor_char);
            if word_range.len() >= 2 {
                let anchor_byte = char_to_byte(&old_text, word_range.start);
                let current_run = &old_text[anchor_byte..char_to_byte(&old_text, word_range.end)];
                let candidates = word_completion_candidates(&old_text, current_run, doc.language, custom_templates);
                *completion = Some(CompletionState::open(anchor_byte, candidates));
            }
        }
    }

    // Spring config property completion (`PLAN.md` Track 12): a dedicated
    // trigger for `.properties`/`.yml` rather than routing through
    // `word_completion_candidates` above (guarded out of that block just
    // above) — dots and dashes are both real, common characters inside a
    // Spring property key segment (`context-path`, `pool-name`, real names
    // straight out of a captured `spring-configuration-metadata.json`, see
    // `fg_core::spring_config_metadata`'s own tests), neither of which
    // `word_before_cursor`'s plain alnum-or-underscore definition treats as
    // part of the same run, and a `.properties` key's dots specifically
    // must stay part of one filterable prefix (`server.po` should match
    // `server.port`), which only this dedicated path's own anchor choice —
    // whole-line for `.properties`, single-segment for `.yml` (`SPEC.md`'s
    // hierarchical-drill-down design, see `spring_config_completion`'s own
    // module doc) — gets right.
    if !multi_cursor_active_at_start
        && !doc.read_only
        && completion.is_none()
        && text_changed_this_frame
        && is_spring_config_language
        && let Some(cursor_char) = shell_out.caret.map(|c| c.primary)
    {
        let typed_key_char = ui.input(|i| {
            i.events.iter().any(|e| {
                matches!(e, Event::Text(s) if s.chars().count() == 1
                    && s.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-'))
            })
        });

        if typed_key_char {
            let cursor_byte = char_to_byte(&old_text, cursor_char);
            let line_start_byte = old_text[..cursor_byte].rfind('\n').map_or(0, |i| i + 1);
            let line_before_cursor = &old_text[line_start_byte..cursor_byte];
            // Key position only: once a `=`/`:` delimiter has been typed on
            // this line, everything after it is a value, not a key — no
            // completions there.
            let in_key_position = !line_before_cursor.contains('=') && !line_before_cursor.contains(':');

            if in_key_position {
                if let Some(project) = project {
                    spring_config.ensure_scanning(&project.root);
                }
                let properties = spring_config.properties();

                match doc.language {
                    Some(Language::Properties) => {
                        let indent = line_before_cursor.len() - line_before_cursor.trim_start().len();
                        if line_before_cursor.trim().chars().count() >= 2 {
                            let anchor_byte = line_start_byte + indent;
                            let candidates = spring_config_completion::properties_completion_candidates(properties);
                            *completion = Some(CompletionState::open(anchor_byte, candidates));
                        }
                    }
                    Some(Language::Yaml) => {
                        let segment_range = spring_config_completion::key_segment_before_cursor(&old_text, cursor_char);
                        if segment_range.len() >= 2 {
                            let anchor_byte = char_to_byte(&old_text, segment_range.start);
                            let current_line = old_text[..line_start_byte].matches('\n').count();
                            let indent = line_before_cursor.len() - line_before_cursor.trim_start().len();
                            let ancestor = spring_config_completion::yaml_ancestor_path(&old_text, current_line, indent);
                            let candidates = spring_config_completion::yaml_completion_candidates(properties, &ancestor);
                            *completion = Some(CompletionState::open(anchor_byte, candidates));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Dot-completion's own trigger (`SPEC.md` §3): typing `.` right after a
    // non-empty identifier run opens the popup anchored at the cursor
    // (right after the dot — nothing typed yet, so every resolved member
    // shows unfiltered), same "checked after this frame's edit lands"
    // timing as the word-completion trigger just above (`parser`'s tree is
    // already reparsed to include the just-typed `.` by this point,
    // `apply_edit` having run synchronously earlier this frame). Resolving
    // the receiver's type and failing to find an in-project source file
    // for it (`dot_completion_candidates` returning `None`) no longer ends
    // the story on its own (`PLAN.md` Track 20 Phase 5): a real language
    // server has no such limitation (a JDK/stdlib-typed receiver like
    // `array.`/`list.` resolves for it fine), so the popup still opens as
    // long as *either* source has something, and an LSP-only response gets
    // merged in asynchronously once it lands (`CompletionState::poll_lsp`,
    // above). Only when neither source is available at all (LSP off/not
    // `Ready`, non-Java/Kotlin, *and* no local candidates) does the popup
    // stay closed, same "degrade gracefully" fallback to word-completion's
    // own trigger above `SPEC.md` §3 already documents.
    if !multi_cursor_active_at_start && !doc.read_only && completion.is_none() && text_changed_this_frame {
        let typed_dot = ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Text(s) if s == ".")));
        if typed_dot
            && let Some(cursor_char) = shell_out.caret.map(|c| c.primary)
            && let Some(language) = doc.language
            && let Some(tree) = parser.as_ref().and_then(|p| p.tree())
        {
            let before_dot_char = cursor_char.saturating_sub(1);
            let word_range = word_before_cursor(&old_text, before_dot_char);
            if !word_range.is_empty() {
                let receiver_start = char_to_byte(&old_text, word_range.start);
                let receiver_end = char_to_byte(&old_text, word_range.end);
                let receiver = old_text[receiver_start..receiver_end].to_string();
                let local_candidates =
                    dot_completion_candidates(language, tree, &old_text, receiver_end, &receiver, project);
                let anchor_byte = char_to_byte(&old_text, cursor_char);
                let lsp_rx = matches!(language, Language::Java | Language::Kotlin)
                    .then(|| lsp.request_completion(doc, anchor_byte))
                    .flatten();
                if local_candidates.is_some() || lsp_rx.is_some() {
                    let mut state = CompletionState::open(anchor_byte, local_candidates.unwrap_or_default());
                    if let Some(rx) = lsp_rx {
                        state.set_pending_lsp(rx);
                    }
                    *completion = Some(state);
                }
            }
        }
    }

    // Spring annotation completion: typing `@` in a Java/Kotlin file opens
    // the popup unfiltered, anchored right after the `@` — same "open
    // immediately, let the ordinary prefix filter narrow it as more is
    // typed" shape dot-completion's own trigger just above uses. Guarded
    // against firing inside a comment/string (a stray `@` in `// see
    // user@example.com`, say) via the same real `syntax::highlight_spans`
    // scope data the editor's own syntax highlighting already computes
    // from the tree — recomputed fresh here rather than reusing the
    // frame's cached, already-color-resolved `HighlightSpan`s (those threw
    // away which `Scope` each span was, only keeping the resolved paint
    // color), but this only runs the one frame `@` is actually typed, not
    // every frame, so the extra pass is cheap.
    if !multi_cursor_active_at_start && !doc.read_only && completion.is_none() && text_changed_this_frame {
        let typed_at = ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Text(s) if s == "@")));
        if typed_at
            && matches!(doc.language, Some(Language::Java) | Some(Language::Kotlin))
            && let Some(cursor_char) = shell_out.caret.map(|c| c.primary)
            && let Some(language) = doc.language
            && let Some(tree) = parser.as_ref().and_then(|p| p.tree())
        {
            let anchor_byte = char_to_byte(&old_text, cursor_char);
            let at_byte = anchor_byte.saturating_sub(1);
            let inside_comment_or_string = syntax::highlight_spans(tree, &old_text, language).iter().any(|(range, scope)| {
                range.contains(&at_byte) && matches!(scope, Scope::Comment | Scope::DocComment | Scope::String)
            });

            if !inside_comment_or_string {
                let candidates = spring_annotation_completion::spring_annotation_candidates();
                *completion = Some(CompletionState::open(anchor_byte, candidates));
            }
        }
    }

    // Completes the Alt+Click interception begun above `shell_out`: place a
    // bare secondary cursor at the click position, then restore the
    // primary cursor to `alt_click_prior_primary` (undoing the move the
    // widget's own click handling just made) — same `manual_caret` override
    // mechanism every other post-frame cursor adjustment in this file uses.
    // `interact_pointer_pos()` is `None` if `alt_click_prior_primary` was
    // captured but the click actually landed outside the widget's own
    // bounds (the gutter, say), which correctly no-ops this rather than
    // adding a caret at a stale/wrong position.
    if let Some(prior_primary) = alt_click_prior_primary
        && let Some(click_pos) = shell_out.base.response.interact_pointer_pos()
    {
        let click_char = text_area::char_offset_for_pos(&shell_out.base, &doc.buffer, click_pos);

        if !doc.extra_selections.contains(&(click_char..click_char)) {
            doc.extra_selections.push(click_char..click_char);
        }
        manual_caret = Some(prior_primary);

        // Must be removed from the queue, not just acted on: the
        // multi-cursor collapse check further below
        // (`is_multi_cursor_collapse_event`) treats *any* primary-button
        // click as "user clicked somewhere, drop back to single-cursor
        // mode" — if this frame's click event were left in the queue, that
        // check would immediately wipe the very extra selection just
        // pushed above, in this same frame.
        take_event(ui, |e| {
            matches!(
                e,
                Event::PointerButton {
                    pressed: true,
                    button: egui::PointerButton::Primary,
                    ..
                }
            )
        });
    }

    let modifiers = ui.input(|i| i.modifiers);
    let ctrl_d_pressed = ui.input(|i| i.key_pressed(Key::D)) && modifiers.command;
    if ctrl_d_pressed && let Some(primary_caret) = shell_out.caret {
        if primary_caret.is_collapsed() {
            let word = multi_cursor::word_range_at(&doc.buffer.to_string(), primary_caret.primary);
            if !word.is_empty() {
                manual_caret = Some(Caret {
                    primary: word.end,
                    anchor: word.start,
                });
            }
        } else {
            let text_now = doc.buffer.to_string();
            let needle_range = primary_caret.range();
            let needle = text_now
                [char_to_byte(&text_now, needle_range.start)..char_to_byte(&text_now, needle_range.end)]
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
                manual_caret = Some(Caret {
                    primary: found.end,
                    anchor: found.start,
                });
            }
        }
    }

    let ctrl_j_pressed = ui.input(|i| i.key_pressed(Key::J)) && modifiers.command;
    if ctrl_j_pressed && let Some(primary_caret) = shell_out.caret {
        let text_now = doc.buffer.to_string();
        if let Some((joined, new_cursor)) = join_lines(&text_now, primary_caret.primary) {
            apply_edit(doc, parser, &text_now, &joined);
            manual_caret = Some(Caret::at(new_cursor));
        }
    }

    // Double-click selects the word under the (second) click; triple-click
    // selects the whole line. Neither is egui's `TextEdit` own behavior —
    // it has no click-count awareness at all (every click, regardless of
    // `i.pointer.button_double_clicked`/`_triple_clicked`, just collapses
    // the cursor to that position) — so both are applied here, purely as a
    // post-frame *selection* override: `show_interactive` already ran this
    // same frame and positioned `shell_out.caret` at the click (both clicks
    // of a double-click land the primary cursor at the same spot), so there
    // is no click position of our own to compute — same "read the caret
    // `show_interactive` already produced" timing every other post-frame
    // block here uses, just turning a collapsed cursor into a selection
    // instead of moving it. Triple-click is checked first since egui counts
    // a third click as *both* triple and double (each click within the
    // window bumps the running count, so `_triple_clicked` and `_double_
    // clicked` can be true the same frame) — line selection is what a
    // third click means, not word selection.
    if let Some(primary_caret) = shell_out.caret {
        let triple_clicked = ui.input(|i| i.pointer.button_triple_clicked(egui::PointerButton::Primary));
        let double_clicked = ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary));

        if triple_clicked {
            let chars: Vec<char> = old_text.chars().collect();
            let (line_start, line_end) = current_line_range(&chars, primary_caret.primary);
            manual_caret = Some(Caret {
                primary: line_end,
                anchor: line_start,
            });
        } else if double_clicked {
            let word = multi_cursor::word_range_at(&old_text, primary_caret.primary);
            if !word.is_empty() {
                manual_caret = Some(Caret {
                    primary: word.end,
                    anchor: word.start,
                });
            }
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
            *last_error = Some(t().errors.accessors_java_only.to_string());
        } else if let Some(tree) = parser.as_ref().and_then(|p| p.tree()) {
            let text_now = doc.buffer.to_string();
            let classes = syntax::java_classes_with_fields(tree, &text_now);
            match classes.len() {
                0 => *last_error = Some(t().errors.no_class_fields.to_string()),
                1 => {
                    let generated = generate_accessors(&classes[0].fields, &indent_settings.unit(), kind);
                    if generated.is_empty() {
                        // Only reachable for `AccessorKind::Setters` when
                        // every field found is `final`.
                        *last_error = Some(t().errors.every_field_is_final.to_string());
                    } else {
                        let (inserted, new_cursor) =
                            insert_at_class_end(&text_now, classes[0].insertion_byte, &generated);
                        apply_edit(doc, parser, &text_now, &inserted);
                        manual_caret = Some(Caret::at(new_cursor));
                    }
                }
                _ => *generate_dialog = Some(GenerateAccessorsDialog::new(classes, kind)),
            }
        } else {
            *last_error = Some(t().errors.accessors_no_tree.to_string());
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
                    manual_caret = Some(Caret::at(new_cursor));
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
            *last_error = Some(t().errors.generate_java_only.to_string());
        } else if let Some(tree) = parser.as_ref().and_then(|p| p.tree()) {
            let text_now = doc.buffer.to_string();
            let classes = syntax::java_classes_with_fields(tree, &text_now);
            match classes.len() {
                0 => *last_error = Some(t().errors.no_class_fields.to_string()),
                1 => {
                    let generated =
                        codegen::generate_method(&classes[0].name, &classes[0].fields, &indent_settings.unit(), kind);
                    let (inserted, new_cursor) = insert_at_class_end(&text_now, classes[0].insertion_byte, &generated);
                    apply_edit(doc, parser, &text_now, &inserted);
                    manual_caret = Some(Caret::at(new_cursor));
                }
                _ => *generate_method_dialog = Some(GenerateMethodDialog::new(classes, kind)),
            }
        } else {
            *last_error = Some(t().errors.generate_no_tree.to_string());
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
            manual_caret = Some(Caret::at(new_cursor));
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
            *last_error = Some(t().errors.override_java_only.to_string());
        } else if let Some(tree) = parser.as_ref().and_then(|p| p.tree()) {
            let text_now = doc.buffer.to_string();
            let cursor_byte = shell_out.caret.map(|c| char_to_byte(&text_now, c.primary));
            let enclosing = cursor_byte.and_then(|c| syntax::enclosing_class(tree, &text_now, c));

            match enclosing {
                None => *last_error = Some(t().errors.override_needs_class.to_string()),
                Some((class_name, insertion_byte)) => match syntax::superclass_name(tree, &text_now, &class_name) {
                    None => {
                        *last_error = Some(msg::no_superclass(&class_name));
                    }
                    Some(super_name) => {
                        let super_path = project.and_then(|p| codegen::find_java_file_by_stem(&p.tree, &super_name));
                        match super_path {
                            None => {
                                *last_error = Some(msg::superclass_not_in_project(&super_name));
                            }
                            Some(super_path) => match std::fs::read_to_string(&super_path) {
                                Err(err) => {
                                    *last_error = Some(msg::failed_to_read(&super_path.display().to_string(), &err.to_string()));
                                }
                                Ok(super_source) => {
                                    let mut super_parser = IncrementalParser::new(Language::Java);
                                    let super_tree = super_parser.parse(&super_source);
                                    let inherited = syntax::methods_in_type(super_tree, &super_source, &super_name);
                                    let already_here = syntax::methods_in_type(tree, &text_now, &class_name);
                                    let candidates: Vec<_> = inherited
                                        .into_iter()
                                        .filter(|m| {
                                            !already_here.iter().any(|existing| {
                                                existing.name == m.name && existing.params.len() == m.params.len()
                                            })
                                        })
                                        .collect();

                                    if candidates.is_empty() {
                                        *last_error = Some(msg::no_overridable_methods(&super_name));
                                    } else {
                                        *override_method_dialog =
                                            Some(OverrideMethodDialog::new(candidates, insertion_byte));
                                    }
                                }
                            },
                        }
                    }
                },
            }
        } else {
            *last_error = Some(t().errors.override_no_tree.to_string());
        }
    }

    if override_method_dialog.is_some() {
        let doc_text_for_dialog = doc.buffer.to_string();
        if let Some(outcome) = codegen::show_override_method_dialog(
            ui,
            override_method_dialog,
            &doc_text_for_dialog,
            &indent_settings.unit(),
        ) {
            match outcome {
                Ok((inserted, new_cursor)) => {
                    apply_edit(doc, parser, &doc_text_for_dialog, &inserted);
                    manual_caret = Some(Caret::at(new_cursor));
                }
                Err(message) => *last_error = Some(message),
            }
        }
    }

    // Right-click context menu: see `context_menu::show_context_menu`.
    // `shell_out.caret` is copied out before calling it so that function
    // doesn't need to borrow `shell_out` at all — `Caret` is `Copy`, so this
    // costs nothing.
    context_menu::show_context_menu(
        &shell_out.base.response,
        widget_id,
        doc,
        parser,
        shell_out.caret,
        &mut old_text,
        &mut manual_caret,
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

    // Completion popup: post-frame close check + render (`SPEC.md` §0).
    // Reads `shell_out.caret`/`old_text` — the cursor and buffer as they
    // stand *after* this frame's own typing/edits landed, the same timing
    // the occurrence highlight below reads them at — so a keystroke that
    // narrows the filtered list to nothing closes the popup the same frame
    // it happens, rather than one frame late. `editor_rect` (the gutter+
    // text row captured above) stands in for "the editor pane" `popup_
    // position` clamps against.
    // A `textDocument/completion` reply (`PLAN.md` Track 20 Phase 5) can
    // land at any time while the popup is open, not just in reaction to a
    // keystroke — polled here, before the "is there anything to show"
    // check right below, so a response arriving this exact frame can turn
    // an until-now-empty popup (a JDK-typed receiver with no local
    // candidates) non-empty without waiting an extra frame.
    if let Some(state) = completion.as_mut() {
        state.poll_lsp();
    }
    if let Some(state) = completion.as_ref() {
        match shell_out.caret.map(|c| char_to_byte(&old_text, c.primary)) {
            Some(cursor_byte) if !state.visible(&old_text, cursor_byte).is_empty() => {
                let popup_id = egui::Id::new(("completion_popup", widget_id));
                state.paint(
                    ui,
                    popup_id,
                    &shell_out.base,
                    &doc.buffer,
                    &old_text,
                    cursor_byte,
                    editor_rect,
                );
            }
            // Empty right now, but an in-flight `textDocument/completion`
            // reply could still populate it (this is exactly the JDK/
            // stdlib-typed-receiver case that has no local candidates at
            // all until the server answers) — stay open with nothing
            // painted this frame instead of closing before that reply ever
            // had a chance to arrive.
            Some(_) if state.has_pending_lsp() => {}
            _ => *completion = None,
        }
    }

    // Hover docs (`PLAN.md` Track 20 Phase 3): tracks the pointer's dwell
    // time over whatever identifier it's currently sitting on and fires a
    // `textDocument/hover` request once it's rested there long enough (see
    // `hover::HoverState::update`'s own doc comment). Skipped entirely
    // while the completion popup is open — the two would otherwise
    // visually collide over the same screen area for no useful combination,
    // the same reasoning `SPEC.md` never has both a completion popup and a
    // hover tooltip open at once for any real editor. `hover_pos()` (not
    // `interact_pointer_pos()`) is what egui's own tooltips key off too:
    // `None` while a drag (a text selection) is in progress, which is
    // exactly when a hover popup would be the most unwelcome.
    if completion.is_some() {
        hover.clear();
    } else {
        let hover_id = egui::Id::new(("hover_popup", widget_id));
        // The tooltip is anchored directly beneath the identifier the
        // pointer is on (`completion::popup_position`), so a couple of
        // pixels of downward drift puts the pointer inside the tooltip's
        // own `Order::Foreground` area — at which point egui stops
        // reporting the text area underneath as hovered at all, `update`
        // below would drop the tracked hover, and the tooltip the user was
        // reaching for would vanish and only come back after another full
        // `HOVER_DELAY`. Freezing the whole hover state while the pointer
        // is over the tooltip breaks that flicker loop. Gated on
        // `has_content` because `area_rect` keeps answering with the last
        // rect an id was shown at, which would otherwise let a stale
        // rectangle from an already-dismissed tooltip freeze this forever.
        let pointer_over_tooltip = hover.has_content()
            && ui.ctx().pointer_hover_pos().is_some_and(|pos| {
                ui.ctx().memory(|mem| mem.area_rect(hover_id)).is_some_and(|rect| rect.contains(pos))
            });
        if !pointer_over_tooltip {
            let hovered = shell_out
                .base
                .response
                .hover_pos()
                .and_then(|pos| super::hover::hovered_span(&shell_out.base, &doc.buffer, pos));
            hover.update(doc, hovered, lsp);
        }
        hover.paint(ui, hover_id, &shell_out.base, &doc.buffer, editor_rect);
    }

    // While Ctrl is held over an identifier the Ctrl+Click below would
    // actually act on, swap the cursor to a pointing hand — the same
    // hovered-span test the click handler itself uses (`hover_pos()`, not
    // `interact_pointer_pos()`, so this reacts to the pointer just resting
    // there, no click needed), giving the same "this is clickable" cue a
    // browser gives over a real hyperlink before Ctrl+Click resolves
    // anywhere. `set_cursor_icon` only takes effect for the rest of this
    // frame — egui resets it to the default every frame — so this has to
    // run on every frame the condition holds, not just once.
    if ui.input(|i| i.modifiers.command)
        && let Some(pos) = shell_out.base.response.hover_pos()
        && super::hover::hovered_span(&shell_out.base, &doc.buffer, pos).is_some()
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // Go to definition (`PLAN.md` Track 20 Phase 4): Ctrl+Click an
    // identifier to request `textDocument/definition` and jump there via
    // `pending_navigation` (`app.rs`), the same cross-tab jump primitive
    // the Spring endpoint map and a clicked compiler-error row both already
    // use. Resolving *which* identifier was clicked reuses `hover`'s own
    // span logic (`hovered_span`) rather than duplicating it — the "is the
    // pointer really on a symbol, not just nearest one" problem is
    // identical either way, just against `interact_pointer_pos()` (the
    // click's own position) instead of `hover_pos()`. `goto_definition`
    // itself only tracks the async request/reply; nothing is painted here.
    if ui.input(|i| i.modifiers.command && i.pointer.primary_clicked())
        && let Some(pos) = shell_out.base.response.interact_pointer_pos()
        && let Some(span) = super::hover::hovered_span(&shell_out.base, &doc.buffer, pos)
    {
        let text = doc.buffer.to_string();
        goto_definition.request(doc, char_to_byte(&text, span.start), lsp);
    }

    // Peek definition (`PLAN.md` Track 17 Phase 1): Alt+F12 requests
    // `textDocument/definition` at the current caret and shows the result
    // inline via `peek.paint` below, rather than jumping there — unlike
    // Ctrl+Click above, this never touches `pending_navigation` or opens/
    // switches a tab, so the main editor's own tab and scroll position stay
    // completely undisturbed, exactly the point of a *peek*. `text_area::
    // peek_caret` (not `shell_out.caret`, which is only set on an edit/
    // click this exact frame) is this widget's own "read the persisted
    // caret regardless of what happened this frame" accessor — the same one
    // `alt_click_prior_primary` above already uses for the same reason.
    if ui.input(|i| i.modifiers.alt && i.key_pressed(Key::F12))
        && let Some(caret) = text_area::peek_caret(ui.ctx(), widget_id)
    {
        let text = doc.buffer.to_string();
        peek.request(doc, caret.primary, char_to_byte(&text, caret.primary), lsp);
    }
    peek.update(lsp, doc);
    peek.paint(ui, egui::Id::new(("peek_popup", widget_id)), &shell_out.base, &doc.buffer, editor_rect);

    // Find references (`PLAN.md` Track 20 Phase 6): Shift+F12 requests
    // `textDocument/references` at the current caret and lists every hit
    // in a popup (`find_references.paint` below) — same caret-source
    // reasoning as Alt+F12's own Peek definition just above.
    if ui.input(|i| i.modifiers.shift && i.key_pressed(Key::F12))
        && let Some(caret) = text_area::peek_caret(ui.ctx(), widget_id)
    {
        let text = doc.buffer.to_string();
        find_references.request(doc, caret.primary, char_to_byte(&text, caret.primary), lsp);
    }
    find_references.update(lsp, doc);
    find_references.paint(ui, egui::Id::new(("find_references_popup", widget_id)), &shell_out.base, &doc.buffer, editor_rect);

    // Rename symbol (`PLAN.md` Track 20 Phase 7): F2 opens an inline "new
    // name" box pre-filled with the identifier under the caret —
    // `rename_box.paint` below renders it and hands a confirmed Enter
    // back through `take_confirmed`, which `app.rs` polls once a frame to
    // actually fire `textDocument/rename` (`crate::rename::RenameState`,
    // an app.rs-level concern since applying the reply's `WorkspaceEdit`
    // can touch files well beyond this one tab).
    if ui.input(|i| i.key_pressed(Key::F2))
        && let Some(caret) = text_area::peek_caret(ui.ctx(), widget_id)
    {
        rename_box.start(doc, caret.primary);
    }
    rename_box.paint(ui, egui::Id::new(("rename_box", widget_id)), &shell_out.base, &doc.buffer, editor_rect);

    // Passive, read-only highlight of every occurrence of the word under
    // (or touching) the cursor — distinct from `Ctrl+D`'s *active*
    // multi-cursor editing, so it only applies with a collapsed cursor and
    // no multi-cursor selections active, to avoid competing visually with
    // either. Uses `shell_out.caret` (post-frame), the same source
    // Ctrl+D/Ctrl+J already read for real, non-test usage — this is purely
    // a display concern, not an edit, so there's no interception timing to
    // get right here.
    if doc.extra_selections.is_empty()
        && let Some(primary_caret) = shell_out.caret
        && primary_caret.is_collapsed()
    {
        let word_range = multi_cursor::word_range_at(&old_text, primary_caret.primary);
        if !word_range.is_empty() {
            let word = &old_text[char_to_byte(&old_text, word_range.start)..char_to_byte(&old_text, word_range.end)];
            let occurrences = occurrences_for(ui, widget_id, &doc.buffer, &old_text, word);
            paint_occurrence_highlights(ui, &shell_out.base, &doc.buffer, &occurrences);
        }
    }

    // Bracket-pair highlighting: same collapsed-cursor gate as the
    // occurrence highlight above (a selection means there's no single
    // cursor position to check bracket-adjacency against), plus requiring
    // a parsed tree — brackets are a tree-sitter-derived concept, so a file
    // with no recognized language (no `parser`) just skips this, same as
    // every other tree-driven feature in this file.
    if doc.extra_selections.is_empty()
        && let Some(primary_caret) = shell_out.caret
        && primary_caret.is_collapsed()
        && let Some(tree) = parser.as_ref().and_then(|p| p.tree())
    {
        let cursor_byte = char_to_byte(&old_text, primary_caret.primary);
        if let Some(pair) = syntax::bracket_match(tree, &old_text, cursor_byte) {
            paint_bracket_match(ui, &shell_out.base, &doc.buffer, pair);
        }
    }

    if view_settings.show_indent_guides {
        paint_indent_guides(ui, &shell_out.base, &doc.buffer, indent_settings);
    }
    if view_settings.show_whitespace {
        paint_whitespace(ui, &shell_out.base, &doc.buffer);
    }

    // Several independent sources feeding one squiggle pipeline (see
    // `Document::checkstyle_diagnostics`'s own doc comment for why they're
    // separate fields) — collected into one slice here, at paint time,
    // rather than a fifth stored field, since the cost scales with this
    // one document's own diagnostic count, not project size.
    let all_diagnostics: Vec<Diagnostic> = doc
        .diagnostics
        .iter()
        .chain(doc.checkstyle_diagnostics.iter())
        .chain(doc.pmd_diagnostics.iter())
        .chain(doc.spotbugs_diagnostics.iter())
        .chain(doc.lsp_diagnostics.iter())
        .cloned()
        .collect();
    paint_diagnostics(ui, &shell_out.base, &doc.buffer, &old_text, &all_diagnostics);
    paint_extra_selections(ui, &shell_out.base, &doc.buffer, &doc.extra_selections);
    // The diff bar sits flush against the text's own left edge (the
    // innermost sliver of the gutter, reserved above via `diff_gutter_
    // width`); line numbers right-align against what's left of the gutter
    // once that sliver is set aside, so their own position is unaffected by
    // whether a diff bar is present this frame or not.
    paint_line_numbers(
        ui,
        &shell_out.base,
        gutter_left + gutter_width - diff_gutter_width - GUTTER_PADDING,
        gutter_font_id,
        ui.visuals().dark_mode,
    );
    diff_gutter::paint_diff_gutter(ui, &shell_out.base, &doc.diff_hunks, gutter_left + gutter_width, dark_mode);
    if view_settings.show_inline_blame
        && let Some(primary_caret) = shell_out.caret
    {
        let cursor_line = doc.buffer.char_to_line(primary_caret.primary.min(doc.buffer.len_chars()));
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        paint_blame_annotation(
            ui,
            &shell_out.base,
            &doc.blame,
            cursor_line,
            now_unix,
            FontId::new(font_size, editor_font.family()),
            dark_mode,
        );
    }
    folding::show_fold_gutter(
        ui,
        &shell_out.base,
        &folds,
        &mut doc.folded_lines,
        &id_salt,
        gutter_left,
        dark_mode,
    );
    folding::paint_collapsed_markers(ui, &shell_out.base, &folds, &doc.folded_lines, dark_mode);

    // Sticky scroll: pin the enclosing class/method header line(s) at the top
    // of the viewport while their body scrolls under them. Painted last so it
    // occludes everything else at the top edge (that's the point — the band
    // is opaque). Tree-driven (Java only today), so a no-parser file skips it
    // like every other tree feature.
    //
    // The top visible *logical* line is found by hit-testing at the
    // viewport's top edge (`char_offset_for_pos`, the same inverse the
    // Alt+Click path uses) and mapping that char to its line — not by
    // counting shaped rows. That distinction matters once Phase 4 brings
    // word-wrap back to this widget: a wrapped line spans several rows but
    // is one logical line, so a raw row count would drift and pin the wrong
    // header, where hit-testing stays wrap-correct. The enclosing scopes
    // come from the reparsed tree via `doc.buffer`, whose line indices
    // match the displayed text's (auto-indent/auto-pair only ever add
    // characters, never lines, so the two agree even if they differ by a few
    // whitespace bytes on the exact frame of such an edit).
    if view_settings.show_sticky_scroll
        && let Some((tree, language)) = parser.as_ref().and_then(|p| p.tree().map(|t| (t, p.language())))
    {
        let top_pos = egui::pos2(shell_out.base.content_origin.x, ui.clip_rect().top());
        let top_char = text_area::char_offset_for_pos(&shell_out.base, &doc.buffer, top_pos);
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
                let headers: Vec<String> = pin_lines
                    .iter()
                    .map(|&line| doc.buffer.line(line).chars().collect())
                    .collect();
                let sticky_font = FontId::new(font_size, editor_font.family());
                paint_sticky_scroll(ui, &shell_out.base, &headers, sticky_font, ui.visuals().dark_mode);
            }
        }
    }

    // Redraws the border `egui::TextEdit` used to paint around itself —
    // highlighted while focused, subdued otherwise — same stroke choice
    // `TextEdit`'s own frame logic makes (`ui.visuals().selection.stroke`
    // focused, `widgets.inactive.bg_stroke` otherwise), so swapping to the
    // virtualized shell didn't also silently drop this cue that the pane is
    // (or isn't) the one keystrokes go to. Painted last so it isn't drawn
    // over by sticky scroll or any other overlay above.
    if view_settings.show_editor_outline {
        let stroke = if shell_out.caret.is_some() {
            ui.visuals().selection.stroke
        } else {
            ui.visuals().widgets.inactive.bg_stroke
        };
        // `Inside`, not `Outside`: the top and left edges of `editor_rect`
        // sit exactly on the surrounding `ScrollArea`/`horizontal`'s own
        // clip rect, so a stroke drawn *outside* those edges (half past the
        // boundary) was silently clipped away there — only the bottom/right
        // edges (with slack beyond them) ever showed up.
        ui.painter().rect_stroke(
            editor_rect,
            ui.visuals().widgets.inactive.corner_radius,
            stroke,
            egui::StrokeKind::Inside,
        );
    }

    // Auto-indent inserts content *before* where the widget placed the
    // cursor (unlike auto-pair, which only ever inserts after it), so the
    // cursor needs to be pushed forward past the inserted indentation
    // manually. The multi-cursor paths above reuse the same mechanism to
    // land the primary cursor after a multi-edit or a Ctrl+D word/
    // occurrence jump.
    if let Some(caret) = manual_caret {
        text_area::set_caret(ui.ctx(), widget_id, caret);
    }
}

/// Overrides the persisted caret for the document at `path` to `char_offset`
/// and gives its widget keyboard focus — the Spring endpoint map's
/// jump-to-handler (PLAN.md Phase 4), and the one place outside this module
/// needs to reach into `text_area`'s otherwise-internal caret storage. Wraps
/// `text_area::set_caret`/`Caret::at` and the exact same `Id::new`
/// computation `show`'s own `widget_id` uses, so a caller (`app.rs`) never
/// needs to touch `Caret`/`text_area` — both stay `pub(super)`, reachable
/// only from within `widgets::editor` — or duplicate that `Id` computation
/// itself.
///
/// The focus request matters as much as the caret move: `shell::show`'s own
/// `has_focus` only ever becomes true from a real click/drag on the widget
/// or already having had focus last frame (see its own doc comment on read-
/// before-shape timing) — nothing about moving the persisted caret alone
/// makes a widget focused, so without this, a jump would land the cursor at
/// the right position but leave the pane looking (and behaving) unfocused,
/// e.g. no blinking caret, arrow keys not landing there. Called ahead of
/// `tabs::show`/`editor::show` in the same frame (see `app.rs`'s own call
/// site), so `ui.memory(|m| m.has_focus(id))` already sees it as focused
/// the very first time that document's widget runs this frame — no extra
/// frame of delay needed for focus, unlike the caret move itself.
pub fn jump_to(ctx: &egui::Context, path: &std::path::Path, char_offset: usize) {
    let widget_id = egui::Id::new(path.to_string_lossy().into_owned());
    text_area::set_caret(ctx, widget_id, Caret::at(char_offset));
    ctx.memory_mut(|m| m.request_focus(widget_id));
}

/// The active language's built-in live-template table and any user-defined
/// overrides for it (`custom_templates.java`/`.kotlin`) — the pairing both
/// the Tab-trigger interception and the completion popup's `Template`
/// candidate sourcing/acceptance need, factored out once a second and third
/// call site wanted the identical `match language { ... }` this used to be
/// inlined just once for.
fn language_template_tables(
    language: Option<Language>,
    custom_templates: &UserTemplates,
) -> (&'static [templates::Template], &[templates::UserTemplate]) {
    match language {
        Some(Language::Java) => (templates::JAVA_TEMPLATES, &custom_templates.java),
        Some(Language::Kotlin) => (templates::KOTLIN_TEMPLATES, &custom_templates.kotlin),
        _ => (&[], &[]),
    }
}

/// Word-completion's real candidate source (`SPEC.md` §1b/§1c), fed into
/// `CompletionState::open` by both the `Ctrl+Space` and just-typed-character
/// triggers above: every distinct identifier-shaped token already in `text`
/// (`templates::identifiers_in`, excluding `current_run` — the word
/// currently being typed itself), the active language's live-template
/// triggers (built-in + user-defined, plus the always-on global group) as
/// `Template` candidates, and — Java/Kotlin only — the language's own
/// keyword list as `Keyword` candidates.
fn word_completion_candidates(
    text: &str,
    current_run: &str,
    language: Option<Language>,
    custom_templates: &UserTemplates,
) -> Vec<CompletionItem> {
    let mut candidates: Vec<CompletionItem> = templates::identifiers_in(text)
        .into_iter()
        .filter(|word| word != current_run)
        .map(|label| CompletionItem {
            label,
            kind: CompletionKind::Word,
            detail: None,
            has_params: false,
        })
        .collect();

    let (language_templates, language_custom) = language_template_tables(language, custom_templates);
    let template_labels = language_templates
        .iter()
        .map(|t| t.trigger.to_string())
        .chain(language_custom.iter().map(|t| t.trigger.clone()))
        .chain(templates::GLOBAL_TEMPLATES.iter().map(|t| t.trigger.to_string()))
        .chain(custom_templates.global.iter().map(|t| t.trigger.clone()));
    candidates.extend(template_labels.map(|label| CompletionItem {
        label,
        kind: CompletionKind::Template,
        detail: None,
        has_params: false,
    }));

    let keywords: &[&str] = match language {
        Some(Language::Java) => templates::JAVA_KEYWORDS,
        Some(Language::Kotlin) => templates::KOTLIN_KEYWORDS,
        _ => &[],
    };
    candidates.extend(keywords.iter().map(|&label| CompletionItem {
        label: label.to_string(),
        kind: CompletionKind::Keyword,
        detail: None,
        has_params: false,
    }));

    candidates
}

/// Dot-completion's candidate source, dispatched by language — the one
/// entry point the trigger above calls. Every other language has none.
fn dot_completion_candidates(
    language: Language,
    tree: &Tree,
    source: &str,
    cursor_byte: usize,
    receiver: &str,
    project: Option<&Project>,
) -> Option<Vec<CompletionItem>> {
    match language {
        Language::Java => java_dot_completion_candidates(tree, source, cursor_byte, receiver, project),
        Language::Kotlin => kotlin_dot_completion_candidates(tree, source, cursor_byte, receiver, project),
        _ => None,
    }
}

/// Java's dot-completion candidates: `this.`/`super.` go through
/// `syntax::enclosing_class`/`superclass_name` to an *unfiltered* member
/// listing (own class sees everything). A bare identifier resolves via
/// `type_of_identifier_java`; if it has a project source file, its members
/// come from `methods_in_type`'s external-visibility filtering, plus one
/// level of inherited members via the same file-finder chain "Override
/// Method" uses. Anything unresolvable is `None` — don't open the popup,
/// not an error.
fn java_dot_completion_candidates(
    tree: &Tree,
    source: &str,
    cursor_byte: usize,
    receiver: &str,
    project: Option<&Project>,
) -> Option<Vec<CompletionItem>> {
    let (class_name, _) = syntax::enclosing_class(tree, source, cursor_byte)?;

    if receiver == "this" {
        return Some(java_members_as_items(tree, source, &class_name, true));
    }

    if receiver == "super" {
        let super_name = syntax::superclass_name(tree, source, &class_name)?;
        let super_path = project.and_then(|p| codegen::find_source_file_by_stem(&p.tree, &super_name, "java"))?;
        let super_source = std::fs::read_to_string(&super_path).ok()?;
        let mut super_parser = IncrementalParser::new(Language::Java);
        let super_tree = super_parser.parse(&super_source).clone();
        return Some(java_members_as_items(&super_tree, &super_source, &super_name, true));
    }

    let type_name = syntax::type_of_identifier_java(tree, source, cursor_byte, receiver)?;
    let type_path = project.and_then(|p| codegen::find_source_file_by_stem(&p.tree, &type_name, "java"))?;
    let type_source = std::fs::read_to_string(&type_path).ok()?;
    let mut type_parser = IncrementalParser::new(Language::Java);
    let type_tree = type_parser.parse(&type_source).clone();

    let mut items = java_members_as_items(&type_tree, &type_source, &type_name, false);

    if let Some(super_name) = syntax::superclass_name(&type_tree, &type_source, &type_name)
        && let Some(super_path) = project.and_then(|p| codegen::find_source_file_by_stem(&p.tree, &super_name, "java"))
        && let Ok(super_source) = std::fs::read_to_string(&super_path)
    {
        let mut super_parser = IncrementalParser::new(Language::Java);
        let super_tree = super_parser.parse(&super_source).clone();
        items.extend(java_members_as_items(&super_tree, &super_source, &super_name, false));
    }

    Some(items)
}

/// `type_name`'s own fields and methods as `CompletionItem`s. `unfiltered`
/// selects `fields_in_type`'s `include_static` and `all_methods_in_type`
/// over `methods_in_type`, for the `this.`/`super.` case.
fn java_members_as_items(tree: &Tree, source: &str, type_name: &str, unfiltered: bool) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = syntax::fields_in_type(tree, source, type_name, unfiltered)
        .into_iter()
        .map(|f| CompletionItem {
            label: f.name,
            kind: CompletionKind::Field,
            detail: Some(f.java_type),
            has_params: false,
        })
        .collect();

    let methods = if unfiltered {
        syntax::all_methods_in_type(tree, source, type_name)
    } else {
        syntax::methods_in_type(tree, source, type_name)
    };
    items.extend(methods.into_iter().map(|m| CompletionItem {
        label: m.name,
        kind: CompletionKind::Method,
        detail: Some(m.return_type),
        has_params: !m.params.is_empty(),
    }));

    items
}

/// Kotlin's dot-completion candidates — same shape as
/// `java_dot_completion_candidates`, just `kotlin_enclosing_class`/
/// `kotlin_superclass_name`/`type_of_identifier_kotlin` in place of the
/// Java equivalents.
fn kotlin_dot_completion_candidates(
    tree: &Tree,
    source: &str,
    cursor_byte: usize,
    receiver: &str,
    project: Option<&Project>,
) -> Option<Vec<CompletionItem>> {
    let class_name = syntax::kotlin_enclosing_class(tree, source, cursor_byte)?;

    if receiver == "this" {
        return Some(kotlin_members_as_items(tree, source, &class_name, true));
    }

    if receiver == "super" {
        let super_name = syntax::kotlin_superclass_name(tree, source, &class_name)?;
        let super_path = project.and_then(|p| codegen::find_source_file_by_stem(&p.tree, &super_name, "kt"))?;
        let super_source = std::fs::read_to_string(&super_path).ok()?;
        let mut super_parser = IncrementalParser::new(Language::Kotlin);
        let super_tree = super_parser.parse(&super_source).clone();
        return Some(kotlin_members_as_items(&super_tree, &super_source, &super_name, true));
    }

    let type_name = syntax::type_of_identifier_kotlin(tree, source, cursor_byte, receiver)?;
    let type_path = project.and_then(|p| codegen::find_source_file_by_stem(&p.tree, &type_name, "kt"))?;
    let type_source = std::fs::read_to_string(&type_path).ok()?;
    let mut type_parser = IncrementalParser::new(Language::Kotlin);
    let type_tree = type_parser.parse(&type_source).clone();

    let mut items = kotlin_members_as_items(&type_tree, &type_source, &type_name, false);

    if let Some(super_name) = syntax::kotlin_superclass_name(&type_tree, &type_source, &type_name)
        && let Some(super_path) = project.and_then(|p| codegen::find_source_file_by_stem(&p.tree, &super_name, "kt"))
        && let Ok(super_source) = std::fs::read_to_string(&super_path)
    {
        let mut super_parser = IncrementalParser::new(Language::Kotlin);
        let super_tree = super_parser.parse(&super_source).clone();
        items.extend(kotlin_members_as_items(&super_tree, &super_source, &super_name, false));
    }

    Some(items)
}

/// `type_name`'s own properties and functions as `CompletionItem`s.
/// `unfiltered` selects `all_kotlin_functions_in_type` over
/// `kotlin_functions_in_type`, for `this.`/`super.`; properties aren't
/// visibility-filtered at all, mirroring Java's `fields_in_type`.
fn kotlin_members_as_items(tree: &Tree, source: &str, type_name: &str, unfiltered: bool) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = syntax::kotlin_properties_in_type(tree, source, type_name)
        .into_iter()
        .map(|f| CompletionItem {
            label: f.name,
            kind: CompletionKind::Field,
            detail: Some(f.java_type),
            has_params: false,
        })
        .collect();

    let functions = if unfiltered {
        syntax::all_kotlin_functions_in_type(tree, source, type_name)
    } else {
        syntax::kotlin_functions_in_type(tree, source, type_name)
    };
    items.extend(functions.into_iter().map(|m| CompletionItem {
        label: m.name,
        kind: CompletionKind::Method,
        detail: Some(m.return_type),
        has_params: !m.params.is_empty(),
    }));

    items
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
/// worth loading the persisted caret for" check and the actual removal, so
/// the two can't drift apart on what counts as a match.
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
            Event::Key {
                key: Key::Backspace, ..
            } => return MultiEditOp::Backspace,
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
mod widget_test;
