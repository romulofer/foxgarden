//! A native "choose a folder" dialog that can't freeze the editor.
//!
//! `rfd::FileDialog::pick_folder()` blocks the calling thread until the
//! user answers it — and on Linux it goes through the XDG desktop portal,
//! which can take seconds to appear, appear on a different display than the
//! app, or (verified live: a portal that never resolves) never come back at
//! all. Called straight from `ui()`, that is the UI thread, so the whole
//! editor stops painting and stops responding to input: not a stall the
//! user can cancel, since the thing to cancel is the dialog that never
//! showed up.
//!
//! So the dialog runs on its own thread and its answer is polled once a
//! frame, the same background-work shape everything else in this app uses.
//! The app stays interactive the entire time — the worst case is a picker
//! that never answers, which now costs one parked thread instead of the
//! editor.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

#[derive(Default)]
pub struct FolderPicker {
    rx: Option<Receiver<Option<PathBuf>>>,
}

impl FolderPicker {
    /// Opens the dialog, unless one is already open — a second click while
    /// the first dialog is still up would spawn a second, competing one.
    /// `start_dir` is where it opens (the project root, typically).
    pub fn open(&mut self, start_dir: Option<PathBuf>) {
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new();
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let _ = tx.send(dialog.pick_folder());
        });
        self.rx = Some(rx);
    }

    /// Whether a dialog is currently open — for disabling the button that
    /// opens it, and for showing that something is in progress.
    pub fn is_open(&self) -> bool {
        self.rx.is_some()
    }

    /// The folder the user picked, once they have. `None` covers both
    /// "still waiting" and "cancelled" — neither is something a caller acts
    /// on.
    pub fn poll(&mut self) -> Option<PathBuf> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(picked) => {
                self.rx = None;
                picked
            }
            Err(TryRecvError::Empty) => None,
            // The dialog thread died without answering; nothing left to
            // wait for.
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "folder_picker_test.rs"]
mod folder_picker_test;
