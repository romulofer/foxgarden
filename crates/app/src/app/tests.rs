//! Unit tests for [`super`](app.rs), extracted verbatim from that
//! file's colocated `#[cfg(test)] mod tests` so the module file stays focused
//! on the code under test. Behavior-identical to the inline module it replaced.

use super::*;
use crate::widgets::editor::UserTemplate;
use eframe::Storage as _;
use std::collections::HashMap;

/// Minimal in-memory `eframe::Storage`, so `restore_session`/
/// `persist_session` can be tested without a real `CreationContext`
/// (which needs a live windowing/render backend to construct).
#[derive(Default)]
struct FakeStorage(HashMap<String, String>);

impl eframe::Storage for FakeStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }
    fn flush(&mut self) {}
}

fn modify_event(path: PathBuf) -> notify::Result<notify::Event> {
    Ok(notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Any)).add_path(path))
}

fn remove_event(path: PathBuf) -> notify::Result<notify::Event> {
    Ok(notify::Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Any)).add_path(path))
}

#[test]
fn process_file_events_reloads_a_clean_tab_transparently() {
    let dir = tempfile::tempdir().unwrap();
    let path = test_support::placeholder_java_file(dir.path(), "Foo.java");
    let mut state = EditorState::new();
    let index = state.open_tab(path.clone()).unwrap();
    let mut parsers = vec![tabs::open_parser_for(&mut state.open_tabs[index])];
    assert!(!state.open_tabs[index].is_dirty());

    std::fs::write(&path, "class Foo { /* changed externally */ }\n").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(modify_event(path.clone())).unwrap();

    let mut conflicts = HashSet::new();
    let mut deleted = HashSet::new();
    process_file_events(&rx, &mut state, &mut parsers, &mut conflicts, &mut deleted);

    assert_eq!(
        state.open_tabs[index].buffer.to_string(),
        "class Foo { /* changed externally */ }\n"
    );
    assert!(
        !state.open_tabs[index].is_dirty(),
        "a transparent reload must leave the tab clean"
    );
    assert!(conflicts.is_empty());
    assert!(deleted.is_empty());
}

#[test]
fn process_file_events_flags_a_conflict_for_a_dirty_tab() {
    let dir = tempfile::tempdir().unwrap();
    let path = test_support::placeholder_java_file(dir.path(), "Foo.java");
    let mut state = EditorState::new();
    let index = state.open_tab(path.clone()).unwrap();
    let mut parsers = vec![tabs::open_parser_for(&mut state.open_tabs[index])];
    state.open_tabs[index].buffer.insert(0, "// my local edit\n");
    assert!(state.open_tabs[index].is_dirty());
    let my_buffer_before = state.open_tabs[index].buffer.to_string();

    std::fs::write(&path, "class Foo { /* changed externally */ }\n").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(modify_event(path.clone())).unwrap();

    let mut conflicts = HashSet::new();
    let mut deleted = HashSet::new();
    process_file_events(&rx, &mut state, &mut parsers, &mut conflicts, &mut deleted);

    assert_eq!(
        state.open_tabs[index].buffer.to_string(),
        my_buffer_before,
        "a conflict must not overwrite local edits"
    );
    assert!(conflicts.contains(&path));
    assert!(deleted.is_empty());
}

#[test]
fn process_file_events_ignores_its_own_recent_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = test_support::placeholder_java_file(dir.path(), "Foo.java");
    let mut state = EditorState::new();
    let index = state.open_tab(path.clone()).unwrap();
    let mut parsers = vec![tabs::open_parser_for(&mut state.open_tabs[index])];

    // Simulates this app's own `Document::save()`: the buffer already
    // matches what's now on disk, so the event this triggers must be a
    // no-op, not a spurious reload.
    state.open_tabs[index].save().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(modify_event(path.clone())).unwrap();

    let mut conflicts = HashSet::new();
    let mut deleted = HashSet::new();
    process_file_events(&rx, &mut state, &mut parsers, &mut conflicts, &mut deleted);

    assert!(!state.open_tabs[index].is_dirty());
    assert!(conflicts.is_empty());
    assert!(deleted.is_empty());
}

