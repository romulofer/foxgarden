use std::borrow::Cow;

use super::text_offset::char_to_byte;
use crate::style::indent::IndentSettings;

/// Auto-indents after Enter: matches the new line's indentation to the line
/// just ended, plus one extra level (per `indent_settings`) if that line
/// ends in `{`. Only fires on a pure single-character insertion of `\n`
/// (same guard as `apply_auto_pair`, for the same reasons — pastes/IME/
/// selection-replace are left alone). Returns `(Cow::Borrowed(text), None)`
/// unchanged if it doesn't apply — called on every text-changing keystroke
/// (`widget.rs`'s call site), not just Enter, so the overwhelmingly common
/// case is this exact no-op, and it used to pay for an unconditional
/// `text.to_string()` clone (SPEC.md §3) that the caller then immediately
/// discarded whenever indentation wasn't actually inserted.
pub(super) fn apply_auto_indent<'t>(
    old_text: &str,
    text: &'t str,
    cursor_char: Option<usize>,
    indent_settings: IndentSettings,
) -> (Cow<'t, str>, Option<usize>) {
    // A single inserted char is 1-4 UTF-8 bytes, so a byte-length delta
    // outside that range can't be a single-char insertion — this rules out
    // deletes, pastes, and multi-char IME commits with one O(1) length
    // comparison, before paying for the O(n) `.chars()` counts below. A
    // delta *inside* 1..=4 still needs those counts: e.g. replacing a
    // 2-byte/1-char selection with a 3-byte/3-char one nets the same +1
    // byte delta as typing a single ASCII character would, so the byte
    // delta alone only ever rules cases *out*, never confirms one in.
    let byte_delta = text.len() as isize - old_text.len() as isize;
    if !(1..=4).contains(&byte_delta) {
        return (Cow::Borrowed(text), None);
    }

    let old_chars = old_text.chars().count();
    let new_chars = text.chars().count();
    if new_chars != old_chars + 1 {
        return (Cow::Borrowed(text), None);
    }
    let Some(cursor_char) = cursor_char.filter(|&c| c > 0 && c <= new_chars) else {
        return (Cow::Borrowed(text), None);
    };

    let inserted_start = char_to_byte(text, cursor_char - 1);
    let inserted_end = char_to_byte(text, cursor_char);
    let inserted = text[inserted_start..inserted_end]
        .chars()
        .next()
        .expect("cursor_char > 0 guarantees a preceding char");
    if inserted != '\n' {
        return (Cow::Borrowed(text), None);
    }

    // The old cursor position (before Enter) is where the line being ended
    // sits; find that line's start and its content up to the cursor.
    let old_cursor_char = cursor_char - 1;
    let old_cursor_byte = char_to_byte(old_text, old_cursor_char);
    let line_start_byte = old_text[..old_cursor_byte].rfind('\n').map_or(0, |i| i + 1);
    let current_line_before_cursor = &old_text[line_start_byte..old_cursor_byte];

    let leading_ws_len = current_line_before_cursor
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(current_line_before_cursor.len());
    let leading_ws = &current_line_before_cursor[..leading_ws_len];
    let extra_indent = if current_line_before_cursor.trim_end().ends_with('{') {
        indent_settings.unit()
    } else {
        String::new()
    };
    let new_indent = format!("{leading_ws}{extra_indent}");
    if new_indent.is_empty() {
        return (Cow::Borrowed(text), None);
    }

    let corrected = format!("{}{new_indent}{}", &text[..inserted_end], &text[inserted_end..]);
    let new_cursor_char = cursor_char + new_indent.chars().count();
    (Cow::Owned(corrected), Some(new_cursor_char))
}

