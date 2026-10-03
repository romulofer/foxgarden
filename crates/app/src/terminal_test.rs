
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