#[test]
fn process_file_events_marks_an_externally_deleted_tab() {
    let dir = tempfile::tempdir().unwrap();
    let path = test_support::placeholder_java_file(dir.path(), "Foo.java");
    let mut state = EditorState::new();
    let index = state.open_tab(path.clone()).unwrap();
    let mut parsers = vec![tabs::open_parser_for(&mut state.open_tabs[index])];

    std::fs::remove_file(&path).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(remove_event(path.clone())).unwrap();

    let mut conflicts = HashSet::new();
    let mut deleted = HashSet::new();
    process_file_events(&rx, &mut state, &mut parsers, &mut conflicts, &mut deleted);

    assert!(deleted.contains(&path));
    assert!(conflicts.is_empty());
}

#[test]
fn process_file_events_ignores_paths_with_no_open_tab() {
    let dir = tempfile::tempdir().unwrap();
    let unrelated = dir.path().join("Unrelated.java");
    std::fs::write(&unrelated, "class Unrelated {}\n").unwrap();
    let mut state = EditorState::new();
    let mut parsers = Vec::new();

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(modify_event(unrelated)).unwrap();

    let mut conflicts = HashSet::new();
    let mut deleted = HashSet::new();
    // Must not panic despite there being no open tabs at all.
    process_file_events(&rx, &mut state, &mut parsers, &mut conflicts, &mut deleted);

    assert!(conflicts.is_empty());
    assert!(deleted.is_empty());
}

#[test]
fn persisted_session_round_trips_open_tabs_and_active_tab() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "A.java");
    let b = test_support::placeholder_java_file(dir.path(), "B.java");

    let mut state = EditorState::new();
    state.open_tab(a.clone()).unwrap();
    state.open_tab(b.clone()).unwrap();
    state.focus_tab(0); // B was opened last (and thus focused); explicitly refocus A

    let mut storage = FakeStorage::default();
    persist_session(&mut storage, &state);

    let mut restored_state = EditorState::new();
    let mut restored_parsers: Vec<Option<IncrementalParser>> = Vec::new();
    restore_session(&storage, &mut restored_state, &mut restored_parsers, &mut None);

    let restored_paths: Vec<&Path> = restored_state.open_tabs.iter().map(|doc| doc.path()).collect();
    assert_eq!(restored_paths, vec![a.as_path(), b.as_path()]);
    assert_eq!(restored_parsers.len(), 2);
    assert_eq!(restored_state.active_tab, Some(0));
    assert_eq!(restored_state.open_tabs[0].path(), a.as_path());
}

#[test]
fn restore_session_skips_tabs_whose_file_no_longer_exists() {
    let dir = tempfile::tempdir().unwrap();
    let kept = test_support::placeholder_java_file(dir.path(), "Kept.java");
    let deleted = dir.path().join("Deleted.java");

    let mut storage = FakeStorage::default();
    let open_tabs = format!("{}\n{}", kept.display(), deleted.display());
    storage.set_string(OPEN_TABS_KEY, open_tabs);

    let mut state = EditorState::new();
    let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
    restore_session(&storage, &mut state, &mut parsers, &mut None);

    assert_eq!(state.open_tabs.len(), 1);
    assert_eq!(state.open_tabs[0].path(), kept.as_path());
    assert_eq!(parsers.len(), 1);
}

#[test]
fn restore_session_with_no_saved_keys_is_a_no_op() {
    let storage = FakeStorage::default();
    let mut state = EditorState::new();
    let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();

    restore_session(&storage, &mut state, &mut parsers, &mut None);

    assert!(state.open_tabs.is_empty());
    assert!(parsers.is_empty());
    assert_eq!(state.active_tab, None);
}

