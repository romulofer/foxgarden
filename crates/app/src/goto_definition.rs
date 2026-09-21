//! `PLAN.md` Track 20 Phase 4: `textDocument/definition`, resolved into a
//! target `app.rs` feeds straight into `pending_navigation` — the same
//! cross-tab jump primitive the Spring endpoint map and a clicked
//! compiler-error row already use. Unlike `widgets::editor::hover`, there's
//! no UI to own here (no popup, no painting), just a background request to
//! poll once a frame, the same shape every other async LSP op in this app
//! already has.
//!
//! A JDK/library type (no real file in the open project) resolves to a
//! `jdt://` URI instead of a real path — that needs a second async
//! round-trip, jdt.ls' own `java/classFileContents` extension request, to
//! fetch the decompiled source before there's anywhere to jump *to* at all.
//! `Stage` tracks which of those two requests is currently in flight.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use directories::ProjectDirs;
use fg_core::Document;
use lsp_types::Uri;

use crate::lsp_client::ResponseError;
use crate::lsp_state::{LspState, uri_to_path, utf16_range_to_bytes};

/// A resolved `textDocument/definition` reply, still needing one more step
/// before it's a `(PathBuf, byte_offset)` `app.rs` can hand to
/// `pending_navigation`.
pub enum Target {
    /// A location inside a real on-disk file. `range` is still a UTF-16
    /// LSP `Position` pair — `app.rs` converts it once the target's real
    /// buffer text is available, right after `open_path` opens it (mirrors
    /// `app.rs`'s own `build_click` handling of a compiler-error row's
    /// line/column, which has the identical "no buffer to convert against
    /// until the file is actually open" shape).
    File { path: PathBuf, range: lsp_types::Range },
    /// A JDK/library type's decompiled source, already written to a local
    /// cache file with its own byte offset already known — computed
    /// directly from the `java/classFileContents` reply text this struct
    /// already had in hand, so there's nothing left to convert.
    Ready { path: PathBuf, byte_offset: usize },
}

enum Stage {
    Locating(Receiver<Result<serde_json::Value, ResponseError>>),
    FetchingSource {
        uri: Uri,
        range: lsp_types::Range,
        rx: Receiver<Result<serde_json::Value, ResponseError>>,
    },
}

/// One slot for whichever request is currently in flight — same "one
/// field, not one per open tab" shape `hover::HoverState`'s own single
/// tracked slot already uses, and for the same reason: a goto-definition
/// request only ever targets wherever the caret/click just was in the
/// focused tab, so there's never more than one in flight.
#[derive(Default)]
pub struct GotoDefinitionState {
    stage: Option<Stage>,
}

impl GotoDefinitionState {
    /// Fires a `textDocument/definition` request at `byte_offset` in `doc`,
    /// replacing whatever request (if any) was already in flight — a second
    /// Ctrl+Click before the first reply lands means the first one's answer
    /// is no longer what the user is asking about.
    pub fn request(&mut self, doc: &mut Document, byte_offset: usize, lsp: &mut LspState) {
        if let Some(rx) = lsp.request_definition(doc, byte_offset) {
            self.stage = Some(Stage::Locating(rx));
        }
    }

    /// Whether a reply (either stage) could still land — same unprompted-
    /// background-message concern `LspState::wants_repaint`/`HoverState::
    /// wants_repaint` already document: a reply arriving between user input
    /// events needs its own repaint request, or it sits unread until an
    /// unrelated one comes along.
    pub fn wants_repaint(&self) -> bool {
        self.stage.is_some()
    }

    /// Polls whatever's in flight. Returns `Some(Target)` once a request
    /// (the location lookup, and — only for a `jdt://` result — the
    /// decompiled-source fetch that follows it) has actually resolved;
    /// `None` on every other frame, including a still-pending one (which
    /// stays tracked for the next poll) and a failed/rejected one (dropped
    /// silently, the same best-effort degrade every other LSP path here
    /// already has — no tooltip, no jump, nothing surfaced as an error).
    pub fn poll(&mut self, lsp: &mut LspState) -> Option<Target> {
        match self.stage.take()? {
            Stage::Locating(rx) => match rx.try_recv() {
                Ok(Ok(value)) => self.handle_location(value, lsp),
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => None,
                Err(TryRecvError::Empty) => {
                    self.stage = Some(Stage::Locating(rx));
                    None
                }
            },
            Stage::FetchingSource { uri, range, rx } => match rx.try_recv() {
                Ok(Ok(value)) => handle_source(&uri, range, value),
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => None,
                Err(TryRecvError::Empty) => {
                    self.stage = Some(Stage::FetchingSource { uri, range, rx });
                    None
                }
            },
        }
    }

