//! Sampling a running JVM with async-profiler (`PLAN.md` Track 26 Phase 1 —
//! "Profiler integration") and parsing its collapsed-stack output into a
//! flame-graph tree.
//!
//! Deliberately shells out to async-profiler's own `asprof` launcher against
//! a target PID rather than embedding a native agent: `asprof -e <event> -d
//! <secs> -o collapsed -f <file> <pid>` is the project's own documented
//! interactive-attach invocation (verified against async-profiler 4.5's
//! `docs/ProfilerOptions.md`, not guessed), and `-d` is itself a shortcut for
//! start/sleep/stop, so one `asprof` process both attaches and detaches on
//! its own — no separate stop command for this app to sequence. The `-o
//! collapsed` format is the folded-stack format FlameGraph established
//! (`frame1;frame2;…;frameN <sample-count>`, one stack per line), which
//! `parse_collapsed` folds into the `FlameNode` tree Track 26 Phase 2's own
//! flame-graph widget paints.
//!
//! async-profiler has no Windows build at all (its release page ships only
//! linux-x64/arm64 tarballs and a macOS zip), so the installer side of this
//! track — not this module — is what refuses that platform; the command
//! assembly here is platform-neutral because a PID and an output path are the
//! same shape everywhere `asprof` runs.

use std::path::Path;
use std::process::Command;

/// Which async-profiler event to sample. `-e cpu` (the default async-profiler
/// itself documents) is the common case; the others are the ones with a
/// single well-known event name that needs no extra configuration, so a
/// caller can offer a small fixed menu without this module having to model
/// async-profiler's full event grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileEvent {
    /// CPU time, sampled via perf events / the JVM's own signal timer.
    Cpu,
    /// Heap allocation profiling (`-e alloc`).
    Alloc,
    /// Lock contention (`-e lock`).
    Lock,
    /// Wall-clock time — every thread, whether on-CPU or blocked (`-e wall`).
    Wall,
}

impl ProfileEvent {
    /// The literal async-profiler event name passed after `-e`.
    pub fn event_name(self) -> &'static str {
        match self {
            ProfileEvent::Cpu => "cpu",
            ProfileEvent::Alloc => "alloc",
            ProfileEvent::Lock => "lock",
            ProfileEvent::Wall => "wall",
        }
    }
}

/// Assembles (but does not spawn) the `asprof` invocation that attaches to
/// `pid`, samples `event` for `duration_secs`, and writes collapsed-stack
/// output to `output_path`.
///
/// `-d` makes one `asprof` process do start/sleep/stop by itself, so the
/// spawned child exits on its own once the duration elapses — the caller
/// waits for it rather than issuing a separate stop, matching how every
/// other build/run process in this app (`build_command`, `run_command`) is a
/// single `Command` the caller drives to completion.
pub fn profiler_command(
    asprof_bin: &Path,
    pid: u32,
    event: ProfileEvent,
    duration_secs: u32,
    output_path: &Path,
) -> Command {
    let mut command = Command::new(asprof_bin);
    command
        .arg("-e")
        .arg(event.event_name())
        .arg("-d")
        .arg(duration_secs.to_string())
        .arg("-o")
        .arg("collapsed")
        .arg("-f")
        .arg(output_path)
        .arg(pid.to_string());
    command
}

/// One node in the flame-graph tree: a call frame, the total number of
/// samples in which it (and everything it called) appeared, and its callees.
///
/// `total` is the *inclusive* sample count — the frame's own samples plus
/// every descendant's — because that's what a flame-graph rectangle's width
/// represents. A frame's own exclusive (self) time is `total` minus the sum
/// of its children's `total`s, which the widget can derive when it needs it
/// rather than this tree storing it redundantly and risking the two drifting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlameNode {
    pub name: String,
    pub total: u64,
    pub children: Vec<FlameNode>,
}

impl FlameNode {
    fn new(name: impl Into<String>) -> Self {
        FlameNode { name: name.into(), total: 0, children: Vec::new() }
    }

    /// Returns the child named `name`, creating it (appended, so first-seen
    /// order is preserved for a stable, reproducible tree) if absent.
    fn child_mut(&mut self, name: &str) -> &mut FlameNode {
        // Linear search rather than a map: a single frame's fan-out is small
        // (a handful of distinct callees), and preserving first-seen order
        // keeps `parse_collapsed`'s output deterministic for tests and for a
        // stable left-to-right paint, which a `HashMap` wouldn't.
        if let Some(index) = self.children.iter().position(|child| child.name == name) {
            &mut self.children[index]
        } else {
            self.children.push(FlameNode::new(name));
            self.children.last_mut().expect("just pushed")
        }
    }
}