#[test]
fn persisted_settings_round_trip() {
    let mut storage = FakeStorage::default();
    let saved_indent = IndentSettings {
        use_tabs: true,
        width: 2,
    };
    let saved_view = ViewSettings {
        word_wrap: false,
        show_whitespace: true,
        show_indent_guides: true,
        show_sticky_scroll: true,
        cursor_blink: false,
        show_editor_outline: false,
    };
    let saved_templates = UserTemplates {
        java: vec![UserTemplate {
            trigger: "myown".to_string(),
            body: "custom body".to_string(),
        }],
        kotlin: vec![],
        global: vec![UserTemplate {
            trigger: "mypipe".to_string(),
            body: "|".to_string(),
        }],
    };
    let saved_tools = ExternalToolPaths {
        checkstyle_binary: "/usr/bin/checkstyle".to_string(),
        checkstyle_config: "/usr/share/checkstyle/sun_checks.xml".to_string(),
        checkstyle_installed_version: "10.26.1".to_string(),
        pmd_binary: "/usr/bin/pmd".to_string(),
        pmd_ruleset: "rulesets/java/quickstart.xml".to_string(),
        pmd_installed_version: "7.26.0".to_string(),
        spotbugs_binary: "/usr/bin/spotbugs".to_string(),
        spotbugs_installed_version: "4.10.3".to_string(),
    };
    persist_settings(
        &mut storage,
        EditorFont::Default,
        22.5,
        false,
        saved_indent,
        saved_view,
        275.0,
        false,
        &saved_templates,
        &saved_tools,
    );

    let mut editor_font = EditorFont::JetBrainsMono;
    let mut font_size = DEFAULT_FONT_SIZE;
    let mut dark_mode = DEFAULT_DARK_MODE;
    let mut indent_settings = IndentSettings::default();
    let mut view_settings = ViewSettings::default();
    let mut side_panel_width = DEFAULT_SIDE_PANEL_WIDTH;
    let mut side_panel_visible = true;
    let mut custom_templates = UserTemplates::default();
    let mut external_tool_paths = ExternalToolPaths::default();
    restore_settings(
        &storage,
        &mut editor_font,
        &mut font_size,
        &mut dark_mode,
        &mut indent_settings,
        &mut view_settings,
        &mut side_panel_width,
        &mut side_panel_visible,
        &mut custom_templates,
        &mut external_tool_paths,
    );

    assert_eq!(editor_font, EditorFont::Default);
    assert_eq!(font_size, 22.5);
    assert!(!dark_mode);
    assert_eq!(indent_settings, saved_indent);
    assert_eq!(view_settings, saved_view);
    assert_eq!(side_panel_width, 275.0);
    assert!(!side_panel_visible);
    assert_eq!(custom_templates.java, saved_templates.java);
    assert_eq!(custom_templates.kotlin, saved_templates.kotlin);
    assert_eq!(custom_templates.global, saved_templates.global);
    assert_eq!(external_tool_paths.checkstyle_binary, saved_tools.checkstyle_binary);
    assert_eq!(external_tool_paths.checkstyle_config, saved_tools.checkstyle_config);
    assert_eq!(external_tool_paths.checkstyle_installed_version, saved_tools.checkstyle_installed_version);
    assert_eq!(external_tool_paths.pmd_binary, saved_tools.pmd_binary);
    assert_eq!(external_tool_paths.pmd_ruleset, saved_tools.pmd_ruleset);
    assert_eq!(external_tool_paths.pmd_installed_version, saved_tools.pmd_installed_version);
    assert_eq!(external_tool_paths.spotbugs_binary, saved_tools.spotbugs_binary);
    assert_eq!(external_tool_paths.spotbugs_installed_version, saved_tools.spotbugs_installed_version);
}

