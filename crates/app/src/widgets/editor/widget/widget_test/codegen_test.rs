//! Java code generation end to end (`codegen.rs`): getter/setter accessors, constructor/toString/equals generation, and Override Method.

use super::super::*;
use super::common_test::*;
use fg_core::Language;
use fg_i18n::{msg, t};

#[test]
fn ctrl_shift_g_generates_getter_and_setter_at_the_cursor() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    // A fresh widget's default cursor sits at char 0, which is still
    // "inside" the class_declaration spanning the whole file — see
    // `syntax::java_fields_in_enclosing_class`'s inclusive containment
    // check.
    focused_frame(&mut doc, &mut parser, vec![command_shift_key_event(egui::Key::G)]);

    let text = doc.buffer.to_string();
    assert!(text.contains("public int getX() {\n        return this.x;\n    }"));
    assert!(text.contains("public void setX(int x) {\n        this.x = x;\n    }"));
}

#[test]
fn ctrl_shift_g_is_a_no_op_for_kotlin_files_but_reports_why() {
    // Kotlin's `val`/`var` properties already are getters/setters;
    // generating explicit Java-shaped ones for them isn't idiomatic
    // (see `widget::show`'s comment on this shortcut), so the command
    // does nothing for a non-Java file — but must say so via
    // `last_error` rather than silently doing nothing.
    let (_dir, mut doc) = open_fixture("class Foo(val x: Int)\n", "Foo.kt");
    let mut parser = parsed(Language::Kotlin, &doc.buffer.to_string());
    let before = doc.buffer.to_string();

    let last_error = focused_frame_with_generate_request(
        &mut doc,
        &mut parser,
        None,
        &mut None,
        vec![command_shift_key_event(egui::Key::G)],
    );

    assert_eq!(doc.buffer.to_string(), before);
    assert!(last_error.is_some_and(|msg| msg.contains("Java")));
}

#[test]
fn ctrl_shift_g_on_a_fieldless_class_is_a_no_op_but_reports_why() {
    let (_dir, mut doc) = open_fixture("public class Empty {\n}\n", "Empty.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let before = doc.buffer.to_string();

    let last_error = focused_frame_with_generate_request(
        &mut doc,
        &mut parser,
        None,
        &mut None,
        vec![command_shift_key_event(egui::Key::G)],
    );

    assert_eq!(doc.buffer.to_string(), before);
    assert_eq!(last_error.as_deref(), Some(t().errors.no_class_fields));
}

#[test]
fn tools_menu_generate_getters_inserts_only_a_getter() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let last_error =
        focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Getters), &mut None, vec![]);

    assert_eq!(last_error, None);
    let text = doc.buffer.to_string();
    assert!(text.contains("getX"));
    assert!(!text.contains("setX"));
}

#[test]
fn tools_menu_generate_setters_inserts_only_a_setter() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let last_error =
        focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Setters), &mut None, vec![]);

    assert_eq!(last_error, None);
    let text = doc.buffer.to_string();
    assert!(!text.contains("getX"));
    assert!(text.contains("setX"));
}

#[test]
fn tools_menu_generate_setters_on_an_all_final_class_reports_why() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private final int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let before = doc.buffer.to_string();

    let last_error =
        focused_frame_with_generate_request(&mut doc, &mut parser, Some(AccessorKind::Setters), &mut None, vec![]);

    assert_eq!(doc.buffer.to_string(), before);
    assert!(last_error.is_some_and(|msg| msg.contains("final")));
}

#[test]
fn tools_menu_generate_getters_on_a_multi_class_file_opens_the_picker_instead_of_generating() {
    let (_dir, mut doc) = open_fixture(
        "class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n",
        "Foo.java",
    );
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let before = doc.buffer.to_string();
    let mut generate_dialog = None;

    let last_error = focused_frame_with_generate_request(
        &mut doc,
        &mut parser,
        Some(AccessorKind::Getters),
        &mut generate_dialog,
        vec![],
    );

    assert_eq!(last_error, None);
    assert_eq!(
        doc.buffer.to_string(),
        before,
        "nothing should be inserted until the picker's Generate is clicked"
    );
    let dialog = generate_dialog.expect("multiple eligible classes should open the picker");
    assert_eq!(
        dialog.classes().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["Foo", "Bar"]
    );
}

#[test]
fn tools_menu_generate_constructor_inserts_immediately_for_a_single_eligible_class() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let last_error = focused_frame_with_generate_method_request(
        &mut doc,
        &mut parser,
        Some(GenerateMethodKind::Constructor),
        &mut None,
    );

    assert_eq!(last_error, None);
    assert!(doc.buffer.to_string().contains("public Foo(int x)"));
}

#[test]
fn tools_menu_generate_to_string_inserts_immediately_for_a_single_eligible_class() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let last_error = focused_frame_with_generate_method_request(
        &mut doc,
        &mut parser,
        Some(GenerateMethodKind::ToString),
        &mut None,
    );

    assert_eq!(last_error, None);
    assert!(doc.buffer.to_string().contains("public String toString()"));
}

#[test]
fn tools_menu_generate_equals_and_hash_code_inserts_both_together() {
    let (_dir, mut doc) = open_fixture("public class Foo {\n    private int x;\n}\n", "Foo.java");
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());

    let last_error = focused_frame_with_generate_method_request(
        &mut doc,
        &mut parser,
        Some(GenerateMethodKind::EqualsAndHashCode),
        &mut None,
    );

    assert_eq!(last_error, None);
    let text = doc.buffer.to_string();
    assert!(text.contains("public boolean equals(Object o)"));
    assert!(text.contains("public int hashCode()"));
}

