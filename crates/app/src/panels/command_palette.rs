//! `Ctrl+Shift+P`: every common action by name, with its shortcut.
//!
//! Without it, an action is reachable only if the user already knows which
//! of five menus holds it, or remembers a shortcut nothing ever showed
//! them. The palette is also the honest answer to this app's shortcuts
//! diverging from what people expect elsewhere (`Ctrl+E` for recent files,
//! `Ctrl+Shift+E` for the Spring endpoint map): a user who types "recent"
//! finds it and learns the key on the way.
//!
//! Commands are deliberately a fixed, hand-picked list rather than a
//! reflection of the menu tree: the menus contain settings toggles and
//! submenus that make no sense as one-shot commands, and a palette that
//! lists everything is as hard to scan as the menus it replaces.

use fg_i18n::t;

/// One thing the palette can do. Executed by `FoxGardenApp`, which owns the
/// state each action touches — the palette itself only chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Save,
    SaveAll,
    CloseTab,
    ReopenClosedTab,
    OpenFolder,
    NewFile,
    NewProject,
    GoToFile,
    RecentFiles,
    NextDiagnostic,
    ToggleSidePanel,
    ToggleTerminal,
    ToggleSourceControl,
    ToggleBuildPanel,
    ToggleTheme,
    ZenMode,
    Build,
    RunProject,
    RunTests,
    RunCheckstyle,
    RunPmd,
    RunSpotBugs,
    FoldAll,
    ExpandAll,
    SortLines,
    UniqueLines,
}

impl Command {
    /// Every command, in the order the palette lists them with an empty
    /// query: the file/editing ones first, then panels, then the
    /// build/analysis actions.
    pub const ALL: [Command; 26] = [
        Command::Save,
        Command::SaveAll,
        Command::CloseTab,
        Command::ReopenClosedTab,
        Command::GoToFile,
        Command::RecentFiles,
        Command::NextDiagnostic,
        Command::NewFile,
        Command::OpenFolder,
        Command::NewProject,
        Command::ToggleSidePanel,
        Command::ToggleTerminal,
        Command::ToggleSourceControl,
        Command::ToggleBuildPanel,
        Command::ToggleTheme,
        Command::ZenMode,
        Command::Build,
        Command::RunProject,
        Command::RunTests,
        Command::RunCheckstyle,
        Command::RunPmd,
        Command::RunSpotBugs,
        Command::FoldAll,
        Command::ExpandAll,
        Command::SortLines,
        Command::UniqueLines,
    ];

    /// The command's name, reusing the exact wording of the menu item it
    /// mirrors — the same action must not be called two different things
    /// depending on where it's found.
    pub fn label(self) -> &'static str {
        let strings = t();
        match self {
            Command::Save => strings.common.save,
            Command::SaveAll => strings.tabs.save_all,
            Command::CloseTab => strings.menu.close_tab,
            Command::ReopenClosedTab => strings.menu.reopen_closed_tab,
            Command::OpenFolder => strings.menu.open_folder,
            Command::NewFile => strings.menu.new_file,
            Command::NewProject => strings.menu.new_project,
            Command::GoToFile => strings.palettes.go_to_file,
            Command::RecentFiles => strings.palettes.go_to_recent_file,
            Command::NextDiagnostic => strings.palettes.next_diagnostic,
            Command::ToggleSidePanel => strings.menu.side_panel,
            Command::ToggleTerminal => strings.menu.terminal_panel,
            Command::ToggleSourceControl => strings.menu.source_control,
            Command::ToggleBuildPanel => strings.menu.build_output,
            Command::ToggleTheme => strings.menu.theme,
            Command::ZenMode => strings.menu.zen_mode,
            Command::Build => strings.menu.build,
            Command::RunProject => strings.menu.run_project,
            Command::RunTests => strings.menu.run_tests,
            Command::RunCheckstyle => strings.menu.run_checkstyle,
            Command::RunPmd => strings.menu.run_pmd,
            Command::RunSpotBugs => strings.menu.run_spotbugs,
            Command::FoldAll => strings.menu.fold_all,
            Command::ExpandAll => strings.menu.expand_all,
            Command::SortLines => strings.menu.sort_lines,
            Command::UniqueLines => strings.menu.unique_lines,
        }
    }

    /// The keyboard shortcut, shown next to the name — which is how a
    /// palette teaches shortcuts instead of just bypassing them. Empty for
    /// commands that have none.
    pub fn shortcut(self) -> &'static str {
        match self {
            Command::Save => "Ctrl+S",
            Command::SaveAll => "Ctrl+Shift+S",
            Command::ReopenClosedTab => "Ctrl+Shift+T",
            Command::GoToFile => "Ctrl+P",
            Command::RecentFiles => "Ctrl+E",
            Command::NextDiagnostic => "F8",
            Command::NewFile => "Ctrl+N",
            Command::ToggleSidePanel => "Ctrl+B",
            Command::ToggleTerminal => "Ctrl+`",
            Command::ZenMode => "F11",
            _ => "",
        }
    }
}

