
use super::*;

fn rope(s: &str) -> Rope {
    Rope::from_str(s)
}

#[test]
fn hidden_line_range_excludes_the_marker_line_and_includes_the_closing_line() {
    // "class Foo {\n    int x;\n}\n" — fold starts right after line 0's
    // newline (byte 12) and ends at the buffer's end (byte 26, the `}`
    // line's own newline included in the node span up to `}`).
    let source = "class Foo {\n    int x;\n}\n";
    let buffer = rope(source);
    let start_byte = source.find('\n').unwrap() + 1;
    let end_byte = source.rfind('}').unwrap() + 1; // one past the closing brace
    let fold = FoldRange {
        marker_line: 0,
        start_byte,
        end_byte,
    };

    let hidden = hidden_line_range(&buffer, &fold);
    assert_eq!(
        hidden,
        1..3,
        "lines 1 (\"int x;\") and 2 (the closing brace) are hidden; line 0 stays visible"
    );
}

#[test]
fn hidden_ranges_skips_folds_not_in_the_folded_set() {
    let buffer = rope("a\nb\nc\nd\n");
    let folds = vec![
        FoldRange {
            marker_line: 0,
            start_byte: 2,
            end_byte: 4,
        },
        FoldRange {
            marker_line: 2,
            start_byte: 6,
            end_byte: 8,
        },
    ];
    let folded = HashSet::from([2]);

    let hidden = hidden_ranges(&buffer, &folds, &folded);
    assert_eq!(hidden, vec![3..4]);
}

#[test]
fn hidden_ranges_merges_a_nested_fold_collapsed_inside_an_outer_one() {
    // "l0\nl1\nl2\nl3\nl4\nl5\n" — each "lN\n" is 3 bytes, so line k
    // starts at byte 3*k. Outer fold (marker line 0) hides lines 1..5;
    // a fold nested inside it (marker line 1) hides lines 2..4. Both
    // collapsed at once must merge to one non-overlapping range.
    let buffer = rope("l0\nl1\nl2\nl3\nl4\nl5\n");
    let outer = FoldRange {
        marker_line: 0,
        start_byte: 3,
        end_byte: 15,
    };
    let inner = FoldRange {
        marker_line: 1,
        start_byte: 6,
        end_byte: 12,
    };
    assert_eq!(
        hidden_line_range(&buffer, &outer),
        1..5,
        "sanity: outer hides lines 1..5"
    );
    assert_eq!(
        hidden_line_range(&buffer, &inner),
        2..4,
        "sanity: inner hides lines 2..4"
    );

    let folds = vec![outer, inner];
    let folded = HashSet::from([0, 1]);
    let hidden = hidden_ranges(&buffer, &folds, &folded);
    assert_eq!(
        hidden,
        vec![1..5],
        "the nested range must merge into the outer one, not appear separately"
    );
}

#[test]
fn fold_all_collapses_every_marker_line() {
    let folds = vec![
        FoldRange {
            marker_line: 0,
            start_byte: 0,
            end_byte: 1,
        },
        FoldRange {
            marker_line: 5,
            start_byte: 0,
            end_byte: 1,
        },
    ];
    let mut folded = HashSet::new();
    fold_all(&folds, &mut folded);
    assert_eq!(folded, HashSet::from([0, 5]));
}

#[test]
fn expand_all_clears_the_set() {
    let mut folded = HashSet::from([1, 2, 3]);
    expand_all(&mut folded);
    assert!(folded.is_empty());
}

#[test]
fn hidden_ranges_ignores_a_stale_marker_line_no_longer_present_in_folds() {
    // Simulates PLAN.md 3e: `folded_lines` still has line 4 from before
    // an edit, but the freshly recomputed `folds` no longer has a fold
    // opening there — it should just be silently dropped, not panic or
    // produce a bogus range.
    let buffer = rope("a\nb\nc\n");
    let folds = vec![FoldRange {
        marker_line: 0,
        start_byte: 2,
        end_byte: 4,
    }];
    let folded = HashSet::from([0, 4]);
    let hidden = hidden_ranges(&buffer, &folds, &folded);
    assert_eq!(hidden, vec![1..2]);
}
