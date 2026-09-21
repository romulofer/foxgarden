
use super::*;

#[test]
fn a_repeated_message_is_only_shown_once() {
    let mut toasts = Toasts::default();
    for _ in 0..5 {
        toasts.push("language server exited".to_string());
    }
    assert_eq!(toasts.messages.len(), 1);
}

#[test]
fn distinct_messages_stack_in_arrival_order() {
    let mut toasts = Toasts::default();
    toasts.push("first".to_string());
    toasts.push("second".to_string());
    assert_eq!(toasts.messages, ["first", "second"]);
}
