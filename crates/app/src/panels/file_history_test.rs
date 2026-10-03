
use super::*;

#[test]
fn format_timestamp_buckets_a_recent_save_as_just_now() {
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    assert_eq!(format_timestamp(now_nanos), "just now");
}

#[test]
fn format_timestamp_buckets_an_hour_old_save_in_minutes() {
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let ten_minutes_ago = now_nanos - 10 * 60 * 1_000_000_000;
    assert_eq!(format_timestamp(ten_minutes_ago), "10m ago");
}
