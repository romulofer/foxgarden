use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Cap on how many past versions of a single file `write_snapshot` keeps —
/// unlike `run_config.rs`'s own `.foxgarden/` file, which is small and
/// meant to be hand-edited, a snapshot holds a whole copy of the file's
/// content on every save, so unbounded growth would slowly fill a
/// project's `.foxgarden/history/` with every keystroke-adjacent save ever
/// made.
const SNAPSHOT_CAP: usize = 50;

/// Where `write_snapshot` stores `file_path`'s own past versions — `.
/// foxgarden/history/<relative path>/`, mirroring `run_config.rs`'s own
/// `.foxgarden/run_configs.json` convention (per-project, not in this
/// app's `eframe::Storage`). `None` if `file_path` isn't under
/// `project_root` at all (a file opened outside the current project, e.g.
/// via go-to-definition into a JDK/library source) — such a file has no
/// sensible project-relative location to snapshot into, so `write_snapshot`
/// simply skips it rather than guessing.
fn snapshot_dir(project_root: &Path, file_path: &Path) -> Option<PathBuf> {
    let relative = file_path.strip_prefix(project_root).ok()?;
    Some(project_root.join(".foxgarden").join("history").join(relative))
}

/// Writes `content` (whatever `Document::save` just wrote to `file_path`
/// itself) as a new timestamped snapshot, then prunes the oldest snapshots
/// beyond `SNAPSHOT_CAP` for that same file. A no-op, not an error, when
/// `file_path` isn't under `project_root` (see `snapshot_dir`) — the same
/// "nothing useful to do" reasoning `load_run_configs` already applies to
/// a missing file, just on the write side instead of the read side.
pub fn write_snapshot(project_root: &Path, file_path: &Path, content: &str) -> std::io::Result<()> {
    let Some(dir) = snapshot_dir(project_root, file_path) else {
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    // Nanoseconds, not milliseconds: a millisecond is coarse enough that two
    // saves in quick succession land in the same millisecond fairly often.
    // Even nanosecond resolution isn't guaranteed by every platform's clock,
    // though, so a bare `timestamp.snapshot` name could still collide with
    // an existing one — the `-1`/`-2`/... suffix loop below guards against
    // that silently overwriting (and so losing) a real prior snapshot.
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut path = dir.join(format!("{timestamp}.snapshot"));
    let mut suffix = 1;
    while path.exists() {
        path = dir.join(format!("{timestamp}-{suffix}.snapshot"));
        suffix += 1;
    }
    std::fs::write(path, content)?;
    prune_snapshots(&dir)
}

/// A filename is `<timestamp>.snapshot` or, on a same-timestamp collision
/// (`write_snapshot`'s own suffix loop), `<timestamp>-<suffix>.snapshot`.
/// Shared by `prune_snapshots` and `list_snapshots` — both need the same
/// `(timestamp, suffix)` ordering key, the former to find the oldest
/// entries, the latter to show the newest first.
fn parse_snapshot_filename(path: &Path) -> Option<(u128, u32)> {
    let stem = path.file_stem()?.to_str()?;
    match stem.split_once('-') {
        Some((timestamp, suffix)) => Some((timestamp.parse().ok()?, suffix.parse().ok()?)),
        None => Some((stem.parse().ok()?, 0)),
    }
}

/// Removes every snapshot in `dir` beyond `SNAPSHOT_CAP`, oldest first.
/// Sorts by the numeric nanosecond timestamp each filename encodes
/// (`write_snapshot`'s own naming), not lexically — a plain string sort
/// happens to agree with timestamp order only as long as every filename is
/// the same digit length, which won't stay true forever; sorting the
/// parsed number instead means this doesn't depend on that.
fn prune_snapshots(dir: &Path) -> std::io::Result<()> {
    let mut snapshots: Vec<(u128, u32, PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter_map(|path| {
            let (timestamp, suffix) = parse_snapshot_filename(&path)?;
            Some((timestamp, suffix, path))
        })
        .collect();
    snapshots.sort_by_key(|(timestamp, suffix, _)| (*timestamp, *suffix));

    if snapshots.len() > SNAPSHOT_CAP {
        let excess = snapshots.len() - SNAPSHOT_CAP;
        for (_, _, path) in &snapshots[..excess] {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// One saved version of a file, as `write_snapshot` left it — just the
/// path and the timestamp parsed back out of its own name; reading a
/// snapshot's actual *content* is left to the caller (`list_snapshots`
/// itself never opens a single one, so listing a file with a full 50-entry
/// history costs 50 `readdir` entries, not 50 file reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub path: PathBuf,
    /// Nanoseconds since the Unix epoch — `write_snapshot`'s own clock
    /// reading at the moment this snapshot was written, not a file's mtime
    /// (which a filesystem operation unrelated to this app, like a backup
    /// tool touching the file, could otherwise disturb).
    pub timestamp_nanos: u128,
}

/// Every snapshot saved for `file_path` under `project_root`, most recent
/// first — the order a browsable history list wants, opposite of `prune_
/// snapshots`'s own oldest-first. Empty, not an error, both when `file_
/// path` has never been snapshotted and when it falls outside `project_
/// root` entirely (see `snapshot_dir`) — either way there's nothing to
/// show, which isn't a failure a caller needs to handle differently from
/// "no history yet".
pub fn list_snapshots(project_root: &Path, file_path: &Path) -> Vec<Snapshot> {
    let Some(dir) = snapshot_dir(project_root, file_path) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut snapshots: Vec<Snapshot> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter_map(|path| {
            let (timestamp_nanos, suffix) = parse_snapshot_filename(&path)?;
            Some((timestamp_nanos, suffix, path))
        })
        .map(|(timestamp_nanos, _, path)| Snapshot { path, timestamp_nanos })
        .collect();
    snapshots.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.timestamp_nanos));
    snapshots
}

#[cfg(test)]
#[path = "file_history_test.rs"]
mod file_history_test;