#[test]
fn restore_settings_with_no_saved_keys_leaves_defaults_untouched() {
    let storage = FakeStorage::default();
    let mut editor_font = EditorFont::default();
    let mut font_size = DEFAULT_FONT_SIZE;
    let mut dark_mode = DEFAULT_DARK_MODE;
    let mut indent_settings = IndentSettings::default();
    let mut view_settings = ViewSettings::default();
    let mut side_panel_width = DEFAULT_SIDE_PANEL_WIDTH;
    let mut side_panel_visible = true;

    restore_settings(
        &storage,
        &mut editor_font,
        &mut font_size,
        &mut dark_mode,
        &mut indent_settings,
        &mut view_settings,
        &mut side_panel_width,
        &mut side_panel_visible,
        &mut UserTemplates::default(),
        &mut ExternalToolPaths::default(),
    );

    assert_eq!(editor_font, EditorFont::default());
    assert_eq!(font_size, DEFAULT_FONT_SIZE);
    assert_eq!(dark_mode, DEFAULT_DARK_MODE);
    assert_eq!(indent_settings, IndentSettings::default());
    assert_eq!(view_settings, ViewSettings::default());
    assert_eq!(side_panel_width, DEFAULT_SIDE_PANEL_WIDTH);
    assert!(side_panel_visible);
}

#[test]
fn restore_settings_ignores_an_unparseable_font_size() {
    let mut storage = FakeStorage::default();
    storage.set_string(FONT_SIZE_KEY, "not-a-number".to_string());
    let mut editor_font = EditorFont::default();
    let mut font_size = DEFAULT_FONT_SIZE;
    let mut dark_mode = DEFAULT_DARK_MODE;
    let mut indent_settings = IndentSettings::default();
    let mut view_settings = ViewSettings::default();
    let mut side_panel_width = DEFAULT_SIDE_PANEL_WIDTH;
    let mut side_panel_visible = true;

    restore_settings(
        &storage,
        &mut editor_font,
        &mut font_size,
        &mut dark_mode,
        &mut indent_settings,
        &mut view_settings,
        &mut side_panel_width,
        &mut side_panel_visible,
        &mut UserTemplates::default(),
        &mut ExternalToolPaths::default(),
    );

    assert_eq!(font_size, DEFAULT_FONT_SIZE);
}

#[test]
fn restore_settings_ignores_an_unparseable_indent_width() {
    let mut storage = FakeStorage::default();
    storage.set_string(INDENT_WIDTH_KEY, "not-a-number".to_string());
    let mut editor_font = EditorFont::default();
    let mut font_size = DEFAULT_FONT_SIZE;
    let mut dark_mode = DEFAULT_DARK_MODE;
    let mut indent_settings = IndentSettings::default();
    let mut view_settings = ViewSettings::default();
    let mut side_panel_width = DEFAULT_SIDE_PANEL_WIDTH;
    let mut side_panel_visible = true;

    restore_settings(
        &storage,
        &mut editor_font,
        &mut font_size,
        &mut dark_mode,
        &mut indent_settings,
        &mut view_settings,
        &mut side_panel_width,
        &mut side_panel_visible,
        &mut UserTemplates::default(),
        &mut ExternalToolPaths::default(),
    );

    assert_eq!(indent_settings.width, IndentSettings::default().width);
}

#[test]
fn close_tabs_under_closes_every_tab_inside_a_deleted_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("pkg/sub")).unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "pkg/A.java");
    let b = test_support::placeholder_java_file(dir.path(), "pkg/sub/B.java");
    let root = test_support::placeholder_java_file(dir.path(), "Root.java");

    let mut state = EditorState::new();
    let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
    for path in [&a, &b, &root] {
        state.open_tab(path.clone()).unwrap();
        parsers.push(None);
    }

    close_tabs_under(&mut state, &mut parsers, &dir.path().join("pkg"));

    let remaining: Vec<&Path> = state.open_tabs.iter().map(|doc| doc.path()).collect();
    assert_eq!(remaining, vec![root.as_path()]);
    assert_eq!(parsers.len(), 1);
}

#[test]
fn close_tabs_under_closes_a_single_file_by_exact_path() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "A.java");

    let mut state = EditorState::new();
    let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
    state.open_tab(a.clone()).unwrap();
    parsers.push(None);

    close_tabs_under(&mut state, &mut parsers, &a);

    assert!(state.open_tabs.is_empty());
    assert!(parsers.is_empty());
}