/// Merges the line below `cursor_char` onto the current line — `Ctrl+J`'s
/// "join lines" command. Returns `None` if the cursor is on the last line
/// (nothing to join). The newline and the next line's leading whitespace
/// are replaced by a single space, except when that would be redundant (the
/// current line is empty or already ends in whitespace) or pointless (the
/// next line is itself blank) — in those cases the join leaves no separator
/// at all, matching how most editors' "join lines" avoids inserting spaces
/// no one would want. Returns the joined text and where the cursor should
/// land: right at the join point, same as most editors default to.
pub(super) fn join_lines(text: &str, cursor_char: usize) -> Option<(String, usize)> {
    let cursor_byte = char_to_byte(text, cursor_char);
    let line_start = text[..cursor_byte].rfind('\n').map_or(0, |i| i + 1);
    let nl_byte = line_start + text[line_start..].find('\n')?;
    let current_line = &text[line_start..nl_byte];

    let after_nl = &text[nl_byte + 1..];
    let ws_len = after_nl.find(|c: char| c != ' ' && c != '\t').unwrap_or(after_nl.len());
    let next_line_start = nl_byte + 1 + ws_len;
    let next_line_first_char = text[next_line_start..].chars().next();

    let needs_space = !current_line.is_empty()
        && !current_line.ends_with([' ', '\t'])
        && !matches!(next_line_first_char, None | Some('\n'));
    let separator = if needs_space { " " } else { "" };

    let joined = format!("{}{separator}{}", &text[..nl_byte], &text[next_line_start..]);
    let new_cursor_char = text[..nl_byte].chars().count() + separator.chars().count();
    Some((joined, new_cursor_char))
}

/// Indents (`dedent == false`) or dedents (`dedent == true`) every line the
/// selection `start_char..end_char` touches, by one level (per
/// `indent_settings`) —
/// Tab/Shift+Tab while a selection is active. `start_char` must be `<=
/// end_char` (the caller sorts, same contract as `wrap_selection`). Returns
/// the new text and where the selection should land afterward: still
/// covering the same set of lines, adjusted for however many characters
/// each touched line gained or lost, matching how most editors keep a
/// block-indent's selection in place rather than collapsing it.
///
/// Exists because egui's own Tab/Shift+Tab handling doesn't do this
/// (`egui::text_edit::builder`'s `Event::Key { key: Key::Tab, .. }` arm):
/// it unconditionally deletes the *entire* selection first, and only then
/// — for Shift+Tab — dedents the single line the resulting cursor lands
/// on (a limitation its own source flags with
/// "TODO(emilk): support removing indentation over a selection?"). Plain
/// Tab has the same problem one step further: it deletes the selection and
/// replaces it with a single literal tab character. Both destroy every
/// selected character instead of adjusting leading whitespace — exactly
/// backwards from what indenting a selection should do, whether that
/// selection spans one line or many.
///
/// A selection whose end sits exactly at the start of a line doesn't touch
/// that line — e.g. selecting from the middle of line 1 down to the very
/// start of line 3 covers only lines 1 and 2, matching most editors' block-
/// indent semantics (the selection merely touches line 3's boundary, it
/// doesn't cover any of its content).
pub(super) fn indent_selected_lines(
    text: &str,
    start_char: usize,
    end_char: usize,
    dedent: bool,
    indent_settings: IndentSettings,
) -> (String, usize, usize) {
    let unit = indent_settings.unit();
    let unit_len = unit.chars().count();

    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let start_char = start_char.min(n);
    let end_char = end_char.min(n);

    let first_line_start = chars[..start_char]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |i| i + 1);

    // Ascending by construction (line starts, in file order) — which is
    // what lets the membership checks below be binary searches instead of
    // the linear `contains` scans they used to be: with one scan per line
    // over a list that holds one entry per *selected* line, Select-All +
    // Tab on a large file was quadratic in line count.
    let mut touched: Vec<usize> = std::iter::once(0)
        .chain(
            chars
                .iter()
                .enumerate()
                .filter(|&(_, &c)| c == '\n')
                .map(|(i, _)| i + 1),
        )
        .filter(|&s| s >= first_line_start && s < end_char)
        .collect();
    if touched.is_empty() {
        touched.push(first_line_start);
    }

    let mut result: Vec<char> = Vec::with_capacity(n + touched.len() * unit_len);
    // (line_start, delta) for every touched line, in the order encountered —
    // used below to remap `start_char`/`end_char` into the rebuilt text.
    let mut line_deltas: Vec<(usize, i64)> = Vec::with_capacity(touched.len());

    let mut pos = 0usize;
    loop {
        let line_end = chars[pos..].iter().position(|&c| c == '\n').map_or(n, |off| pos + off);
        let has_newline = line_end < n;

        if touched.binary_search(&pos).is_ok() {
            if dedent {
                let removable = if chars.get(pos) == Some(&'\t') {
                    1
                } else {
                    chars[pos..line_end]
                        .iter()
                        .take_while(|&&c| c == ' ')
                        .count()
                        .min(unit_len)
                };
                result.extend_from_slice(&chars[pos + removable..line_end]);
                line_deltas.push((pos, -(removable as i64)));
            } else {
                result.extend(unit.chars());
                result.extend_from_slice(&chars[pos..line_end]);
                line_deltas.push((pos, unit_len as i64));
            }
        } else {
            result.extend_from_slice(&chars[pos..line_end]);
        }

        if has_newline {
            result.push('\n');
            pos = line_end + 1;
        } else {
            break;
        }
    }

    let remap = |p: usize| -> usize {
        let p_line_start = chars[..p].iter().rposition(|&c| c == '\n').map_or(0, |i| i + 1);
        let mut delta_before = 0i64;
        let mut this_line_delta = 0i64;
        for &(line_start, delta) in &line_deltas {
            if line_start < p_line_start {
                delta_before += delta;
            } else if line_start == p_line_start {
                this_line_delta = delta;
            }
        }
        let column = p - p_line_start;
        let new_column = if touched.binary_search(&p_line_start).is_err() {
            column
        } else if dedent {
            column.saturating_sub((-this_line_delta) as usize)
        } else if column == 0 {
            // A selection that starts exactly at column 0 of a line being
            // indented stays at column 0 rather than jumping past the new
            // indentation — the added spaces land "inside" the selection,
            // matching how most editors keep a whole-line selection
            // covering the whole (now-indented) line, so repeated Tab
            // presses keep growing the selection instead of leaving its
            // start behind.
            0
        } else {
            column + unit_len
        };
        (p_line_start as i64 + delta_before + new_column as i64) as usize
    };

    let new_start = remap(start_char);
    let new_end = remap(end_char);

    (result.into_iter().collect(), new_start, new_end)
}