    /// A `jdt://` location has no real path to resolve yet — fires the
    /// follow-up `java/classFileContents` request and stays pending another
    /// stage rather than returning a `Target` this frame. Any other scheme
    /// is assumed `file://` (the only other kind a real server here ever
    /// sends); `uri_to_path` itself degrades to `None`, dropped the same
    /// best-effort way as everything else in this module, if that
    /// assumption is ever wrong.
    fn handle_location(&mut self, value: serde_json::Value, lsp: &mut LspState) -> Option<Target> {
        let (uri, range) = first_location(value)?;
        if uri.as_str().starts_with("jdt:") {
            let rx = lsp.request_class_file_contents(uri.as_str())?;
            self.stage = Some(Stage::FetchingSource { uri, range, rx });
            None
        } else {
            Some(Target::File {
                path: uri_to_path(&uri)?,
                range,
            })
        }
    }
}

/// Decodes a raw `textDocument/definition` response into its first location
/// — `GotoDefinitionResponse`'s three wire shapes (a bare `Location`, a
/// `Location` array, or a `LocationLink` array — the last is what a real
/// jdt.ls sends) collapsed into one `(Uri, Range)` pair. `None` covers a
/// `null` result (the protocol's own "no definition here"), a malformed
/// response, and an empty array alike — this module's every caller already
/// treats "nothing to jump to" as a silent no-op, same as `hover`'s own
/// decode step.
fn first_location(value: serde_json::Value) -> Option<(Uri, lsp_types::Range)> {
    let response = serde_json::from_value::<Option<lsp_types::GotoDefinitionResponse>>(value)
        .ok()
        .flatten()?;
    match response {
        lsp_types::GotoDefinitionResponse::Scalar(location) => Some((location.uri, location.range)),
        lsp_types::GotoDefinitionResponse::Array(locations) => locations
            .into_iter()
            .next()
            .map(|location| (location.uri, location.range)),
        // `target_selection_range` (the definition's own name span), not
        // `target_range` (the whole declaration, doc comment and all) — the
        // same "jump to the symbol itself, not its surrounding block" target
        // a `Location`'s single `range` already is for the other two shapes.
        lsp_types::GotoDefinitionResponse::Link(links) => links
            .into_iter()
            .next()
            .map(|link| (link.target_uri, link.target_selection_range)),
    }
}

/// A `java/classFileContents` reply's decompiled text, written to a stable
/// local cache file and opened like any other. `range` (still UTF-16, the
/// same shape `handle_location` left it in) is resolved against that exact
/// text right here — unlike the `Target::File` case, there's no separate
/// buffer to wait on, since the text this converts against *is* what gets
/// written to disk.
fn handle_source(uri: &Uri, range: lsp_types::Range, value: serde_json::Value) -> Option<Target> {
    let content = value.as_str()?;
    let path = jdt_cache_path(uri.as_str()).ok()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    std::fs::write(&path, content).ok()?;
    let byte_offset = utf16_range_to_bytes(content, range).map_or(0, |range| range.start);
    Some(Target::Ready { path, byte_offset })
}

/// A stable filesystem path for `uri`'s decompiled source — the same URI
/// always maps to the same path, so re-visiting a type already looked up
/// this run (or a previous one) reuses the cached file instead of asking
/// jdt.ls to redecompile it. Named from the URI's own trailing `.class`
/// segment (falls back to `"Source"` if that can't be found) purely for a
/// human glancing at the cache directory or an editor tab title; the hash
/// suffix — not the name — is what actually guarantees no two distinct URIs
/// collide (two different JARs can each contain a class of the same simple
/// name).
fn jdt_cache_path(uri: &str) -> Result<PathBuf, String> {
    let dir = ProjectDirs::from("", "", "foxgarden")
        .map(|dirs| dirs.cache_dir().join("jdt-classes"))
        .ok_or_else(|| "couldn't determine a cache directory for this platform".to_string())?;
    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    let hash = hasher.finish();
    let name = uri
        .rsplit('/')
        .next()
        .map(|segment| segment.split(['?', '#']).next().unwrap_or(segment))
        .map(|segment| segment.strip_suffix(".class").unwrap_or(segment))
        .filter(|segment| !segment.is_empty())
        .unwrap_or("Source");
    Ok(dir.join(format!("{name}-{hash:016x}.java")))
}

#[cfg(test)]
#[path = "goto_definition_test.rs"]
mod goto_definition_test;
