
use super::*;
use fg_extension::Registry;

fn entry(method: &str, path: &str, owner: &str, handler: &str) -> HttpRoute {
    HttpRoute {
        method: method.to_string(),
        path: path.to_string(),
        owner: owner.to_string(),
        handler: handler.to_string(),
        handler_byte: 0,
    }
}

#[test]
fn row_label_renders_method_path_controller_and_handler() {
    let e = entry("GET", "/api/users/{id}", "UserController", "getUser");
    assert_eq!(row_label(&e), "GET /api/users/{id} — UserController#getUser");
}

#[test]
fn matching_routes_filters_by_path_fragment() {
    let routes = vec![
        (PathBuf::from("a"), entry("GET", "/api/users", "UserController", "list")),
        (
            PathBuf::from("b"),
            entry("GET", "/api/orders", "OrderController", "list"),
        ),
    ];
    let matched = matching_routes(&routes, "users");
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].1.owner, "UserController");
}

#[test]
fn matching_routes_filters_by_controller_or_handler_name() {
    let routes = vec![
        (PathBuf::from("a"), entry("GET", "/x", "UserController", "getUser")),
        (PathBuf::from("b"), entry("GET", "/y", "OrderController", "createOrder")),
    ];
    let matched = matching_routes(&routes, "createOrder");
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].1.handler, "createOrder");
}

#[test]
fn matching_routes_empty_query_returns_everything() {
    let routes = vec![
        (PathBuf::from("a"), entry("GET", "/x", "A", "a")),
        (PathBuf::from("b"), entry("GET", "/y", "B", "b")),
    ];
    assert_eq!(matching_routes(&routes, "").len(), 2);
}

#[test]
fn toggle_opens_and_resets_query_and_selection() {
    let mut popup = HttpRoutesState {
        open: false,
        query: "leftover".to_string(),
        selected: 3,
        routes: Vec::new(),
        scan_rx: None,
        cache: RouteCache::new(),
    };

    popup.toggle(None, &Registry::new());

    assert!(popup.open);
    assert!(popup.query.is_empty());
    assert_eq!(popup.selected, 0);
}

#[test]
fn toggle_twice_closes_it_again() {
    let mut popup = HttpRoutesState::default();
    popup.toggle(None, &Registry::new());
    popup.toggle(None, &Registry::new());
    assert!(!popup.open);
}

#[test]
fn toggle_with_no_project_leaves_routes_empty_and_starts_no_scan() {
    let mut popup = HttpRoutesState::default();
    popup.toggle(None, &Registry::new());
    assert!(popup.routes.is_empty());
    assert!(popup.scan_rx.is_none());
}

#[test]
fn toggle_starts_a_background_scan_that_delivers_the_project_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Foo.java"),
        "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n",
    )
    .unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    test_support::install_grammars();
    let mut popup = HttpRoutesState::default();
    popup.toggle(Some(&project.tree), test_support::languages());

    let rx = popup.scan_rx.take().expect("toggle should start a background scan");
    let (cache, routes) = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("scan should complete");
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].1.handler, "run");
    assert_eq!(
        cache.len(),
        1,
        "the scan should have cached the one source file it read"
    );
}

#[test]
fn poll_scan_fills_in_routes_once_the_background_scan_completes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Foo.java"),
        "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n",
    )
    .unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    test_support::install_grammars();
    let mut popup = HttpRoutesState::default();
    popup.toggle(Some(&project.tree), test_support::languages());
    assert!(popup.scan_rx.is_some(), "a scan should be in flight right after toggle");

    // Block on the same channel poll_scan itself drains from, so this
    // doesn't race the background thread — the real `show` call polls
    // non-blockingly every frame instead, relying on repaint requests to
    // get called again once the thread finishes.
    loop {
        poll_scan(&mut popup);
        if popup.scan_rx.is_none() {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(popup.routes.len(), 1);
    assert_eq!(popup.routes[0].1.handler, "run");
}

#[test]
fn toggle_keeps_the_cache_across_a_close_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Foo.java"),
        "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n",
    )
    .unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    test_support::install_grammars();
    let mut popup = HttpRoutesState::default();
    popup.toggle(Some(&project.tree), test_support::languages());
    while popup.scan_rx.is_some() {
        poll_scan(&mut popup);
        std::thread::yield_now();
    }
    assert_eq!(
        popup.cache.len(),
        1,
        "the first scan should have cached the one source file"
    );

    popup.toggle(None, &Registry::new());
    assert_eq!(
        popup.cache.len(),
        1,
        "closing must not reset the cache the way it resets routes"
    );

    popup.toggle(Some(&project.tree), test_support::languages());
    let rx = popup.scan_rx.take().expect("reopening should start a new scan");
    let (cache, routes) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(
        cache.len(),
        1,
        "the reopened scan should still have the cached entry, not start from empty"
    );
}
