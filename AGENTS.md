# AGENTS.md

Guidance for AI coding agents working in this repository. See `README.md` for
a human-facing overview.

## Design principle: lightweight AND functional

FoxGarden's whole reason to exist is a fast, small, native editor — not
"Electron but slower to write." [Zed](https://zed.dev) is the concrete bar for
"lightning fast": its source is checked out at `../references/zed` (relative
to this repo) specifically so architecture/performance questions have a
real, production-grade answer to check against instead of a guess — e.g. how
it structures incremental parsing/rendering, what it defers vs. does eagerly,
how it keeps a hot path (input, scroll, redraw) free of anything that scales
with project or file size. It's a reference to *learn from*, not a
dependency or a codebase to copy wholesale — FoxGarden's own scope (a
Java/Kotlin-focused editor, not Zed's general-purpose multi-language/
collaborative feature set) means plenty of what Zed does is out of scope
here; pull the *principles* (its approach to a specific hot path, a specific
caching strategy) rather than porting its architecture verbatim.

Two more references sit alongside it for the same reason, scoped narrower:
`../references/java` and `../references/kotlin` are Zed's own Java and
Kotlin extensions (language server wiring, tree-sitter grammar choices,
Gradle/Maven task and debug-adapter integration). These are checkpoint-1
non-goals today (see `README.md`'s "Non-goals" section — no LSP, no Maven
awareness yet), but they're the concrete answer for *when* that work starts:
how a real editor structures JDTLS/Kotlin Language Server startup, what a
Gradle/Maven-aware task runner actually needs to detect, how debug adapter
wiring is scoped. Don't reach for them on unrelated work; do read them before
designing the LSP/build-tooling checkpoint from scratch.

Every feature decision has to hold both halves at once:

- **Lightweight**: fast startup, low idle CPU/memory, no needless
  dependencies, no re-parsing or re-querying more than an edit actually
  requires. The one-frame highlighting lag, the incremental tree-sitter
  reparse instead of full reparse, the thin `app` layer over tested
  `core`/`syntax` state — these aren't corners cut, they're the point.
  Before adding a dependency or a background computation, ask whether it's
  earning its weight.
