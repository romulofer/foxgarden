use std::ops::Range;

use tree_sitter::{Node, Tree};

const OPENERS: [&str; 3] = ["(", "{", "["];

fn matching_closer(opener: &str) -> Option<&'static str> {
    match opener {
        "(" => Some(")"),
        "{" => Some("}"),
        "[" => Some("]"),
        _ => None,
    }
}

fn is_bracket_token(node: Node, source: &str, candidates: &[&str]) -> bool {
    !node.is_named() && candidates.contains(&&source[node.byte_range()])
}

/// If `node`'s first and last direct children are a matching opener/closer
/// pair (`{ ... }`, `( ... )`, `[ ... ]`), returns their two byte ranges.
/// Brackets are anonymous tokens in tree-sitter's grammar, not their own
/// named node kind, so this is a structural check over `node`'s direct
/// children rather than a query.
fn bracket_pair_of(node: Node, source: &str) -> Option<(Range<usize>, Range<usize>)> {
    let count = node.child_count();
    if count < 2 {
        return None;
    }
    let first = node.child(0)?;
    let last = node.child(count as u32 - 1)?;
    if !is_bracket_token(first, source, &OPENERS) {
        return None;
    }
    let expected_close = matching_closer(&source[first.byte_range()])?;
    if !is_bracket_token(last, source, &[expected_close]) {
        return None;
    }
    Some((first.byte_range(), last.byte_range()))
}

/// Finds the bracket pair `byte_offset` sits on or immediately beside, if
/// any — the data source for bracket-pair highlighting. Walks up from the
/// smallest node touching `byte_offset` to the nearest ancestor whose
/// direct children open and close with a matching bracket pair, stopping at
/// the first one `byte_offset` actually touches (one of the four
/// characters' start/end offsets) rather than just any bracket pair that
/// happens to enclose the cursor — a cursor deep inside a block's body,
/// nowhere near either brace, should not highlight that block's braces.
/// Returns `None` if no such ancestor exists (unbalanced/incomplete code)
/// or `byte_offset` never actually touches a bracket on the way up to the
/// root.
pub fn bracket_match(tree: &Tree, source: &str, byte_offset: usize) -> Option<(Range<usize>, Range<usize>)> {
    let byte_offset = byte_offset.min(source.len());
    let mut node = tree.root_node().descendant_for_byte_range(byte_offset, byte_offset)?;

    loop {
        if let Some((open, close)) = bracket_pair_of(node, source) {
            let touches = |r: &Range<usize>| byte_offset == r.start || byte_offset == r.end;
            if touches(&open) || touches(&close) {
                return Some((open, close));
            }
        }
        node = node.parent()?;
    }
}

#[cfg(test)]
#[path = "brackets_test.rs"]
mod brackets_test;
