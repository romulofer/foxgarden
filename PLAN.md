# PLAN.md

Execution plan for every track `SPEC.md` covers — every `FEATURES.md`
`[TODO]`/`[SKIP]` entry, plus the remaining scope of its `[WIP]` ones.
Replaces this file's previous single-purpose scope (Spring endpoint map +
terminal panel); both are shipped — see this file's own prior build-status
history in git, and `FEATURES.md`'s Shipped section — and everything below
is what's left. Local-only planning doc (tracked on `ide-henshin`, same as
`SPEC.md`) — a commitment to an _order_, not a timeline.

**22 tracks**, grouped into the same three tiers, ordered easiest-to-
hardest within each tier as `FEATURES.md` itself orders them. Track
numbers match `SPEC.md`'s own section numbers and are **not** renumbered
after trimming — a track's number is a stable id, not a position, so a
removed track just leaves a gap rather than shifting every later one. A
track's own phases are independent of every _other_ track's phases unless
a dependency is named explicitly (mirroring the previous pass's own
"Phases 0/1 don't depend on each other" convention) — most tracks here
are fully independent of each other and can be picked up in any order or
in parallel across sessions; the explicit dependency notes below (e.g.
`Spring config autocomplete` needs `Maven/Gradle awareness` first) are the
exceptions, not the rule.

Same checkpoint discipline throughout: `cargo build --workspace`, `cargo
test --workspace`, `cargo clippy --workspace --all-targets` all green,
plus a live click-through for anything with a UI-facing surface, per
`AGENTS.md`'s testing conventions — build it, get the automated checkpoint
green, then hand the user exact numbered steps and wait for them to
report back, never claim a click-through passed without that.

**Cross-track dependency graph** (only the tracks with a real dependency
on another track are shown; everything else is independent):

```
Track 21  Maven/Gradle awareness
    │
    ├──► Track 12  Spring config property autocomplete
    ├──► Track 20  LSP integration (classpath feeds jdtls's own init config)
    └──► Track 27  DI/bean graph visualizer ◄── Track 20 (both needed)

Track 22  Build/run/test integration
    │
    ├──► Track 13  Code coverage overlay
    ├──► Track 23  Debugger
    ├──► Track 26  Profiler integration
    └──► Track 14  Docker/container run integration (shares its output-panel infra)

Track 20  LSP integration
    │
    ├──► Track 15  Quick-fix intention actions
    ├──► Track 17  Peek definition
    └──► Track 27  DI/bean graph visualizer ◄── Track 21 (both needed)

Track 18  Inline diff viewer widget
    │
    └──► Track 4   Local (non-git) file history (its own revert-diff view)

Track 19  Large file handling — full virtualization
    │
    └──► Track 10  Code folding (re-verify this dependency before treating
                    it as hard — see Track 10's own Phase 1)
```

---

# Moderate tier

## Track 1 — Multi-select in the tree

**Phase 1 — selection state + gestures.** `side_panel.rs`'s
`SidePanelState` gains `selected: HashSet<PathBuf>`; Ctrl+Click
toggles a node, Shift+Click selects the contiguous visual range from the
last click, a plain click collapses back to single-selection.

**Checkpoint 1:** full suite green; live-verify Ctrl+Click builds up a
multi-selection, Shift+Click extends a range, a plain click clears it.

**Phase 2 — batch actions.** Delete confirms once for the whole set
("Delete N items?"); Cut/Copy serialize every selected path. Rename/"New
File" stay disabled (or hidden) in the context menu whenever more than one
node is selected.

**Checkpoint 2:** full suite green; live-verify a multi-selected batch
delete, a multi-selected cut-then-paste round-trip, and that Rename is
unavailable with more than one node selected.

---

## Track 2 — Richer Java/Kotlin syntax highlighting

