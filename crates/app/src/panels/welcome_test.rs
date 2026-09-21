
use super::*;

#[test]
fn the_newest_project_comes_first_and_is_never_duplicated() {
    let mut recent = Vec::new();
    remember_project(&mut recent, Path::new("/a"));
    remember_project(&mut recent, Path::new("/b"));
    remember_project(&mut recent, Path::new("/a"));

    assert_eq!(recent, [PathBuf::from("/a"), PathBuf::from("/b")]);
}

#[test]
fn the_list_stays_bounded() {
    let mut recent = Vec::new();
    for i in 0..50 {
        remember_project(&mut recent, &PathBuf::from(format!("/p{i}")));
    }
    assert_eq!(recent.len(), MAX_RECENT * 2);
    assert_eq!(recent[0], PathBuf::from("/p49"));
}

#[test]
fn the_hint_line_only_promises_what_works_without_a_project() {
    assert!(!shortcut_hints(false).contains(t().welcome.hint_go_to_file));
    assert!(shortcut_hints(true).contains(t().welcome.hint_go_to_file));
}