/// The `start..end` char range of the line containing `cursor_char`
/// (`end` excludes the line's own trailing `\n`, if any) — shared by
/// `duplicate_line`/`move_line_up`/`move_line_down` to find "the current
/// line" the same way each time, and by `widget.rs`'s triple-click
/// select-current-line interception (the same "current line" concept, not
/// a text transform, but no reason to re-derive line boundaries a second
/// way).
pub(super) fn current_line_range(chars: &[char], cursor_char: usize) -> (usize, usize) {
    let n = chars.len();
    let cursor_char = cursor_char.min(n);
    let start = chars[..cursor_char]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |i| i + 1);
    let end = chars[cursor_char..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(n, |off| cursor_char + off);
    (start, end)
}

/// "Smart Home": where the Home key should move the cursor on the line
/// containing `cursor_char` — the line's first non-whitespace character if
/// the cursor isn't already there, otherwise column 0 (so a second Home
/// press from the first-non-whitespace position goes all the way to the
/// true line start, and a third press — now at column 0 — goes right back
/// to first-non-whitespace, matching most editors' toggle behavior). A
/// blank (all-whitespace) line has no "first non-whitespace" to toggle
/// with, so Home always goes to column 0 there.
pub(super) fn smart_home_target(text: &str, cursor_char: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let (line_start, line_end) = current_line_range(&chars, cursor_char);
    let first_non_ws = chars[line_start..line_end]
        .iter()
        .position(|&c| c != ' ' && c != '\t')
        .map(|off| line_start + off);

    match first_non_ws {
        Some(pos) if cursor_char.min(chars.len()) != pos => pos,
        _ => line_start,
    }
}