#[test]
fn handle_rename_repoints_every_tab_inside_a_renamed_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("old_pkg/sub")).unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "old_pkg/A.java");
    let b = test_support::placeholder_java_file(dir.path(), "old_pkg/sub/B.java");

    let mut state = EditorState::new();
    let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
    for path in [&a, &b] {
        let index = state.open_tab(path.clone()).unwrap();
        parsers.push(tabs::open_parser_for(&mut state.open_tabs[index]));
    }

    let old_pkg = dir.path().join("old_pkg");
    let new_pkg = dir.path().join("new_pkg");
    handle_rename(&mut state, &mut parsers, &old_pkg, &new_pkg);

    let repointed: Vec<PathBuf> = state.open_tabs.iter().map(|doc| doc.path().to_path_buf()).collect();
    assert_eq!(repointed, vec![new_pkg.join("A.java"), new_pkg.join("sub/B.java")]);
}

#[test]
fn renaming_the_open_file_itself_leaves_it_saveable() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "Old.java");

    let mut state = EditorState::new();
    let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
    let index = state.open_tab(a.clone()).unwrap();
    parsers.push(tabs::open_parser_for(&mut state.open_tabs[index]));
    state.open_tabs[index].buffer.insert(0, "// edited\n");

    let new_path = dir.path().join("New.java");
    std::fs::rename(&a, &new_path).unwrap();
    handle_rename(&mut state, &mut parsers, &a, &new_path);

    let mut last_error = None;
    tabs::save_active_tab(&mut state, &mut parsers, &mut last_error);

    assert_eq!(last_error, None, "save produced an error: {last_error:?}");
    assert_eq!(
        std::fs::read_to_string(&new_path).unwrap(),
        "// edited\nclass Old.java {}"
    );
}

#[test]
fn handle_rename_repoints_a_single_tab_by_exact_path() {
    let dir = tempfile::tempdir().unwrap();
    let a = test_support::placeholder_java_file(dir.path(), "Old.java");

    let mut state = EditorState::new();
    let mut parsers: Vec<Option<IncrementalParser>> = Vec::new();
    state.open_tab(a.clone()).unwrap();
    parsers.push(tabs::open_parser_for(&mut state.open_tabs[0]));

    let new_path = dir.path().join("New.java");
    handle_rename(&mut state, &mut parsers, &a, &new_path);

    assert_eq!(state.open_tabs[0].path(), new_path.as_path());
    // `Path`'s `==` normalizes away a trailing separator, so it alone
    // wouldn't have caught `new.join("")` producing "New.java/" — the
    // OS-facing representation has to be checked too.
    assert_eq!(state.open_tabs[0].path().as_os_str(), new_path.as_os_str());
}

#[test]
fn resolve_pending_navigation_converts_byte_to_char_offset_and_clears_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Foo.java");
    // "café"'s `é` is 2 bytes, so `bar`'s byte offset and char offset
    // genuinely differ — a real test of the conversion, not a coincidence
    // that would also pass with no conversion happening at all.
    let source = "// café\nclass Foo { void bar() {} }\n";
    std::fs::write(&path, source).unwrap();

    let mut state = EditorState::new();
    state.open_tab(path.clone()).unwrap();

    let byte_offset = source.find("bar").unwrap();
    let expected_char_offset = source[..byte_offset].chars().count();
    assert_ne!(byte_offset, expected_char_offset, "sanity: café's multi-byte é must make these differ");

    let mut pending = Some((path.clone(), byte_offset));
    let resolved = resolve_pending_navigation(&state, &mut pending);

    assert_eq!(resolved, Some((path, expected_char_offset)));
    assert!(pending.is_none(), "resolving clears the field");
}

#[test]
fn resolve_pending_navigation_is_none_with_nothing_pending() {
    let state = EditorState::new();
    let mut pending = None;
    assert_eq!(resolve_pending_navigation(&state, &mut pending), None);
}

#[test]
fn resolve_pending_navigation_leaves_the_field_pending_if_the_document_isnt_open() {
    let state = EditorState::new();
    let mut pending = Some((PathBuf::from("/not/open.java"), 5));
    assert_eq!(resolve_pending_navigation(&state, &mut pending), None);
    assert!(pending.is_some(), "left pending for a later frame to retry");
}
