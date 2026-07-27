use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use fg_core::{EditorState, FileNode};
use syntax::EndpointInfo;

use crate::panels::go_to_file::fuzzy_score;
use crate::widgets::editor::scan_project_endpoints;

/// Transient state for the `Ctrl+Shift+E` Spring endpoint map popup, owned
/// by the caller across frames — structurally a near-twin of
/// `GoToFileState`/`QuickSwitcherState`, plus two fields neither of those
/// carries: `endpoints`, the last completed whole-project scan's result,
/// and `scan_rx`, `Some` while a scan is still running on a background
/// thread. A real project's worth of `.java`/`.kt` files can take long
/// enough to read-and-parse that running the scan on the UI thread freezes
/// the whole app for that whole stretch (measured directly, not assumed —
/// noticeably long on a large real project); spawning it onto its own
/// thread and polling `scan_rx` from `show` keeps the UI responsive and
/// takes typing in the query box while the popup's own "Scanning…" state
/// shows, the same background-thread-plus-channel shape `SPEC.md` §8.3
/// already calls for the terminal-tabs track's own pty reads.
#[derive(Default)]
pub struct SpringEndpointsState {
    open: bool,
    query: String,
    selected: usize,
    endpoints: Vec<(PathBuf, EndpointInfo)>,
    scan_rx: Option<Receiver<Vec<(PathBuf, EndpointInfo)>>>,
}

impl SpringEndpointsState {
    /// Opens the popup and kicks off a fresh background scan of `root`
    /// (`None` when no project is open, same as every other project-scoped
    /// popup here); closing needs no scan. Same "recomputed on open"
    /// reasoning `CompletionState::open` already uses for its own
    /// candidate list, not every frame the popup is shown. Dropping a
    /// still-running previous scan's receiver (if the popup is closed and
    /// reopened quickly) is harmless: the worker thread's own `send` then
    /// just fails silently and the thread exits, same as `endpoints`
    /// filling in a frame late ever would.
    pub fn toggle(&mut self, root: Option<&FileNode>) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
        self.scan_rx = None;
        self.endpoints.clear();
        if self.open && let Some(root) = root {
            let root = root.clone();
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(scan_project_endpoints(&root));
            });
            self.scan_rx = Some(rx);
        }
    }
}

/// The row text a popup entry renders as, and what `fuzzy_score` filters
/// against — typing either a path fragment or a method/controller name
/// narrows the list.
fn row_label(entry: &EndpointInfo) -> String {
    format!(
        "{} {} — {}#{}",
        entry.http_method, entry.path, entry.controller_name, entry.handler_name
    )
}

/// Every cached endpoint whose rendered row fuzzy-matches `query`, best
/// match first (ties broken by source order, for stable ordering) — reused
/// straight from `go_to_file.rs` rather than a second fuzzy matcher.
fn matching_endpoints<'a>(endpoints: &'a [(PathBuf, EndpointInfo)], query: &str) -> Vec<&'a (PathBuf, EndpointInfo)> {
    let mut scored: Vec<(i32, usize, &(PathBuf, EndpointInfo))> = endpoints
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| fuzzy_score(&row_label(&entry.1), query).map(|score| (score, index, entry)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

/// Drains `popup.scan_rx` if the background scan `toggle` started has
/// finished — `Empty` means still running (the caller keeps repainting so
/// this gets checked again next frame instead of only on the next input
/// event), `Disconnected` means the worker thread died without sending
/// (treated the same as "found nothing" rather than spinning forever).
fn poll_scan(popup: &mut SpringEndpointsState) {
    let Some(rx) = &popup.scan_rx else {
        return;
    };
    match rx.try_recv() {
        Ok(result) => {
            popup.endpoints = result;
            popup.scan_rx = None;
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => popup.scan_rx = None,
    }
}

/// Draws the popup if `popup.open`, returning `(path, handler_byte)` for
/// the entry the user picked (by click or Enter on the highlighted row)
/// this frame, if any — the caller acts on it (open the file, jump to the
/// byte) elsewhere; picking here doesn't navigate by itself. Closes the
/// popup on a pick or on Escape, same shape as `go_to_file::show`/
/// `quick_switcher::show`. `_state` isn't read yet (every row's text comes
/// from `popup.endpoints`, filled in once the background scan `toggle`
/// started completes) but is kept so this call site matches those two
/// popups' own `show(ui, &state, &mut popup)` shape.
pub fn show(ui: &egui::Ui, _state: &EditorState, popup: &mut SpringEndpointsState) -> Option<(PathBuf, usize)> {
    if !popup.open {
        return None;
    }

    poll_scan(popup);
    let scanning = popup.scan_rx.is_some();
    if scanning {
        // Nothing else drives a repaint while the popup just sits there
        // waiting on the worker thread — without this, the query box would
        // still take typed input fine (that's a real input event), but the
        // "Scanning…" state wouldn't visibly resolve into the finished list
        // until some other event happened to trigger a redraw.
        ui.ctx().request_repaint();
    }

    let candidates = if scanning {
        Vec::new()
    } else {
        matching_endpoints(&popup.endpoints, &popup.query)
    };
    if !candidates.is_empty() {
        popup.selected = popup.selected.min(candidates.len() - 1);
    }

    let ctx = ui.ctx().clone();
    let mut chosen = None;
    let mut escaped = false;

    egui::Modal::new(egui::Id::new("spring_endpoints")).show(&ctx, |ui| {
        ui.set_min_width(560.0);
        ui.label("Spring endpoints");

        let response = ui.text_edit_singleline(&mut popup.query);
        response.request_focus();

        escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !candidates.is_empty() {
            popup.selected = (popup.selected + 1).min(candidates.len() - 1);
        }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            popup.selected = popup.selected.saturating_sub(1);
        }
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        ui.separator();
        if scanning {
            ui.weak("Scanning project…");
        } else if candidates.is_empty() {
            ui.weak("No matches");
        }
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (index, (path, entry)) in candidates.iter().enumerate() {
                let is_selected = index == popup.selected;
                let response = ui.selectable_label(is_selected, row_label(entry));
                if response.clicked() || (is_selected && enter_pressed) {
                    chosen = Some((path.clone(), entry.handler_byte));
                }
            }
        });
    });

    if chosen.is_some() || escaped {
        popup.open = false;
    }
    chosen
}

#[cfg(test)]
mod tests {
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
            (PathBuf::from("b"), entry("GET", "/api/orders", "OrderController", "list")),
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
        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("scan should complete");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.handler_name, "run");
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
}