/// Duplicates the line containing `cursor_char`, inserting the copy
/// immediately below the original — `Alt+Shift+ArrowDown`/`Up`'s shared
/// transform (the two shortcuts differ only in where the cursor ends up
/// afterward, decided by the caller). Returns the new text and the cursor
/// position at the same column on the duplicate line.
///
/// Operates on the cursor's line only, not the full extent of any active
/// selection — a deliberate scope choice, matching `Ctrl+D`'s existing
/// word/occurrence semantics rather than adding a second, differently-
/// shaped "duplicate the selection" behavior.
pub(super) fn duplicate_line(text: &str, cursor_char: usize) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (line_start, line_end) = current_line_range(&chars, cursor_char);
    let column = cursor_char.min(chars.len()) - line_start;

    let mut result: Vec<char> = Vec::with_capacity(chars.len() + (line_end - line_start) + 1);
    result.extend_from_slice(&chars[..line_end]);
    result.push('\n');
    result.extend_from_slice(&chars[line_start..line_end]);
    result.extend_from_slice(&chars[line_end..]);

    let cursor_on_duplicate = line_end + 1 + column;
    (result.into_iter().collect(), cursor_on_duplicate)
}

/// Swaps the line containing `cursor_char` with the line above it —
/// `Alt+ArrowUp`. Returns `None` if the cursor is already on the first
/// line (nothing above to swap with). The cursor follows its line, at the
/// same column.
pub(super) fn move_line_up(text: &str, cursor_char: usize) -> Option<(String, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let (line_start, line_end) = current_line_range(&chars, cursor_char);
    if line_start == 0 {
        return None;
    }
    let column = cursor_char.min(chars.len()) - line_start;
    let prev_line_start = chars[..line_start - 1]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |i| i + 1);

    let mut result: Vec<char> = Vec::with_capacity(chars.len());
    result.extend_from_slice(&chars[..prev_line_start]);
    result.extend_from_slice(&chars[line_start..line_end]);
    result.push('\n');
    result.extend_from_slice(&chars[prev_line_start..line_start - 1]);
    result.extend_from_slice(&chars[line_end..]);

    Some((result.into_iter().collect(), prev_line_start + column))
}