#[test]
fn generate_method_request_on_a_kotlin_file_reports_why() {
    let (_dir, mut doc) = open_fixture("class Foo(val x: Int)\n", "Foo.kt");
    let mut parser = parsed(Language::Kotlin, &doc.buffer.to_string());
    let before = doc.buffer.to_string();

    let last_error = focused_frame_with_generate_method_request(
        &mut doc,
        &mut parser,
        Some(GenerateMethodKind::Constructor),
        &mut None,
    );

    assert_eq!(doc.buffer.to_string(), before);
    assert!(last_error.is_some_and(|msg| msg.contains("Java")));
}

#[test]
fn generate_method_request_on_a_multi_class_file_opens_the_picker_instead_of_generating() {
    let (_dir, mut doc) = open_fixture(
        "class Foo {\n    private int x;\n}\nclass Bar {\n    private int y;\n}\n",
        "Foo.java",
    );
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let before = doc.buffer.to_string();
    let mut generate_method_dialog = None;

    let last_error = focused_frame_with_generate_method_request(
        &mut doc,
        &mut parser,
        Some(GenerateMethodKind::ToString),
        &mut generate_method_dialog,
    );

    assert_eq!(last_error, None);
    assert_eq!(
        doc.buffer.to_string(),
        before,
        "nothing should be inserted until the picker's Generate is clicked"
    );
    let dialog = generate_method_dialog.expect("multiple eligible classes should open the picker");
    assert_eq!(
        dialog.classes().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["Foo", "Bar"]
    );
}

#[test]
fn override_method_finds_an_inherited_method_via_the_project_tree() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Base.java"),
        "public class Base {\n    public void run() {\n    }\n}\n",
    )
    .unwrap();
    let foo_path = dir.path().join("Foo.java");
    std::fs::write(&foo_path, "public class Foo extends Base {\n}\n").unwrap();

    let mut doc = Document::open(foo_path).unwrap();
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
    let cursor = doc.buffer.to_string().find('{').unwrap() + 1; // inside Foo's body

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            0, // pane (Track 11)
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            false,
            &mut None,
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
        true,
        &mut None,
        );
    });
    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: cursor,
            anchor: cursor,
        },
    );

    let mut override_method_dialog = None;
    let mut last_error = None;
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            0, // pane (Track 11)
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            true,
            &mut override_method_dialog,
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut last_error,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
        true,
        &mut None,
        );
    });

    assert_eq!(last_error, None);
    let dialog = override_method_dialog.expect("Base.run() should be found as an overridable method");
    assert_eq!(dialog.methods().len(), 1);
    assert_eq!(dialog.methods()[0].name, "run");
}

#[test]
fn override_method_excludes_a_method_the_current_class_already_overrides() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Base.java"),
        "public class Base {\n    public void run() {\n    }\n    public void stop() {\n    }\n}\n",
    )
    .unwrap();
    let foo_path = dir.path().join("Foo.java");
    let foo_source = "public class Foo extends Base {\n    public void run() {\n    }\n}\n";
    std::fs::write(&foo_path, foo_source).unwrap();

    let mut doc = Document::open(foo_path).unwrap();
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
    let cursor = foo_source.find('{').unwrap() + 1;

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            0, // pane (Track 11)
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            false,
            &mut None,
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
        true,
        &mut None,
        );
    });
    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: cursor,
            anchor: cursor,
        },
    );

    let mut override_method_dialog = None;
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            0, // pane (Track 11)
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            true,
            &mut override_method_dialog,
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
        true,
        &mut None,
        );
    });

    let dialog = override_method_dialog.expect("stop() should still be offered");
    assert_eq!(dialog.methods().len(), 1);
    assert_eq!(dialog.methods()[0].name, "stop");
}

#[test]
fn override_method_on_a_superclass_not_found_in_the_project_reports_why() {
    let dir = tempfile::tempdir().unwrap();
    let foo_path = dir.path().join("Foo.java");
    let foo_source = "public class Foo extends SomeLibraryClass {\n}\n";
    std::fs::write(&foo_path, foo_source).unwrap();

    let mut doc = Document::open(foo_path).unwrap();
    let mut parser = parsed(Language::Java, &doc.buffer.to_string());
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let id = egui::Id::new(doc.path.to_string_lossy().into_owned());
    let cursor = foo_source.find('{').unwrap() + 1;

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            0, // pane (Track 11)
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            false,
            &mut None,
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut None,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
        true,
        &mut None,
        );
    });
    text_area::set_caret(
        &ctx,
        id,
        Caret {
            primary: cursor,
            anchor: cursor,
        },
    );

    let mut override_method_dialog = None;
    let mut last_error = None;
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.memory_mut(|mem| mem.request_focus(id));
        show(
            ui,
            &mut doc,
            &mut parser,
            0, // pane (Track 11)
            EditorFont::Default,
            14.0,
            IndentSettings::default(),
            ViewSettings::default(),
            None,
            &mut None,
            None,
            &mut None,
            Some(&project),
            true,
            &mut override_method_dialog,
            &mut None,
            &mut HoverState::default(),
            &mut GotoDefinitionState::default(),
            &mut PeekState::default(),
            EditorRequests::default(),
            &mut last_error,
            &mut Vec::new(),
            &mut None,
            &UserTemplates::default(),
            &mut crate::panels::spring_config::SpringConfigState::default(),
            &mut crate::lsp_state::LspState::default(),
            &mut FindReferencesState::default(),
            &mut RenameBox::default(),
            &mut CodeActionGutter::default(),
            &crate::debug_state::DebugState::default(),
        true,
        &mut None,
        );
    });

    assert!(override_method_dialog.is_none());
    assert_eq!(last_error, Some(msg::superclass_not_in_project("SomeLibraryClass")));
}