/// Folds async-profiler's collapsed-stack output into a `FlameNode` tree
/// rooted at a synthetic `"all"` frame whose `total` is the whole profile's
/// sample count.
///
/// Each input line is `frame1;frame2;…;frameN <count>` — semicolon-separated
/// frames (root-most first), then a single space, then the sample count. The
/// count is split off the *end* (`rsplit_once(' ')`) rather than the start,
/// because a frame label can itself contain spaces (e.g. async-profiler's own
/// `[unknown]` / `<init>`-style synthetic frames, or inlined lambda names)
/// while the trailing count never does. Malformed lines — blank, no space, a
/// non-numeric count, or an empty stack — are skipped rather than aborting the
/// parse, so one stray line from a partial capture doesn't discard an
/// otherwise good profile.
pub fn parse_collapsed(text: &str) -> FlameNode {
    let mut root = FlameNode::new("all");

    for line in text.lines() {
        let line = line.trim_end();
        let Some((stack, count)) = line.rsplit_once(' ') else {
            continue;
        };
        let Ok(count) = count.trim().parse::<u64>() else {
            continue;
        };
        let frames: Vec<&str> = stack.split(';').filter(|frame| !frame.is_empty()).collect();
        if frames.is_empty() {
            continue;
        }

        root.total += count;
        let mut node = &mut root;
        for frame in frames {
            node = node.child_mut(frame);
            node.total += count;
        }
    }

    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn profiler_command_matches_async_profilers_documented_attach_invocation() {
        let command = profiler_command(
            &PathBuf::from("/opt/async-profiler/bin/asprof"),
            12345,
            ProfileEvent::Cpu,
            30,
            &PathBuf::from("/tmp/profile.collapsed"),
        );

        assert_eq!(command.get_program(), "/opt/async-profiler/bin/asprof");
        let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(
            args,
            vec!["-e", "cpu", "-d", "30", "-o", "collapsed", "-f", "/tmp/profile.collapsed", "12345"]
        );
    }

    #[test]
    fn profiler_command_carries_the_selected_event_name() {
        let command =
            profiler_command(&PathBuf::from("asprof"), 7, ProfileEvent::Alloc, 5, &PathBuf::from("out"));
        let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[0..2], &["-e", "alloc"]);
    }

    #[test]
    fn parse_collapsed_builds_an_inclusive_sample_tree() {
        // Two stacks sharing a common a;b prefix, plus a standalone one.
        let text = "a;b;c 5\na;b;d 3\ne 2\n";
        let root = parse_collapsed(text);

        assert_eq!(root.name, "all");
        assert_eq!(root.total, 10);

        let a = root.children.iter().find(|c| c.name == "a").expect("a present");
        assert_eq!(a.total, 8, "a is inclusive of both a;b;c and a;b;d");
        let b = a.children.iter().find(|c| c.name == "b").expect("b present");
        assert_eq!(b.total, 8);
        assert_eq!(b.children.iter().find(|c| c.name == "c").unwrap().total, 5);
        assert_eq!(b.children.iter().find(|c| c.name == "d").unwrap().total, 3);

        assert_eq!(root.children.iter().find(|c| c.name == "e").unwrap().total, 2);
    }

    #[test]
    fn parse_collapsed_preserves_first_seen_child_order() {
        // Deterministic left-to-right order matters for a stable paint.
        let root = parse_collapsed("z 1\nm 1\na 1\n");
        let names: Vec<&str> = root.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["z", "m", "a"]);
    }

    #[test]
    fn parse_collapsed_accumulates_repeated_identical_stacks() {
        let root = parse_collapsed("a;b 2\na;b 3\n");
        let a = &root.children[0];
        assert_eq!(a.total, 5);
        assert_eq!(a.children[0].total, 5);
        assert_eq!(a.children.len(), 1, "the second a;b folds into the same node");
    }

    #[test]
    fn parse_collapsed_keeps_frame_labels_that_contain_spaces() {
        // Only the trailing count is split off; a spaced label survives whole.
        let root = parse_collapsed("java/lang/Thread.run [unknown Java] 4\n");
        let outer = &root.children[0];
        assert_eq!(outer.name, "java/lang/Thread.run [unknown Java]");
        assert_eq!(outer.total, 4);
    }

    #[test]
    fn parse_collapsed_skips_malformed_lines() {
        let root = parse_collapsed("\nno_count_here\na;b notanumber\n; 5\nvalid 7\n");
        // Only `valid 7` is well-formed; the empty-stack `; 5` is dropped too.
        assert_eq!(root.total, 7);
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].name, "valid");
    }
}