/// Swaps the line containing `cursor_char` with the line below it —
/// `Alt+ArrowDown`. Returns `None` if the cursor is already on the last
/// line (nothing below to swap with). The cursor follows its line, at the
/// same column.
pub(super) fn move_line_down(text: &str, cursor_char: usize) -> Option<(String, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let (line_start, line_end) = current_line_range(&chars, cursor_char);
    if line_end == chars.len() {
        return None;
    }
    let column = cursor_char.min(chars.len()) - line_start;
    let next_line_start = line_end + 1;
    let next_line_end = chars[next_line_start..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(chars.len(), |off| next_line_start + off);

    let mut result: Vec<char> = Vec::with_capacity(chars.len());
    result.extend_from_slice(&chars[..line_start]);
    result.extend_from_slice(&chars[next_line_start..next_line_end]);
    result.push('\n');
    result.extend_from_slice(&chars[line_start..line_end]);
    result.extend_from_slice(&chars[next_line_end..]);

    let new_line_start = line_start + (next_line_end - next_line_start) + 1;
    Some((result.into_iter().collect(), new_line_start + column))
}

/// Toggles `//` line comments on every line the selection
/// `start_char..end_char` touches (or just the cursor's line, for a
/// collapsed selection) — `Ctrl+/`. Uncomments only if every non-blank
/// touched line is already commented; otherwise comments every touched
/// line, blank ones included. Returns the new text and the remapped
/// `start..end` selection.
///
/// The marker is always inserted/removed at column 0, regardless of a
/// line's own indentation — a deliberate simplification, not an oversight:
/// it lets the cursor/selection remap below reuse exactly
/// `indent_selected_lines`' already-proven-correct "touched line +
/// per-line delta" shape, rather than a second bespoke one that has to
/// separately account for a cursor sitting inside a line's leading
/// whitespace. The tradeoff is that the comment marker doesn't line up
/// with an indented line's code — acceptable for a `Ctrl+/` toggle, which
/// only two supported languages (both `//`-commented) need at all.
pub(super) fn toggle_line_comments(text: &str, start_char: usize, end_char: usize) -> (String, usize, usize) {
    const MARKER: &str = "// ";

    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let start_char = start_char.min(n);
    let end_char = end_char.min(n);

    let first_line_start = chars[..start_char]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |i| i + 1);
    // Ascending by construction (line starts, in file order) — which is
    // what lets the membership checks below be binary searches instead of
    // the linear `contains` scans they used to be: with one scan per line
    // over a list that holds one entry per *selected* line, Select-All +
    // Tab on a large file was quadratic in line count.
    let mut touched: Vec<usize> = std::iter::once(0)
        .chain(
            chars
                .iter()
                .enumerate()
                .filter(|&(_, &c)| c == '\n')
                .map(|(i, _)| i + 1),
        )
        .filter(|&s| s >= first_line_start && s < end_char)
        .collect();
    if touched.is_empty() {
        touched.push(first_line_start);
    }

    let line_end_of = |s: usize| chars[s..].iter().position(|&c| c == '\n').map_or(n, |off| s + off);
    let is_commented = |s: usize| chars[s..line_end_of(s)].starts_with(&['/', '/']);
    let is_blank = |s: usize| chars[s..line_end_of(s)].iter().all(|&c| c == ' ' || c == '\t');

    let mut any_non_blank = false;
    let mut all_commented = true;
    for &s in &touched {
        if !is_blank(s) {
            any_non_blank = true;
            all_commented &= is_commented(s);
        }
    }
    let uncomment = any_non_blank && all_commented;

    let marker_len = MARKER.chars().count() as i64;
    let mut result: Vec<char> = Vec::with_capacity(n + touched.len() * marker_len as usize);
    let mut line_deltas: Vec<(usize, i64)> = Vec::with_capacity(touched.len());

    let mut pos = 0usize;
    loop {
        let line_end = line_end_of(pos);
        let has_newline = line_end < n;

        if touched.binary_search(&pos).is_ok() {
            if uncomment && is_commented(pos) {
                let removable = if chars.get(pos + 2) == Some(&' ') { 3 } else { 2 };
                result.extend_from_slice(&chars[pos + removable..line_end]);
                line_deltas.push((pos, -(removable as i64)));
            } else if !uncomment {
                result.extend(MARKER.chars());
                result.extend_from_slice(&chars[pos..line_end]);
                line_deltas.push((pos, marker_len));
            } else {
                // Uncomment mode, but this touched line (necessarily
                // blank — `uncomment` requires every *non-blank* touched
                // line to already be commented) has nothing to remove.
                result.extend_from_slice(&chars[pos..line_end]);
            }
        } else {
            result.extend_from_slice(&chars[pos..line_end]);
        }

        if has_newline {
            result.push('\n');
            pos = line_end + 1;
        } else {
            break;
        }
    }

    let remap = |p: usize| -> usize {
        let p_line_start = chars[..p].iter().rposition(|&c| c == '\n').map_or(0, |i| i + 1);
        let mut delta_before = 0i64;
        let mut this_line_delta = 0i64;
        for &(line_start, delta) in &line_deltas {
            if line_start < p_line_start {
                delta_before += delta;
            } else if line_start == p_line_start {
                this_line_delta = delta;
            }
        }
        let column = p - p_line_start;
        let new_column = if touched.binary_search(&p_line_start).is_err() {
            column
        } else if uncomment {
            column.saturating_sub((-this_line_delta) as usize)
        } else if column == 0 {
            0
        } else {
            column + this_line_delta as usize
        };
        (p_line_start as i64 + delta_before + new_column as i64) as usize
    };

    let new_start = remap(start_char);
    let new_end = remap(end_char);

    (result.into_iter().collect(), new_start, new_end)
}

