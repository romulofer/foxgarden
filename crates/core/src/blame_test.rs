use super::*;

/// Captured verbatim from a real `git blame --porcelain` run against a
/// throwaway repo: line 1 committed, line 2 an uncommitted local edit
/// (git's own all-zero "Not Committed Yet" sha), line 3 reusing line 1's
/// already-seen commit (metadata omitted the second time).
const MIXED: &str = "\
166946239809d8710781eab8130beb2e91fc5768 1 1 1
author test
author-mail <a@b.com>
author-time 1785368866
author-tz -0300
committer test
committer-mail <a@b.com>
committer-time 1785368866
committer-tz -0300
summary init
boundary
filename f.txt
\tl1
0000000000000000000000000000000000000000 2 2 1
author Not Committed Yet
author-mail <not.committed.yet>
author-time 1785368900
author-tz -0300
committer Not Committed Yet
committer-mail <not.committed.yet>
committer-time 1785368900
committer-tz -0300
summary Version of f.txt from f.txt
previous 166946239809d8710781eab8130beb2e91fc5768 f.txt
filename f.txt
\tCHANGED
166946239809d8710781eab8130beb2e91fc5768 3 3 1
\tl3
";

#[test]
fn parses_a_first_seen_commit_with_full_metadata() {
    let lines = parse_porcelain_blame(MIXED);
    assert_eq!(
        lines[0],
        BlameLine {
            sha: "166946239809d8710781eab8130beb2e91fc5768".to_string(),
            author: "test".to_string(),
            author_time: 1785368866,
            summary: "init".to_string(),
        }
    );
}

#[test]
fn parses_an_uncommitted_local_edit_as_not_committed_yet() {
    let lines = parse_porcelain_blame(MIXED);
    assert_eq!(lines[1].sha, "0000000000000000000000000000000000000000");
    assert_eq!(lines[1].author, "Not Committed Yet");
    assert_eq!(lines[1].author_time, 1785368900);
}

#[test]
fn fills_in_a_repeated_commits_metadata_from_the_cache() {
    // Line 3 reuses the same commit as line 1, but the real porcelain
    // output only carries a bare header + content for it — no metadata
    // lines at all — so this only passes if the cache actually fills
    // the gap rather than leaving line 3's fields empty/default.
    let lines = parse_porcelain_blame(MIXED);
    assert_eq!(lines[2].sha, lines[0].sha);
    assert_eq!(lines[2].author, "test");
    assert_eq!(lines[2].author_time, 1785368866);
    assert_eq!(lines[2].summary, "init");
}

#[test]
fn empty_output_produces_no_lines() {
    assert_eq!(parse_porcelain_blame(""), vec![]);
}

/// End-to-end against a real `git` binary and a real temp repository,
/// mirroring `diff`'s own `git_diff_hunks_runs_a_real_git_diff_against_
/// a_real_repo` test — catches a wrong flag/argument order that a
/// parser-only test against a captured fixture never could.
#[test]
fn git_blame_runs_a_real_git_blame_against_a_real_repo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("f.txt");
    std::fs::write(&file, "l1\nl2\nl3\n").unwrap();

    let run = |args: &[&str]| Command::new("git").current_dir(root).args(args).output().unwrap();
    run(&["init", "-q"]);
    run(&["config", "user.email", "a@b.com"]);
    run(&["config", "user.name", "test"]);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);

    let lines = git_blame(&file, root).expect("git ran");
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| l.author == "test" && l.summary == "init"));
}

#[test]
fn git_blame_on_a_path_outside_any_repo_is_empty_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("f.txt");
    std::fs::write(&file, "hello\n").unwrap();

    let lines = git_blame(&file, root).expect("git still launches fine");
    assert_eq!(lines, vec![]);
}