**Phase 1 — Java: parameters, operators, labels, doc comments.** New
`Scope` variant(s) per `SPEC.md` §2 (resolve the parameter-vs-`Scope::
Property` design question there, not before); grammar shapes verified
fresh against real `tree-sitter-java` parse output before writing each
query (`TECHNICAL_DEBT.md` #3's own discipline); a color added to both of
`theme.rs`'s light/dark tables per new variant. Table tests per
distinction, one commit-sized change at a time (the feature's own
"ongoing, one distinction at a time by design" framing, not a single
sweeping rewrite).

**Checkpoint 1:** `cargo test -p syntax` green per distinction landed —
done: `Scope::Parameter`/`Operator`/`Label`/`DocComment` all landed with
table tests against the extended `valid.java` fixture.

**Phase 2 — Kotlin: modifiers, regex literals, `it`/`field`.** Same
discipline, against `tree-sitter-kotlin-ng`'s real parse output (never
Zed's own reference query, which targets a different grammar —
`TECHNICAL_DEBT.md` #3's own established warning). Regex-literal detection
specifically needs a first checked look at whether tree-sitter can
distinguish it from an ordinary string-call at all before committing to
building it — flag and skip rather than force a syntactic answer to what
may be a semantic-only distinction.

**Checkpoint 2:** `cargo test -p syntax` green per distinction landed —
done: modifier-keyword coverage turned out already complete (verified
against `grammar.js` directly, nothing to add); regex-literal detection
confirmed no dedicated grammar node exists and was skipped per this
phase's own instruction; `it`/`field` landed, reusing `Scope::Keyword`.

---

## Track 4 — Local (non-git) file history

Depends on Track 18 (`Inline diff viewer widget`) for its own revert-diff
view — land that track first, or this one's Phase 2 duplicates its own
diff rendering, which the whole point of Track 18 existing is to avoid.

**Phase 1 — snapshot on save.** Every `Document::save` additionally writes
the pre-save buffer into `.foxgarden/history/<relative path>/
<timestamp>.snapshot`; a per-file cap (e.g. 50) prunes the oldest beyond
it.

**Checkpoint 1:** `cargo test -p fg-core`/`-p app` green (a temp-project
fixture asserting snapshots accumulate and prune correctly); no live
click-through needed yet (no UI reads them).

**Phase 2 — history UI + revert.** Tab context-menu "File History…" lists
snapshots (timestamp + diff-stat); selecting one shows Track 18's diff
widget (snapshot vs. live buffer); "Revert to this version" replaces the
live buffer through the normal edit path.

**Checkpoint 2:** full suite green; live-verify saving a file several
times populates history, opening it shows a real diff against each past
version, and Revert actually restores that version's content (undoably,
via a normal Ctrl+Z afterward).

---

## Track 5 — Static analysis integration

**Phase 1 — shared plumbing + Checkstyle.** Settings > External Tools gains
a path field per tool; Tools > "Run Checkstyle" shells out against the
project root, parses its XML report, converts each finding into the
existing `Diagnostic` shape feeding the current squiggle pipeline.

**Checkpoint 1:** full suite green (a fixture Checkstyle XML report parsed
into expected `Diagnostic`s, headless); live-verify running Checkstyle
against a real project with a known violation shows a squiggle at the
right line.

**Phase 2 — PMD.** Same shape, PMD's own XML report format.

**Checkpoint 2:** same as above, PMD-specific fixture + live-verify.

**Phase 3 — SpotBugs.** Same shape, SpotBugs' own XML schema (bytecode-
based — verify it reports source line numbers accurately enough to map
back to a `Diagnostic` range before assuming parity with the other two).

**Checkpoint 3:** same as above, SpotBugs-specific fixture + live-verify.

---

## Track 6 — Auto-save

**Phase 1 — settings + triggers.** Settings > Auto-save toggle (off by
default) with "on focus loss" / "after N seconds idle" modes, both calling
the existing `Document::save` unchanged.

**Checkpoint 1:** full suite green (a fake-clock/fake-focus-event test
asserting the right trigger fires `save` at the right moment); live-verify
both modes against a real dirty tab.

**Phase 2 — conflict-banner interaction.** Auto-save suppressed for any
tab currently showing the "changed on disk" banner; resumes once
Reload/Keep Mine resolves it.

**Checkpoint 2:** full suite green; live-verify auto-save does _not_ fire
while the conflict banner is showing, and does resume normally after
resolving it.

---

## Track 7 — Rectangular (block) paste

**Phase 1 — column/block selection.** `Alt`+drag produces a
`BlockSelection { start_line, end_line, start_col, end_col }`; `text_area`
gains a second highlight-painting path for it alongside the existing
linear-range one.

**Checkpoint 1:** full suite green; live-verify `Alt`+drag visibly
highlights a rectangular region across several lines.

**Phase 2 — block-scoped editing.** Typing/Backspace/Delete over an
active block selection applies the same column-range edit to every row
the block spans.

**Checkpoint 2:** full suite green; live-verify typing over a block
selection edits every spanned row identically, Backspace/Delete likewise.

