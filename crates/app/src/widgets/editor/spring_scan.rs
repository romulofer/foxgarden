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
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    collect_endpoints(root, cache, &mut seen, &mut out);
    cache.retain(|path, _| seen.contains(path));
    out
}

fn collect_endpoints(node: &FileNode, cache: &mut EndpointCache, seen: &mut HashSet<PathBuf>, out: &mut Vec<(PathBuf, EndpointInfo)>) {
    match node.kind {
        FileKind::Dir => {
            for child in &node.children {
                collect_endpoints(child, cache, seen, out);
            }
        }
        FileKind::File => {
            let Some(language) = node
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(Language::from_extension)
            else {
                return;
            };
            if !matches!(language, Language::Java | Language::Kotlin) {
                return;
            }
            let Ok(metadata) = std::fs::metadata(&node.path) else {
                return;
            };
            let Ok(mtime) = metadata.modified() else {
                return;
            };
            seen.insert(node.path.clone());

            if let Some(cached) = cache.get(&node.path)
                && cached.mtime == mtime
            {
                out.extend(cached.endpoints.iter().cloned().map(|e| (node.path.clone(), e)));
                return;
            }

            let Ok(source) = std::fs::read_to_string(&node.path) else {
                return;
            };
            let mut parser = IncrementalParser::new(language);
            let tree = parser.parse(&source).clone();
            let endpoints = syntax::endpoints_in_file(language, &tree, &source);
            out.extend(endpoints.iter().cloned().map(|e| (node.path.clone(), e)));
            cache.insert(node.path.clone(), CachedFileScan { mtime, endpoints });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fg_core::Project;

    fn scan(files: &[(&str, &str)]) -> Vec<(PathBuf, EndpointInfo)> {
        let dir = tempfile::tempdir().unwrap();
        for (name, contents) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, contents).unwrap();
        }
        let project = Project::open(dir.path().to_path_buf()).unwrap();
        let mut cache = EndpointCache::new();
        scan_project_endpoints_cached(&project.tree, &mut cache)
    }

    #[test]
    fn aggregates_endpoints_across_java_and_kotlin_files_with_the_right_paths() {
        let results = scan(&[
            (
                "UserController.java",
                "@RequestMapping(\"/api\")\nclass UserController {\n    @GetMapping(\"/{id}\")\n    public void getUser() {}\n}\n",
            ),
            (
                "OrderController.kt",
                "class OrderController {\n    @PostMapping(\"/orders\")\n    fun createOrder() {}\n}\n",
            ),
            ("NotAController.java", "class NotAController {\n    public void run() {}\n}\n"),
        ]);

        assert_eq!(results.len(), 2);

        let java_entry = results.iter().find(|(_, e)| e.handler_name == "getUser").unwrap();
        assert!(java_entry.0.ends_with("UserController.java"));
        assert_eq!(java_entry.1.path, "/api/{id}");

        let kotlin_entry = results.iter().find(|(_, e)| e.handler_name == "createOrder").unwrap();
        assert!(kotlin_entry.0.ends_with("OrderController.kt"));
        assert_eq!(kotlin_entry.1.path, "/orders");
    }

    #[test]
    fn a_non_source_file_in_the_tree_is_ignored() {
        let results = scan(&[("README.md", "# hi"), ("pom.xml", "<project/>")]);
        assert_eq!(results, vec![]);
    }

    #[test]
    fn an_empty_project_returns_empty() {
        let results = scan(&[]);
        assert_eq!(results, vec![]);
    }

    #[test]
    fn cached_scan_reuses_a_file_whose_mtime_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Foo.java");
        std::fs::write(&path, "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n").unwrap();
        let project = Project::open(dir.path().to_path_buf()).unwrap();

        let mut cache = EndpointCache::new();
        let first = scan_project_endpoints_cached(&project.tree, &mut cache);
        assert_eq!(first.len(), 1);
        assert!(cache.contains_key(&path));

        // Rewrite the file on disk *without* going through the cache (an
        // out-of-band edit an external tool made, say) but restore the
        // mtime `cache` already has on record afterward — the second scan
        // must still report the *old* result, proving it actually skipped
        // re-reading rather than happening to reparse identical content.
        let cached_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, "class Foo {\n    @PostMapping(\"/y\")\n    public void other() {}\n}\n").unwrap();
        std::fs::File::open(&path).unwrap().set_modified(cached_mtime).unwrap();

        let second = scan_project_endpoints_cached(&project.tree, &mut cache);
        assert_eq!(second, first, "unchanged mtime must serve the cached result, not the file's new content");
    }

    #[test]
    fn cached_scan_rereads_a_file_whose_mtime_changed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Foo.java");
        std::fs::write(&path, "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n").unwrap();
        let project = Project::open(dir.path().to_path_buf()).unwrap();

        let mut cache = EndpointCache::new();
        let first = scan_project_endpoints_cached(&project.tree, &mut cache);
        assert_eq!(first[0].1.handler_name, "run");

        // A real edit: new content *and* an mtime bumped a full second past
        // whatever was cached, since some filesystems only have 1-second
        // mtime resolution and a same-instant rewrite could otherwise look
        // unchanged.
        let bumped = std::fs::metadata(&path).unwrap().modified().unwrap() + std::time::Duration::from_secs(1);
        std::fs::write(&path, "class Foo {\n    @PostMapping(\"/y\")\n    public void other() {}\n}\n").unwrap();
        std::fs::File::open(&path).unwrap().set_modified(bumped).unwrap();

        let second = scan_project_endpoints_cached(&project.tree, &mut cache);
        assert_eq!(second[0].1.handler_name, "other");
    }

    #[test]
    fn cached_scan_drops_entries_for_files_no_longer_in_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Foo.java");
        std::fs::write(&path, "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n").unwrap();
        let mut project = Project::open(dir.path().to_path_buf()).unwrap();

        let mut cache = EndpointCache::new();
        scan_project_endpoints_cached(&project.tree, &mut cache);
        assert!(cache.contains_key(&path));

        std::fs::remove_file(&path).unwrap();
        project = Project::open(dir.path().to_path_buf()).unwrap();
        let results = scan_project_endpoints_cached(&project.tree, &mut cache);

        assert_eq!(results, vec![]);
        assert!(!cache.contains_key(&path), "a deleted file's stale entry must not linger in the cache");
    }
}
