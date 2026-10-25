use super::*;

fn choice(label: &str, name: &str) -> Counterpart {
    Counterpart { path: PathBuf::from("/p/src").join(name), byte_offset: 4, label: label.to_string() }
}

fn frame(ctx: &egui::Context, state: &mut CounterpartsState, events: Vec<egui::Event>) -> Option<(PathBuf, usize)> {
    let mut picked = None;
    let _ = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        picked = show(ui, state);
    });
    picked
}

fn escape() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn nothing_is_open_until_choices_are_given() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut state = CounterpartsState::default();
    assert!(!state.is_open());
    assert_eq!(frame(&ctx, &mut state, vec![]), None);

    state.open(Path::new("/p"), vec![choice("jvmMain", "A.kt"), choice("iosMain", "B.kt")]);
    assert!(state.is_open());
}

#[test]
fn escape_dismisses_the_picker_without_choosing() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut state = CounterpartsState::default();
    state.open(Path::new("/p"), vec![choice("jvmMain", "A.kt"), choice("iosMain", "B.kt")]);
    // The first frame lets the modal learn it is topmost, as in `modal_test`.
    assert_eq!(frame(&ctx, &mut state, vec![]), None);
    assert_eq!(frame(&ctx, &mut state, vec![escape()]), None);
    assert!(!state.is_open());
}