**Phase 3 — block paste.** Clipboard text split on `\n`, row _i_ inserted
at `(start_line + i, start_col)`; a row-count mismatch (fewer/more
clipboard lines than the block spans) leaves the surplus/shortfall
untouched rather than wrapping or clearing.

**Checkpoint 3:** full suite green (table tests for exact-match,
fewer-lines, and more-lines cases); live-verify a real block-select →
copy → block-paste round-trip.

---

# Substantial tier

## Track 9 — Git diff gutter, inline blame, commit/stage/push UI

**Phase 1 — diff gutter.** `git diff --no-color -U0` per open/save/
reload, hunk headers parsed into added/removed/modified line ranges,
painted alongside the line-number gutter.

**Checkpoint 1:** full suite green (a fixture diff-output string parsed
into expected ranges, headless); live-verify editing a tracked file shows
the right gutter marks against a real git repo.

**Phase 2 — inline blame.** `git blame --porcelain` parsed per line,
shown as a dimmed cursor-line annotation.

**Checkpoint 2:** full suite green; live-verify the annotation updates as
the cursor moves between lines with different blame authors/dates.

**Phase 3 — stage/commit panel.** A dockable panel listing `git status
--porcelain` as a checkbox tree, a commit-message box + Commit button
(`git commit -F -`).

**Checkpoint 3:** full suite green; live-verify staging a file and
committing it via the panel produces a real commit matching what `git
log` shows afterward.

**Phase 4 — hunk-level staging + push.** Per-hunk stage via a hand-built
patch + `git apply --cached`; a Push button surfacing real failure
reasons (auth, no upstream, rejected) through the existing error modal.

**Checkpoint 4:** full suite green; live-verify staging a single hunk
(not the whole file) reflects correctly in `git diff --cached`, and Push
against a real (test) remote succeeds/fails with an accurate message.

---

## Track 10 — Code folding

**Phase 1 — verify the `FoldMap` dependency, don't assume it.** Read the
current `FoldMap`/import-folding implementation directly; determine
concretely whether it already generalizes to arbitrary user-toggled
regions or needs its own second mechanism, and whether `Track 19`'s full
virtualization work is actually a hard prerequisite or `FEATURES.md`'s
own conservative guess. This phase's _output_ is that determination, not
code — don't write Phase 2 against an assumed answer.

**Phase 2 — fold-range computation.** Per-language tree-sitter query for
foldable node kinds (class/method/interface bodies); a collapse/expand
gutter marker at each range's opening line.

**Checkpoint 2:** `cargo test -p syntax` green (fold ranges match
expected line numbers per fixture); live-verify the gutter marker appears
at the right lines on a real file.

**Phase 3 — fold state + toggle.** `folded_ranges: HashSet<usize>` per
document (or per-tab side structure); toggling updates the layout fold-
map (built from the union of this and the existing auto-import folding).

**Checkpoint 3:** full suite green; live-verify clicking a fold marker
collapses/expands the right region and scrolling/editing around a folded
region doesn't corrupt layout.

---

## Track 11 — Multi-window / split-pane editing

