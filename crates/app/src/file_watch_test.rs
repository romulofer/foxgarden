
use super::*;

#[test]
fn reconcile_is_unchanged_when_disk_matches_the_buffer() {
    assert_eq!(reconcile(false, "hello", Some("hello")), ReconcileOutcome::Unchanged);
    // Also true while dirty: a dirty buffer whose content happens to
    // already match disk (e.g. the user undid back to the saved
    // state) still has nothing to reconcile.
    assert_eq!(reconcile(true, "hello", Some("hello")), ReconcileOutcome::Unchanged);
}

#[test]
fn reconcile_reloads_transparently_when_not_dirty() {
    assert_eq!(
        reconcile(false, "old", Some("new")),
        ReconcileOutcome::ReloadTransparently
    );
}

#[test]
fn reconcile_flags_a_conflict_when_dirty() {
    assert_eq!(
        reconcile(true, "my edits", Some("their edits")),
        ReconcileOutcome::Conflict
    );
}

#[test]
fn reconcile_reports_deleted_for_no_disk_content() {
    assert_eq!(reconcile(false, "hello", None), ReconcileOutcome::Deleted);
    assert_eq!(reconcile(true, "hello", None), ReconcileOutcome::Deleted);
}

#[test]
fn reconcile_treats_a_self_triggered_save_as_unchanged() {
    // Exactly what `Document::save()` leaves behind: the buffer and
    // the just-written disk content are identical, so this must not
    // be mistaken for an external change — `is_dirty` is `false`
    // immediately after a save, same as the plain "no real change"
    // case, and both correctly resolve to `Unchanged` rather than
    // `ReloadTransparently` (which would still be harmless here, but
    // `Unchanged` avoids the pointless re-parse).
    let content = "class Foo {}\n";
    assert_eq!(reconcile(false, content, Some(content)), ReconcileOutcome::Unchanged);
}

#[test]
fn watched_dirs_for_dedupes_a_shared_parent() {
    let paths = [PathBuf::from("/proj/src/A.java"), PathBuf::from("/proj/src/B.java")];
    let dirs = watched_dirs_for(paths.iter().map(|p| p.as_path()));
    assert_eq!(dirs, HashSet::from([PathBuf::from("/proj/src")]));
}

#[test]
fn watched_dirs_for_covers_every_distinct_parent() {
    let paths = [PathBuf::from("/proj/src/A.java"), PathBuf::from("/proj/test/B.java")];
    let dirs = watched_dirs_for(paths.iter().map(|p| p.as_path()));
    assert_eq!(
        dirs,
        HashSet::from([PathBuf::from("/proj/src"), PathBuf::from("/proj/test")])
    );
}

#[test]
fn watched_dirs_for_empty_input_is_empty() {
    assert_eq!(watched_dirs_for(std::iter::empty()), HashSet::new());
}
