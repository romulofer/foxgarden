
use super::*;

fn diagnostic_at(start: usize) -> Diagnostic {
    Diagnostic {
        range: start..start + 1,
        message: "boom".to_string(),
        severity: fg_core::Severity::Error,
    }
}

#[test]
fn f8_walks_forward_and_wraps_at_the_end() {
    let diagnostics = [diagnostic_at(10), diagnostic_at(40), diagnostic_at(90)];
    let refs: Vec<&Diagnostic> = diagnostics.iter().collect();

    assert_eq!(neighbouring_diagnostic(&refs, 0, true), Some(10));
    assert_eq!(neighbouring_diagnostic(&refs, 10, true), Some(40));
    assert_eq!(
        neighbouring_diagnostic(&refs, 95, true),
        Some(10),
        "past the last one, wrap to the first"
    );
}

#[test]
fn shift_f8_walks_backward_and_wraps_at_the_start() {
    let diagnostics = [diagnostic_at(10), diagnostic_at(40)];
    let refs: Vec<&Diagnostic> = diagnostics.iter().collect();

    assert_eq!(neighbouring_diagnostic(&refs, 40, false), Some(10));
    assert_eq!(
        neighbouring_diagnostic(&refs, 0, false),
        Some(40),
        "before the first one, wrap to the last"
    );
}

/// Several tools can report the same position (a syntax error the LSP
/// also flags); stepping must not stall on it.
#[test]
fn duplicate_positions_count_once() {
    let diagnostics = [diagnostic_at(10), diagnostic_at(10), diagnostic_at(50)];
    let refs: Vec<&Diagnostic> = diagnostics.iter().collect();

    assert_eq!(neighbouring_diagnostic(&refs, 10, true), Some(50));
}

#[test]
fn nothing_to_step_through_is_not_a_jump_to_zero() {
    assert_eq!(neighbouring_diagnostic(&[], 0, true), None);
}
