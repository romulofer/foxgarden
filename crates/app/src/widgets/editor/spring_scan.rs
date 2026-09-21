//! Whole-project scan for the Spring endpoint map popup (`SPEC.md` §4) —
//! walks every `.java`/`.kt` file in the project tree, parses each
//! throwaway (same "read + throwaway-parse" sequence Override Method/
//! dot-completion's cross-project lookup already do for a single
//! resolved-by-name file, just looped over every file here instead), and
//! collects whatever `syntax::endpoints_in_file` finds. Not `codegen.rs`:
//! that file is its own cohesive "code generation" concern (getter/setter/
//! constructor/`toString`/`equals`+`hashCode`, plus the shared file-finder)
//! this scan doesn't belong in — it doesn't generate anything, and shares
//! no logic with those functions beyond the same recursive-tree-walk shape
//! `find_source_file_by_stem`/`go_to_file.rs`'s `all_files` already use
//! independently of each other.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use fg_core::{FileKind, FileNode, Language};
use syntax::{EndpointInfo, IncrementalParser};

/// A file's endpoints as of the last time it was actually read + parsed,
/// alongside the modification time that was current then — `EndpointCache`'s
/// per-file entry, letting `scan_project_endpoints_cached` skip a file
/// entirely when its mtime hasn't moved since.
#[derive(Debug, Clone)]
pub(crate) struct CachedFileScan {
    mtime: SystemTime,
    endpoints: Vec<EndpointInfo>,
}

/// Carried across popup opens (owned by `panels::spring_endpoints::
/// SpringEndpointsState`, not reset on `toggle()`) so a re-open only
/// re-reads + re-parses files that actually changed since the last scan —
/// SPEC.md §4's original "no caching in this first pass" call, revisited
/// per that same entry's own escape hatch ("only if a real large project
/// open feels slow and is actually measured") once exactly that happened on
/// a real project.
pub type EndpointCache = HashMap<PathBuf, CachedFileScan>;

