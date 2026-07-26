//! Launching a system terminal at a given directory. Platform-specific by
//! necessity — there's no cross-platform "open a terminal here" API — so
//! this is the one place in the app that branches on `target_os` at all.

use std::path::Path;
use std::process::Command;

/// Terminal binaries to try, in order, on any Unix that isn't macOS (which
/// has its own `Terminal.app`, reached through `open` instead) — there's no
/// single standard terminal emulator across Linux desktop environments, so
/// this tries the most common ones and takes whichever is actually
/// installed. `x-terminal-emulator` first: Debian/Ubuntu (and derivatives)
/// maintain it as an `update-alternatives` symlink to the user's configured
/// default, so it's the best single guess when present. `xterm` last:
/// virtually always available wherever X11 is (a transitive dependency of
/// a lot of tooling), so it's the most reliable final fallback, not the
/// best UX.
#[cfg(all(unix, not(target_os = "macos")))]
const LINUX_TERMINAL_CANDIDATES: &[&str] = &[
    "x-terminal-emulator",
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "alacritty",
    "kitty",
    "terminator",
    "tilix",
    "xterm",
];

/// The command(s) to try, in order, to open a terminal at `dir`. Building
/// this as data instead of spawning directly keeps "which candidates, with
/// which arguments" testable (via `Command::get_program`/`get_args`)
/// without actually launching a window.
fn candidates(dir: &Path) -> Vec<Command> {
    #[cfg(target_os = "windows")]
    {
        // `start` is a `cmd.exe` builtin, not its own executable, so it has
        // to run through `cmd /C`. The empty `""` right after `start` is
        // the window-title argument `start` expects first — without it,
        // `start` would treat `/D` (or a `dir` containing spaces) as the
        // title instead of a flag, per `start`'s own argument rules.
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", "/D"]).arg(dir).arg("cmd");
        vec![command]
    }
    #[cfg(target_os = "macos")]
    {
        // `open -a Terminal <dir>` launches (or focuses) Terminal.app with
        // a new window's working directory set to `dir` — the standard,
        // documented way to do this on macOS, no AppleScript needed.
        let mut command = Command::new("open");
        command.args(["-a", "Terminal"]).arg(dir);
        vec![command]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        LINUX_TERMINAL_CANDIDATES
            .iter()
            .map(|terminal| {
                let mut command = Command::new(terminal);
                // Every terminal emulator here starts its own default
                // shell inheriting *this* process's working directory
                // (none of them reset it themselves), so setting it on the
                // terminal binary itself is enough — no per-terminal
                // "--working-directory"-shaped flag needed, which is good,
                // because those flags aren't even spelled consistently
                // across this list.
                command.current_dir(dir);
                command
            })
            .collect()
    }
}

/// Opens a terminal window with its working directory set to `dir`.
/// Fire-and-forget: doesn't wait for the spawned process (a terminal
/// window is meant to outlive this call) and can't confirm a window
/// actually appeared — there's no cross-platform way to check that, only
/// that *something* was successfully spawned. Returns an error only once
/// every candidate has failed to even spawn (wrong binary name, not on
/// `PATH`, ...) — on Windows/macOS that's always exactly one candidate, on
/// other Unix it means none of `LINUX_TERMINAL_CANDIDATES` is installed.
pub fn open(dir: &Path) -> std::io::Result<()> {
    let mut last_err = None;
    for mut command in candidates(dir) {
        match command.spawn() {
            Ok(_child) => return Ok(()),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.expect("candidates() always returns at least one Command"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each test below only compiles on the platform its `candidates()`
    // branch targets — there's no way to exercise Windows' or macOS'
    // process-construction logic from a Linux test run (or vice versa)
    // without actually running on that OS. None of these spawn a real
    // process (that would pop up an actual terminal window mid test run,
    // and be flaky wherever the relevant binary isn't installed) — they
    // only inspect the `Command` `candidates()` built, via its own public
    // `get_program`/`get_args`/`get_current_dir` accessors.

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_candidate_starts_cmd_at_the_given_directory() {
        let dir = Path::new(r"C:\Some Project");
        let commands = candidates(dir);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].get_program(), "cmd");
        let args: Vec<_> = commands[0].get_args().collect();
        assert_eq!(args, ["/C", "start", "", "/D", "C:\\Some Project", "cmd"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_candidate_opens_terminal_app_at_the_given_directory() {
        let dir = Path::new("/Users/x/Some Project");
        let commands = candidates(dir);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].get_program(), "open");
        let args: Vec<_> = commands[0].get_args().collect();
        assert_eq!(args, ["-a", "Terminal", "/Users/x/Some Project"]);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_candidates_cover_every_terminal_in_priority_order_with_cwd_set() {
        let dir = Path::new("/home/x/Some Project");
        let commands = candidates(dir);
        let programs: Vec<_> = commands.iter().map(|c| c.get_program()).collect();
        assert_eq!(programs, LINUX_TERMINAL_CANDIDATES);
        for command in &commands {
            assert_eq!(command.get_current_dir(), Some(dir));
            assert!(
                command.get_args().next().is_none(),
                "cwd, not a flag, is what carries the directory"
            );
        }
    }
}