- **Functional**: it still has to be a genuinely usable daily editor, not
  a toy that proves a concept and stops. `FEATURES.md` (gitignored, local
  roadmap doc — regenerate/consult it rather than assuming it's stale)
  tracks the gap between what exists and what a real editor needs. Don't
  read "lightweight" as license to leave that gap unaddressed indefinitely.

Concretely: prefer the smaller/simpler implementation when both options are
equally functional, but don't reach for "lightweight" to justify skipping a
feature that's actually needed for the editor to be useful. When those two
pull in different directions on a specific feature, that's worth surfacing
to the user rather than silently picking one side.

**Respond to user input as fast as possible.** Typing, clicking, and
scrolling must never wait on avoidable work. Concretely: nothing on the
keystroke-to-pixels path should do work that scales with project size (a
full directory re-walk, a full-buffer operation repeated needlessly) or
redo work whose result doesn't change between calls (recompiling a parser
query that's a pure function of `Language`). When you touch a hot path —
anything called from the editor's layouter, from a per-frame `show()`, or
from an input handler — ask whether it's doing more work than the input
that triggered it actually requires. Examples fixed for exactly this
reason: `syntax::highlight_spans` used to recompile its `tree_sitter::Query`
from source on every frame (now cached per language, see
`crates/syntax/src/highlight.rs`); opening a file used to re-walk the whole
project tree from disk (now only genuinely tree-changing actions —
create/rename/delete — trigger that, see `crates/app/src/panels/side_panel.rs`);
`FileNode::build` used to walk *every* directory with no ignore list,
turning a project with a large `.git`/`node_modules`/`target` into a
multi-second scan on open (now skips a fixed list of VCS/build/dependency
directory names outright — see `SKIPPED_DIR_NAMES` in
`crates/core/src/project.rs`); and the editor's layouter used to reshape a
document's entire text from scratch on every frame it was drawn, including
merely switching back to a tab that was already open, because egui's own
shaped-text cache is flushed of anything not painted *that exact frame*
(see the galley-cache gotcha below for the full story — now cached
ourselves in `crates/app/src/widgets/editor/widget.rs`).

## Constraints

- **Do not author commits. Do not push upstream.** (Carried over from
  `CLAUDE.md` — these apply regardless of which agent is working here.)
- **On the rare occasion the user explicitly authorizes a commit anyway,
  the user is the sole author — no `Co-Authored-By` trailer, no mention of
  the agent in the message.** This repo's commit history is attributed to
  the human, full stop, even when an agent typed the diff.
- `PLAN.md`, `SPEC.md`, and `FEATURES.md` are intentionally gitignored
  (local planning docs, not part of the published repo). Don't assume they
  exist in a fresh clone.
- `../references/zed`, `../references/java`, and `../references/kotlin`
  (checked out one level *above* this repo, not inside it) are local
  reference checkouts, not part of this repo at all — nothing about them is
  gitignored *from* this repo because they were never in it to begin with.
  Don't assume they exist in a fresh clone, and don't reference their paths
  from anything that ships with the repo (`README.md`, `LICENSE`, etc.) —
  `AGENTS.md`/`CLAUDE.md` are the right place for pointers to them, since
  those are read by whoever/whatever is actually working in this checkout.

## Commands

```sh
cargo build --workspace        # build everything
cargo test --workspace         # run all tests (core + syntax + foxgarden)
cargo run -p foxgarden         # launch the editor
cargo test -p <core|syntax|foxgarden>  # test a single crate
```

The first build after a clean checkout, `cargo clean`, or a `Cargo.lock`
change is slower than you'd expect from this project's own small size —
`[profile.dev.package."*"]` in the workspace `Cargo.toml` compiles
dependencies optimized even in debug builds (see the gotcha below on why).
That cost is one-time; it doesn't recur on ordinary edit-compile-run
iteration against `core`/`syntax`/`app`.

**Run `cargo sweep --time 14 .` every so often** (needs `cargo install
cargo-sweep` once). Cargo never garbage-collects `target/` on its own —
every dependency-version change (a `cargo add`, a `Cargo.lock` bump)
leaves the *previous* fingerprinted build artifacts behind under
`target/debug/deps`/`target/debug/incremental` alongside the new ones,
unbounded. This is a real, fast-growing cost, not a theoretical one: a
single session's worth of routine dependency churn (five or six `cargo
add`s while landing Track 5's static-analysis integration) grew `target/`
to 34GB before a `cargo clean` reset it. `--time 14` deletes only
artifacts untouched for 14+ days, so it won't disrupt an actively-changing
dependency; a plain `cargo clean` is still the right call for "the crate
graph clearly changed a lot and I just want a guaranteed-clean rebuild
right now" — reach for `sweep` for the routine, no-thinking-required
version of that maintenance instead.

A stable Rust toolchain is required (`rustup` if none is installed). This
project was scaffolded against Rust 1.97 / edition 2024.

## Architecture

**Organize code into files and folders by concern, not into one growing
file per crate.** A file earns a split into its own module once it's doing
one identifiable thing (a widget's pure edit logic, its painting, its
event-classification helpers); a cluster of related modules earns a folder
once there's more than one of them (see `crates/app/src/widgets/`,
`panels/`, `style/` below). Don't over-fragment a small, cohesive file just
to have more files, though — folder-per-single-file adds path depth with no
organizational benefit, which cuts against the lightweight principle above.

Three-crate workspace, dependency direction is strict:

```
core  <-  syntax  <-  app
```

A fourth crate, **`crates/test-support`**, sits outside that chain as a
`[dev-dependencies]`-only leaf: `core`, `syntax` (once it needs one), and
`app` each depend on it purely for shared test fixtures (`temp_file`/
`temp_document`/`write_file` — a temp directory plus a file, or an opened
`Document`, ready for a test to act on). It normally-depends on `fg-core`
itself (to return `Document`s) — a `core` -> `test-support` dev-dependency
alongside a `test-support` -> `core` normal dependency is a cycle, but
Cargo explicitly allows dev-dependency cycles specifically because they
only ever matter when building/testing the crate on the near end, never
when it's pulled in as a library. Add a new fixture helper here, not as
another per-file `fn open_fixture(...)`/`fn java_file(...)` — that
duplication (identical helpers independently reimplemented in `core` and
`app` before this crate existed) is what it exists to stop from
recurring.

- **`crates/core`** (package name `fg-core`, crate name `fg_core` — named to
  avoid colliding with Rust's built-in `core` crate): `Document`, `Project`/
  `FileNode`, `EditorState`, `Diagnostic`. No `egui` or `tree-sitter`
  dependency — everything here is unit-testable headless. Dirty state is
  *derived* (`buffer != saved_buffer`), never a manually toggled flag.
  `Project::open`/`FileNode::build` sort directories before files
  (alphabetically within each group) and skip a fixed denylist of
  VCS/build/dependency directory names (`SKIPPED_DIR_NAMES`) without ever
  `read_dir`-ing into them. `Document::open` accepts any file — no
  extension allowlist — but sniffs the first 8KB for a NUL byte and rejects
  binaries (`OpenDocumentError::Binary`) before attempting a full
  `read_to_string`, since the tree has no ignore list for non-source files
  and a click on a large binary shouldn't block the UI thread reading it.
- **`crates/syntax`**: wraps tree-sitter. `IncrementalParser` owns a
  `tree_sitter::Parser` + cached `Tree` per document. `highlight_spans()` and
  `syntax_errors()` walk/query that tree — both fully generic over
  `Language` (Java, Kotlin, YAML, XML, `.properties`); adding a language is
  `fg_core::Language` + `Language::from_extension`, a `ts_language`/
  `highlights_query_source` arm in `crates/syntax/src/language.rs`, and a
  `cached_query` cell in `highlight.rs` — nothing about parsing, diagnostics,
  or the editor widget is Java/Kotlin-specific. `diff_edit(old, new)`
  computes an `InputEdit` from two full-text snapshots by common-prefix/
  suffix diffing — needed because egui's `TextEdit` hands back a plain
  `String`, not a structured edit op.
- **`crates/app`**: eframe/egui shell, organized by concern into
  `src/widgets/`, `src/panels/`, `src/style/`, plus `app.rs`/`main.rs`/
  `terminal.rs` at the root:
  - `app.rs` owns `EditorState` plus a `parsers: Vec<Option<IncrementalParser>>`
    kept **index-aligned** with `state.open_tabs` — every tab open/close
    must update both in lockstep, or the wrong parser (or lack of one) ends
    up attached to the wrong document. A slot is `None` for a document with
    no recognized language (any file that isn't `.java`/`.kt` still opens
    and edits fine, it just gets no parser/highlighting/diagnostics).
    `panels::tabs::open_parser_for` is the one shared helper for creating
    (or correctly omitting) a tab's parser — new file opens, `Ctrl+Shift+T`
    reopen of a closed tab, and a rename that changes a file's language (or
    clears it) all funnel through it. `EditorState::closed_tabs` is the
    LIFO stack `close_tab`/`reopen_last_closed_tab` push/pop to make reopen
    possible. Session persistence (`restore_session`/`persist_session` in
    `app.rs`) round-trips the project folder, every open tab's path (in tab
    order), and which one was focused through `eframe::Storage` as plain
    newline-joined strings — no `serde` dependency for just a list of paths.
    Both are free functions taking `&dyn eframe::Storage` rather than
    methods on `FoxGardenApp`, specifically so they're unit-testable against
    a hand-rolled fake `Storage` (see `app/tests.rs`) — a real
    `eframe::CreationContext` isn't practically constructible outside a live
    windowing/render backend, so `FoxGardenApp::new` itself stays untested,
    same as the rest of its GUI-wiring functions.
  - `widgets/editor/` is the custom highlighted/squiggled text widget
    (SPEC.md §5.4–5.5), split one file per identifiable concern:
    - `widget.rs`: the `show()` entry point and its `TextEdit` wiring
      (including the line-number gutter, laid out via a `ui.horizontal`
      with the gutter's width reserved before `TextEdit` is shown), plus
      every interception block that has to run *before* `TextEdit::show()`
      to compete with its own default key handling (Tab, Alt+Arrow, Home,
      Ctrl+/, wrap-selection, multi-cursor). This is the one file in the
      module that's allowed to stay large — it's genuinely one thing
      (orchestrating the widget's single frame), not several things that
      happen to be adjacent; splitting it further would mean breaking
      apart blocks that all share the same `text`/`old_text`/
      `manual_cursor_range` threading, which is a worse trade than the
      length.
    - `auto_edit.rs`: pure auto-pair/auto-indent/join-lines/
      case-conversion/line-comment/move-and-duplicate-line text
      transforms — no `egui` types, just `&str` in, `(String, ...)` out.
    - `text_offset.rs`: `char_to_byte`/`byte_to_char`, the byte↔char offset
      conversion every other file in this module needs (a syntax tree only
      ever deals in bytes; egui's cursor API only ever deals in chars).
      Promoted out of `auto_edit.rs` once a second and third file started
      importing from it for something with nothing to do with auto-pair/
      auto-indent — the tell that a helper has outgrown the file it was
      born in: something reused *across* files, with zero domain
      knowledge of any one of them, is exactly what earns its own file
      (see `crates/test-support` below for the same principle one level
      up, across whole crates).
    - `painting.rs`: diagnostic squiggles + hover tooltips, multi-cursor
      overlay, and the line-number gutter's own paint call, which reads
      row positions straight off the same `Galley` `TextEdit` renders
      rather than recomputing them from font metrics — see
      `paint_line_numbers`.
    - `multi_cursor.rs`: Ctrl+D's pure search/edit logic — see the gotchas
      below for why this can't just use egui's `TextEdit` directly.
    - `codegen.rs`: Java getters/setters generation end to end —
      `syntax::java_classes_with_fields` finds every eligible class in the
      whole file, `GenerateAccessorsDialog` is the picker's state when
      more than one qualifies, and `show_generate_accessors_dialog`
      renders it (lives here, not in `widget.rs`, precisely so this
      feature's data/logic/rendering aren't split across files for no
      reason).
    - `templates.rs`: live-template trigger-word expansion.
    - `context_menu.rs`: the right-click menu (Undo/Redo/Cut/Copy/Paste/
      Select All/Toggle Line Comment/Duplicate Line/Save) — see the
      `pending_input` gotcha below for why Undo/Redo/Select All are
      queued rather than applied directly, unlike everything else here.

    Only `widgets::editor::show`, `AccessorKind`, `GenerateAccessorsDialog`,
    and `CaseConversion` are public outside the module.
  - `widgets/modal.rs` (one level up, not inside `editor/`, since it's
    shared by both the editor widget and the panels below it): the
    `show_modal` helper every confirm/about/error/picker dialog in this
    app is built on — including its `Escape`-closes-the-topmost-modal
    behavior, which every caller must apply itself (see the return value's
    doc comment).
  - `panels/` is the surrounding UI chrome built from that widget:
    `menu_bar.rs`, `side_panel.rs` (project tree + file create/rename/
    delete), `tabs.rs` (tab bar + the parser-lifecycle helper above, plus
    `save_document` — the save-then-reparse sequence shared by `Ctrl+S`,
    the close-confirmation modal's Save button, and the editor's
    right-click Save), `quick_switcher.rs` (`Ctrl+E`'s recent-files popup).
  - `style/` is cross-cutting presentation: `fonts.rs` (`EditorFont`
    selection, JetBrains Mono registration), `theme.rs` (light/dark color
    palette), `indent.rs` (tabs-vs-spaces + width settings).
  - `terminal.rs` (crate root, not under `panels/` — it's system
    integration the side panel's "Open Terminal" button calls into, not UI
    chrome itself): opens a terminal window at a given directory. The one
    place in the app that branches on `target_os` — see the cross-platform
    gotcha below before touching it.

## Non-obvious gotchas (learned the hard way this session)

- **egui 0.35's `App` trait is not the "classic" egui API you'll find in most
  tutorials/training data.** `eframe::App::ui` takes `&mut egui::Ui` directly
  — there is no `fn update(&mut self, ctx: &Context, frame: &mut Frame)`
  anymore. Likewise `egui::SidePanel` no longer exists as its own type; use
  the unified `egui::Panel::left(id)` / `::right(id)` / `::top(id)` /
  `::bottom(id)`, and `.show(ui, ...)` (not `.show_inside`, which is
  deprecated). `CentralPanel` is unchanged. If something from an older egui
  example doesn't compile, check `~/.cargo/registry/src/*/egui-<version>/src/`
  directly rather than trusting memorized API shape.
- **Kotlin tree-sitter grammar crate**: plain `tree-sitter-kotlin` (0.3.x)
  only supports `tree-sitter` 0.21–0.22 and conflicts (via Cargo's `links =
  "tree-sitter"` uniqueness rule) with `tree-sitter-java` 0.23+, which needs
  `tree-sitter` 0.26. Use `tree-sitter-kotlin-ng` instead — it tracks current
  `tree-sitter`. It also ships **no bundled highlight query**
  (`queries/highlights_kotlin.scm` here is hand-written); Java's grammar
  crate does bundle one (`tree_sitter_java::HIGHLIGHTS_QUERY`).
- **`QueryCursor::captures` returns a `StreamingIterator`**, not a normal
  `Iterator` — `use tree_sitter::StreamingIterator` and call `.next()` in a
  `while let` loop, not a `for` loop.
- **One-frame highlighting lag is expected and fine.** The editor widget's
  layouter runs against the tree from *before* the current frame's edit
  (reparsing happens after `response.changed()` is known). Highlighting and
  squiggles catch up on the next frame. Don't try to eliminate this lag; it's
  the standard immediate-mode-editor tradeoff, and SPEC.md's "recomputed on
  re-parse" language already assumes it.
- **New tabs must get their initial diagnostics computed at open time**, not
  only on first edit — `Document::open` never runs `syntax_errors` itself
  (core has no `syntax` dependency), so `app::open_parser_for` does it after
  the first `parser.parse(...)`. Skipping this means a file opened with a
  pre-existing syntax error shows no squiggle until the user types something
  (a real bug caught via a screenshot smoke test during initial
  implementation — easy to reintroduce if this helper is bypassed).
- **Bracket/quote auto-close cannot be implemented by diffing
  `old_text`/`text` full-buffer snapshots** — a prefix/suffix diff of the two
  strings is ambiguous in exactly the case it needs to detect: typing a
  closer immediately before an identical existing one (e.g. `(a|)` -> type
  `)`) produces the same two strings as appending a new `)` at the end,
  so the diff can't tell them apart. `widgets::editor::auto_edit::apply_auto_pair` instead
  uses egui's real post-edit cursor position (`TextEditOutput.cursor_range`)
  to locate the just-typed character unambiguously. `syntax::diff_edit` is
  still used afterward, but only to feed the *already-corrected* text to
  `IncrementalParser::reparse` — not to detect the correction itself.
- **Auto-pair and auto-indent insert on opposite sides of the cursor, and
  only one of them needs manual cursor correction because of it.** Auto-pair
  (`apply_auto_pair`) always inserts its closer *after* the cursor's current
  char-index, so that index stays valid without touching anything else.
  Auto-indent (`apply_auto_indent`) inserts whitespace *before* where egui
  already placed the cursor (right after the newline), so skipping the
  cursor fix-up there would leave the cursor sitting before the
  auto-inserted indentation instead of after it. `widgets::editor::show` fixes
  this via `output.state.cursor.set_char_range(...)` +
  `output.state.store(ui.ctx(), id)` — the exact pattern from egui's own
  `TextEditState` doc example. If you add another edit-time correction,
  check which side of the cursor it inserts on before assuming either
  approach (or lack of one) carries over.
- **eframe's file-backed persistence stores a plain `HashMap<String, String>`
  as RON, which means `{ "key": "value" }` (map/brace syntax), not
  `("key": "value")` (RON's tuple/struct syntax) — easy to get backwards when
  hand-editing or seeding `~/.local/share/foxgarden/app.ron` for testing.
  Wrong syntax fails silently: `ron::de::from_reader` errors are swallowed
  and treated as "no stored value" rather than a visible error, so a bad seed
  file just looks like persistence isn't working at all, with no panic or
  log to point at the real cause.
- **All icon files live under `crates/app/assets/icon/`** — the pristine
  1024x1024 source (`icon.png`) and the small bundled copy actually embedded
  by `main.rs`'s `ICON_PNG` (`icon_128.png`, downscaled further from 256px
  after the 256px version still didn't confirm-fix the issue below).
  Regenerate the small copy via `convert crates/app/assets/icon/icon.png
  -resize 128x128 crates/app/assets/icon/icon_128.png` if the source art
  changes — never point `include_bytes!` at the 1024px source directly.
- **The window icon fix (bundling a small copy instead of the 1024px
  source) is confirmed working.** winit's X11 backend writes the icon via
  `_NET_WM_ICON` (an X property holding raw ARGB pixels), and the call to
  set it is wrapped in `.ignore_error()` on the egui-winit side — so if the
  property write fails (plausible for a 1024x1024 source, ~4MB once
  expanded to ARGB), the window silently falls back to the WM's default
  icon with **no error, panic, or log anywhere**. `xprop -id <window>
  _NET_WM_ICON` came back empty both before *and after* switching to a
  resized copy in this sandbox's nested X11 setup, so the size theory
  couldn't be verified there — but it's since been confirmed fixed on a
  real (non-sandboxed) desktop, so the 128px bundling is the right call,
  not a no-op that happened to coincide with an unrelated fix.
- **A `grammar.js` literal string is not proof that `tree_sitter::Query` can
  match it.** `queries/highlights_kotlin.scm` originally listed `"break"`,
  `"continue"`, and `"reified"` as keyword tokens — all three appear as
  literal strings in `tree-sitter-kotlin-ng`'s `grammar.js` source, but none
  of them survive as matchable node types in the grammar crate's *compiled*
  parser (no corresponding entry in `node-types.json` either). `Query::new`
  panicked with "Invalid node type" the first time the app actually rendered
  a `.kt` file — none of the existing tests caught it because
  `highlight_spans_cover_expected_keyword_string_comment_ranges` only
  exercised Java. If you touch either language's highlight query, bisect
  candidate tokens individually through `tree_sitter::Query::new(&lang,
  "(\"token\")")` rather than trusting the grammar source, and make sure
  `crates/syntax/tests/syntax_tests.rs` has a `highlight_spans` test for
  *each* language, not just one.
- **A single node can match more than one capture pattern in a bundled
  highlight query — `highlight_spans` must resolve that itself, not return
  duplicates and hope the caller sorts it out.** YAML's `highlights.scm`
  captures every `(string_scalar)` node generically as `@string`, and
  *separately, later in the file*, captures the same node as `@property`
  specifically when it's a mapping key — so an unquoted key produces two
  captures spanning the identical byte range. `highlight_spans` used to
  push both into its result unfiltered; `widgets::editor::show`'s layouter
  then painted whichever one came first when consuming the sorted list,
  which was `@string` (declared earlier, so yielded first for the tied
  range) — meaning every YAML/properties key rendered in the *string*
  color instead of `Property`, invisibly, because a membership-style test
  (`spans.iter().any(|(r, s)| ...)`) can't tell "the right scope is present
  somewhere in the list" apart from "the right scope is what actually gets
  painted." Fixed by deduplicating in `highlight_spans` itself — a
  `HashMap<Range<usize>, Scope>` insert per capture keeps the *last* one
  seen for an exact-duplicate range, matching the standard
  tree-sitter-highlight convention that a later pattern in the query file
  takes priority over an earlier, more general one. If you add another
  language, don't just check that the right scope *appears* in
  `highlight_spans`'s output for a given range — check it's the *only* one
  for that exact range (see `yaml_highlight_query_covers_mapping_key_
  string_and_comment`'s regression-guard assertions in
  `crates/syntax/tests/syntax_tests.rs` for the pattern), or the same class
  of bug can reappear silently.
- **JetBrains Mono is registered under a custom `FontFamily::Name(...)`, not
  merged into `FontFamily::Monospace`** (see `style/fonts.rs`), so both it
  and egui's built-in monospace font (Hack) stay independently selectable
  via `EditorFont`. This means `egui::__run_test_ui` — which runs against a
  fresh `Context` with an *empty* `FontDefinitions` (no families registered
  at all beyond the hardcoded `Monospace`/`Proportional` slots) — panics
  with `"FontFamily::Name(\"JetBrainsMono\") is not bound to any fonts"` if
  a test passes `EditorFont::JetBrainsMono` to `widgets::editor::show`.
  Tests must pass `EditorFont::Default` instead; only real `main()` (via
  `style::fonts::install`) registers the custom family.
- **egui 0.35's `TextEdit` has zero multi-cursor support and no hook to
  intercept key events before its own single-cursor logic runs** (the
  `events()` fn that does this in `builder.rs` is private). `Ctrl+D`
  multi-cursor is layered on top instead: `Document::extra_selections`
  tracks secondary cursors ourselves, and `widgets::editor::show` pulls
  mutating events (`Text`, `Paste`, `Backspace`/`Delete`/`Enter`) out of
  `ui.input_mut(|i| i.events...)` *before* calling `TextEdit::show` whenever
  extras are active, so egui's own handler never sees them and can't
  double-edit the primary cursor — `multi_cursor::apply_multi_edit` then
  replays the same op at every active cursor in one pass. See
  `crates/app/src/widgets/editor/multi_cursor.rs` and the wiring in
  `widgets::editor::show` (in `widget.rs`).
- **`TextEditState::store(self, ...)` takes `self` by value, not `&self`**
  — it can only be called once per frame per widget. Calling it from more
  than one branch (e.g. once for a Ctrl+D word-selection jump, again at the
  end for auto-indent's cursor fix-up) fails to compile with "borrow of
  moved value". `widgets::editor::show` instead threads a single
  `manual_cursor_range: Option<CCursorRange>` through every branch that
  wants to override the cursor, and calls `set_char_range` + `store` exactly
  once at the very end.
- **Every block in `widgets::editor::show` that edits the buffer must
  refresh its local `text`/`old_text` right after, not just call
  `apply_edit` and move on** — `apply_edit` updates `doc.buffer`, but
  `text`/`old_text` are separate local `String`s computed once near the
  top of the function; any later block that reads them (a subsequent
  feature check, the trailing `manual_cursor_range` apply, right-click
  menu items rendered near the end) sees stale content for the rest of
  *that* frame otherwise. This bit for real with the right-click menu's
  Save button: `Document::save` trims trailing whitespace as a side
  effect, changing `doc.buffer`'s length, but the button only called
  `tabs::save_document` and moved on — self-healed by the next frame (same
  class of one-frame lag as the highlighting-catch-up gotcha above), but
  it broke the invariant every *other* block in this function already
  follows for no good reason. Fixed by adding the same
  `old_text = saved_text.clone(); text = saved_text;` refresh every edit
  path already does. If you add a new block here that can change the
  buffer — including indirectly, like a save-time formatter would — refresh
  both afterward.
- **`TextEdit::id_salt(salt)` does not hash `salt` directly into the widget
  id — it combines it with whichever `Ui` calls `.show()`, so the id is
  *not* actually independent of where in the ui tree the widget renders,
  despite that being the whole point of using a stable salt.** Internally,
  `.id_salt(salt)` wraps it in an `egui::IdSalt` and combines that with the
  `Ui`'s own id via `ui.make_persistent_id(id_salt)` — meaning the same
  salt string produces a *different* final id if the widget ends up shown
  through a different nested `Ui` than before. This bit for real: adding
  the line-number gutter wrapped `TextEdit::show` one level deeper inside a
  `ui.horizontal(|ui| ...)` closure, and every test relying on
  `focused_frame`'s simulated focus broke — `request_focus` was targeting
  the *old* id, computed against the outer `Ui`, while the real widget now
  resolved to a different id via the inner one. `widgets::editor::show` now
  sets the id via `.id(egui::Id::new(id_salt))` instead — `Id::new` is a
  pure hash of the salt with no `Ui` involved at all, so it's stable
  regardless of internal layout changes, which is what "tied to the
  document, not to where `show` is called from" actually requires. Prefer
  `.id(Id::new(...))` over `.id_salt(...)` for any widget whose identity
  needs to survive its own internal restructuring, not just movement by its
  caller. A test that wants to `ui.memory_mut(|m| m.request_focus(id))` on
  such a widget just computes `egui::Id::new(salt)` directly — no
  `ui.make_persistent_id` wrapping needed, since there's no `Ui` in the
  computation to replicate. This is also how `widgets::editor::show`'s
  tests simulate a *focused* keyboard event at all: `TextEditOutput.
  cursor_range` is only `Some` when `ui.memory(|m| m.has_focus(id))` is
  true (see `focused_frame` in `widget.rs`'s tests).
- **egui/epaint's own shaped-text (`Galley`) cache is flushed of anything not
  painted *this exact frame*, every frame** (`GalleyCache::flush_cache` in
  `epaint::text::fonts` — `self.cache.retain(|_, c| c.last_used ==
  current_generation)`). Since only the active tab's editor renders each
  frame, this means an inactive tab's shaped galley is gone by the very next
  frame no matter how recently it was visible — switching back to a tab
  that's merely been sitting open (not edited, not even scrolled) forces a
  full reshape of its entire buffer from scratch, because `egui::TextEdit`
  lays out the whole document in one `LayoutJob`, not just the visible
  lines. The fix (`widget.rs`'s layouter) keeps its own `CachedLayout` in
  `egui::Context`'s persistent temp storage (`ctx.data()`/`ctx.data_mut()`,
  keyed by a `LayoutCacheKey` derived from content hash + language + theme +
  wrap width), which is *not* subject to that per-frame flush — it survives
  tab switches and only gets replaced when the key actually changes. This
  only fixes the tab-switch case; a huge file's *first* open or *every*
  keystroke still pays the full-document layout cost, since that requires
  actual viewport virtualization (see `FEATURES.md`'s "Large file handling"
  entry) — a much bigger change than this cache.
- **Debug builds leave every dependency fully unoptimized, including
  math/shaping-heavy libraries you never actually debug.** Opening a file
  with a lot of distinct glyphs shown at once (e.g. a real-world `pom.xml`'s
  dependency names and version numbers) measured ~750ms in a plain `cargo
  build`/`cargo test` debug build vs. ~25ms with `--release` — almost
  entirely inside egui/epaint's text-shaping engine (`harfrust`), not
  FoxGarden's own code, which is microseconds either way regardless of
  profile. Fixed via a `[profile.dev.package."*"]` `opt-level = 2` override
  in the workspace `Cargo.toml` — this only affects dependencies (workspace
  crates stay unoptimized under plain `[profile.dev]`, so normal
  edit-compile-run iteration doesn't slow down), at the cost of a slower
  *first* build after a `cargo clean` or a `Cargo.lock` change, since every
  dependency has to be recompiled at the new opt level once. If a
  performance complaint doesn't reproduce in `--release` but does in a plain
  debug build, suspect this class of issue before assuming an algorithmic
  problem in our own code — measure both before concluding which it is.
- **There is no cross-platform "open a terminal here" API, and no single
  standard terminal emulator even within Linux desktops.** `terminal.rs`
  branches on `target_os`: Windows goes through `cmd /C start "" /D <dir>
  cmd` (`start` is a `cmd.exe` builtin, not its own executable, and needs
  an explicit empty-string title argument before `/D` or it misparses a
  spaced path as the title); macOS goes through `open -a Terminal <dir>`
  (documented, no AppleScript needed); everything else tries a fixed list
  of common terminal binaries (`LINUX_TERMINAL_CANDIDATES`) in order via
  plain `Command::new(name).current_dir(dir).spawn()`, taking whichever one
  is actually installed — no per-terminal `--working-directory`-shaped flag
  needed, since every one of them starts its default shell inheriting the
  *terminal binary's own* working directory rather than resetting it.
  Everything else in this codebase already runs on Windows/macOS/Linux
  without special-casing — `Path`/`PathBuf` (including `.join("a/b")` with
  a forward slash literal, which Rust resolves correctly on Windows too),
  `std::fs`, and every dependency in use (`egui`/`eframe`, `rfd`, `ropey`,
  `tree-sitter`, `arboard`) are already cross-platform by design — so
  `terminal.rs` is the *only* place that needs to know which OS it's on.
  Keep it that way: reach for a portable `std`/existing-dependency API
  before adding a second `#[cfg(target_os = ...)]` site anywhere else in
  the app.

## Testing conventions

- `core` and `syntax` are fully headless-testable; prefer adding coverage
  there over the `app` crate when the logic doesn't strictly need a GUI.
- `app` is a binary crate (no `[lib]` target), so its tests live as
  `#[cfg(test)] mod tests` colocated with the code they cover, not under
  `tests/` (integration tests there can't `use` binary-crate internals). A
  small file keeps that module inline at the bottom; once a file grows large,
  the module moves to a sibling `<module>/tests.rs` file (the module file just
  declares `#[cfg(test)] mod tests;`) so it stays focused on the code under
  test — e.g. `widget.rs`'s tests are in `widget/tests.rs`, `app.rs`'s in
  `app/tests.rs`, `side_panel.rs`'s in `side_panel/tests.rs`. Same module,
  separate file; `use super::*;` reaches the code exactly as an inline module
  would.
- GUI widget logic (highlighting, squiggle painting) can be exercised
  headlessly via `egui::__run_test_ui(|ui| { ... })` — it runs a real egui
  frame without needing a window, so panics/layout bugs surface in `cargo
  test` without any display or click-automation tooling. See
  `crates/app/src/widgets/editor/widget/tests.rs` for the pattern.
- **Do not drive the running app with `xdotool`/`wmctrl`/`import` (or any
  other click-automation tooling) — that approach has repeatedly produced
  false reads** (a screenshot racing the app's own redraw and showing stale
  pixels, a click landing on the wrong window/tab because focus wasn't what
  it looked like, timing that "worked" on one pass and silently didn't on
  the next). It cost real turns chasing phantom failures caused by the
  automation itself, not the feature under test. Instead: build the change,
  run `cargo build`/`cargo test`/`cargo clippy`, then hand the live
  click-through back to the user as **explicit, numbered steps** — exactly
  what to run (`cargo run -p foxgarden`, plus any fixture file/project to
  open), exactly what to click/type in what order, and exactly what result
  confirms the feature works vs. what would indicate it's broken — and wait
  for them to report back what actually happened before claiming the
  checkpoint passed. This is slower per checkpoint but the result is
  trustworthy, which a flaky automated click never reliably was here.
  A native file-picker dialog (`rfd`, "Open Folder…") is a separate OS
  dialog outside egui's own event loop and likely needs different handling
  than in-app widgets — worth flagging to the user in the steps if the
  scenario touches one. Either way, don't claim a mouse-driven flow was
  verified without the user actually having driven it and reported back.
- Anything that needs `eframe::Storage` (session persistence) or
  `egui::Context`'s persistent temp data (the layout cache) is testable
  without a real window: implement `eframe::Storage` yourself over a plain
  `HashMap` (`FakeStorage` in `app/tests.rs`) for the former; for
  the latter, drive a real, reused `egui::Context` through two or more
  `ctx.run_ui(...)` passes and read `ctx.data(|d| d.get_temp::<T>(id))`
  directly (see `widget/tests.rs`'s `layout_cache_reuses_galley_across_
  unchanged_frames` / `_reshapes_after_an_edit`, which assert `Arc::ptr_eq`
  to prove a galley either was or wasn't reused across frames). Neither
  needs a real `eframe::CreationContext`, which isn't practically
  constructible in a unit test.
- When chasing a specific "this is slow" report, measure the actual
  operation with real data before proposing a fix — a plausible-sounding
  theory (file size, project size) can be wrong even when a real
  performance bug exists nearby. `std::time::Instant` timing directly in a
  throwaway `#[ignore]`d test (or a temporary `examples/` binary, deleted
  once it's done its job) against the user's real file/project, comparing
  debug vs. `--release` and a "warm-up" frame vs. the frame under
  suspicion, is what actually distinguished "the project tree walk is slow"
  from "this one file's first render is slow" from "debug builds are slow"
  in this session — each looked similar from the outside but had a
  different root cause and fix.