/// Every Spring MVC endpoint found under `root`, each paired with the path
/// of the file it was declared in (§6's jump needs to know which file to
/// open, and `EndpointInfo` itself carries no per-file identity of its own),
/// serving a file straight from `cache` instead of re-reading + re-parsing
/// it when its modification time matches what's already recorded there —
/// the expensive part of this scan (read + throwaway-parse of every source
/// file) only happens for files that are new or have actually changed since
/// the last call with this same `cache`. A file that fails to read
/// (permission error, race with a delete) is skipped silently, same "don't
/// fail the whole operation over one bad entry" reasoning already
/// established elsewhere in this codebase. `cache` is updated in place: a
/// hit is left untouched, a miss gets a fresh entry, and any path no longer
/// present in `root`'s tree at all (deleted/renamed since the last scan) is
/// dropped at the end rather than lingering forever. Call with a fresh
/// `EndpointCache::new()` for an uncached, always-read-every-file scan.
pub fn scan_project_endpoints_cached(root: &FileNode, cache: &mut EndpointCache) -> Vec<(PathBuf, EndpointInfo)> {
    let mut files = Vec::new();
    collect_source_files(root, &mut files);

    let mut seen = HashSet::with_capacity(files.len());
    // One slot per file in `files`' order, so the read+parse work below can
    // run out of order across threads while the final `out` is still
    // assembled in a stable, deterministic order at the end.
    let mut endpoints: Vec<Option<Vec<EndpointInfo>>> = Vec::with_capacity(files.len());
    let mut fresh_mtimes: Vec<Option<SystemTime>> = vec![None; files.len()];
    let mut needs_scan = Vec::new();

    for (i, (path, _language)) in files.iter().enumerate() {
        // A file whose metadata can't be read (deleted in a race with this
        // scan, say) is left out of `seen` entirely, same as the rest of
        // this loop skips it — `retain` below then drops any stale cache
        // entry for it exactly as it would for a file no longer in the tree
        // at all.
        let Some(mtime) = std::fs::metadata(path).ok().and_then(|m| m.modified().ok()) else {
            endpoints.push(None);
            continue;
        };
        seen.insert(path.clone());
        match cache.get(path) {
            Some(cached) if cached.mtime == mtime => {
                endpoints.push(Some(cached.endpoints.clone()));
            }
            _ => {
                endpoints.push(None);
                fresh_mtimes[i] = Some(mtime);
                needs_scan.push(i);
            }
        }
    }

    // The expensive part — read + throwaway-parse of every file that missed
    // the cache — is embarrassingly parallel (each file is independent), so
    // it's split across every available core the same way Zed's worktree
    // scanner spreads its own directory walk across `num_cpus` workers,
    // rather than running the whole project single-threaded on whichever
    // thread called this function. Below a couple of files, though, thread
    // spawning itself is the more expensive part (each `toggle()` already
    // runs this whole function on its own background thread, so a same-file
    // re-scan pays that overhead on top of a scope's under a real editing
    // session's typical CPU contention), so a tiny miss set is just scanned
    // inline on the calling thread instead.
    let scanned: Vec<(usize, Option<Vec<EndpointInfo>>)> = if needs_scan.len() <= 1 {
        needs_scan.iter().map(|&i| scan_file(&files, i)).collect()
    } else {
        let workers = std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(1);
        let chunk_size = needs_scan.len().div_ceil(workers).max(1);
        std::thread::scope(|scope| {
            needs_scan
                .chunks(chunk_size)
                .map(|chunk| {
                    let files = &files;
                    scope.spawn(move || chunk.iter().map(|&i| scan_file(files, i)).collect::<Vec<_>>())
                })
                .collect::<Vec<_>>()
                .into_iter()
                .flat_map(|handle| handle.join().unwrap())
                .collect()
        })
    };

    for (i, result) in scanned {
        endpoints[i] = result;
    }

    let mut out = Vec::new();
    for (i, (path, _language)) in files.iter().enumerate() {
        let Some(file_endpoints) = &endpoints[i] else { continue };
        out.extend(file_endpoints.iter().cloned().map(|e| (path.clone(), e)));
        if let Some(mtime) = fresh_mtimes[i] {
            cache.insert(
                path.clone(),
                CachedFileScan {
                    mtime,
                    endpoints: file_endpoints.clone(),
                },
            );
        }
    }

    cache.retain(|path, _| seen.contains(path));
    out
}

/// Reads + throwaway-parses `files[i]`, returning its endpoints alongside
/// `i` so a caller that dispatched this across chunks out of order (the
/// parallel path in `scan_project_endpoints_cached`) can still place the
/// result back at the right slot.
fn scan_file(files: &[(PathBuf, Language)], i: usize) -> (usize, Option<Vec<EndpointInfo>>) {
    let (path, language) = &files[i];
    let result = std::fs::read_to_string(path).ok().and_then(|source| {
        let mut parser = IncrementalParser::new(*language)?;
        let tree = parser.parse(&source).clone();
        Some(syntax::endpoints_in_file(*language, &tree, &source))
    });
    (i, result)
}

fn collect_source_files(node: &FileNode, out: &mut Vec<(PathBuf, Language)>) {
    match node.kind {
        FileKind::Dir => {
            for child in &node.children {
                collect_source_files(child, out);
            }
        }
        FileKind::File => {
            // Matched against this scan's own two languages directly
            // rather than resolved through the language registry: a Spring
            // endpoint scan is only ever interested in Java and Kotlin, so
            // consulting the registry would only be a longer way of
            // reaching the same two-way filter on the line below. This
            // whole module belongs to the `spring` extension (Track 24
            // Phase 6), and naming its own languages is exactly what an
            // extension is allowed to do.
            let Some(language) = node
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(|ext| match ext {
                    "java" => Some(Language::Java),
                    "kt" => Some(Language::Kotlin),
                    _ => None,
                })
            else {
                return;
            };
            out.push((node.path.clone(), language));
        }
    }
}

#[cfg(test)]
#[path = "spring_scan_test.rs"]
mod spring_scan_test;