/// The whole-line block `start_char..end_char` touches — from the start of
/// the line `start_char` sits on, to the end of the line the selection's
/// last touched character sits on. Falls back to just the cursor's own line
/// when `start_char == end_char` (nothing selected), matching `Ctrl+/`'s
/// "no selection means the current line" behavior. Deliberately simpler
/// than `indent_selected_lines`/`toggle_line_comments`'s own "touched line
/// set" (a `Vec` of every line start in the range): `sort_lines`/
/// `unique_lines` only ever need the block's outer boundaries, since they
/// replace the whole block in one piece rather than editing each touched
/// line independently.
fn line_block_range(chars: &[char], start_char: usize, end_char: usize) -> (usize, usize) {
    let n = chars.len();
    let start_char = start_char.min(n);
    let end_char = end_char.min(n);
    let block_start = chars[..start_char]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |i| i + 1);
    let last_touched = if end_char > start_char {
        end_char - 1
    } else {
        start_char
    };
    let block_end = chars[last_touched..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(n, |off| last_touched + off);
    (block_start, block_end)
}

/// Sorts (by plain `str::cmp`, no case-insensitive/locale option needed for
/// a first cut) the lines touched by `start_char..end_char` — a Tools menu
/// command, not a keyboard shortcut. Returns the new text and a selection
/// covering the now-reordered block. A collapsed selection touches only the
/// cursor's own single line, which sorting trivially leaves unchanged.
pub(super) fn sort_lines(text: &str, start_char: usize, end_char: usize) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (block_start, block_end) = line_block_range(&chars, start_char, end_char);

    let block: String = chars[block_start..block_end].iter().collect();
    let mut lines: Vec<&str> = block.split('\n').collect();
    lines.sort_unstable();
    let sorted_block = lines.join("\n");

    let new_text = format!(
        "{}{sorted_block}{}",
        chars[..block_start].iter().collect::<String>(),
        chars[block_end..].iter().collect::<String>()
    );
    let new_end = block_start + sorted_block.chars().count();
    (new_text, block_start, new_end)
}

/// Collapses the lines touched by `start_char..end_char` down to only their
/// first occurrence, preserving order — a separate operation from
/// `sort_lines`, not one that implies it, so the two can be invoked
/// independently or combined. Returns the new text and a selection covering
/// the now-deduplicated block.
pub(super) fn unique_lines(text: &str, start_char: usize, end_char: usize) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (block_start, block_end) = line_block_range(&chars, start_char, end_char);

    let block: String = chars[block_start..block_end].iter().collect();
    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<&str> = block.split('\n').filter(|line| seen.insert(*line)).collect();
    let deduped_block = deduped.join("\n");

    let new_text = format!(
        "{}{deduped_block}{}",
        chars[..block_start].iter().collect::<String>(),
        chars[block_end..].iter().collect::<String>()
    );
    let new_end = block_start + deduped_block.chars().count();
    (new_text, block_start, new_end)
}

/// Which case transform `convert_selection_case` applies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseConversion {
    Upper,
    Lower,
    Title,
}

/// Uppercases the first letter of every word (a maximal run of alphanumeric
/// characters) and lowercases the rest — "hello WORLD_2day" becomes
/// "Hello World_2day" (`_` isn't alphanumeric, so it still ends a word the
/// same way a space would).
fn title_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut at_word_start = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if at_word_start {
                result.extend(c.to_uppercase());
            } else {
                result.extend(c.to_lowercase());
            }
            at_word_start = false;
        } else {
            result.push(c);
            at_word_start = true;
        }
    }
    result
}

