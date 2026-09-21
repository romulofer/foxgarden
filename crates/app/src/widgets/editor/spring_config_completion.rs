//! Spring config property autocomplete (`PLAN.md` Track 12 Phase 1) — pure
//! candidate-generation logic for `application.properties`/`.yml`, fed by
//! `fg_core::SpringConfigProperty` (the resolved-classpath jar scan;
//! `crate::panels::spring_config` owns the background scan that produces
//! it). Two shapes, matching the two real file formats: `.properties` is
//! flat (`server.port=8080`, the whole dotted key on one line), `.yml` is
//! nested (`server:` / `  port: 8080`), so a completed *segment* there has
//! to be reconstructed from indentation ancestry rather than read straight
//! off the current line.

use std::ops::Range;

use fg_core::SpringConfigProperty;

use super::completion::{CompletionItem, CompletionKind};

/// The maximal run of key-segment characters (letters, digits, underscore,
/// **or dash**) ending exactly at `cursor_char` — `templates::
/// word_before_cursor`'s own definition minus the dash doesn't fit here:
/// real Spring property key segments are routinely kebab-case
/// (`context-path`, `pool-name`, both real names straight out of a
/// captured `spring-configuration-metadata.json`), and a dash splitting
/// what's really one segment into two would anchor the popup, and filter
/// candidates, against only the half after the last dash.
pub(super) fn key_segment_before_cursor(text: &str, cursor_char: usize) -> Range<usize> {
    let chars: Vec<char> = text.chars().collect();
    let end = cursor_char.min(chars.len());
    let mut start = end;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_' || chars[start - 1] == '-') {
        start -= 1;
    }
    start..end
}

/// A `.properties` file's own completion source: every known property's
/// full dotted name as one candidate, unfiltered here — `CompletionState::
/// visible`'s own prefix filter (anchored at the current line's start, by
/// this feature's caller) does the narrowing against whatever's typed so
/// far, the same as every other candidate source in this codebase.
pub(super) fn properties_completion_candidates(properties: &[SpringConfigProperty]) -> Vec<CompletionItem> {
    properties
        .iter()
        .map(|p| CompletionItem {
            label: p.name.clone(),
            kind: CompletionKind::Property,
            detail: property_detail(p),
            has_params: false,
        })
        .collect()
}

/// A `.yml` file's own completion source at a given nesting level
/// (`ancestor_prefix`, e.g. `"server."` — empty at the document's own top
/// level): one candidate per *distinct next segment* among every known
/// property whose full name starts with `ancestor_prefix`, not the whole
/// remaining dotted tail — offering `"servlet.jsp.class-name"` as a single
/// flat candidate at the `server:` nesting level would produce invalid YAML
/// if accepted verbatim (that's three more nested lines, not one), so this
/// drills exactly one level at a time, matching how a user actually
/// extends a YAML mapping line by line. A segment that's also a leaf
/// property in its own right (`ancestor_prefix` + segment names a real
/// property, not just a common prefix of several) gets that property's own
/// detail text; a segment that's only ever a prefix of something deeper
/// gets none.
pub(super) fn yaml_completion_candidates(
    properties: &[SpringConfigProperty],
    ancestor_prefix: &str,
) -> Vec<CompletionItem> {
    let mut candidates: Vec<CompletionItem> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for p in properties {
        let Some(rest) = p.name.strip_prefix(ancestor_prefix) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let segment = rest.split('.').next().expect("split always yields at least one item");
        if !seen.insert(segment.to_string()) {
            continue;
        }
        let is_leaf_here = segment == rest;
        candidates.push(CompletionItem {
            label: segment.to_string(),
            kind: CompletionKind::Property,
            detail: if is_leaf_here { property_detail(p) } else { None },
            has_params: false,
        });
    }

    candidates
}

fn property_detail(p: &SpringConfigProperty) -> Option<String> {
    match (&p.type_name, &p.default_value) {
        (Some(t), Some(d)) => Some(format!("{t} = {d}")),
        (Some(t), None) => Some(t.clone()),
        (None, Some(d)) => Some(format!("= {d}")),
        (None, None) => None,
    }
}

/// Reconstructs the dotted ancestor key path enclosing `current_line` in a
/// YAML document, via indentation — not a tree-sitter parse: the line
/// actually being completed is, by construction, either not yet a valid
/// `block_mapping_pair` at all (no `:` typed yet) or in the middle of
/// having one typed, so a parse-tree walk would have to fight the exact
/// spot this needs to read, whereas every line *above* the one being typed
/// is already complete text this can trust. Returns `""` at the document's
/// own top level (empty prefix, matching `yaml_completion_candidates`'
/// own `ancestor_prefix` convention), or `"a.b."` (trailing dot) when
/// nested under `a:` → `b:`.
///
/// Walks upward from `current_line`, tracking an indentation `threshold`
/// that starts at `current_indent` and only ever decreases: a preceding
/// line only counts as an ancestor if its own indentation is strictly less
/// than the current threshold (a sibling or deeper-nested line is never an
/// ancestor), and only if it looks like a mapping key (`"key:"`, with
/// `key` non-empty) — a blank line, a comment, or a list item (`"- foo"`,
/// no bare `key:` shape) is skipped without changing the threshold.
pub(super) fn yaml_ancestor_path(source: &str, current_line: usize, current_indent: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut segments: Vec<&str> = Vec::new();
    let mut threshold = current_indent;

    for line in lines[..current_line.min(lines.len())].iter().rev() {
        if threshold == 0 {
            break;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if indent >= threshold {
            continue;
        }
        let Some(colon) = trimmed.find(':') else { continue };
        let key = trimmed[..colon].trim();
        if key.is_empty() || key.starts_with('-') {
            continue;
        }
        segments.push(key);
        threshold = indent;
    }

    segments.reverse();
    if segments.is_empty() {
        String::new()
    } else {
        format!("{}.", segments.join("."))
    }
}

#[cfg(test)]
#[path = "spring_config_completion_test.rs"]
mod spring_config_completion_test;
