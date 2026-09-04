//! One terminal panel session's real pty child process (`PLAN.md`'s
//! terminal-panel track, Phases 6-7 and 9) — kept index-aligned with `state.
//! terminal_tabs` on `FoxGardenApp`, the same "index-aligned side vec,
//! not on `EditorState`" shape `parsers` already uses for `open_tabs`.
//! Lives in `crates/app`, not `fg-core`: `crates/core` is unit-testable
//! headless with no process/thread dependency at all (`AGENTS.md`), which
//! a live child process and its background reader thread can't be. The
//! `vt100::Parser` this session feeds (`PLAN.md` Phase 7) is pure data with
//! no I/O of its own, but lives here rather than on `fg-core`'s
//! `TerminalTab` anyway — every other piece of this session's runtime state
//! already sits in this app-side, index-aligned-with-`terminal_tabs` type
//! (the same "index-aligned side vec, not on `EditorState`" shape `parsers`
//! already uses for `open_tabs`), and splitting the parser onto `TerminalTab`
//! while the child/writer/thread stay here would mean two different places
//! own one session's state for no real benefit.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

/// The pty's (and `vt100::Parser`'s) starting size, matching a common
/// terminal-emulator default — immediately superseded by `terminal_widget::
/// show`'s own first-frame `resize` call to whatever the panel's actual
/// rect computes to (`PLAN.md` Phase 9), so this only matters for the
/// otherwise-blank instant between `spawn` returning and that first frame.
const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;

/// How many scrolled-off lines `vt100::Parser` keeps beyond the visible
/// screen. This phase's widget only ever reads the *visible* `Screen`
/// (`SPEC.md` §8.4 — no scrollback UI yet), but the parser needs a nonzero
/// value up front since it can't be changed except via `set_scrollback`.
const SCROLLBACK_LINES: usize = 1000;

/// The command used to spawn the user's default shell (`SPEC.md` §8.3):
/// `$SHELL` on Unix (falling back to `/bin/sh` if unset), `%COMSPEC%` on
/// Windows (falling back to `cmd.exe`). `crates/app/src/terminal.rs`'s own
/// external-terminal spawn never picks a shell itself — every candidate
/// there is a terminal *emulator* that starts its own default shell — so
/// there's no existing fallback to match; this is the first place in the
/// app that has to choose a shell directly.
fn shell_command() -> CommandBuilder {
    #[cfg(windows)]
    let mut command = {
        let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        CommandBuilder::new(shell)
    };
    #[cfg(not(windows))]
    let mut command = {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        CommandBuilder::new(shell)
    };
    // The shell inherits this process's own environment, and a GUI app
    // started from a desktop launcher (rather than from a terminal) has no
    // `TERM` at all — which every curses/termcap consumer treats as "this
    // isn't a terminal": `tput: No value for $TERM and no -T specified`,
    // prompts printing their own escape codes as literal text, `vim`/`htop`
    // refusing to start. Observed directly in this panel with a real zsh.
    //
    // `xterm-256color` because that is what this panel actually emulates:
    // the output is parsed by `vt100`, whose supported sequence set is the
    // xterm one, and its cell attributes carry 256-colour indices.
    // `COLORTERM` alongside it is the de-facto way programs detect 24-bit
    // colour support, which `vt100` also parses.
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command
}

/// A running shell session: the child process, its pty writer half, and
/// the background reader thread's output funneled through a channel.
pub struct PtySession {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Kept only for `resize` (`PLAN.md` Phase 9) — the reader/writer
    /// halves below are independent handles cloned/taken from it up front,
    /// so this field's sole remaining job is `MasterPty::resize`'s own
    /// `&self` call telling the kernel (and thus the child) the window
    /// changed size.
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    output_rx: mpsc::Receiver<Vec<u8>>,
    /// Parses the raw byte stream into a fixed-size cell grid (`PLAN.md`
    /// Phase 7) — replaces this phase's own previous plain `String` buffer,
    /// which had no notion of cursor position, color, or any other cell
    /// attribute a real terminal needs to render recognizably.
    parser: vt100::Parser,
}

impl PtySession {
    /// Spawns the default shell in a real pty, `cwd` if given (the open
    /// project's root, when there is one). The background reader thread
    /// calls `request_repaint` on `ctx` whenever new output arrives, since
    /// unlike the user's own typing, a long-running command's output has no
    /// other event to trigger a repaint on (`SPEC.md` §8.3).
    pub fn spawn(cwd: Option<&Path>, ctx: egui::Context) -> std::io::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: DEFAULT_ROWS,
                cols: DEFAULT_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(std::io::Error::other)?;

