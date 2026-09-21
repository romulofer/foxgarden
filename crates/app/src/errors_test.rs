
use super::*;

#[test]
fn the_first_message_becomes_the_error() {
    let mut error = None;
    report(&mut error, "disk full".to_string());
    assert_eq!(error.as_deref(), Some("disk full"));
}

#[test]
fn a_second_distinct_message_is_kept_alongside_the_first() {
    let mut error = None;
    report(&mut error, "saving A failed".to_string());
    report(&mut error, "saving B failed".to_string());
    assert_eq!(error.as_deref(), Some("saving A failed\nsaving B failed"));
}

#[test]
fn the_same_message_reported_again_is_not_repeated() {
    let mut error = None;
    for _ in 0..10 {
        report(&mut error, "language server exited".to_string());
    }
    assert_eq!(error.as_deref(), Some("language server exited"));
}

#[test]
fn messages_stop_accumulating_at_the_cap() {
    let mut error = None;
    for i in 0..50 {
        report(&mut error, format!("failure {i}"));
    }
    assert_eq!(error.as_deref().unwrap().lines().count(), MAX_MESSAGES);
}