**Phase 1 — split-pane only (multi-window explicitly out of scope for
this track, per `SPEC.md` §11's own recommendation).** `active_tab:
Option<usize>` generalized to a per-pane focus model; every existing
`state.active_tab` call site (an audit, not a guess — grep every call
site first and confirm the full list before starting the change) updated
to be pane-aware.

**Checkpoint 1:** full suite green; live-verify opening a second pane,
each pane independently switching/closing tabs without affecting the
other, and every existing single-pane behavior (save, dirty-tracking,
session persistence) still working correctly with only one pane open.

---

## Track 12 — Spring config property autocomplete

**Hard dependency on Track 21 (`Maven/Gradle awareness`) — not startable
before it lands.**

**Phase 1 — metadata extraction + candidates.** Once classpath resolution
exists: scan resolved dependency jars for bundled `spring-configuration-
metadata.json`, parse into completion candidates, feed the existing
completion popup keyed by typed prefix.

**Checkpoint 1:** full suite green (a fixture jar/metadata file producing
expected candidates, headless); live-verify typing a partial property key
in `application.properties`/`.yml` in a real Spring Boot project offers
real completions.

---

## Track 13 — Code coverage overlay

**Hard dependency on Track 22 (`Build/run/test integration`) — not
startable before it lands.**

**Phase 1 — coverage run + gutter marks.** "Run with Coverage" invokes
the build tool with JaCoCo enabled, parses `jacoco.xml`, paints per-line
hit/miss gutter marks.

**Checkpoint 1:** full suite green (a fixture `jacoco.xml` parsed into
expected per-line marks, headless); live-verify running coverage on a
real project shows accurate hit/miss marks matching the actual test run.

---

## Track 14 — Docker/container run integration

Shares its output-panel infrastructure with Track 22 (`Build/run/test
integration`) — whichever track lands first builds that shared piece;
this track's Phase 1 assumes it doesn't exist yet and builds a Docker-
specific version only if Track 22 genuinely hasn't landed first.

**Phase 1 — build & run.** Run > "Docker: Build & Run" / "Docker Compose:
Up" shells out, streams output into the (possibly newly-built) output
panel.

**Checkpoint 1:** full suite green; live-verify building/running a real
Dockerfile/compose stack streams real output and the container actually
starts (checked via `docker ps`, not just panel output).

**Phase 2 — lifecycle + stop.** A Stop button per running container/
stack; app-close stops every tracked container.

**Checkpoint 2:** full suite green; live-verify Stop actually ends the
container, closing the panel does _not_, and quitting the app stops
everything still tracked.

---

## Track 15 — Quick-fix intention actions

**Hard dependency on Track 20 (`LSP integration`) supplying real
`CodeAction` data — not startable before it lands.**

**Phase 1 — lightbulb + apply.** A gutter lightbulb on any line with an
active diagnostic that has associated `CodeAction`s; picking one applies
its `WorkspaceEdit` via the existing edit-application path.

**Checkpoint 1:** full suite green; live-verify a real diagnostic with a
known quick fix (e.g. an unused import a language server flags) offers
and correctly applies it.

---

## Track 17 — Peek definition

**Hard dependency on Track 20 (`LSP integration`)'s go-to-definition —
not startable before it lands.**

**Phase 1 — inline peek panel.** A shortcut/gutter icon opens an inline
expandable read-only panel showing the resolved definition's surrounding
lines, without switching tabs; Escape/click-outside collapses it.

**Checkpoint 1:** full suite green; live-verify peeking a real symbol
shows its definition inline, and the main editor's own tab/scroll position
is completely undisturbed afterward.

---

## Track 18 — Inline diff viewer widget

**Phase 1 — diff computation + rendering.** `show_diff(ui, old, new,
mode: DiffMode)` using a line-level diff (verify the `similar` crate's
current status before pinning it, or an equivalent) rendered as
side-by-side or inline colored rows, via the editor's own font/theme.

**Checkpoint 1:** `cargo test -p app` green (known old/new pairs producing
expected diff ops, headless); live-verify both `DiffMode`s render legibly
against a real changed file.

---

# Major tier

## Track 19 — Large file handling — full viewport virtualization

**Phase 1 — bounded word-wrap row-count computation.** The current
`cached_row_counts`/`layout_visible_wrapped` path (the part that still
scales with total file size even after the tab-switch cache fix) replaced
with an incrementally-maintained or viewport-bounded equivalent that
doesn't need every line's row-count computed up front.

**Checkpoint 1:** `cargo test -p app` green; a synthetic huge-file
benchmark (documented, not necessarily a hard-asserted threshold) showing
first-open/per-keystroke cost no longer scales with total file size.

**Phase 2 — hand-built widget: layout + click-to-position.** Replaces
`egui::TextEdit` for the visible-row-only case, reusing `layout_visible`'s
existing `char_rect`/`row_galleys` helpers for hit-testing, scoped to only
the visible slice.

**Checkpoint 2:** full suite green; live-verify clicking anywhere in a
huge file positions the cursor correctly and instantly (no perceptible
lag versus a small file).

**Phase 3 — drag-select.** Reimplemented against the new widget, studied
against `../references/zed`'s own non-`TextEdit` editor as the concrete
precedent (real production code, not egui's own internals, which never
had to solve this at this file's own scale).

**Checkpoint 3:** full suite green; live-verify drag-select across a
scrolled viewport (crossing the visible/invisible boundary mid-drag)
works correctly.

**Phase 4 — IME composition.** Reimplemented against the new widget; same
`../references/zed` precedent.

**Checkpoint 4:** full suite green; live-verify IME composition (a CJK
input method, if available to test with) works correctly in the new
widget.

---

## Track 20 — LSP integration

**Phase 1 — server lifecycle + handshake.** Child-process management for
`jdtls`/`kotlin-language-server` (external-tool paths via Settings,
mirroring Track 5's own convention); `lsp-types` for protocol structs;
stdio JSON-RPC framing read/write loop on a background thread per server
(mirroring `PtySession`'s own background-reader-thread shape).
`initialize`/`initialized` handshake, no user-visible feature yet.

**Checkpoint 1:** `cargo test -p app` green (a fake-server-process
handshake test, headless where possible); live-verify a real `jdtls`
process launches and completes its handshake against a real Java project
(inspectable via logging, not yet any visible feature).

**Phase 2 — diagnostics.** `textDocument/publishDiagnostics` feeds the
existing `Diagnostic`/squiggle pipeline as a second source.

**Checkpoint 2:** full suite green; live-verify a real semantic error
(not just a syntax error) shows a squiggle, with a message
`javac`/`kotlinc` — not just this codebase's own parser — actually
produced.

**Phase 3 — hover docs.** `textDocument/hover` feeds a tooltip,
structurally mirroring the existing syntax-error hover.

**Checkpoint 3:** full suite green; live-verify hovering a real symbol
shows real documentation (a JDK type's own Javadoc, for instance).

**Phase 4 — go-to-definition.** `textDocument/definition` reuses
`pending_navigation`'s existing cross-tab-jump primitive.

**Checkpoint 4:** full suite green; live-verify go-to-definition on a
symbol whose source isn't the open project's own tree (a JDK/library
type) actually jumps there — the case this codebase's own existing
lookups can't handle today.

**Phase 5 — autocomplete.** `textDocument/completion` as a second
candidate source merged into the existing completion popup.

**Checkpoint 5:** full suite green; live-verify LSP candidates and this
codebase's own existing candidates appear together, sensibly ranked, with
no visible duplication.

**Phase 6 — find-references.** `textDocument/references`, a results-list
UI (popup or panel depending on typical result count observed live).

**Checkpoint 6:** full suite green; live-verify find-references on a
widely-used symbol returns a real, complete list.

**Phase 7 — rename-symbol.** `textDocument/rename`, applying a
`WorkspaceEdit` across every affected file (open or not).

**Checkpoint 7:** full suite green; live-verify renaming a symbol used
across multiple files correctly updates every one of them, including
files that weren't open in a tab beforehand.

---

## Track 21 — Maven/Gradle awareness

**Phase 1 — `pom.xml` parsing.** `MavenProject` struct from
`<dependencies>`/`<modules>`/`<properties>` via `quick-xml`/`roxmltree`
(verify current crate health before pinning).

**Checkpoint 1:** `cargo test -p fg-core` green against real-world
`pom.xml` fixtures (a simple project, a multi-module parent).

**Phase 2 — Gradle model extraction.** Validate the offline-init-script-
dump approach against a real multi-module Gradle project before
committing further; if it holds up, build the extraction against it
rather than attempting to parse Groovy/Kotlin DSL as text.

**Checkpoint 2:** `cargo test -p fg-core` green against real Gradle
project fixtures; live-verify against an actual local Gradle project (not
just a fixture) since this phase's own approach depends on shelling out
to a real Gradle wrapper.

**Phase 3 — dependency-aware classpath resolution.** `mvn
dependency:build-classpath` / Gradle's own resolution task, parsed into a
resolved jar-file list.

**Checkpoint 3:** full suite green; live-verify against a real project
with actual third-party dependencies that the resolved classpath contains
real, correct jar paths on disk.

---

## Track 22 — Build/run/test integration

**Phase 1 — output-panel infra + plain run.** Shared dockable output
panel (reused by Track 14 if it lands after this); Run/Test actions
invoke the project's `RunConfig` via its build tool's wrapper script,
streaming stdout/stderr.

**Checkpoint 1:** full suite green; live-verify running a real Maven/
Gradle project's build/test task streams real, live output.

**Phase 2 — problem-matcher wiring.** Per-tool regex patterns recognizing
compiler-error line shapes (verified against real Maven/Gradle-wrapped
build output, not just bare `javac`'s own format); clickable jump via
`pending_navigation`.

**Checkpoint 2:** full suite green (fixture build-output strings matched
into expected file/line); live-verify a real compile error in a real
project produces a clickable entry that jumps to the right line.

---

## Track 23 — Debugger

Depends on Track 22 (`Build/run/test integration`) for process launch.

**Phase 1 — DAP client + launch.** JSON-RPC-over-stdio client (shared
framing code with Track 20's LSP client where genuinely reusable); launch
`java-debug` (verify its Kotlin support concretely, per `SPEC.md` §23,
before assuming one adapter covers both languages).

**Checkpoint 1:** full suite green; live-verify launching a real Java
program under the debugger successfully attaches (no breakpoints/stepping
yet — just a running, attached, controllable process).

**Phase 2 — breakpoints + stepping.** Gutter breakpoint markers; a debug
toolbar (Continue/Step Over/Step Into/Step Out/Stop); inline
current-line highlight while paused.

**Checkpoint 2:** full suite green; live-verify setting a real breakpoint
actually pauses execution there, and every stepping command moves
execution as expected.

**Phase 3 — variable/call-stack panel.** A dockable panel showing local
variables and the call stack while paused.

**Checkpoint 3:** full suite green; live-verify the panel shows accurate,
live variable values and an accurate call stack while paused at a real
breakpoint.

---

## Track 26 — Profiler integration

Depends on Track 22 (`Build/run/test integration`) for process launch/
attach.

**Phase 1 — async-profiler attach + capture.** Shell out to
`asprof`/`profiler.sh` against a target PID (verify current invocation
against `async-profiler`'s own current docs before committing); capture a
flame-graph-format (collapsed-stack) sample.

**Checkpoint 1:** full suite green; live-verify profiling a real running
JVM process produces a real, non-empty collapsed-stack output.

**Phase 2 — flame graph widget.** Custom-painted interactive flame graph
(stacked rectangles by call depth, hover for symbol name, click to zoom).

**Checkpoint 2:** full suite green; live-verify a real captured profile
renders a legible, correctly-proportioned flame graph, and zoom/hover both
work.

---

## Track 27 — Dependency-injection / bean graph visualizer

**Hard dependency on both Track 21 (`Maven/Gradle awareness`) and Track
20 (`LSP integration`) — not startable before both land, per `SPEC.md`
§27's own explicit warning against a syntax-only version of this feature
being misleading rather than honestly-scoped.**

**Phase 1 — bean/injection-point extraction.** Whole-project +
whole-classpath scan for bean definitions and injection points, resolved
by type (and `@Qualifier` name, if present) to candidate beans.

**Checkpoint 1:** full suite green (a fixture multi-module project with a
cross-module injection, asserting the resolved edge is found — the exact
case a syntax-only pass would miss); live-verify against a real,
non-trivial Spring project.

**Phase 2 — graph visualization panel.** A dockable panel rendering the
resolved graph (pan/zoom node/edge diagram); click a node to jump to its
source.

**Checkpoint 2:** full suite green; live-verify the rendered graph
matches the real project's actual bean wiring, and clicking a node jumps
to the right file/line.

---

## Build status (live)

### Moderate tier

- [x] Track 1 — Multi-select in the tree (live-verified: Ctrl/Cmd/Shift-
      click gestures, batch delete/copy/cut/paste, and New File/Rename
      disabled under multi-selection all confirmed working)
- [x] Track 2 — Richer Java/Kotlin syntax highlighting (code green, no
      live click-through required per this track's own Checkpoint 1 note —
      headless-testable via the highlight-span test shape)
- [ ] Track 4 — Local (non-git) file history
- [ ] Track 5 — Static analysis integration
- [ ] Track 6 — Auto-save
- [ ] Track 7 — Rectangular (block) paste

### Substantial tier

- [ ] Track 9 — Git diff gutter, inline blame, commit/stage/push UI
- [ ] Track 10 — Code folding
- [ ] Track 11 — Multi-window / split-pane editing
- [ ] Track 12 — Spring config property autocomplete
- [ ] Track 13 — Code coverage overlay
- [ ] Track 14 — Docker/container run integration
- [ ] Track 15 — Quick-fix intention actions
- [ ] Track 17 — Peek definition
- [ ] Track 18 — Inline diff viewer widget

### Major tier

- [ ] Track 19 — Large file handling — full viewport virtualization
- [ ] Track 20 — LSP integration
- [ ] Track 21 — Maven/Gradle awareness
- [ ] Track 22 — Build/run/test integration
- [ ] Track 23 — Debugger
- [ ] Track 26 — Profiler integration
- [ ] Track 27 — Dependency-injection / bean graph visualizer
