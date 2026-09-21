
use super::*;
use fg_core::Project;

#[test]
#[ignore]
fn bench_scan_real_project() {
    let path = std::env::var("FG_BENCH_PATH").expect("set FG_BENCH_PATH");
    let project = Project::open(path.into()).unwrap();
    let mut cache = EndpointCache::new();
    let start = std::time::Instant::now();
    let out = scan_project_endpoints_cached(&project.tree, &mut cache);
    eprintln!("cold: endpoints={} elapsed={:?}", out.len(), start.elapsed());
    let start = std::time::Instant::now();
    let out = scan_project_endpoints_cached(&project.tree, &mut cache);
    eprintln!("warm: endpoints={} elapsed={:?}", out.len(), start.elapsed());
}

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
        (
            "NotAController.java",
            "class NotAController {\n    public void run() {}\n}\n",
        ),
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
    std::fs::write(
        &path,
        "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n",
    )
    .unwrap();
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
    std::fs::write(
        &path,
        "class Foo {\n    @PostMapping(\"/y\")\n    public void other() {}\n}\n",
    )
    .unwrap();
    std::fs::File::open(&path).unwrap().set_modified(cached_mtime).unwrap();

    let second = scan_project_endpoints_cached(&project.tree, &mut cache);
    assert_eq!(
        second, first,
        "unchanged mtime must serve the cached result, not the file's new content"
    );
}

#[test]
fn cached_scan_rereads_a_file_whose_mtime_changed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Foo.java");
    std::fs::write(
        &path,
        "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n",
    )
    .unwrap();
    let project = Project::open(dir.path().to_path_buf()).unwrap();

    let mut cache = EndpointCache::new();
    let first = scan_project_endpoints_cached(&project.tree, &mut cache);
    assert_eq!(first[0].1.handler_name, "run");

    // A real edit: new content *and* an mtime bumped a full second past
    // whatever was cached, since some filesystems only have 1-second
    // mtime resolution and a same-instant rewrite could otherwise look
    // unchanged.
    let bumped = std::fs::metadata(&path).unwrap().modified().unwrap() + std::time::Duration::from_secs(1);
    std::fs::write(
        &path,
        "class Foo {\n    @PostMapping(\"/y\")\n    public void other() {}\n}\n",
    )
    .unwrap();
    std::fs::File::open(&path).unwrap().set_modified(bumped).unwrap();

    let second = scan_project_endpoints_cached(&project.tree, &mut cache);
    assert_eq!(second[0].1.handler_name, "other");
}

#[test]
fn cached_scan_drops_entries_for_files_no_longer_in_the_tree() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Foo.java");
    std::fs::write(
        &path,
        "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n",
    )
    .unwrap();
    let mut project = Project::open(dir.path().to_path_buf()).unwrap();

    let mut cache = EndpointCache::new();
    scan_project_endpoints_cached(&project.tree, &mut cache);
    assert!(cache.contains_key(&path));

    std::fs::remove_file(&path).unwrap();
    project = Project::open(dir.path().to_path_buf()).unwrap();
    let results = scan_project_endpoints_cached(&project.tree, &mut cache);

    assert_eq!(results, vec![]);
    assert!(
        !cache.contains_key(&path),
        "a deleted file's stale entry must not linger in the cache"
    );
}
