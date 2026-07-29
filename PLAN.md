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
right line — done: `fg_core::static_analysis` parses/converts a real
`checkstyle -c sun_checks.xml -f xml` report captured from a live run
against a fixture file, `crates/app/src/panels/static_analysis.rs` wires
Settings > External Tools… and Tools > Run Checkstyle end to end via a
background thread, and the live click-through (Settings > External
Tools… configured with a real `checkstyle` binary + `sun_checks.xml`,
Tools > Run Checkstyle against a real violation) was confirmed working by
the user.

Real Checkstyle CLI (verified against an installed `8.36.1` binary, not
assumed): `-c <config>` is required (no usable default ruleset), `-f xml`
is the report flag, and it exits non-zero whenever it finds _any_
violation — its exit code is the violation count, not a success signal,
so success is judged by whether stdout parses as a report. Checkstyle's
`column` attribute is a 1-based **character** offset into the line, only
byte-equivalent for pure-ASCII source. Settings > External Tools also
grew `pmd_binary`/`spotbugs_binary` fields now (Phase 1's own "shared
plumbing" scope) even though nothing consumes them until Phase 2/3.

**Phase 2 — PMD.** Same shape, PMD's own XML report format.

**Checkpoint 2:** same as above, PMD-specific fixture + live-verify — done:
`fg_core::static_analysis` gained `PmdFinding`/`parse_pmd_xml`/
`pmd_severity`/`pmd_diagnostics`, verified against a real
`pmd check -R rulesets/java/quickstart.xml -f xml --no-cache` run (PMD
7.26.0, downloaded from the official GitHub release since this
environment's package manager has no real PMD package — see
`README.md`'s new "External tools" section); a from-scratch end-to-end
smoke run (`fg_core::pmd_diagnostics` against the real downloaded binary
and a real fixture file, not just the captured-XML unit tests) confirmed
the assembled `pmd check -d ... -R ... -f xml --no-cache` invocation and
byte-range math both work before handing off. `Document` gained a second,
independent `pmd_diagnostics` field (Checkstyle's own `static_diagnostics`
renamed to `checkstyle_diagnostics` alongside it) — one shared field would
have meant a PMD run silently wiping out Checkstyle's still-valid
squiggles and vice versa, since Phase 1's "replace wholesale, don't merge"
design was written before a second tool existed to collide with it.
Settings > External Tools gained a PMD "Ruleset (-R)" field (PMD, like
Checkstyle, has no usable default and errors without one — confirmed via
`pmd check` with no `-R`: "Missing required option: '--rulesets=<rulesets>'").
`StaticAnalysisState` now tracks Checkstyle's and PMD's scans as two
independent slots so one running doesn't block the other from starting.
Live click-through (Settings > External Tools… configured with the real
downloaded PMD binary + `rulesets/java/quickstart.xml`, Tools > Run PMD
against real violations, Checkstyle's own squiggles confirmed undisturbed
by the PMD run) was confirmed working by the user.

PMD CLI specifics worth remembering (verified against a real `7.26.0`
binary): the subcommand is `pmd check` (not a bare `pmd` invocation), `-R`
is required the same way Checkstyle's `-c` is, and it also exits non-zero
(status 4) on any violation — same "exit code is the finding count, judge
success by whether stdout parses" pattern as Checkstyle. Unlike
Checkstyle, PMD's `<violation>` carries its message as element _text
content_, not an attribute, and reports a real inclusive `begincolumn`/
`endcolumn` range rather than a single point — `Diagnostic.range`'s end is
computed by querying one column _past_ `endcolumn`, not `endcolumn`
itself. PMD has no error/warning concept, just a 1(high)-5(low)
`priority`; this codebase maps 1-2 to `Severity::Error` and 3-5 to
`Severity::Warning` as its own judgment call, not a PMD convention.

**Addendum — install/update the tools from inside the app (not in the
original per-phase plan above; added once the user asked for it directly,
after "shared plumbing" had already shipped for Checkstyle+PMD).**
`crates/app/src/tool_manager.rs` downloads Checkstyle/PMD/SpotBugs'
official GitHub releases into `directories::ProjectDirs`'s cache dir on an
explicit Settings > External Tools "Install" click (`ureq` for the HTTP
fetch, `zip` to extract PMD's/SpotBugs' own archives — Checkstyle ships a
bare jar, no extraction needed), then fills in the binary/config fields
itself. Every version/URL/tag-shape/archive-layout claim here was verified
against a real download and a real run this session (`SPEC.md` §5's own
revised text has the durable version of this reasoning) — three real bugs
this caught before they shipped, each the kind of thing a "should just
work" assumption would have missed:

1. Checkstyle's _newest_ GitHub release needs a newer JDK than a real Java
   17 install has (a genuine `UnsupportedClassVersionError` running it) —
   `Tool::recommended_version` pins Checkstyle to `10.26.1`, the newest
   release confirmed (by running it) to still work under Java 17, rather
   than always chasing whatever GitHub calls "latest."
2. PMD's real git tag is `pmd_releases/7.26.0`, not the bare `7.26.0` a
   normal-looking semver tag would suggest (SpotBugs' and Checkstyle's own
   tags don't have this quirk) — caught by an actual failed download (a
   real 404), not a code-review guess.
3. A downloaded Checkstyle install is a bare `.jar` with no launcher
   script (unlike PMD's/SpotBugs' own `bin/<script>`), so it can't be
   `Command::new`'d directly — `fg_core::static_analysis::
command_for_binary` wraps any `.jar`-suffixed binary path in `java
-jar`, transparently to both `run_checkstyle_process` and the (already
   jar-free) `run_pmd_process`. This lives in `core`, not `app::
tool_manager`, despite `tool_manager` being what produces the jar path
   in the first place — `app` cannot be a dependency `core` reaches back
   into (`core <- syntax <- app`), and this is really "how do I invoke a
   configured Checkstyle binary correctly" logic, which belongs with the
   rest of that invocation code regardless of where the binary came from.

"Install" always installs the pinned `recommended_version`, never
whatever GitHub's `releases/latest` says — a separate "Check for Updates"
button shows the latest tag as plain informational text (`StaticAnalysisState::latest_versions`,
session-only, not persisted) without ever installing it automatically, so
a JVM-incompatible newer release (see bug 1 above) is never a surprise.
`zip::ZipArchive::extract` was verified (via a real extraction of PMD's
own archive) to already preserve the Unix executable bit from the
archive's own metadata — no manual `chmod` step needed for PMD's/
SpotBugs' launcher scripts. `ExternalToolPaths` gained a
`{checkstyle,pmd,spotbugs}_installed_version` field per tool (persisted,
`""` meaning "not installed via this downloader" — also the correct state
for someone who pointed a binary field at an existing system install
instead) so Settings can show "Installed: X" without re-deriving it from
the path string.

Live click-through (Install/Reinstall/Check for Updates for all three
tools against the real cache directory, Checkstyle/PMD still running
correctly off the freshly-installed binaries afterward) was confirmed
working by the user. One round of UI-copy follow-up: the user read
"Installed: 10.26.1" next to "Latest on GitHub: 13.9.0" as a possible bug
rather than the intended pinned-vs-latest distinction, so the dialog's own
top explanation and the per-tool "Latest on GitHub" label (now "Up to
date (X)" when the pin already matches, otherwise a hover tooltip
spelling out _why_ Install doesn't just chase latest) were reworded to
say so up front rather than requiring the user to ask.

**Phase 3 — SpotBugs.** Same shape, SpotBugs' own XML schema (bytecode-
based — verify it reports source line numbers accurately enough to map
back to a `Diagnostic` range before assuming parity with the other two).

**Deferred, not started** — investigated this session (real download,
real compile-then-analyze run against a fixture, not assumed) and found a
real blocker: unlike Checkstyle/PMD, SpotBugs analyzes **compiled `.class`
files**, not source — there's no `project_root`-shaped entry point at all,
and this app has no build step yet (`Maven/Gradle awareness`, §21, and
`Build/run/test integration`, §22, are both still un-started) to produce
one. User's own call: defer the actual analysis wiring until §22 lands.
SpotBugs' own _binary_ is still installable today via the tool-manager
addendum above (`Tool::SpotBugs`, `bin/fb` launcher) — that part doesn't
depend on the classes-directory question, only "Run SpotBugs" and its
Diagnostic-conversion parser do. Also verified and worth keeping for
whenever this phase resumes: `-xml:withMessages` (not bare `-xml`) is
needed for a `<LongMessage>` at all; each `<BugInstance>` carries several
`<SourceLine>` elements (class range, method range, the specific culprit
line) and the useful one is the _last_ direct child of `<BugInstance>`
itself, not any of the ones nested inside `<Class>`/`<Method>`/`<Type>`/
etc. — real depth-tracking during parsing, not a flat structure like
Checkstyle's/PMD's own reports.

**Checkpoint 3:** same as above, SpotBugs-specific fixture + live-verify.
Not reached.

---

## Track 6 — Auto-save

**Phase 1 — settings + triggers.** Settings > Auto-save toggle (off by
default) with "on focus loss" / "after N seconds idle" modes, both calling
the existing `Document::save` unchanged.

**Checkpoint 1:** full suite green (a fake-clock/fake-focus-event test
asserting the right trigger fires `save` at the right moment); live-verify
both modes against a real dirty tab — done: `crates/app/src/auto_save.rs`
holds the pure trigger logic (`AutoSaveSettings`/`AutoSaveMode`/
`AutoSaveState::tick`, unit-tested headlessly with a fake `i.time`/
`i.focused` clock, no egui context needed) and `tabs::save_all_dirty_tabs`
(new — saves every dirty open tab, not just the active one, since a
focus-loss/idle trigger is app-level, not tab-level); `FoxGardenApp::ui`
reads `ui.input(|i| (i.time, i.focused, !i.events.is_empty()))` once per
frame to drive `tick`/`record_activity` and calls `save_all_dirty_tabs`
when it fires. `crates/app/src/app/tests.rs` gained two integration tests
(`auto_save_focus_loss_trigger_saves_only_the_dirty_tab`,
`auto_save_idle_trigger_fires_only_after_the_threshold_with_no_activity`)
wiring the fake clock through to a real temp-file save, not just
`auto_save`'s own isolated unit tests — the Checkpoint 1 fake-clock/
fake-focus test explicitly called for. One real deviation from `SPEC.md`
§6's "reset on every keystroke" wording: the idle clock resets on _any_
input event (`!i.events.is_empty()`, so pointer moves/clicks/scroll count
too), not keystrokes only — a keystroke-only reset would make moving the
mouse around while reading code (no typing) still count as "idle" and
fire a save mid-thought, which reads as more surprising than useful.
Settings > Auto-save is a new submenu alongside Theme/Font/Indentation
(a checkbox for `enabled`, two radios for the mode, a `DragValue` for idle
seconds clamped `5..=600`), persisted the same hand-rolled
`eframe::Storage` way every other Settings value here already is (no
serde in this crate — `app.rs`'s own established note). Live click-through
(Settings > Auto-save, both modes against a real dirty tab) confirmed
working by the user.

**Phase 2 — conflict-banner interaction.** Auto-save suppressed for any
tab currently showing the "changed on disk" banner; resumes once
Reload/Keep Mine resolves it.

**Checkpoint 2:** full suite green; live-verify auto-save does _not_ fire
while the conflict banner is showing, and does resume normally after
resolving it — code done: `tabs::save_all_dirty_tabs` gained an
`external_conflicts: &HashSet<PathBuf>` parameter and skips any dirty tab
whose path is in it (same set `show_external_change_banner` itself reads
to decide whether to render the banner, and that Reload/Keep Mine already
clear on resolution — Phase 2 needed no new state, just reading the
existing one). `FoxGardenApp::ui`'s auto-save trigger check was moved to
_after_ `process_file_events` (was before it in Phase 1) specifically so a
conflict that appears this very frame already suppresses this same
frame's auto-save, not one frame late. `crates/app/src/app/tests.rs`
gained `auto_save_skips_a_tab_showing_the_external_conflict_banner`
(conflicted tab stays dirty and untouched on disk; a second, unconflicted
dirty tab still saves normally in the same call). Live click-through (a
dirty tab with the conflict banner showing doesn't get auto-saved out from
under it; resolving via Reload/Keep Mine lets auto-save resume normally)
confirmed working by the user.

---

## Track 7 — Rectangular (block) paste

**Phase 1 — column/block selection.** `Alt`+drag produces a
`BlockSelection { start_line, end_line, start_col, end_col }`; `text_area`
gains a second highlight-painting path for it alongside the existing
linear-range one.

**Checkpoint 1:** full suite green; live-verify `Alt`+drag visibly
highlights a rectangular region across several lines — code done:
`text_area::input` gained `BlockSelection { anchor_line, anchor_col,
primary_line, primary_col }` (anchor/primary shape, like `Caret` itself,
rather than pre-sorted bounds — an in-progress drag that crosses back over
its own start point doesn't need to separately remember which corner was
the anchor; `lines()`/`cols()` derive the sorted, PLAN-described
`start_line..end_line`/`start_col..end_col` view on demand). `ShellState`
gained `block_selection: Option<BlockSelection>`, checked first in
`shell::show`'s pointer-handling block whenever `modifiers.alt` is held
during `drag_started()`/`dragged()` — entirely separate from `Caret`
(never touches it), and any non-Alt click/drag clears it (also covers
releasing Alt mid-drag while the mouse stays down). Plain Alt+Click (no
drag) is untouched — it still falls through to the ordinary click branch,
same as before this track; `widget.rs`'s own Alt+Click multi-cursor
interception runs after `shell::show` returns and was never touched.
`paint_block_selection` (new, alongside `paint_caret`) paints a filled
rect at the *same* column range on every spanned row, deliberately not
clamped to each row's own length (unlike `paint_caret`'s per-line clamp) —
that's what makes it a rectangle rather than a per-line-linear selection.
One real discovery while testing: egui's own drag classification needs a
dedicated frame boundary between the press and the first move for
`drag_started()` to fire — a synthetic test that folds press+move into one
frame never sees a drag at all (reads as a plain click); the checkpoint
test (`shell::tests::alt_drag_produces_a_rectangular_block_selection_
spanning_multiple_rows`) drives three real frames (press, then a small
move — the frame `drag_started()` actually fires and anchors the block,
then a further move that extends it) once this was caught. Three more
`BlockSelection` unit tests cover `lines()`/`cols()` sorting/anchor-
stability directly in `text_area::input::tests`. Live click-through
(`Alt`+drag paints a rectangle spanning several rows, including past a
shorter row's own text; releasing keeps it; a plain click elsewhere clears
it; plain Alt+Click still drops a bare multi-cursor as before) confirmed
working by the user.

**Phase 2 — block-scoped editing.** Typing/Backspace/Delete over an
active block selection applies the same column-range edit to every row
the block spans.

**Checkpoint 2:** full suite green; live-verify typing over a block
selection edits every spanned row identically, Backspace/Delete likewise —
code done: `text_area::input` gained `replace_block_selection`/
`block_backspace`/`block_delete_forward`, all building their per-row char
ranges from `BlockSelection::cols()` clamped to each row's own length
(`block_row_ranges`), then applying the shared edit via `widgets::editor::
multi_cursor::apply_multi_edit` — the same "apply one op at N ranges,
correcting for cumulative delta" engine `Ctrl+D`'s own multi-cursor typing
already uses, reused rather than reimplemented since it's already pure and
Document-free. The resulting block's column is computed directly from the
edit's own known width (`cols().start + insert.len()` for typing,
`cols().start` for delete/selection-backspace), not from any one row's
actual post-edit position — rows shorter than the block's column land
their own edit at their own end (no padding), which would otherwise
disagree row-to-row on "where the new column is"; the block's target
column stays fixed independent of any single row's clamp, same as every
real block-select editor's own convention. A real correctness risk caught
before shipping: a zero-width row already sitting at column 0 (Backspace)
or at its own line's end (Delete) is skipped rather than falling through
to `apply_multi_edit`'s raw *absolute-offset* boundary check — that check
only guards start/end of the whole buffer, not start/end of a line, so
without this a block Backspace at column 0 across several rows would have
silently deleted the *previous* line's trailing newline on each one,
merging rows into each other instead of leaving them alone. `shell::show`'s
`process_events` intercepts `Event::Text`/`Key::Backspace`/`Key::Delete`
ahead of their ordinary single-`Caret` arms whenever `state.block_
selection` is `Some`, and only those three — every other event (arrows,
Enter, Tab, Cut/Paste, …) is out of this phase's scope and still acts on
`state.caret` exactly as before. Ten new tests: nine pure table tests in
`text_area::input::tests` (uniform insert, real-range replace, short-line
clamping without padding, both merge-guard cases for Backspace/Delete, the
"only rows genuinely at column 0 are skipped" distinction) plus one
`shell::tests` integration test seeding a real `ShellState` and driving a
real `Event::Text` through `process_events`. Live click-through (typing
into a zero-width block inserts on every row and stays active for further
typing; typing over a real-width block replaces that column range on
every row; Backspace/Delete at both zero and real width all work; a short
line inside the block neither crashes nor merges with its neighbor)
confirmed working by the user.

**Phase 3 — block paste.** Clipboard text split on `\n`, row _i_ inserted
at `(start_line + i, start_col)`; a row-count mismatch (fewer/more
clipboard lines than the block spans) leaves the surplus/shortfall
untouched rather than wrapping or clearing.

**Checkpoint 3:** full suite green (table tests for exact-match,
fewer-lines, and more-lines cases); live-verify a real block-select →
copy → block-paste round-trip — code done: `text_area::input` gained
`block_selection_text` (reads `block`'s own `cols()` range from every
spanned row, joined by `\n` — the Copy/Cut side) and `block_paste`
(`clipboard.split('\n')`, `zip`ped against `block_row_ranges` so an
unmatched row or an unmatched clipboard line is simply left alone rather
than wrapped/cleared, applied back-to-front so each row's own differently-
sized insert never invalidates an earlier row's already-computed range —
`multi_cursor::apply_multi_edit` wasn't reusable here since it only
supports one `MultiEditOp` shared across every range, and each row's
pasted line can differ in length). `shell::process_events` gained
block-scoped `Event::Copy`/`Event::Cut`/`Event::Paste` arms (same
"checked first, never falls through" placement as Phase 2's block Text/
Backspace/Delete arms), Copy/Cut guarded by `!block.cols().is_empty()`
matching the ordinary-caret Copy/Cut arms' own `!is_collapsed()` guard;
Cut reuses `replace_block_selection(..., "")` to clear the block rather
than a new deletion path. Five new `input::tests` table tests (join/
short-line-clamp for `block_selection_text`; exact-match, fewer-lines,
more-lines, and a `block_selection_text` → `block_paste` round-trip for
`block_paste`) plus one `shell::tests` integration test seeding a real
`ShellState` and driving a real `Event::Paste` through `process_events`,
mirroring Phase 2's own checkpoint test shape. Live click-through (a real
block-select → copy → block-paste round-trip, including the fewer-lines
and more-lines mismatch cases) confirmed working by the user.

---

# Substantial tier

## Track 9 — Git diff gutter, inline blame, commit/stage/push UI

**Phase 1 — diff gutter.** `git diff --no-color -U0` per open/save/
reload, hunk headers parsed into added/removed/modified line ranges,
painted alongside the line-number gutter.

**Checkpoint 1:** full suite green (a fixture diff-output string parsed
into expected ranges, headless); live-verify editing a tracked file shows
the right gutter marks against a real git repo — code done: this is the
first git-aware code in the project (no `git2` dependency — shells out to
the real CLI, mirroring `static_analysis`'s own approach, per a from-
scratch check this session confirmed there was nothing existing to build
on). `fg_core::diff` (new) has `git_diff_hunks(path, root)` (`git diff
--no-color -U0 -- <path>` with `root` as cwd — `root` only needs to be
*inside* the working tree, not necessarily the git root itself, so a
Maven/Gradle multi-module project root still works) and the pure
`parse_unified_diff`, converting real `@@ -old[,count] +new[,count] @@`
headers (verified against several real captured `git diff -U0` runs this
session — a plain modify, a pure add/remove at the start/middle/end of a
file — not assumed) into 0-based `DiffHunk { kind: Added|Removed|
Modified, lines }`. A `Removed` hunk has no surviving line of its own, so
`lines` is an empty `at..at` marker rather than a real range — git's own
`+0,0` zero-count convention already reports the correct 0-based index
with no adjustment needed, including the real edge case of a deletion at
the very start of the file. "Not a git repository"/"untracked file"/"no
changes" are all deliberately left indistinguishable (empty stdout, no
`Err`) — every one of them means the same thing to this gutter: nothing to
show, not an error to surface; only a failure to launch `git` at all is a
real `Err`. `Document` gained `diff_hunks: Vec<DiffHunk>`, refreshed
wholesale, the same lifecycle `checkstyle_diagnostics`/`pmd_diagnostics`
already have and for the same reason (see that field's own doc comment).

App-side wiring (`panels::git_diff::DiffState`) mirrors `static_analysis`'s
own `spawn_scan`/`poll_scan` background-thread shape, but keyed per-path
(`HashMap`, not one `Option` slot) since a diff run is triggered per-
document from several independent points rather than one project-wide
action at a time. Open (`app::open_path`) and reload
(`app::reload_tab_from_disk`) each trigger `DiffState::run` explicitly, a
few call sites each. Save is deliberately *not* threaded through every one
of this app's several save call sites (Ctrl+S, File > Save, the close-
confirmation modal, the editor's own right-click Save, auto-save) — instead
`DiffState::check_for_saves`, called once a frame after `tabs::show` runs,
detects any open tab's dirty state going `true` -> `false` since the last
frame and fires generically, catching every save path (and the external-
change banner's manual "Reload" button, itself a dirty -> clean transition)
without any of those call sites needing to know this feature exists. Only
the file-watcher's *transparent* auto-reload doesn't fit that heuristic
(never dirty before or after, since it only fires when there were no local
edits to begin with) — `reload_tab_from_disk`'s own explicit trigger is
what actually covers that one case; the other trigger points are real but
redundant with `check_for_saves` where they overlap it, which is harmless.
Results are silently dropped on failure (including "not a git repository")
rather than surfaced through `last_error` — an automatic background
refresh showing no marks is the right degrade, not an error toast on every
non-git file.

Gutter painting (`widgets::editor::diff_gutter`, new sibling to
`folding`) reserves an extra 4px column flush against the gutter's own
inner edge (immediately before the text starts), only when `doc.diff_hunks`
is non-empty — same "only reserve it when there's something to show" rule
`folding::FOLD_GUTTER_WIDTH` already established, so an untouched file's
gutter width is unaffected. `Added`/`Modified` paint a filled rect per
line in `hunk.lines`; a `Removed` hunk's empty-range marker paints a thin
3px notch at a row boundary instead (the top edge of line 0 for a
deletion at the very start of the file, else the bottom edge of the line
right before the marker — which, with no extra casing, also correctly
lands on the last real line's own bottom edge for a deletion at the very
end of the file). Three new theme colors (`diff_added`/`diff_removed`/
`diff_modified`, dark+light) reuse the same green/red/blue vocabulary
every real diff gutter (VS Code, IntelliJ) already uses.

18 new tests: 12 in `fg_core::diff` (parser table tests against the real
captured fixtures above, plus two end-to-end tests running a real `git`
binary against a real temp repo — one with a real commit+edit, one
against a path outside any repository, confirming the "empty, not an
error" degrade for real rather than just by parser inspection) and 6 in
`panels::git_diff` (spawn-and-poll round trip, a dropped result for a
since-closed tab, the dirty-transition trigger's no-false-positive-on-
first-sighting and no-project-root cases). Live click-through (against
this repo's own working tree — modify+save shows a blue bar, add+save
shows a green bar, delete+save shows a red notch at the boundary, undo
back to clean removes the marks, close/reopen reflects the current diff
immediately, and an on-disk change while the tab is open and clean
refreshes the marks without an explicit save) confirmed working by the
user.

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

## Build status (live)

### Moderate tier

- [x] Track 1 — Multi-select in the tree (live-verified: Ctrl/Cmd/Shift-
      click gestures, batch delete/copy/cut/paste, and New File/Rename
      disabled under multi-selection all confirmed working)
- [x] Track 2 — Richer Java/Kotlin syntax highlighting (code green, no
      live click-through required per this track's own Checkpoint 1 note —
      headless-testable via the highlight-span test shape)
- [ ] Track 4 — Local (non-git) file history
- [ ] Track 5 — Static analysis integration (Phase 1/Checkstyle and Phase
      2/PMD both live-verified, including the in-app install/update
      addition for all three tools' binaries; Phase 3/SpotBugs analysis
      wiring deferred until Track 22 — Build/run/test integration —
      lands, per the user's own call once the compiled-classes-directory
      blocker surfaced)
- [x] Track 6 — Auto-save (both phases shipped and live-verified)
- [x] Track 7 — Rectangular (block) paste (all 3 phases shipped and
      live-verified)

### Substantial tier

- [ ] Track 9 — Git diff gutter, inline blame, commit/stage/push UI
      (Phase 1/diff gutter shipped and live-verified; Phases 2-4 not
      started)
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
