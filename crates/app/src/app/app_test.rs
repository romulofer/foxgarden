//! Unit tests for [`super`](app.rs), extracted verbatim from that
//! file's colocated `#[cfg(test)] mod tests` so the module file stays focused
//! on the code under test. Behavior-identical to the inline module it replaced.

use super::*;
use crate::auto_save::{AutoSaveMode, AutoSaveSettings, AutoSaveState};
use crate::jdk_registry::RegisteredJdk;
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
    process_file_events(
        &rx,
        &mut state,
        &mut parsers,
        &mut conflicts,
        &mut deleted,
        &mut DiffState::default(),
        None,
    );

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
    process_file_events(
        &rx,
        &mut state,
        &mut parsers,
        &mut conflicts,
        &mut deleted,
        &mut DiffState::default(),
        None,
    );

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
    state.open_tabs[index].save(true).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(modify_event(path.clone())).unwrap();

    let mut conflicts = HashSet::new();
    let mut deleted = HashSet::new();
    process_file_events(
        &rx,
        &mut state,
        &mut parsers,
        &mut conflicts,
        &mut deleted,
        &mut DiffState::default(),
        None,
    );

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
    process_file_events(
        &rx,
        &mut state,
        &mut parsers,
        &mut conflicts,
        &mut deleted,
        &mut DiffState::default(),
        None,
    );

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
    process_file_events(
        &rx,
        &mut state,
        &mut parsers,
        &mut conflicts,
        &mut deleted,
        &mut DiffState::default(),
        None,
    );

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
        show_inline_blame: false,
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
    let saved_auto_save = AutoSaveSettings {
        enabled: true,
        mode: AutoSaveMode::AfterIdle,
        idle_seconds: 45,
    };
    let saved_lsp = LspSettings {
        enabled: true,
        jdtls_binary: "/usr/bin/jdtls".to_string(),
        jdtls_installed_version: "1.60.0".to_string(),
        jdtls_java_home: "/usr/lib/jvm/java-21".to_string(),
        kotlin_language_server_binary: "/usr/bin/kotlin-language-server".to_string(),
        kotlin_language_server_installed_version: "1.3.13".to_string(),
    };
    let saved_jdk_registry = JdkRegistry {
        jdks: vec![RegisteredJdk {
            label: "Java 21".to_string(),
            home: PathBuf::from("/usr/lib/jvm/java-21"),
            major_version: Some(21),
        }],
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
        true,
        true,
        true,
        &saved_templates,
        &saved_tools,
        saved_auto_save,
        false,
        &saved_lsp,
        &saved_jdk_registry,
    );

    let mut editor_font = EditorFont::JetBrainsMono;
    let mut font_size = DEFAULT_FONT_SIZE;
    let mut dark_mode = DEFAULT_DARK_MODE;
    let mut indent_settings = IndentSettings::default();
    let mut view_settings = ViewSettings::default();
    let mut side_panel_width = DEFAULT_SIDE_PANEL_WIDTH;
    let mut side_panel_visible = true;
    let mut terminal_panel_visible = false;
    let mut source_control_visible = false;
    let mut build_panel_visible = false;
    let mut custom_templates = UserTemplates::default();
    let mut external_tool_paths = ExternalToolPaths::default();
    let mut auto_save_settings = AutoSaveSettings::default();
    let mut lsp_settings = LspSettings::default();
    let mut jdk_registry = JdkRegistry::default();
    let mut trim_trailing_whitespace_on_save = true;
    restore_settings(
        &storage,
        &mut editor_font,
        &mut font_size,
        &mut dark_mode,
        &mut indent_settings,
        &mut view_settings,
        &mut side_panel_width,
        &mut side_panel_visible,
        &mut terminal_panel_visible,
        &mut source_control_visible,
        &mut build_panel_visible,
        &mut custom_templates,
        &mut external_tool_paths,
        &mut auto_save_settings,
        &mut trim_trailing_whitespace_on_save,
        &mut lsp_settings,
        &mut jdk_registry,
    );

    assert_eq!(editor_font, EditorFont::Default);
    assert_eq!(font_size, 22.5);
    assert!(!dark_mode);
    assert_eq!(indent_settings, saved_indent);
    assert_eq!(view_settings, saved_view);
    assert_eq!(side_panel_width, 275.0);
    assert!(!side_panel_visible);
    assert!(terminal_panel_visible);
    assert!(source_control_visible);
    assert!(build_panel_visible);
    assert_eq!(custom_templates.java, saved_templates.java);
    assert_eq!(custom_templates.kotlin, saved_templates.kotlin);
    assert_eq!(custom_templates.global, saved_templates.global);
    assert_eq!(external_tool_paths.checkstyle_binary, saved_tools.checkstyle_binary);
    assert_eq!(external_tool_paths.checkstyle_config, saved_tools.checkstyle_config);
    assert_eq!(
        external_tool_paths.checkstyle_installed_version,
        saved_tools.checkstyle_installed_version
    );
    assert_eq!(external_tool_paths.pmd_binary, saved_tools.pmd_binary);
    assert_eq!(external_tool_paths.pmd_ruleset, saved_tools.pmd_ruleset);
    assert_eq!(
        external_tool_paths.pmd_installed_version,
        saved_tools.pmd_installed_version
    );
    assert_eq!(external_tool_paths.spotbugs_binary, saved_tools.spotbugs_binary);
    assert_eq!(
        external_tool_paths.spotbugs_installed_version,
        saved_tools.spotbugs_installed_version
    );
    assert_eq!(auto_save_settings, saved_auto_save);
    assert_eq!(lsp_settings, saved_lsp);
    assert_eq!(jdk_registry, saved_jdk_registry);
}

