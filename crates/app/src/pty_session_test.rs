
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