#[derive(Default)]
pub struct CommandPaletteState {
    open: bool,
    query: String,
    selected: usize,
}

impl CommandPaletteState {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
    }

}

/// Case-insensitive substring match on the command's own name, the same
/// rule the other palettes in this app use.
fn matches(command: Command, query: &str) -> bool {
    query.is_empty() || command.label().to_lowercase().contains(&query.to_lowercase())
}

/// Draws the palette, returning whichever command was picked this frame.
pub fn show(ui: &egui::Ui, state: &mut CommandPaletteState) -> Option<Command> {
    if !state.open {
        return None;
    }

    let candidates: Vec<Command> = Command::ALL.into_iter().filter(|c| matches(*c, &state.query)).collect();
    if !candidates.is_empty() {
        state.selected = state.selected.min(candidates.len() - 1);
    }

    let ctx = ui.ctx().clone();
    let mut chosen = None;
    let mut escaped = false;

    egui::Modal::new(egui::Id::new("command_palette")).show(&ctx, |ui| {
        ui.set_min_width(460.0);
        ui.label(t().palettes.run_a_command);

        let response = ui.text_edit_singleline(&mut state.query);
        response.request_focus();

        escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !candidates.is_empty() {
            state.selected = (state.selected + 1).min(candidates.len() - 1);
        }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            state.selected = state.selected.saturating_sub(1);
        }
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        ui.separator();
        if candidates.is_empty() {
            ui.weak(t().common.no_matches);
        }
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (index, command) in candidates.iter().enumerate() {
                let is_selected = index == state.selected;
                let response = ui
                    .horizontal(|ui| {
                        let picked = ui.selectable_label(is_selected, command.label()).clicked();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.weak(command.shortcut());
                        });
                        picked
                    })
                    .inner;
                if response || (is_selected && enter_pressed) {
                    chosen = Some(*command);
                }
            }
        });
    });

    if chosen.is_some() || escaped {
        state.open = false;
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_a_name() {
        for command in Command::ALL {
            assert!(!command.label().is_empty(), "{command:?} has no label");
        }
    }

    /// A palette that lists the same action twice is worse than the menus:
    /// the user has to guess which one they mean.
    #[test]
    fn no_two_commands_share_a_name() {
        let mut labels: Vec<&str> = Command::ALL.iter().map(|c| c.label()).collect();
        labels.sort_unstable();
        let before = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), before, "two commands share a label");
    }

    #[test]
    fn filtering_is_case_insensitive_and_matches_anywhere_in_the_name() {
        assert!(matches(Command::SaveAll, ""));
        assert!(matches(Command::SaveAll, &Command::SaveAll.label().to_uppercase()));
        assert!(!matches(Command::SaveAll, "definitely not a command"));
    }

    #[test]
    fn toggle_opens_and_resets_the_query() {
        let mut state = CommandPaletteState::default();
        state.query.push_str("stale");
        state.selected = 7;

        state.toggle();

        assert!(state.open);
        assert!(state.query.is_empty());
        assert_eq!(state.selected, 0);
    }
}
