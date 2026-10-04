//! Whole-project scan for the HTTP route map (`SPEC.md` §4) — reads and
//! throwaway-parses every file some extension finds routes in, and collects
//! whatever `syntax::http_routes` reports for it. Which files those are is
//! the registry's answer (`Registry::has_http_routes`), so this scan names
//! no language and no framework of its own. Not `codegen.rs`: that file is
//! its own cohesive "code generation" concern this scan doesn't belong in —
//! it doesn't generate anything, and shares no logic with those functions
//! beyond the same recursive-tree-walk shape `go_to_file.rs`'s `all_files`
//! uses.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use fg_core::{FileKind, FileNode, Language};
use fg_extension::Registry;
use syntax::{HttpRoute, IncrementalParser};

/// A file's routes as of the last time it was actually read + parsed,
/// alongside the modification time that was current then — `RouteCache`'s
/// per-file entry, letting `scan_project_routes_cached` skip a file
/// entirely when its mtime hasn't moved since.
#[derive(Debug, Clone)]
pub(crate) struct CachedFileScan {
    mtime: SystemTime,
    routes: Vec<HttpRoute>,
}

/// Carried across popup opens (owned by `panels::http_routes::
/// HttpRoutesState`, not reset on `toggle()`) so a re-open only
/// re-reads + re-parses files that actually changed since the last scan —
/// SPEC.md §4's original "no caching in this first pass" call, revisited
/// per that same entry's own escape hatch ("only if a real large project
/// open feels slow and is actually measured") once exactly that happened on
/// a real project.
pub type RouteCache = HashMap<PathBuf, CachedFileScan>;

/// Every HTTP route declared in `files` (see [`route_files`]), each paired
/// with the path of the file it was declared in (§6's jump needs to know
/// which file to open, and `HttpRoute` itself carries no per-file identity
/// of its own), serving a file straight from `cache` instead of re-reading +
/// re-parsing it when its modification time matches what's already recorded
/// there — the expensive part of this scan (read + throwaway-parse of every
/// source file) only happens for files that are new or have actually
/// changed since the last call with this same `cache`. A file that fails to
/// read (permission error, race with a delete) is skipped silently, same
/// "don't fail the whole operation over one bad entry" reasoning already
/// established elsewhere in this codebase. `cache` is updated in place: a
/// hit is left untouched, a miss gets a fresh entry, and any path no longer
/// in `files` at all (deleted/renamed since the last scan) is dropped at the
/// end rather than lingering forever. Call with a fresh `RouteCache::new()`
/// for an uncached, always-read-every-file scan.
pub fn scan_project_routes_cached(files: &[(PathBuf, Language)], cache: &mut RouteCache) -> Vec<(PathBuf, HttpRoute)> {
    let mut seen = HashSet::with_capacity(files.len());
    // One slot per file in `files`' order, so the read+parse work below can
    // run out of order across threads while the final `out` is still
    // assembled in a stable, deterministic order at the end.
    let mut routes: Vec<Option<Vec<HttpRoute>>> = Vec::with_capacity(files.len());
    let mut fresh_mtimes: Vec<Option<SystemTime>> = vec![None; files.len()];
    let mut needs_scan = Vec::new();

    for (i, (path, _language)) in files.iter().enumerate() {
        // A file whose metadata can't be read (deleted in a race with this
        // scan, say) is left out of `seen` entirely, same as the rest of
        // this loop skips it — `retain` below then drops any stale cache
        // entry for it exactly as it would for a file no longer in the tree
        // at all.
        let Some(mtime) = std::fs::metadata(path).ok().and_then(|m| m.modified().ok()) else {
            routes.push(None);
            continue;
        };
        seen.insert(path.clone());
        match cache.get(path) {
            Some(cached) if cached.mtime == mtime => {
                routes.push(Some(cached.routes.clone()));
            }
            _ => {
                routes.push(None);
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
    let scanned: Vec<(usize, Option<Vec<HttpRoute>>)> = if needs_scan.len() <= 1 {
        needs_scan.iter().map(|&i| scan_file(files, i)).collect()
    } else {
        let workers = std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(1);
        let chunk_size = needs_scan.len().div_ceil(workers).max(1);
        std::thread::scope(|scope| {
            needs_scan
                .chunks(chunk_size)
                .map(|chunk| {
                    scope.spawn(move || chunk.iter().map(|&i| scan_file(files, i)).collect::<Vec<_>>())
                })
                .collect::<Vec<_>>()
                .into_iter()
                .flat_map(|handle| handle.join().unwrap())
                .collect()
        })
    };

    for (i, result) in scanned {
        routes[i] = result;
    }

    let mut out = Vec::new();
    for (i, (path, _language)) in files.iter().enumerate() {
        let Some(file_routes) = &routes[i] else { continue };
        out.extend(file_routes.iter().cloned().map(|route| (path.clone(), route)));
        if let Some(mtime) = fresh_mtimes[i] {
            cache.insert(
                path.clone(),
                CachedFileScan {
                    mtime,
                    routes: file_routes.clone(),
                },
            );
        }
    }

    cache.retain(|path, _| seen.contains(path));
    out
}

/// Reads + throwaway-parses `files[i]`, returning its routes alongside
/// `i` so a caller that dispatched this across chunks out of order (the
/// parallel path in `scan_project_routes_cached`) can still place the
/// result back at the right slot.
fn scan_file(files: &[(PathBuf, Language)], i: usize) -> (usize, Option<Vec<HttpRoute>>) {
    let (path, language) = &files[i];
    let result = std::fs::read_to_string(path).ok().and_then(|source| {
        let mut parser = IncrementalParser::new(*language)?;
        let tree = parser.parse(&source).clone();
        Some(syntax::http_routes(&tree, &source, *language))
    });
    (i, result)
}

/// Every file under `root` whose language some extension finds HTTP routes
/// in, with that language — the input [`scan_project_routes_cached`] takes.
/// A walk of the in-memory tree, no I/O, so it is cheap enough to do on the
/// UI thread when the popup opens, which is what lets the scan thread do
/// without the registry.
pub fn route_files(root: &FileNode, languages: &Registry) -> Vec<(PathBuf, Language)> {
    let mut out = Vec::new();
    collect_route_files(root, languages, &mut out);
    out
}

fn collect_route_files(node: &FileNode, languages: &Registry, out: &mut Vec<(PathBuf, Language)>) {
    match node.kind {
        FileKind::Dir => {
            for child in &node.children {
                collect_route_files(child, languages, out);
            }
        }
        FileKind::File => {
            let Some(registered) = languages
                .language_for_path(&node.path)
                .filter(|registered| languages.has_http_routes(registered.static_id))
            else {
                return;
            };
            out.push((node.path.clone(), Language::new(registered.static_id)));
        }
    }
}

#[cfg(test)]
#[path = "route_scan_test.rs"]
mod route_scan_test;