        let mut cmd = shell_command();
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }
        let child = pair.slave.spawn_command(cmd).map_err(std::io::Error::other)?;
        // The slave end is only needed to spawn the child.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(std::io::Error::other)?;
        let writer = pair.master.take_writer().map_err(std::io::Error::other)?;

        let (tx, output_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break, // shell exited, pty closed
                    Ok(count) => {
                        if tx.send(chunk[..count].to_vec()).is_err() {
                            break; // PtySession (and its Receiver) dropped
                        }
                        ctx.request_repaint();
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            child,
            master: pair.master,
            writer,
            output_rx,
            parser: vt100::Parser::new(DEFAULT_ROWS, DEFAULT_COLS, SCROLLBACK_LINES),
        })
    }

    /// Tells the kernel (and thus the shell, via `SIGWINCH`) and the
    /// `vt100::Parser` that the session is now `rows`x`cols` (`PLAN.md`
    /// Phase 9, `SPEC.md` §8.6) — called whenever `terminal_widget::show`'s
    /// own per-frame size computation disagrees with the parser's current
    /// size. A no-op if they already match (the caller checks first, but
    /// this only ever runs on an actual change regardless). The pty
    /// `resize` and the parser `set_size` can't drift relative to each
    /// other — the shell needs to see the same size actually being
    /// rendered — so both always happen together, never one without the
    /// other.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser.set_size(rows, cols);
    }

    /// Drains every chunk the background reader thread has sent since the
    /// last call, feeding each straight into the `vt100::Parser`. Non-
    /// blocking: called once per frame regardless of whether new output has
    /// arrived.
    pub fn drain(&mut self) {
        while let Ok(chunk) = self.output_rx.try_recv() {
            self.parser.process(&chunk);
        }
    }

    /// The parsed screen contents `terminal_widget::show` paints this frame
    /// — always the *current* state, not a snapshot from whenever `drain`
    /// was last called, so a caller that (unusually) skips a `drain` still
    /// sees whatever the parser already had.
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Writes bytes straight to the shell — the caller (`panels::
    /// terminal_panel`) has already translated whatever typed/pasted/
    /// special-key input this came from into the real terminal byte
    /// sequence (`PLAN.md` Phase 8, `widgets::terminal_input`); this is
    /// just the raw pipe.
    pub fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(bytes)
    }
}

impl Drop for PtySession {
    /// Closing a session kills its real child process — no "still running,
    /// are you sure" confirmation (`SPEC.md` §8.3: a terminal has no
    /// "unsaved" concept the way a dirty file tab does).
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test, not two: `cargo test` runs tests in parallel threads by
    // default, and both cases mutate the same process-global `SHELL` env
    // var — as two separate `#[test]`s this was a genuine, observed race
    // (whichever ran last "won" and could flip the other's assertion),
    // despite an earlier claim here that no other test in this crate reads/
    // writes `SHELL`. Sequential within one test is the actual fix, not a
    // workaround.
    /// Regression for a shell started from a desktop launcher, which
    /// inherits no `TERM` and then prints its own prompt escapes as
    /// literal text (observed with a real zsh in this panel).
    #[test]
    fn shell_command_declares_the_terminal_it_actually_emulates() {
        let command = shell_command();
        assert_eq!(command.get_env("TERM"), Some(std::ffi::OsStr::new("xterm-256color")));
        assert_eq!(command.get_env("COLORTERM"), Some(std::ffi::OsStr::new("truecolor")));
    }

    #[test]
    #[cfg(not(windows))]
    fn shell_command_reflects_the_shell_env_var_with_a_bin_sh_fallback() {
        // SAFETY: test-only env mutation; sequential within this one test,
        // which is what makes it safe now.
        unsafe {
            std::env::remove_var("SHELL");
        }
        assert_eq!(shell_command().get_argv()[0], std::ffi::OsString::from("/bin/sh"));

        unsafe {
            std::env::set_var("SHELL", "/bin/definitely-not-a-real-shell");
        }
        assert_eq!(
            shell_command().get_argv()[0],
            std::ffi::OsString::from("/bin/definitely-not-a-real-shell")
        );
        unsafe {
            std::env::remove_var("SHELL");
        }
    }
}