/// Settings > Language is persisted like any other setting, but through
/// `fg_i18n`'s process-global rather than a field on `FoxGardenApp` — these
/// cover both ends of that without ever writing to the global, which the
/// rest of this test binary is reading in parallel.
#[test]
fn persist_settings_writes_the_active_language() {
    let mut storage = FakeStorage::default();

    persist_settings(
        &mut storage,
        EditorFont::default(),
        DEFAULT_FONT_SIZE,
        DEFAULT_DARK_MODE,
        IndentSettings::default(),
        ViewSettings::default(),
        DEFAULT_SIDE_PANEL_WIDTH,
        true,
        false,
        false,
        false,
        &UserTemplates::default(),
        &ExternalToolPaths::default(),
        AutoSaveSettings::default(),
        true,
        &LspSettings::default(),
        &JdkRegistry::default(),
    );

    assert_eq!(storage.get_string(LANGUAGE_KEY).as_deref(), Some(fg_i18n::lang().tag()));
}

#[test]
fn stored_language_reads_back_an_explicit_choice() {
    let mut storage = FakeStorage::default();
    storage.set_string(LANGUAGE_KEY, "en-US".to_string());

    assert_eq!(stored_language(Some(&storage)), Some(fg_i18n::Lang::EnUs));
}

/// All three "nobody chose" shapes answer `None`, which is what makes
/// `FoxGardenApp::new` fall through to detecting the host locale instead of
/// pinning an arbitrary language.
#[test]
fn stored_language_is_none_when_nothing_valid_was_saved() {
    assert_eq!(stored_language(None), None, "no storage at all");
    assert_eq!(
        stored_language(Some(&FakeStorage::default())),
        None,
        "storage, but no key"
    );

    let mut storage = FakeStorage::default();
    storage.set_string(LANGUAGE_KEY, "tlh-Piqd".to_string());
    assert_eq!(stored_language(Some(&storage)), None, "a tag this build doesn't know");
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
    let mut terminal_panel_visible = false;
    let mut source_control_visible = false;

    let mut auto_save_settings = AutoSaveSettings::default();
    let mut lsp_settings = LspSettings::default();
    let mut trim_trailing_whitespace_on_save = true;
    restore_settings(
        &storage,
        &mut editor_font,
        &mut font_size,
        &mut dark_mode,
        &mut indent_settings,
        &mut view_settings,
        &mut side_panel_width,
        &mut side_panel_visible,
        &mut terminal_panel_visible,
        &mut source_control_visible,
        &mut false,
        &mut UserTemplates::default(),
        &mut ExternalToolPaths::default(),
        &mut auto_save_settings,
        &mut trim_trailing_whitespace_on_save,
        &mut lsp_settings,
        &mut JdkRegistry::default(),
    );

    assert_eq!(editor_font, EditorFont::default());
    assert_eq!(font_size, DEFAULT_FONT_SIZE);
    assert_eq!(dark_mode, DEFAULT_DARK_MODE);
    assert_eq!(indent_settings, IndentSettings::default());
    assert_eq!(view_settings, ViewSettings::default());
    assert_eq!(side_panel_width, DEFAULT_SIDE_PANEL_WIDTH);
    assert!(side_panel_visible);
    assert_eq!(auto_save_settings, AutoSaveSettings::default());
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
    let mut terminal_panel_visible = false;
    let mut source_control_visible = false;

    let mut auto_save_settings = AutoSaveSettings::default();
    let mut lsp_settings = LspSettings::default();
    let mut trim_trailing_whitespace_on_save = true;
    restore_settings(
        &storage,
        &mut editor_font,
        &mut font_size,
        &mut dark_mode,
        &mut indent_settings,
        &mut view_settings,
        &mut side_panel_width,
        &mut side_panel_visible,
        &mut terminal_panel_visible,
        &mut source_control_visible,
        &mut false,
        &mut UserTemplates::default(),
        &mut ExternalToolPaths::default(),
        &mut auto_save_settings,
        &mut trim_trailing_whitespace_on_save,
        &mut lsp_settings,
        &mut JdkRegistry::default(),
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
    let mut terminal_panel_visible = false;
    let mut source_control_visible = false;

    let mut auto_save_settings = AutoSaveSettings::default();
    let mut lsp_settings = LspSettings::default();
    let mut trim_trailing_whitespace_on_save = true;
    restore_settings(
        &storage,
        &mut editor_font,
        &mut font_size,
        &mut dark_mode,
        &mut indent_settings,
        &mut view_settings,
        &mut side_panel_width,
        &mut side_panel_visible,
        &mut terminal_panel_visible,
        &mut source_control_visible,
        &mut false,
        &mut UserTemplates::default(),
        &mut ExternalToolPaths::default(),
        &mut auto_save_settings,
        &mut trim_trailing_whitespace_on_save,
        &mut lsp_settings,
        &mut JdkRegistry::default(),
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
    tabs::save_active_tab(&mut state, &mut parsers, &mut last_error, true);

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
    assert_ne!(
        byte_offset, expected_char_offset,
        "sanity: café's multi-byte é must make these differ"
    );

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

#[test]
fn restore_settings_ignores_an_unparseable_auto_save_idle_seconds() {
    let mut storage = FakeStorage::default();
    storage.set_string(AUTO_SAVE_IDLE_SECONDS_KEY, "not-a-number".to_string());
    let mut editor_font = EditorFont::default();
    let mut font_size = DEFAULT_FONT_SIZE;
    let mut dark_mode = DEFAULT_DARK_MODE;
    let mut indent_settings = IndentSettings::default();
    let mut view_settings = ViewSettings::default();
    let mut side_panel_width = DEFAULT_SIDE_PANEL_WIDTH;
    let mut side_panel_visible = true;
    let mut terminal_panel_visible = false;
    let mut source_control_visible = false;
    let mut auto_save_settings = AutoSaveSettings::default();

    let mut lsp_settings = LspSettings::default();
    let mut trim_trailing_whitespace_on_save = true;
    restore_settings(
        &storage,
        &mut editor_font,
        &mut font_size,
        &mut dark_mode,
        &mut indent_settings,
        &mut view_settings,
        &mut side_panel_width,
        &mut side_panel_visible,
        &mut terminal_panel_visible,
        &mut source_control_visible,
        &mut false,
        &mut UserTemplates::default(),
        &mut ExternalToolPaths::default(),
        &mut auto_save_settings,
        &mut trim_trailing_whitespace_on_save,
        &mut lsp_settings,
        &mut JdkRegistry::default(),
    );

    assert_eq!(
        auto_save_settings.idle_seconds,
        AutoSaveSettings::default().idle_seconds
    );
}

#[test]
fn restore_settings_falls_back_to_on_focus_loss_for_an_unrecognized_mode() {
    let mut storage = FakeStorage::default();
    storage.set_string(AUTO_SAVE_MODE_KEY, "not-a-real-mode".to_string());
    let mut editor_font = EditorFont::default();
    let mut font_size = DEFAULT_FONT_SIZE;
    let mut dark_mode = DEFAULT_DARK_MODE;
    let mut indent_settings = IndentSettings::default();
    let mut view_settings = ViewSettings::default();
    let mut side_panel_width = DEFAULT_SIDE_PANEL_WIDTH;
    let mut side_panel_visible = true;
    let mut terminal_panel_visible = false;
    let mut source_control_visible = false;
    let mut auto_save_settings = AutoSaveSettings {
        enabled: true,
        mode: AutoSaveMode::AfterIdle,
        idle_seconds: 30,
    };

    let mut lsp_settings = LspSettings::default();
    let mut trim_trailing_whitespace_on_save = true;
    restore_settings(
        &storage,
        &mut editor_font,
        &mut font_size,
        &mut dark_mode,
        &mut indent_settings,
        &mut view_settings,
        &mut side_panel_width,
        &mut side_panel_visible,
        &mut terminal_panel_visible,
        &mut source_control_visible,
        &mut false,
        &mut UserTemplates::default(),
        &mut ExternalToolPaths::default(),
        &mut auto_save_settings,
        &mut trim_trailing_whitespace_on_save,
        &mut lsp_settings,
        &mut JdkRegistry::default(),
    );

    assert_eq!(auto_save_settings.mode, AutoSaveMode::OnFocusLoss);
}

/// The Track 6 Phase 1 checkpoint test: a fake egui clock/focus signal
/// (mirroring `FoxGardenApp::ui`'s own `ui.input(|i| (i.time, i.focused,
/// ...))` read) driving `AutoSaveState::tick`, wired to the same
/// `tabs::save_all_dirty_tabs` the app calls when it fires — proves the
/// full path (not just `auto_save`'s own isolated trigger-logic unit
/// tests) actually writes a dirty tab to disk at the right moment and
/// leaves a clean one untouched.
#[test]
fn auto_save_focus_loss_trigger_saves_only_the_dirty_tab() {
    let dir = tempfile::tempdir().unwrap();
    let dirty_path = test_support::placeholder_java_file(dir.path(), "Dirty.java");
    let clean_path = test_support::placeholder_java_file(dir.path(), "Clean.java");

    let mut state = EditorState::new();
    let dirty_index = state.open_tab(dirty_path.clone()).unwrap();
    let clean_index = state.open_tab(clean_path.clone()).unwrap();
    let mut parsers = vec![
        tabs::open_parser_for(&mut state.open_tabs[dirty_index]),
        tabs::open_parser_for(&mut state.open_tabs[clean_index]),
    ];
    state.open_tabs[dirty_index].buffer.insert(0, "// unsaved edit\n");
    assert!(state.open_tabs[dirty_index].is_dirty());
    assert!(!state.open_tabs[clean_index].is_dirty());

    let settings = AutoSaveSettings {
        enabled: true,
        mode: AutoSaveMode::OnFocusLoss,
        idle_seconds: 30,
    };
    let mut auto_save_state = AutoSaveState::default();
    let mut last_error = None;

    // Frame 1: app is focused — no trigger, nothing saved yet.
    assert!(!auto_save_state.tick(settings, true, 0.0));
    assert!(state.open_tabs[dirty_index].is_dirty(), "no trigger yet: still dirty");

    // Frame 2: focus is lost — the edge fires, and the dirty tab (only) saves.
    assert!(auto_save_state.tick(settings, false, 1.0));
    tabs::save_all_dirty_tabs(&mut state, &mut parsers, &mut last_error, &HashSet::new(), true);

    assert_eq!(last_error, None, "auto-save produced an error: {last_error:?}");
    assert!(
        !state.open_tabs[dirty_index].is_dirty(),
        "focus loss must save the dirty tab"
    );
    assert_eq!(
        std::fs::read_to_string(&dirty_path).unwrap(),
        "// unsaved edit\nclass Dirty.java {}"
    );
    assert_eq!(
        std::fs::read_to_string(&clean_path).unwrap(),
        "class Clean.java {}",
        "the already-clean tab must not be rewritten"
    );
}

#[test]
fn auto_save_idle_trigger_fires_only_after_the_threshold_with_no_activity() {
    let dir = tempfile::tempdir().unwrap();
    let path = test_support::placeholder_java_file(dir.path(), "Idle.java");

    let mut state = EditorState::new();
    let index = state.open_tab(path.clone()).unwrap();
    let mut parsers = vec![tabs::open_parser_for(&mut state.open_tabs[index])];
    state.open_tabs[index].buffer.insert(0, "// idle edit\n");

    let settings = AutoSaveSettings {
        enabled: true,
        mode: AutoSaveMode::AfterIdle,
        idle_seconds: 10,
    };
    let mut auto_save_state = AutoSaveState::default();
    auto_save_state.record_activity(0.0);
    let mut last_error = None;

    assert!(
        !auto_save_state.tick(settings, true, 5.0),
        "only 5s idle: no trigger yet"
    );
    assert!(state.open_tabs[index].is_dirty());

    assert!(
        auto_save_state.tick(settings, true, 10.0),
        "10s idle: threshold reached"
    );
    tabs::save_all_dirty_tabs(&mut state, &mut parsers, &mut last_error, &HashSet::new(), true);

    assert_eq!(last_error, None, "auto-save produced an error: {last_error:?}");
    assert!(!state.open_tabs[index].is_dirty());
}

/// Track 6 Phase 2's own checkpoint: a tab currently showing the "changed
/// on disk" conflict banner must not be silently overwritten by an
/// auto-save trigger — `external_conflicts` is exactly what
/// `show_external_change_banner` reads to decide whether that banner is
/// showing (see `process_file_events_flags_a_conflict_for_a_dirty_tab`
/// above for how a real conflict populates it).
#[test]
fn auto_save_skips_a_tab_showing_the_external_conflict_banner() {
    let dir = tempfile::tempdir().unwrap();
    let conflicted_path = test_support::placeholder_java_file(dir.path(), "Conflicted.java");
    let plain_path = test_support::placeholder_java_file(dir.path(), "Plain.java");

    let mut state = EditorState::new();
    let conflicted_index = state.open_tab(conflicted_path.clone()).unwrap();
    let plain_index = state.open_tab(plain_path.clone()).unwrap();
    let mut parsers = vec![
        tabs::open_parser_for(&mut state.open_tabs[conflicted_index]),
        tabs::open_parser_for(&mut state.open_tabs[plain_index]),
    ];
    state.open_tabs[conflicted_index].buffer.insert(0, "// my local edit\n");
    state.open_tabs[plain_index].buffer.insert(0, "// unconflicted edit\n");
    let conflicted_buffer_before = state.open_tabs[conflicted_index].buffer.to_string();

    let mut external_conflicts = HashSet::new();
    external_conflicts.insert(conflicted_path.clone());
    let mut last_error = None;

    tabs::save_all_dirty_tabs(&mut state, &mut parsers, &mut last_error, &external_conflicts, true);

    assert_eq!(last_error, None, "auto-save produced an error: {last_error:?}");
    assert!(
        state.open_tabs[conflicted_index].is_dirty(),
        "a conflicted tab must not be auto-saved out from under its banner"
    );
    assert_eq!(
        state.open_tabs[conflicted_index].buffer.to_string(),
        conflicted_buffer_before
    );
    assert!(
        !state.open_tabs[plain_index].is_dirty(),
        "an unconflicted dirty tab still saves normally"
    );
}

#[test]
fn resolve_pending_navigation_clamps_a_byte_offset_stale_past_the_buffers_current_length() {
    let dir = tempfile::tempdir().unwrap();
    let path = test_support::placeholder_java_file(dir.path(), "Foo.java");
    let mut state = EditorState::new();
    let index = state.open_tab(path.clone()).unwrap();
    let len_bytes = state.open_tabs[index].buffer.len_bytes();

    // Queued when the buffer was longer (or the navigation target was
    // computed against a different version of it) — now points well past
    // this buffer's own current end.
    let mut pending_navigation = Some((path.clone(), len_bytes + 1_000));

    let resolved = resolve_pending_navigation(&state, &mut pending_navigation);

    let (resolved_path, char_offset) = resolved.expect("the target document is open");
    assert_eq!(resolved_path, path);
    assert_eq!(char_offset, state.open_tabs[index].buffer.len_chars());
    assert!(pending_navigation.is_none(), "resolving clears the pending navigation");
}
