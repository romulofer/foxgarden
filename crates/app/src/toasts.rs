//! Non-blocking failure notices, stacked in the bottom-right corner.
//!
//! Every failure used to be a modal: a language server that died while the
//! user was mid-word, a background install that couldn't reach GitHub, a
//! `git` refresh that failed — each one stole focus and had to be
//! dismissed before typing could continue. None of those are decisions;
//! they're news. A modal is for a question ("this file changed on disk —
//! reload or keep yours?"), and that's what the remaining modals are for.
//!
//! A toast stays until it's read (no auto-dismiss on a fixed timer would be
//! long enough for a message the user glanced away from), can be dismissed
//! individually, and stacks oldest-first so a burst of failures doesn't
//! hide the first — usually most explanatory — one.

use fg_i18n::t;

use crate::style::icons;

/// How many toasts are shown at once. Past a handful the stack covers the
/// editor, which is worse than not showing the overflow: the count of
/// what's hidden is shown instead.
const MAX_VISIBLE: usize = 4;

#[derive(Default)]
pub struct Toasts {
    messages: Vec<String>,
}

impl Toasts {
    /// Adds `message`, unless the same text is already showing — a failure
    /// re-reported every frame (a server that stays down) should read as
    /// one problem, not hundreds.
    pub fn push(&mut self, message: String) {
        if self.messages.contains(&message) {
            return;
        }
        self.messages.push(message);
    }

    /// Draws the stack over the bottom-right of `ui`'s own area. Called
    /// once per frame, after the panels, so toasts sit above everything.
    pub fn show(&mut self, ui: &egui::Ui) {
        if self.messages.is_empty() {
            return;
        }
        let mut dismissed = None;
        let hidden = self.messages.len().saturating_sub(MAX_VISIBLE);

        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -32.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.vertical(|ui| {
                    if hidden > 0 {
                        ui.weak(format!("+{hidden}"));
                    }
                    for (index, message) in self.messages.iter().enumerate().take(MAX_VISIBLE) {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.set_max_width(420.0);
                            ui.horizontal(|ui| {
                                ui.colored_label(ui.visuals().error_fg_color, icons::ERROR.to_string());
                                ui.label(message);
                                if ui.small_button(icons::CLOSE.to_string()).clicked() {
                                    dismissed = Some(index);
                                }
                            });
                        });
                    }
                    if self.messages.len() > 1 && ui.small_button(t().common.dismiss_all).clicked() {
                        dismissed = Some(usize::MAX);
                    }
                });
            });

        match dismissed {
            Some(usize::MAX) => self.messages.clear(),
            Some(index) => {
                self.messages.remove(index);
            }
            None => {}
        }
    }
}

#[cfg(test)]
#[path = "toasts_test.rs"]
mod toasts_test;