/// Applies `case` to `text[start_char..end_char]`, returning the new text
/// and the selection's new `start..end` (unchanged from the input unless
/// the conversion itself changes the run's char count, which full Unicode
/// case mapping occasionally does — e.g. German `ß` uppercases to `SS`).
/// `None` for an empty range: case conversion has nothing to do without an
/// actual selection, unlike the line-based transforms above, which fall
/// back to "the cursor's line".
pub(super) fn convert_selection_case(
    text: &str,
    start_char: usize,
    end_char: usize,
    case: CaseConversion,
) -> Option<(String, usize, usize)> {
    if start_char == end_char {
        return None;
    }
    let start_byte = char_to_byte(text, start_char);
    let end_byte = char_to_byte(text, end_char);
    let selected = &text[start_byte..end_byte];

    let converted = match case {
        CaseConversion::Upper => selected.to_uppercase(),
        CaseConversion::Lower => selected.to_lowercase(),
        CaseConversion::Title => title_case(selected),
    };

    let new_text = format!("{}{converted}{}", &text[..start_byte], &text[end_byte..]);
    let new_end = start_char + converted.chars().count();
    Some((new_text, start_char, new_end))
}

/// Maps an auto-pairable opening character to its closing counterpart —
/// shared by `apply_auto_pair` (typing an opener with no selection) and
/// `wrap_selection` (typing one *over* a selection), so the two features
/// can't quietly disagree on which characters are paired or what they pair
/// with.
///
/// `<`/`>` is a deliberate tradeoff, not an oversight: in Java/Kotlin `<` is
/// also the less-than operator, so auto-closing it unconditionally means
/// typing `x < 5` inserts an unwanted `>` after the `<`. Every other paired
/// character here is unambiguous in context; `<` isn't, and this doesn't
/// attempt the type-position analysis that would be needed to tell "generic"
/// from "comparison" apart.
fn closing_char(opener: char) -> Option<char> {
    match opener {
        '{' => Some('}'),
        '(' => Some(')'),
        '[' => Some(']'),
        '<' => Some('>'),
        '"' => Some('"'),
        '\'' => Some('\''),
        _ => None,
    }
}

/// Whether `c` is a character this editor auto-pairs — used by
/// `widgets::editor::show` to decide whether a single typed character while
/// a selection is active should be intercepted for `wrap_selection` instead
/// of falling through to egui's default replace-selection behavior.
pub(super) fn is_pairable(c: char) -> bool {
    closing_char(c).is_some()
}

/// Auto-closes brackets/quotes: typing an opener (`{`, `(`, `[`, `<`, `"`,
/// `'`) inserts its matching closer right after the cursor, and typing a
/// closer that's already sitting right there just types over it instead of
/// duplicating it. Only fires on a pure single-character insertion (so
/// pastes, multi-char IME commits, and replacing a selection are untouched
/// — a selection is `wrap_selection`'s job instead).
///
/// Deliberately locates the just-typed character via `cursor_char` (egui's
/// own post-edit cursor position) rather than by diffing `old_text`/`text`:
/// a prefix/suffix diff of the two full-text snapshots is ambiguous exactly
/// in the case this needs to detect — typing a closer immediately before an
/// identical existing one (e.g. `(a|)` -> type `)`) is indistinguishable, by
/// pure text diffing, from appending a new `)` at the end. Only the real
/// cursor position disambiguates it.
pub(super) fn apply_auto_pair(old_text: &str, text: &str, cursor_char: Option<usize>) -> String {
    let old_chars = old_text.chars().count();
    let new_chars = text.chars().count();
    if new_chars != old_chars + 1 {
        return text.to_string();
    }
    let Some(cursor_char) = cursor_char.filter(|&c| c > 0 && c <= new_chars) else {
        return text.to_string();
    };

    let inserted_start = char_to_byte(text, cursor_char - 1);
    let inserted_end = char_to_byte(text, cursor_char);
    let inserted = text[inserted_start..inserted_end]
        .chars()
        .next()
        .expect("cursor_char > 0 guarantees a preceding char");

    let old_char_at_same_pos = old_text.chars().nth(cursor_char - 1);

    match inserted {
        '{' | '(' | '[' | '<' => {
            let closer = closing_char(inserted).expect("matched only auto-pairable openers");
            format!("{}{closer}{}", &text[..inserted_end], &text[inserted_end..])
        }
        '"' | '\'' if old_char_at_same_pos == Some(inserted) => {
            // Typing over an existing quote: drop the duplicate, cursor
            // effectively moves past the original.
            format!("{}{}", &text[..inserted_start], &text[inserted_end..])
        }
        '"' | '\'' => format!("{}{inserted}{}", &text[..inserted_end], &text[inserted_end..]),
        '}' | ')' | ']' | '>' if old_char_at_same_pos == Some(inserted) => {
            format!("{}{}", &text[..inserted_start], &text[inserted_end..])
        }
        _ => text.to_string(),
    }
}

