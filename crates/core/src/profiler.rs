//! Parsing a sampling profiler's collapsed-stack output into a flame-graph
//! tree (`PLAN.md` Track 26).
//!
//! Which profiler produced the file, and how it was started, is an
//! extension's (`fg_extension::ProfilerContribution`); the format is the
//! folded-stack one FlameGraph established (`frame1;frame2;…;frameN
//! <sample-count>`, one stack per line), which `parse_collapsed` folds into the
//! `FlameNode` tree the flame-graph widget paints.

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
        FlameNode {
            name: name.into(),
            total: 0,
            children: Vec::new(),
        }
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

/// Folds a profiler's collapsed-stack output into a `FlameNode` tree
/// rooted at a synthetic `"all"` frame whose `total` is the whole profile's
/// sample count.
///
/// Each input line is `frame1;frame2;…;frameN <count>` — semicolon-separated
/// frames (root-most first), then a single space, then the sample count. The
/// count is split off the *end* (`rsplit_once(' ')`) rather than the start,
/// because a frame label can itself contain spaces (e.g. a JVM profiler's
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
#[path = "profiler_test.rs"]
mod profiler_test;
