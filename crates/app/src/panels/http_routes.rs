use fg_i18n::t;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use fg_core::{EditorState, FileNode};
use fg_extension::Registry;
use syntax::HttpRoute;

use crate::panels::go_to_file::fuzzy_score;
use crate::widgets::editor::{RouteCache, route_files, scan_project_routes_cached};

/// What a completed background scan sends back: the updated cache (for
/// `HttpRoutesState::cache` to keep across the next open) alongside this
/// scan's flattened route list.
type ScanResult = (RouteCache, Vec<(PathBuf, HttpRoute)>);

/// Transient state for the `Ctrl+Shift+E` HTTP route map popup, owned
/// by the caller across frames — structurally a near-twin of
/// `GoToFileState`/`QuickSwitcherState`, plus fields neither of those
/// carries: `routes`, the last completed whole-project scan's result;
/// `scan_rx`, `Some` while a scan is still running on a background thread;
/// and `cache`, carried *across* opens (unlike `routes`, not reset in
/// `toggle()`) so a re-open only re-reads + re-parses files that actually
/// changed since the last scan (`route_scan::scan_project_routes_cached`).
/// A real project's worth of source files can take long enough to
/// read-and-parse that running the scan on the UI thread freezes the whole
/// app for that whole stretch, and repeating that full cost on *every* open
/// (rather than just the first) was directly reported as too slow on a real
/// project — both measured, not assumed. Spawning the scan onto its own
/// thread and polling `scan_rx` from `show` keeps the UI responsive and
/// takes typing in the query box while the popup's own "Scanning…" state
/// shows, the same background-thread-plus-channel shape `SPEC.md` §8.3
/// already calls for the terminal-tabs track's own pty reads.
#[derive(Default)]
pub struct HttpRoutesState {
    open: bool,
    query: String,
    selected: usize,
    routes: Vec<(PathBuf, HttpRoute)>,
    scan_rx: Option<Receiver<ScanResult>>,
    cache: RouteCache,
}

impl HttpRoutesState {
    /// Opens the popup and kicks off a fresh background scan of `root`'s
    /// route files (which ones, `languages` decides) against `self.cache` (`None` when no project is open, same as every
    /// other project-scoped popup here); closing needs no scan. Same
    /// "recomputed on open" reasoning `CompletionState::open` already uses
    /// for its own candidate list, not every frame the popup is shown.
    ///
    /// `self.cache` is moved into the worker thread (not cloned — a large
    /// project's cache is exactly the data this exists to avoid copying
    /// around) and swapped back in once the scan completes, via `poll_scan`.
    /// Dropping a still-running previous scan's receiver (if the popup is
    /// closed and reopened quickly) loses that in-flight cache along with
    /// its result — the worker thread's own `send` then just fails silently
    /// and the thread exits — same "harmless, just a frame late or a re-scan
    /// short of ideal" tolerance `routes` filling in a frame late already
    /// has, just extended to the cache; the next successful scan rebuilds it
    /// from scratch.
    pub fn toggle(&mut self, root: Option<&FileNode>, languages: &Registry) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
        self.scan_rx = None;
        self.routes.clear();
        if self.open
            && let Some(root) = root
        {
            let files = route_files(root, languages);
            let mut cache = std::mem::take(&mut self.cache);
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let routes = scan_project_routes_cached(&files, &mut cache);
                let _ = tx.send((cache, routes));
            });
            self.scan_rx = Some(rx);
        }
    }
}

/// The row text a popup entry renders as, and what `fuzzy_score` filters
/// against — typing either a path fragment or a method/controller name
/// narrows the list.
fn row_label(route: &HttpRoute) -> String {
    format!("{} {} — {}#{}", route.method, route.path, route.owner, route.handler)
}

/// Every cached route whose rendered row fuzzy-matches `query`, best
/// match first (ties broken by source order, for stable ordering) — reused
/// straight from `go_to_file.rs` rather than a second fuzzy matcher.
fn matching_routes<'a>(routes: &'a [(PathBuf, HttpRoute)], query: &str) -> Vec<&'a (PathBuf, HttpRoute)> {
    let mut scored: Vec<(i32, usize, &(PathBuf, HttpRoute))> = routes
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
fn poll_scan(popup: &mut HttpRoutesState) {
    let Some(rx) = &popup.scan_rx else {
        return;
    };
    match rx.try_recv() {
        Ok((cache, routes)) => {
            popup.cache = cache;
            popup.routes = routes;
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
/// from `popup.routes`, filled in once the background scan `toggle`
/// started completes) but is kept so this call site matches those two
/// popups' own `show(ui, &state, &mut popup)` shape.
pub fn show(ui: &egui::Ui, _state: &EditorState, popup: &mut HttpRoutesState) -> Option<(PathBuf, usize)> {
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
        matching_routes(&popup.routes, &popup.query)
    };
    if !candidates.is_empty() {
        popup.selected = popup.selected.min(candidates.len() - 1);
    }

    let ctx = ui.ctx().clone();
    let mut chosen = None;
    let mut escaped = false;

    egui::Modal::new(egui::Id::new("http_routes")).show(&ctx, |ui| {
        ui.set_min_width(560.0);
        ui.label(t().palettes.http_routes);

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
            ui.weak(t().palettes.scanning_project);
        } else if candidates.is_empty() {
            ui.weak(t().common.no_matches);
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
#[path = "http_routes_test.rs"]
mod http_routes_test;