/// The reverse of `apply_auto_pair`'s bracket-closing: deleting the opening
/// half of an adjacent, empty pair (`"()"`, `"[]"`, `"{}"`, `"<>"`, `""`,
/// `''`) deletes the closing half too — whether the deletion was a
/// Backspace (cursor was right after the opener before the edit) or a
/// forward Delete (cursor was right before it), both leave the post-edit
/// cursor sitting exactly where the opener used to be, with the closer now
/// immediately after it. No "was this pair actually auto-inserted" tracking
/// — a manually-typed adjacent pair gets the same treatment, matching how
/// most editors' own bracket-delete behavior works. Only fires on a pure
/// single-character deletion (so deleting a multi-char selection is
/// untouched), same "diff the two full-text snapshots plus the real
/// post-edit cursor" technique `apply_auto_pair` itself already uses, for
/// the same reason: text-diffing alone can't disambiguate this from an
/// unrelated single-char deletion elsewhere in the buffer.
pub(super) fn apply_auto_pair_delete(old_text: &str, text: &str, cursor_char: Option<usize>) -> String {
    let old_chars = old_text.chars().count();
    let new_chars = text.chars().count();
    if new_chars + 1 != old_chars {
        return text.to_string();
    }
    let Some(cursor_char) = cursor_char else {
        return text.to_string();
    };

    // The char now at `cursor_char` in `old_text` is exactly the one that
    // just got deleted, regardless of which key did it: a Backspace moves
    // the cursor back by one to land there, a forward Delete never moves it
    // at all since it was already there.
    let Some(deleted) = old_text.chars().nth(cursor_char) else {
        return text.to_string();
    };
    let Some(expected_closer) = closing_char(deleted) else {
        return text.to_string();
    };
    if old_text.chars().nth(cursor_char + 1) != Some(expected_closer) {
        return text.to_string();
    }

    let closer_start = char_to_byte(text, cursor_char);
    let closer_end = char_to_byte(text, cursor_char + 1);
    format!("{}{}", &text[..closer_start], &text[closer_end..])
}

/// Wraps `old_text[start_char..end_char]` in `opener`/its matching closer,
/// replacing egui's default "typing a bracket over a selection deletes it"
/// behavior. Used when a selection is active and the typed character is one
/// this editor auto-pairs — `widgets::editor::show` detects that *before*
/// `TextEdit::show()` runs, since by the time a normal post-edit diff would
/// see it, the selected text egui replaced is already gone; there's nothing
/// left for a diff-based approach (like `apply_auto_pair` and
/// `apply_auto_indent` use) to recover it from.
///
/// Returns the new text and the char range the originally selected text now
/// occupies — kept selected in the result, matching how most editors leave
/// a just-wrapped selection selected rather than collapsing the cursor, so
/// wrapping it again (to nest) or moving on with an arrow key both stay one
/// step away. `None` if `opener` isn't a character this editor auto-pairs.
pub(super) fn wrap_selection(
    old_text: &str,
    start_char: usize,
    end_char: usize,
    opener: char,
) -> Option<(String, usize, usize)> {
    let closer = closing_char(opener)?;
    let start_byte = char_to_byte(old_text, start_char);
    let end_byte = char_to_byte(old_text, end_char);
    let wrapped = format!(
        "{}{opener}{}{closer}{}",
        &old_text[..start_byte],
        &old_text[start_byte..end_byte],
        &old_text[end_byte..],
    );
    Some((wrapped, start_char + 1, end_char + 1))
}

#[cfg(test)]
mod auto_edit_test;
