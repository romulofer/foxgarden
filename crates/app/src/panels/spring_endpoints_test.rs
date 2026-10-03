
use super::*;

fn entry(http_method: &str, path: &str, controller_name: &str, handler_name: &str) -> EndpointInfo {
    EndpointInfo {
        http_method: http_method.to_string(),
        path: path.to_string(),
        controller_name: controller_name.to_string(),
        handler_name: handler_name.to_string(),
        handler_byte: 0,
    }
}

#[test]
fn row_label_renders_method_path_controller_and_handler() {
    let e = entry("GET", "/api/users/{id}", "UserController", "getUser");
    assert_eq!(row_label(&e), "GET /api/users/{id} — UserController#getUser");
}

#[test]
fn matching_endpoints_filters_by_path_fragment() {
    let endpoints = vec![
        (PathBuf::from("a"), entry("GET", "/api/users", "UserController", "list")),
        (
            PathBuf::from("b"),
            entry("GET", "/api/orders", "OrderController", "list"),
        ),
    ];
    let matched = matching_endpoints(&endpoints, "users");
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].1.controller_name, "UserController");
}

#[test]
fn matching_endpoints_filters_by_controller_or_handler_name() {
    let endpoints = vec![
        (PathBuf::from("a"), entry("GET", "/x", "UserController", "getUser")),
        (PathBuf::from("b"), entry("GET", "/y", "OrderController", "createOrder")),
    ];
    let matched = matching_endpoints(&endpoints, "createOrder");
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].1.handler_name, "createOrder");
}

#[test]
fn matching_endpoints_empty_query_returns_everything() {
    let endpoints = vec![
        (PathBuf::from("a"), entry("GET", "/x", "A", "a")),
        (PathBuf::from("b"), entry("GET", "/y", "B", "b")),
    ];
    assert_eq!(matching_endpoints(&endpoints, "").len(), 2);
}

#[test]
fn toggle_opens_and_resets_query_and_selection() {
    let mut popup = SpringEndpointsState {
        open: false,
        query: "leftover".to_string(),
        selected: 3,
        endpoints: Vec::new(),
        scan_rx: None,
        cache: EndpointCache::new(),
    };

    popup.toggle(None);

    assert!(popup.open);
    assert!(popup.query.is_empty());
    assert_eq!(popup.selected, 0);
}

#[test]
fn toggle_twice_closes_it_again() {
    let mut popup = SpringEndpointsState::default();
    popup.toggle(None);
    popup.toggle(None);
    assert!(!popup.open);
}

#[test]
fn toggle_with_no_project_leaves_endpoints_empty_and_starts_no_scan() {
    let mut popup = SpringEndpointsState::default();
    popup.toggle(None);
    assert!(popup.endpoints.is_empty());
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

    let mut popup = SpringEndpointsState::default();
    popup.toggle(Some(&project.tree));

    let rx = popup.scan_rx.take().expect("toggle should start a background scan");
    let (cache, endpoints) = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("scan should complete");
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].1.handler_name, "run");
    assert_eq!(
        cache.len(),
        1,
        "the scan should have cached the one source file it read"
    );
}

#[test]
fn poll_scan_fills_in_endpoints_once_the_background_scan_completes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Foo.java"),
        "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n",
    )
    .unwrap();
    let project = fg_core::Project::open(dir.path().to_path_buf()).unwrap();

    let mut popup = SpringEndpointsState::default();
    popup.toggle(Some(&project.tree));
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
    assert_eq!(popup.endpoints.len(), 1);
    assert_eq!(popup.endpoints[0].1.handler_name, "run");
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

    let mut popup = SpringEndpointsState::default();
    popup.toggle(Some(&project.tree));
    while popup.scan_rx.is_some() {
        poll_scan(&mut popup);
        std::thread::yield_now();
    }
    assert_eq!(
        popup.cache.len(),
        1,
        "the first scan should have cached the one source file"
    );

    popup.toggle(None);
    assert_eq!(
        popup.cache.len(),
        1,
        "closing must not reset the cache the way it resets endpoints"
    );

    popup.toggle(Some(&project.tree));
    let rx = popup.scan_rx.take().expect("reopening should start a new scan");
    let (cache, endpoints) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(endpoints.len(), 1);
    assert_eq!(
        cache.len(),
        1,
        "the reopened scan should still have the cached entry, not start from empty"
    );
}
