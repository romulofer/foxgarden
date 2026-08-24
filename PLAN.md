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

**Track 20 is now fully done, all 7 phases** (go-to-definition, hover
docs, find-references, rename-symbol among them) — see their own
checkpoints below for what shipped and how each was live-verified
(Phase 7's own checkpoint also flags one live-verify gap worth a second
look once a real Maven/Gradle project is available). Phase 3 (hover
docs) closed in a later session: live-verified against a real JDK type's
Javadoc (`ArrayList`), which also closed `TECHNICAL_DEBT.md` #22 and
surfaced/fixed one small real gap in its own Markdown-stripping (single-
asterisk `*italic*` survived as literal punctuation; see that checkpoint
and #22's own closing note). No other track currently has an unstarted
hard dependency blocking it, so the next track is an open choice rather
than a forced one.

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

**Checkpoint 1:** done — `fg_core::static_analysis` parses/converts a real
Checkstyle XML report into `Diagnostic`s; `crates/app/src/panels/
static_analysis.rs` wires Settings > External Tools… and Tools > Run
Checkstyle end to end via a background thread. Live-verified against a
real project.

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

**Checkpoint 2:** done — `fg_core::static_analysis` gained `PmdFinding`/
`parse_pmd_xml`/`pmd_severity`/`pmd_diagnostics`, verified against a real
`pmd check -R rulesets/java/quickstart.xml -f xml --no-cache` run (PMD
7.26.0, downloaded from the official GitHub release since this
environment's package manager has no real PMD package — see `README.md`'s
"External tools" section). `Document` gained a second, independent
`pmd_diagnostics` field (Checkstyle's own `static_diagnostics` renamed to
`checkstyle_diagnostics` alongside it) — one shared field would have meant
a PMD run silently wiping out Checkstyle's still-valid squiggles and vice
versa, since Phase 1's "replace wholesale, don't merge" design was written
before a second tool existed to collide with it. `StaticAnalysisState`
tracks Checkstyle's and PMD's scans as two independent slots so one
running doesn't block the other from starting. Live-verified against a
real project, including that Checkstyle's own squiggles survive a PMD run
undisturbed.

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

**Addendum — install/update the tools from inside the app** (not in the
original per-phase plan above; added after "shared plumbing" had already
shipped for Checkstyle+PMD). `crates/app/src/tool_manager.rs` downloads
Checkstyle/PMD/SpotBugs' official GitHub releases into `directories::
ProjectDirs`'s cache dir on an explicit Settings > External Tools
"Install" click (`ureq` for the HTTP fetch, `zip` to extract PMD's/
SpotBugs' own archives — Checkstyle ships a bare jar, no extraction
needed), then fills in the binary/config fields itself. Three real gotchas
found via real downloads/runs, worth keeping:

1. Checkstyle's _newest_ GitHub release needs a newer JDK than a real Java
   17 install has (a genuine `UnsupportedClassVersionError` running it) —
   `Tool::recommended_version` pins Checkstyle to `10.26.1`, the newest
   release confirmed (by running it) to still work under Java 17, rather
   than always chasing whatever GitHub calls "latest."
2. PMD's real git tag is `pmd_releases/7.26.0`, not the bare `7.26.0` a
   normal-looking semver tag would suggest (SpotBugs' and Checkstyle's own
   tags don't have this quirk) — caught by an actual failed download (404).
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

Live-verified end-to-end (Install/Reinstall/Check for Updates for all
three tools against the real cache directory, Checkstyle/PMD still
running correctly off the freshly-installed binaries afterward).

**Phase 3 — SpotBugs.** Same shape, SpotBugs' own XML schema (bytecode-
based — verify it reports source line numbers accurately enough to map
back to a `Diagnostic` range before assuming parity with the other two).

**Checkpoint 3:** done — unblocked by Track 22 (`Build/run/test
integration`) landing, which finally gave SpotBugs a real compiled-classes
directory to analyze (`fg_core::default_classes_dir`, factored out of
`run::run_command`'s own classpath-prefix logic so both share the exact
`target/classes`/`build/classes/java/main` convention rather than
duplicating it). `fg_core::static_analysis` gained `SpotBugsFinding`,
`parse_spotbugs_xml`, `spotbugs_severity`, `spotbugs_source_file`, and
`spotbugs_diagnostics` — verified against a real `fb analyze
-xml:withMessages -output report.xml <classes_dir>` run (SpotBugs 4.10.3,
a real fixture class compiled with `javac`, both a real
`ES_COMPARING_PARAMETER_STRING_WITH_EQ` and a real
`OBL_UNSATISFIED_OBLIGATION` finding), not assumed.

Two real, found-not-assumed corrections to this section's own earlier
notes, from checking that real report side by side rather than trusting
the prior session's guess:

1. **The "useful `<SourceLine>` is the last direct child of
   `<BugInstance>`" claim above was wrong.** The real
   `OBL_UNSATISFIED_OBLIGATION` finding has *three* direct-child
   `<SourceLine>`s (an "obligation created" line plus two "path continues"
   lines) — the real primary one is the *first*, distinguished only by its
   own `primary="true"` attribute, not by position. `parse_spotbugs_xml`
   tracks nesting depth relative to the enclosing `<BugInstance>` and
   only accepts a depth-`0` `<Class>`/`<SourceLine>` that also carries
   `primary="true"`, which correctly rejects every nested occurrence
   inside `<Class>`/`<Method>`/`<Type>`/`<Int>`/`<String>` along the way.
2. **SpotBugs' own exit code is a real success/failure signal**, unlike
   Checkstyle's/PMD's (both of which encode their violation count as the
   exit code — `SPEC.md` §5's "judge success by whether stdout parses, not
   the exit code" pattern). Verified live: a run against a nonexistent
   classes directory exits `1` with a Java stack trace on stderr and no
   report written; a real run — with findings or without — exits `0`
   either way. `run_spotbugs_process` treats a non-zero exit as the real
   failure signal (`StaticAnalysisError::Report`, carrying stderr), not by
   whether the `-output` file parses.

Also unlike Checkstyle/PMD, SpotBugs reports no column (bytecode has no
character offsets) and no real absolute file path — only a class name and
a bytecode-debug-info source filename. `spotbugs_source_file` resolves a
finding's own primary `classname` back onto `src/main/java/<package/path>/
<ClassName>.java` (same standard-layout convention `test_report::
test_source_file` already established for `src/test/java`), reducing a
nested/inner/anonymous class (`Outer$Inner`) to its outer class first —
Java always compiles those into the outer class's own `.java` file. A
finding whose class can't be resolved this way is silently dropped, same
degrade `diagnostics_from_findings` already establishes for an unreadable
file.

App-side (`panels::static_analysis`): `StaticAnalysisState` gained a third,
independent `spotbugs_scan_rx` slot (`run_spotbugs`/`poll_spotbugs`/
`spotbugs_running`, same shape as Checkstyle's/PMD's own) and
`apply_spotbugs_results` writes into `Document`'s new, independent
`spotbugs_diagnostics` field — same "its own field, not a shared bucket"
reasoning `checkstyle_diagnostics`/`pmd_diagnostics` already established,
now chained into `widget.rs`'s squiggle pipeline alongside the other two.
Tools > "Run SpotBugs" (`menu_bar.rs`) requires the binary to be configured
*and* a real, already-built classes directory to exist
(`fg_core::detect_build_tool` + `default_classes_dir`, `.is_dir()`
checked before spawning) — surfacing "run Build first" as its own distinct
error rather than a confusing SpotBugs failure when there's nothing to
analyze yet. Settings > External Tools' SpotBugs row needed no changes: its
Install/Reinstall + Check for Updates buttons (`crate::tool_manager`) were
already wired in Phase 1's "shared plumbing" addendum, unused until now.

Full `cargo build --workspace`/`cargo test --workspace` green (`fg_core`
gained real-report-fixture unit tests proving the primary-vs-last-child
correction above, plus `spotbugs_source_file`/`spotbugs_findings_to_
diagnostics` coverage; `panels::static_analysis` gained the same
run/poll/independence/apply-results test shape Checkstyle's/PMD's own
tests already have). Live click-through (Tools > Run SpotBugs against a
real built project, confirming real squiggles land and Settings' Install
row still works) not yet done — owed before this checkpoint is fully
closed, per this project's own testing discipline.

---

## Track 6 — Auto-save

**Phase 1 — settings + triggers.** Settings > Auto-save toggle (off by
default) with "on focus loss" / "after N seconds idle" modes, both calling
the existing `Document::save` unchanged.

**Checkpoint 1:** done — `crates/app/src/auto_save.rs` holds the pure
trigger logic (`AutoSaveSettings`/`AutoSaveMode`/`AutoSaveState::tick`,
unit-tested headlessly with a fake `i.time`/`i.focused` clock, no egui
context needed) and `tabs::save_all_dirty_tabs` (saves every dirty open
tab, not just the active one, since a focus-loss/idle trigger is
app-level, not tab-level); `FoxGardenApp::ui` reads `ui.input(|i|
(i.time, i.focused, !i.events.is_empty()))` once per frame to drive
`tick`/`record_activity` and calls `save_all_dirty_tabs` when it fires.
One real deviation from `SPEC.md` §6's "reset on every keystroke" wording:
the idle clock resets on _any_ input event (pointer moves/clicks/scroll
count too), not keystrokes only — a keystroke-only reset would make
moving the mouse around while reading code (no typing) still count as
"idle" and fire a save mid-thought, which reads as more surprising than
useful. Settings > Auto-save is a new submenu alongside Theme/Font/
Indentation (checkbox for `enabled`, two radios for the mode, a
`DragValue` for idle seconds clamped `5..=600`), persisted the same
hand-rolled `eframe::Storage` way every other Settings value here already
is (no serde in this crate). Live-verified against a real dirty tab.

**Phase 2 — conflict-banner interaction.** Auto-save suppressed for any
tab currently showing the "changed on disk" banner; resumes once
Reload/Keep Mine resolves it.

**Checkpoint 2:** done — `tabs::save_all_dirty_tabs` gained an
`external_conflicts: &HashSet<PathBuf>` parameter and skips any dirty tab
whose path is in it (same set `show_external_change_banner` reads to
decide whether to render the banner, and that Reload/Keep Mine already
clear on resolution — Phase 2 needed no new state, just reading the
existing one). `FoxGardenApp::ui`'s auto-save trigger check runs _after_
`process_file_events` (was before it in Phase 1) specifically so a
conflict that appears this very frame already suppresses this same
frame's auto-save, not one frame late. Live-verified: a dirty tab with the
conflict banner showing doesn't get auto-saved out from under it;
resolving via Reload/Keep Mine lets auto-save resume normally.

---

## Track 7 — Rectangular (block) paste

**Phase 1 — column/block selection.** `Alt`+drag produces a
`BlockSelection { start_line, end_line, start_col, end_col }`; `text_area`
gains a second highlight-painting path for it alongside the existing
linear-range one.

**Checkpoint 1:** done — `text_area::input` gained `BlockSelection {
anchor_line, anchor_col, primary_line, primary_col }` (anchor/primary
shape, like `Caret` itself, rather than pre-sorted bounds — an
in-progress drag that crosses back over its own start point doesn't need
to separately remember which corner was the anchor; `lines()`/`cols()`
derive the sorted view on demand). `ShellState` gained `block_selection:
Option<BlockSelection>`, checked first in `shell::show`'s
pointer-handling block whenever `modifiers.alt` is held during
`drag_started()`/`dragged()` — entirely separate from `Caret`, and any
non-Alt click/drag clears it (also covers releasing Alt mid-drag while
the mouse stays down). Plain Alt+Click (no drag) is untouched — it still
falls through to the ordinary click branch; `widget.rs`'s own Alt+Click
multi-cursor interception runs after `shell::show` returns and was never
touched. `paint_block_selection` paints a filled rect at the *same*
column range on every spanned row, deliberately not clamped to each
row's own length (unlike `paint_caret`'s per-line clamp) — that's what
makes it a rectangle rather than a per-line-linear selection. One real
testing gotcha: egui's own drag classification needs a dedicated frame
boundary between the press and the first move for `drag_started()` to
fire — a synthetic test that folds press+move into one frame never sees
a drag at all (reads as a plain click); a real test needs at least three
frames (press, a move that anchors the block, a further move that
extends it). Live-verified.

**Phase 2 — block-scoped editing.** Typing/Backspace/Delete over an
active block selection applies the same column-range edit to every row
the block spans.

**Checkpoint 2:** done — `text_area::input` gained
`replace_block_selection`/`block_backspace`/`block_delete_forward`, all
building their per-row char ranges from `BlockSelection::cols()` clamped
to each row's own length (`block_row_ranges`), then applying the shared
edit via `widgets::editor::multi_cursor::apply_multi_edit` — the same
"apply one op at N ranges, correcting for cumulative delta" engine
`Ctrl+D`'s own multi-cursor typing already uses, reused since it's
already pure and Document-free. The resulting block's column is computed
directly from the edit's own known width, not from any one row's actual
post-edit position — rows shorter than the block's column land their own
edit at their own end (no padding), since the block's target column must
stay fixed independent of any single row's clamp. A real correctness
trap caught before shipping: a zero-width row already sitting at column 0
(Backspace) or at its own line's end (Delete) must be skipped rather than
falling through to `apply_multi_edit`'s raw *absolute-offset* boundary
check — that check only guards start/end of the whole buffer, not
start/end of a line, so without this a block Backspace at column 0 across
several rows would have silently deleted the *previous* line's trailing
newline on each one, merging rows into each other. `shell::show`'s
`process_events` intercepts `Event::Text`/`Key::Backspace`/`Key::Delete`
ahead of their ordinary single-`Caret` arms whenever `state.block_
selection` is `Some`, and only those three — every other event (arrows,
Enter, Tab, Cut/Paste, …) still acts on `state.caret` exactly as before.
Live-verified.

**Phase 3 — block paste.** Clipboard text split on `\n`, row _i_ inserted
at `(start_line + i, start_col)`; a row-count mismatch (fewer/more
clipboard lines than the block spans) leaves the surplus/shortfall
untouched rather than wrapping or clearing.

**Checkpoint 3:** done — `text_area::input` gained `block_selection_text`
(reads `block`'s own `cols()` range from every spanned row, joined by
`\n` — the Copy/Cut side) and `block_paste` (`clipboard.split('\n')`,
`zip`ped against `block_row_ranges` so an unmatched row or an unmatched
clipboard line is simply left alone rather than wrapped/cleared, applied
back-to-front so each row's own differently-sized insert never
invalidates an earlier row's already-computed range — `multi_cursor::
apply_multi_edit` wasn't reusable here since it only supports one
`MultiEditOp` shared across every range, and each row's pasted line can
differ in length). `shell::process_events` gained block-scoped
`Event::Copy`/`Event::Cut`/`Event::Paste` arms (same "checked first,
never falls through" placement as Phase 2's block arms), Copy/Cut guarded
by `!block.cols().is_empty()` matching the ordinary-caret arms' own
`!is_collapsed()` guard; Cut reuses `replace_block_selection(..., "")` to
clear the block rather than a new deletion path. Live-verified, including
the fewer-lines and more-lines mismatch cases.

---

# Substantial tier

## Track 9 — Git diff gutter, inline blame, commit/stage/push UI

**Phase 1 — diff gutter.** `git diff --no-color -U0` per open/save/
reload, hunk headers parsed into added/removed/modified line ranges,
painted alongside the line-number gutter.

**Checkpoint 1:** done — this is the first git-aware code in the project
(no `git2` dependency — shells out to the real CLI, mirroring
`static_analysis`'s own approach). `fg_core::diff` has `git_diff_hunks(path,
root)` (`git diff --no-color -U0 -- <path>` with `root` as cwd — `root`
only needs to be *inside* the working tree, not necessarily the git root
itself, so a Maven/Gradle multi-module project root still works) and the
pure `parse_unified_diff`, converting `@@ -old[,count] +new[,count] @@`
headers into 0-based `DiffHunk { kind: Added|Removed|Modified, lines }`. A
`Removed` hunk has no surviving line of its own, so `lines` is an empty
`at..at` marker rather than a real range — git's own `+0,0` zero-count
convention already reports the correct 0-based index with no adjustment
needed. "Not a git repository"/"untracked file"/"no changes" are all
deliberately left indistinguishable (empty stdout, no `Err`) — every one
of them means the same thing to this gutter: nothing to show, not an
error to surface; only a failure to launch `git` at all is a real `Err`.
`Document` gained `diff_hunks: Vec<DiffHunk>`, refreshed wholesale, the
same lifecycle `checkstyle_diagnostics`/`pmd_diagnostics` already have.

App-side wiring (`panels::git_diff::DiffState`) mirrors `static_analysis`'s
own `spawn_scan`/`poll_scan` background-thread shape, but keyed per-path
(`HashMap`, not one `Option` slot) since a diff run is triggered per-
document from several independent points. Save is deliberately *not*
threaded through every one of this app's several save call sites (Ctrl+S,
File > Save, the close-confirmation modal, the editor's own right-click
Save, auto-save) — instead `DiffState::check_for_saves`, called once a
frame after `tabs::show` runs, detects any open tab's dirty state going
`true` -> `false` since the last frame and fires generically, catching
every save path without any of those call sites needing to know this
feature exists. Only the file-watcher's *transparent* auto-reload doesn't
fit that heuristic (never dirty before or after) — `reload_tab_from_disk`'s
own explicit trigger covers that one case. Results are silently dropped on
failure (including "not a git repository") rather than surfaced through
`last_error` — an automatic background refresh showing no marks is the
right degrade, not an error toast on every non-git file.

Gutter painting (`widgets::editor::diff_gutter`, sibling to `folding`)
reserves an extra 4px column flush against the gutter's own inner edge,
only when `doc.diff_hunks` is non-empty — same "only reserve it when
there's something to show" rule `folding::FOLD_GUTTER_WIDTH` already
established. `Added`/`Modified` paint a filled rect per line in
`hunk.lines`; a `Removed` hunk's empty-range marker paints a thin 3px
notch at a row boundary instead. Three new theme colors (`diff_added`/
`diff_removed`/`diff_modified`, dark+light) reuse the same green/red/blue
vocabulary every real diff gutter (VS Code, IntelliJ) already uses.
Live-verified against this repo's own working tree.

**Phase 2 — inline blame.** `git blame --porcelain` parsed per line,
shown as a dimmed cursor-line annotation.

**Checkpoint 2:** done — `fg_core::blame` has `git_blame(path, root)`
(`git blame --porcelain -- <path>`, same "only needs to be inside the
working tree" contract as `git_diff_hunks`) and the pure
`parse_porcelain_blame`, producing one dense, 0-indexed `BlameLine { sha,
author, author_time, summary }` per line of the file. Same "empty stdout,
no `Err`" degrade for "not a git repository"/"untracked file" that
`git_diff_hunks` already established. `panels::git_diff` (renamed in
spirit, not in module path, to cover both halves) now runs `git diff`
**and** `git blame` on the same background thread per document (one
`ScanResult` tuple of two independent `Result`s, so a failure on one half
never discards the other's still-good result) — same trigger points Phase
1 already wired.

`widgets::editor::painting` gained `paint_blame_annotation` (paints just
past the cursor line's own shaped text, in `theme::line_number`'s color),
`blame_annotation_text` (`"<author> • <relative time> • <summary>"`), and
`relative_time` (coarse bucketed "Xm/h/d/mo/y ago" via integer division on
two Unix-second timestamps, no date/time crate needed; a future timestamp
— clock skew — clamps to "just now" rather than a negative duration). A
blame-porcelain all-zero sha marks an uncommitted working-tree line;
rather than surface git's own generated "Not Committed Yet"/"Version of X
from X" text (accurate but reads as clutter next to a real commit's
summary), `UNCOMMITTED_SHA` gets a short "Uncommitted change" label
instead. View > Inline Blame (`ViewSettings::show_inline_blame` checkbox)
lets it be turned off. Live-verified.

**Phase 3 — stage/commit panel.** A dockable panel listing `git status
--porcelain` as a checkbox tree, a commit-message box + Commit button
(`git commit -F -`).

**Checkpoint 3:** done — `fg_core::status` has `git_status(root)` (`git
status --porcelain -uall` — `-uall` so an entirely-new directory lists
each file individually rather than collapsing to one `?? dir/` line), the
pure `parse_porcelain_status` (one `StatusEntry { path, index_status,
worktree_status }` per line; a rename's `"old -> new"` keeps only `new`,
since the panel only ever displays/toggles a file's *current* path), plus
`git_add`/`git_reset_paths`/`git_commit` (the last piping the message over
stdin via `git commit -F -`, sidestepping shell-escaping/argv-length
concerns a multi-line message typed into the panel would otherwise raise).
Unlike `diff`/`blame` (Phases 1-2, silent auto-refreshes where even "not a
git repository" degrades to empty output), the three mutating calls are
real user-triggered actions — a new `GitCommandError::Failed(String)`
(real stderr, not just "`git` didn't launch") exists specifically so a
real failure (nothing staged, no `user.name`/`user.email` configured, ...)
reaches the user instead of silently degrading.

`panels::git_stage::GitStageState` mirrors `static_analysis`'s own
`spawn_scan`/`poll_scan` background-thread shape (one status slot, one
shared add/reset/commit slot — the panel disables every checkbox and the
Commit button while any of the three is in flight). `poll_op` tracks
whether the just-finished op was specifically a commit (`committing:
bool`) so a successful commit also clears `commit_message`. The panel
groups rows into "Staged Changes"/"Changes" sections with a
`[A]/[D]/[M]/[R]/[C]/[U]` badge per row, and a multi-line commit-message
box + Commit button (disabled with nothing staged, an empty message, or an
op already running).

Dock/persistence follows `terminal_panel_visible`'s own exact shape (View
> "Source Control" checkbox, `egui::Panel::right`). Unlike the terminal
panel there's no session to resume, so `app.rs` instead detects a `false
-> true` visibility transition each frame and fires one
`git_stage.refresh` right then — the panel would otherwise open to a
stale/empty list until the user found the Refresh button.

Live-verify caught a real, generally-applicable egui gotcha: staging/
unstaging via the checkbox felt like it hung — the op finishes almost
instantly on its background thread, but this app runs in egui's reactive
(not continuous) repaint mode, and `git_stage::show` wasn't calling
`ctx.request_repaint()` while a scan/op was in flight, so nothing redrew
the panel until some unrelated input event happened to trigger the next
frame. Fixed by calling `ui.ctx().request_repaint()` whenever
`status_running() || op_running()` — the same pattern
`spring_endpoints::show` already uses for its own background scan; any
future background-op panel needs the same call.

**Phase 4 — hunk-level staging + push.** Per-hunk stage via a hand-built
patch + `git apply --cached`; a Push button surfacing real failure
reasons (auth, no upstream, rejected) through the existing error modal.

**Checkpoint 4:** done — `fg_core::diff` gained `RawHunk`/`FileDiff` (a
hunk's real header + content lines, verbatim — unlike Phase 1's own
`DiffHunk`, which only keeps the *line range* a hunk covers, this keeps
the full text a hand-built patch needs), `parse_file_diff`/`git_file_diff`/
`git_file_diff_cached` (the latter two `git diff --no-color [--cached] --
<path>`, deliberately real default 3-line context rather than Phase 1's own
`-U0` — a hand-built hunk patch needs surrounding context for `git apply`
to locate it unambiguously), and `hunk_patch` (rebuilds one hunk, by
index, into a standalone single-hunk patch: the file's shared preamble
plus just that hunk's own header/lines). `fg_core::status` gained
`git_apply_cached(root, patch, reverse)` (`git apply --cached[--reverse]`,
patch piped over stdin the same way `git_commit` already feeds its own
message) and `git_push` (bare `git push`, relying entirely on the repo's
own configured upstream).

App-side, `panels::git_stage::GitStageState` gained `expanded:
Option<PathBuf>` (one file's hunk breakdown open at a time) with its own
background fetch (`toggle_expand`/`refresh_expanded`, fetching both
unstaged *and* staged `FileDiff`s together — a single `StatusEntry` can
carry both a staged and a further-unstaged change at once, git's own
`MM`-shaped status, and this view's whole point is the complete real hunk
picture for that file) and `stage_hunk`/`unstage_hunk`/`push` (all through
the same shared `op_rx` slot stage/unstage/commit already established).
`poll_op`'s existing "refresh on success" follow-up now also calls
`refresh_expanded` when a row is open, since a hunk stage/unstage shifts
every later hunk's own index.

The panel gained a collapse/expand control per non-untracked row (a
"Stage Hunk"/"Unstage Hunk" row per hunk, staged hunks first) and a Push
button. Live-verify caught a reusable font-coverage gotcha: the expand
control was first built as a `▸`/`▾` text-glyph `small_button`, which
rendered as a tofu box — neither the app's bundled fonts nor egui's
built-ins cover those glyphs (same *class* of bug as the terminal panel's
earlier missing-Nerd-Font-glyph issue, but here in the default UI font,
not the editor font) — and the broken button also silently failed to
expand the row. Fixed by dropping the glyph in favor of a vector-painted
triangle via egui's own `collapsing_header::paint_default_icon` (the same
drawing `CollapsingHeader` uses for itself), which is immune to font
coverage since nothing is shaped as text. Live-verified end to end
afterward, including Push's real error text surfacing through the
existing error modal.

---

## Track 10 — Code folding

**Phase 1 — verify the `FoldMap` dependency, don't assume it.** Read the
current `FoldMap`/import-folding implementation directly; determine
concretely whether it already generalizes to arbitrary user-toggled
regions or needs its own second mechanism, and whether `Track 19`'s full
virtualization work is actually a hard prerequisite or `FEATURES.md`'s
own conservative guess. This phase's _output_ is that determination, not
code — don't write Phase 2 against an assumed answer.

**Determination (this session):** `FEATURES.md`'s `[TODO]` for this track
and `PLAN.md`'s own Build status were both stale — Phases 2 and 3 were
already fully shipped for Java in an earlier session (`crates/syntax/src/
folding.rs`'s `foldable_ranges`, `crates/app/src/widgets/editor/
folding.rs`'s gutter/toggle code, `Document::folded_lines`, and the
Tools/View "Fold All"/"Expand All" menu items all already existed and are
wired end to end). `text_area::FoldMap` (`crates/app/src/widgets/editor/
text_area.rs`) already generalizes to **arbitrary** user-toggled line
ranges — it takes a plain sorted `&[Range<usize>]`, with no dependency on
how those ranges were chosen — so no second mechanism is needed for any
future arbitrary-region folding. `Track 19`'s full virtualization is
confirmed **not** a hard prerequisite: folding already works today against
the existing non-virtualized `text_area` widget, which is direct proof
against `FEATURES.md`'s own conservative guess. The one real gap found:
`syntax::node_kinds::foldable_kinds` returned `&[]` for Kotlin — only
import-block folding worked for it; class/method/control-flow body folding
was Java-only.

**Phase 2 — fold-range computation.** Per-language tree-sitter query for
foldable node kinds (class/method/interface bodies); a collapse/expand
gutter marker at each range's opening line.

**Checkpoint 2:** done — Java side already shipped (pre-existing).
Kotlin's own gap (found in Phase 1's determination above) closed this
session: `foldable_kinds(Language::Kotlin)` now returns `["class_body",
"enum_class_body", "block", "block_comment"]`, verified fresh against
`tree-sitter-kotlin-ng` 1.1.0's real parse output (a throwaway probe test
dumping a real parse tree, per `TECHNICAL_DEBT.md` #3's established
discipline — never assumed from the Java grammar or from `node-types.json`
alone). Real findings from that probe: Kotlin's grammar has no separate
`interface_body`/`enum_body` node kinds the way Java does — `interface`/
`object` declarations reuse `class_declaration`/`class_body`,
distinguished only by keyword, so `class_body` alone already covers
class, interface, and object bodies; `enum_class_body` is the one real
exception with its own kind; a `function_body` node wraps a `block` node
at the exact same span, so folding `block` alone (Java's own "method and
control-flow bodies" convention) already covers method bodies without a
second, redundant `function_body` entry. Live-verified against a real
`.kt` file (block comment, class body, method body, `if`-block, enum
class body, including a nested collapse and both Fold All/Expand All).

**Phase 3 — fold state + toggle.** `folded_ranges: HashSet<usize>` per
document (or per-tab side structure); toggling updates the layout fold-
map (built from the union of this and the existing auto-import folding).

**Checkpoint 3:** done (pre-existing, confirmed by reading `crates/core/
src/document.rs`'s `Document::folded_lines` and `crates/app/src/widgets/
editor/widget.rs`'s wiring directly): fold state is exactly this shape
already. Java's own gutter/toggle behavior was live-verified in the
earlier session that shipped it; the Kotlin gutter markers this session's
fold-kind addition newly makes visible were live-verified this session
too.

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
before it lands.** Unblocked this session once Track 21's own 3 phases
landed.

**Phase 1 — metadata extraction + candidates.** Once classpath resolution
exists: scan resolved dependency jars for bundled `spring-configuration-
metadata.json`, parse into completion candidates, feed the existing
completion popup keyed by typed prefix.

**Checkpoint 1:** done.

`crates/core/src/spring_config_metadata.rs` has `SpringConfigProperty {
name, type_name, description, default_value: Option<String> }`,
`parse_metadata_json`, `scan_jar_for_metadata` (opens a jar as a zip via
the `zip` crate and reads just its `META-INF/spring-configuration-
metadata.json` entry without a full extraction), and
`scan_classpath_for_metadata` (scans every jar in a resolved classpath,
silently skipping one that can't be opened). `properties[].defaultValue`
is genuinely heterogeneous JSON (a bool, string, or number depending on
the property's own type — confirmed against a real
`spring-boot-autoconfigure` jar), handled via a custom `deserialize_with`
that stringifies whatever value is there rather than modeling it as a
fixed Rust type. A jar with no bundled metadata at all correctly degrades
to an empty result rather than an error.

App-side: `crates/app/src/panels/spring_config.rs` has `SpringConfigState`
— lazily triggered (`ensure_scanning`, called from the completion trigger
itself the first time a `.properties`/`.yml` file actually needs
candidates, not eagerly on every project open, since real classpath
resolution shells out to `mvn`/`gradle` and can be genuinely slow) rather
than following `static_analysis`'s own menu-triggered shape. `scan_project`
detects the build tool by file presence and handles a real gap
`maven_classpath`/`gradle_classpaths` (Track 21 Phase 3) left open on
their own: a Maven **aggregator** `pom.xml` (`<packaging>pom</packaging>`,
real multi-module `<modules>`, no dependencies of its own) has nothing for
`mvn dependency:build-classpath` to resolve at its own root, so
`maven_module_tree_classpath` reads the aggregator's own `<modules>`
(`fg_core::parse_pom`) and unions each real module directory's own
classpath instead; Gradle needs no such special-casing since
`gradle_classpaths` already walks the whole multi-module tree from one
root invocation.

`crates/app/src/widgets/editor/spring_config_completion.rs` is the pure
candidate-generation half: `properties_completion_candidates` (flat,
every property's full dotted name — `.properties`) and
`yaml_completion_candidates` (one candidate per *distinct next segment*
under a given ancestor prefix, deduped — offering a deeply-nested key as
one flat candidate would be invalid YAML if accepted verbatim, so this
drills exactly one level at a time, matching how a user actually extends
a mapping). `yaml_ancestor_path` reconstructs the dotted key path
enclosing a given line via **indentation**, deliberately not a
tree-sitter parse: the line actually being completed is, by construction,
either not yet a valid `block_mapping_pair` at all or mid-being-typed, so
a parse-tree walk would have to fight exactly the spot this needs to
read, whereas every line above the one being typed is already complete,
trustworthy text. `key_segment_before_cursor` is a dash-inclusive sibling
of `templates::word_before_cursor`: real Spring property key segments are
routinely kebab-case (`context-path`, `pool-name`), which the existing
alnum-or-underscore-only definition would incorrectly split mid-segment.

Wired into `widget.rs`'s existing completion-trigger machinery as a new,
mutually-exclusive-with-the-generic-one trigger gated on `Language::
Properties`/`Language::Yaml`: anchored whole-line for `.properties` (a
flat key's dots must all stay part of one filterable prefix — typing
`server.po` has to match `server.port`) versus per-segment for `.yml`
(`key_segment_before_cursor`, paired with `yaml_ancestor_path` to build
the filter prefix); gated to key position only (nothing opens once a
`=`/`:` has been typed on the current line). `CompletionKind` gained a
`Property` variant. As a real side effect of finally giving
`CompletionItem::detail` a producer (a property's type and default, e.g.
`"java.lang.Integer = 8080"`), the popup's paint function was extended
(`row_text`, an `egui::text::LayoutJob`) to render `detail` dimmed after
the label, via `ui.visuals().weak_text_color()`.

Live-verified against a real Gradle/Kotlin/Spring Boot 4 project: a
partial key in a real `application.properties` opened the popup with real
candidates and their type/default shown; a `.yml` file typing `server:`
then a nested `po` correctly offered just `port`.

**Addendum — Spring annotation completion + auto-import (not in the
original per-phase plan above; a second Spring-flavored completion source
added alongside property autocomplete, sharing this track since both are
"complete a Spring thing, backed by a fixed candidate table" in shape).**
Typing `@` in a `.java`/`.kt` file opens the completion popup unfiltered
(same "open immediately, let the ordinary prefix filter narrow it" trigger
shape property autocomplete's own `.properties`/`.yml` trigger already
uses), offering every annotation in a fixed `SPRING_ANNOTATIONS` table
(`crates/app/src/widgets/editor/spring_annotation_completion.rs`, new) —
scoped to genuine `org.springframework.*` packages only, verified against
real jars on this machine (`spring-context`/`spring-beans`/`spring-web`/
`spring-tx`/`spring-boot-autoconfigure`). JSR-250 annotations
(`@PostConstruct`/`@PreDestroy` and similar) are deliberately excluded even
though they're routinely used in Spring code: their real package is
`javax.annotation.*` pre-Spring-Boot-3 or `jakarta.annotation.*` for
Spring Boot 3+, and there's no reliable signal yet for which namespace a
given project is on — inserting the wrong one would be a silent
correctness bug, not just an incomplete list. The trigger itself is
guarded against firing inside a comment/string (a stray `@` in `//
user@example.com`) via `syntax::highlight_spans`, recomputed fresh only on
the frame `@` is actually typed.

Accepting a candidate also inserts a matching `import`, if the file
doesn't already have one — `crates/syntax/src/imports.rs` is the shared
primitive: `existing_imports` walks the root node's direct
`import_declaration`/`import` children (both languages' own grammars keep
imports un-nested, so no recursive walk is needed) into an ordered
`Vec<ExistingImport>`, and `import_insertion` does a pure string
comparison against a new path to decide `AlreadyImported` / `Before(byte)`
/ `AfterLast(byte)` — a real gap disclosed rather than hidden: an existing
wildcard import (`import org.springframework.stereotype.*;`) covering the
new path isn't detected as "already imported." `spring_annotation_
completion::apply_with_import` splices the import into the already-
completed text (computed from the *pre*-completion tree/text, since
`import_insertion`'s byte offsets are only valid against that coordinate
space) and shifts the cursor by the inserted text's own length; a file
with no imports yet lands the first one right after the `package`
declaration (`package_declaration`/`package_header`, Java/Kotlin), or at
the very top of the file if there's no package declaration either. Kotlin
imports omit the trailing `;` Java's own get.

`CompletionKind` gained an `Annotation` variant; the popup's `detail`
shows the annotation's own import path so the popup doubles as a reminder
of which package it comes from before accepting.

Live-verified: typing `@` in a real `.java` file, accepting a candidate,
confirms the annotation and a correctly-placed `import` both land, and
that an already-imported annotation doesn't duplicate its import.

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

**Checkpoint 1:** done — `cargo test -p foxgarden` green (905 passed).
`lsp_state::LspState::request_code_action` sends `textDocument/codeAction`
for a diagnostic's own range, reconstructing it as a real `lsp_types::
Diagnostic` (range/severity/message only — `fg_core::Diagnostic` keeps no
more) for `context.diagnostics`. `widgets::editor::code_action::
CodeActionGutter` tracks the caret's own line (no dwell delay, unlike
`hover` — re-requests the instant the line/its diagnostic changes),
decodes the reply into `Offer { title, edit }` — only entries that already
carry a real `WorkspaceEdit` are ever kept, per this phase's own stated
scope — paints the lightbulb, and owns the picker popup. Applying a picked
offer needed no new app.rs-level polling state (unlike rename): the edit
is already resolved by the time it's picked, so `take_confirmed` hands the
`WorkspaceEdit` straight to a newly shared `crate::workspace_edit::apply`
— extracted from `rename.rs`'s own private per-file-edit logic (`edits_by_
file`/`apply_file_edits`/`apply_text_edits`, unchanged) once a second real
caller needed the exact same thing, rather than duplicating it.

Two real, live-verify-driven findings, neither assumed:

1. **jdtls' real quick fixes for the checkpoint's own named example (an
   unused import) don't carry `CodeAction.edit` at all.** Every offer a
   real jdtls 1.60.0 returned for `"The import java.util.List is never
   used"` — "Organize imports" (×2), "Generate toString()", "Change
   modifiers to final where possible" — came back as a bare `Command`
   named `java.apply.workspaceEdit` whose single argument *is* the
   `WorkspaceEdit`, meant to be applied client-side with nothing sent back
   to the server. Found by dumping the actual raw reply (a first pass that
   only accepted `CodeAction.edit` parsed **zero** offers from a real
   4-offer reply). `offer_from_item` now special-cases exactly this
   command name; every other bare `Command` (a genuine server-side
   `workspace/executeCommand` action) is still correctly left out — this
   app implements neither `workspace/executeCommand` nor a server-
   initiated `workspace/applyEdit` reply, so attempting one would silently
   do nothing.
2. **Clicking the lightbulb itself used to make it vanish**, because the
   gutter-width/lightbulb code originally gated on `shell_out.caret` —
   which `ShellOutput`'s own doc comment states plainly is `None`
   whenever the text area *isn't focused this exact frame*, and clicking a
   separate `ui.interact` widget elsewhere in the same frame (the
   lightbulb, in the gutter, is exactly this) clears that focus under
   egui's default input handling. Switched to `text_area::peek_caret`
   (the persisted caret, already what the gutter-width precomputation
   correctly used) for the `update`/`clear` call too. Generally
   applicable beyond this one feature: any future gutter-region clickable
   (this codebase's fold-arrow gutter already has the same shape of
   `ui.interact` call) needs the persisted caret, not `shell_out.caret`,
   for anything that must survive being clicked itself.

Live-verified end to end, headless (`Xvfb`+`xdotool`, same harness as
Track 17/19/20; `app.ron` backed up/restored byte-for-byte around a
throwaway single-file project with `import java.util.List;` genuinely
unused) against the real jdtls 1.60.0 above: placing the caret on the
diagnostic's line showed the amber lightbulb; clicking it opened a popup
listing all four real titles; picking "Organize imports" removed the
unused import from the *open tab's own buffer* (correctly left the disk
file untouched — no open tab is what triggers a disk write, per
`workspace_edit::apply_file_edits`'s own existing rule) and marked the tab
dirty (`*Main.java`), exactly the existing edit-application path Checkpoint
7 (rename) already established.

---

## Track 17 — Peek definition

**Hard dependency on Track 20 (`LSP integration`)'s go-to-definition —
not startable before it lands.**

**Phase 1 — inline peek panel.** A shortcut/gutter icon opens an inline
expandable read-only panel showing the resolved definition's surrounding
lines, without switching tabs; Escape/click-outside collapses it.

**Checkpoint 1:** done — full suite green. Live-verified headless (`Xvfb`
+ `xdotool`, screenshots read back frame-by-frame — no real display/mouse/
keyboard touched) against a real `jdtls` and a throwaway two-file Java
project (`~/.local/share/foxgarden/app.ron` backed up before pointing it
at the scratch project, byte-for-byte restored after): Alt+F12 on a
cross-file call (`h.greet(...)`, defined in a sibling file) opened the
inline panel showing `Helper.greet`'s real source with the target line
highlighted, `Main.java` stayed the active tab throughout (no switch), the
`+`/`-` toggle button responded, and Escape closed the panel leaving the
main editor's tab/scroll position untouched.

---

## Track 18 — Inline diff viewer widget

**Phase 1 — diff computation + rendering.** `show_diff(ui, old, new,
mode: DiffMode)` using a line-level diff (verify the `similar` crate's
current status before pinning it, or an equivalent) rendered as
side-by-side or inline colored rows, via the editor's own font/theme.

**Checkpoint 1:** done — `crates/app/src/widgets/diff_view.rs` has
`diff_line_ops` (the `similar` crate, pinned `3.1.1`; `TextDiff::
from_lines`'s `ops()` mapped 1:1 into this module's own `DiffLineOp::
{Equal,Delete,Insert,Replace}`, each carrying 0-based line-*index* ranges
matching `old.lines()`/`new.lines()` directly) and `show_diff(ui, old,
new, mode, editor_font, font_size, dark_mode)`. `SideBySide` (an
`egui::Grid`, two columns, a `Replace`'s uneven old/new line counts
padding the shorter side with blank rows so both columns stay aligned)
and `Inline` (one column, unified-style `+`/`-`/`  `-prefixed rows, a
`Replace`'s old lines immediately followed by its new lines) both build
from the same pure `side_by_side_rows`/`inline_rows` row-builder functions
— `theme::diff_added`/`diff_removed` (Track 9 Phase 1's own diff gutter
colors) reused as a translucent row-background wash rather than the
gutter's own full-opacity bar, since a full-strength fill behind text
would overwhelm it; the text itself stays `theme::default_text`
throughout, the background alone carrying the added/removed distinction.

Live-verify surfaced a real follow-up: a long untouched stretch showed
every single line rather than being abridged the way a real `git diff`'s
own limited context already is. Fixed by making `side_by_side_rows`/
`inline_rows` collapse the middle of any `Equal` run longer than `2 *
context` lines to a single `Collapsed` placeholder (`SideBySideRow::
{Line, Collapsed}`/`InlineRow::{Line, Collapsed}`), keeping
`DEFAULT_CONTEXT_LINES = 3` lines bordering each side — git's own `-U3`
default, already this codebase's own choice for `fg_core::git_file_diff`'s
real context (Track 9 Phase 4), so this widget now abridges the same way
a real `git diff` does. The collapsed marker's own label (`"... N
unchanged lines ..."`) is deliberately plain ASCII, not a Unicode
ellipsis/box-drawing glyph — the Source Control panel's `▸`/`▾` tofu-box
bug (Track 9 Phase 4) already proved neither this app's bundled fonts nor
egui's built-ins can be trusted to cover an arbitrary glyph in the default
UI font.

A second live follow-up: stage a hunk directly from inside the diff
window, rather than needing to close it and use the row's own separate
inline hunk list. `panels::git_stage`'s "Full Diff" window
(`open_full_diff`/`show_full_diff_window`) gained a "Hunks" section
reusing `show_hunks` and the exact same underlying `expanded_diffs` data
(from `fg_core::git_file_diff`/`git_file_diff_cached`, a real `git
diff`/`git diff --cached` run) the row's own inline expand arrow already
renders. This was a deliberate design choice, not a shortcut: `show_diff`'s
own diff is computed by the `similar` crate, a *different* diffing
algorithm than git's own, and while both would very likely group the same
two texts' changes identically in the common case, "very likely" isn't a
safe foundation for deriving a `git apply --cached` patch — a mismatched
hunk boundary could stage the wrong lines. Sourcing the stage/unstage
action from `expanded_diffs`' real, git-sourced hunks instead sidesteps
that risk entirely rather than trying to reconcile two independent diff
engines. Live-verified, including staging/unstaging a hunk from inside the
window and confirming via `git diff --cached`.

---

# Major tier

## Track 19 — Large file handling — full viewport virtualization

**Phase 1 — bounded word-wrap row-count computation.** The current
`cached_row_counts`/`layout_visible_wrapped` path (the part that still
scales with total file size even after the tab-switch cache fix) replaced
with an incrementally-maintained or viewport-bounded equivalent that
doesn't need every line's row-count computed up front.

**Checkpoint 1:** done — `cached_row_counts` (`render.rs`) no longer shapes
every line on a miss; it seeds a cheap "1 row per line" baseline
(`default_row_counts`, `usize` writes only, no font layout) and lets
`layout_visible_wrapped` write real counts back (`record_shaped_rows`) for
just the lines it already shapes to paint, so a scroll-only sequence of
frames progressively learns the file without ever shaping an off-screen
line. `cargo test -p app` green (897 passed); new tests assert shaped-line
count stays viewport-bounded and identical between a 2,000- and a
200,000-line buffer, that a learned row count survives a scroll-only
frame, and that a post-edit reshape stays bounded too. The synthetic
benchmark (`huge_file_first_open_and_per_keystroke_cost_stays_bounded`,
200,000 long lines, word-wrap on) measured **19 lines shaped / 14.865ms**
on first open — not the 200,000 real `shape_line` calls the old path paid
— with a post-edit reshape equally bounded. Live-verified headless
(`Xvfb` + `xdotool`, screenshots read back — real display/mouse/keyboard
never touched; the app's own `rfd::FileDialog::pick_folder()` routes
through the system's `xdg-desktop-portal` rather than the app's own X
display, so this session instead pre-seeded `app.ron`'s `last_project`/
`open_tabs`/`active_tab` keys directly under an isolated `XDG_DATA_HOME`
to open the scratch project with no dialog at all) against a real
200,000-line file: instant open, smooth mouse-wheel scrolling with no
stutter or redraw artifacts once settled, a keystroke registered instantly
(dirty-tab asterisk), and Ctrl+End landed exactly on line 200001 (the
file's true last, empty line after its final newline) — despite that line
never having been shaped before, no imprecision was actually observed in
practice, better than this phase's own accepted-approximation trade-off
anticipated. Known, accepted limitations from this design (never
observed as a live problem, but worth having read before touching this
code again): a freshly opened huge file's scrollbar undercounts
never-shaped wrapped lines until scrolled through; an off-screen keyboard
jump landing on a never-visited wrapped line is only as accurate as the
1-row baseline until a subsequent scroll settles it (same accepted class
as `TECHNICAL_DEBT.md` #15's `tabs.rs` jump math). Separately, live-verify
surfaced a pre-existing, unrelated crash: `xdotool windowclose` against a
real running instance under Xvfb (no window manager) panics inside
`winit::platform_impl::linux::x11::window` on `TranslateCoordinates`
during shutdown — not touched by this phase, not investigated further,
flagged here in case it resurfaces in a future headless click-through.

**Phase 2 — hand-built widget: layout + click-to-position.** Replaces
`egui::TextEdit` for the visible-row-only case, reusing `layout_visible`'s
existing `char_rect`/`row_galleys` helpers for hit-testing, scoped to only
the visible slice.

**Checkpoint 2:** done — turns out already shipped, ahead of this track's
own numbering: `text_area::shell::show_interactive` (commit `27d78dc`,
2026-07-25) replaced `egui::TextEdit` for the live editor entirely before
this Track's own write-up (`a4178a2`, 2026-07-27) was drafted, using
exactly `char_rect`/`row_galleys` for hit-testing as described. Re-verified
live at this phase's actual target scale now that Phase 1 makes it
meaningful: headless (`Xvfb`+`xdotool`) against the same real 200,000-line
file, clicking (then `Home`) at a point deep in the file (~line 199978)
placed the caret exactly there instantly, including live-triggering the
completion popup on the next keystroke — no perceptible lag versus a small
file, the checkpoint's own bar.

**Phase 3 — drag-select.** Reimplemented against the new widget, studied
against `../references/zed`'s own non-`TextEdit` editor as the concrete
precedent (real production code, not egui's own internals, which never
had to solve this at this file's own scale).

**Checkpoint 3:** done, code-level — same pre-existing `show_interactive`
covers this too (`response.dragged()`/`drag_started()`, `shell.rs`
~292-313), including the block-selection case
(`alt_drag_produces_a_rectangular_block_selection_spanning_multiple_rows`,
a real egui-synthetic-event drag, not a mocked one). Live-verify against
the huge file only partially landed this session: keyboard-driven
selection (`Shift+Right` × 40 after `Ctrl+Home`) rendered a correct
selection highlight instantly on line 1 of the real 200,000-line file,
proving the selection/rendering half works at scale — but reproducing an
actual mouse-button-held drag via `xdotool mousedown`/`mousemove`/
`mouseup` as separate process invocations never once registered as a drag
under this no-window-manager Xvfb session (each attempt landed as a plain
click, no selection), despite click-to-position itself working reliably
moments earlier with the same pointer-position pipeline. Not chased
further — X11 motion-event coalescing/timing across separate `xdotool`
process spawns is a plausible, mundane explanation, and the underlying
drag code path already has real (non-mocked) coverage via Phase 3's own
`alt_drag_...` test. Flagged rather than assumed: a genuine mouse-drag
live-verify at huge-file scale, crossing a scrolled viewport boundary
specifically, is still owed if a more reliable input-synthesis method
becomes available (e.g. a real display instead of Xvfb, or a Playwright-
style continuous-pointer driver).

**Follow-up session (2026-08-23):** revisited specifically to close this
gap. Found and fixed a real harness bug along the way: `xdotool click`
(the single-shot subcommand) was silently non-functional against this
app's window this session — no press/release ever reached the editor,
menu, or side panel, only hover styling — while explicit
`xdotool mousedown 1; sleep; mouseup 1` worked correctly every time
(confirmed: File menu opened, editor accepted clicks and typed text).
Recorded in memory for future sessions. With that fixed, a real
mouse-button-held drag (`mousedown` → several `mousemove` steps with
short sleeps → `mouseup`, all real X11 motion, not egui-synthetic)
was reproduced exactly once, right after a fresh app launch, at the
top of the file — a genuine blue selection spanning lines 2-5 of the
wrapped huge file. Every subsequent attempt at the actual target scale
(scrolled ~200,000 lines deep, crossing the viewport) landed as a plain
click regardless of step count, spacing, or click-vs-mousedown/up choice
— including immediately after a fresh relaunch with no prior interaction
at all, ruling out stuck X server button/modifier state as the cause.
Net: still not reliably reproducible at the scale that matters, and
still not chased further — the code path's non-mocked test coverage
stands unchanged, and this now reads as a genuine limitation of
synthesizing a held drag via `xdotool` process calls against a
WM-less Xvfb session, not an app defect. Same "still owed" verdict,
now with a ruled-out cause (stuck server state) and a confirmed
harness fix (mousedown/mouseup over click) that removed a confound
from earlier attempts.

**Phase 4 — IME composition.** Reimplemented against the new widget; same
`../references/zed` precedent.

**Checkpoint 4:** done, code-level — same pre-existing `show_interactive`
handles this too (`ime_preedit_previews_text_then_commit_finalizes_it`,
a real headless test of the preview/commit sequence). Not live-verified
this session: no CJK input method is installed in this sandbox, and
setting one up (ibus/fcitx + a CJK engine) was judged out of scope for
this track's own time budget. Recorded as owed, not assumed, matching
this file's own convention for a gap the environment — not the code —
currently blocks (e.g. Track 20's own still-owed Kotlin GUI
click-through).

**Follow-up session (2026-08-23):** the sandbox gained `ibus-mozc` (user
installed it with sudo mid-session, resolving the earlier no-apt-root
blocker), so a real attempt became possible. Set up `ibus-daemon` on the
same Xvfb display and `XMODIFIERS=@im=ibus` for the app process; an
`ibus-xim` window appeared on the display, confirming the XIM server was
live and registered. Typed romaji (`k`, `a`) with real pacing: no preedit
text appeared in the app while composing (consistent with the app not
implementing XIM's on-the-spot preedit callback, so if IBus was holding
a composition buffer it was invisible rather than drawn inline), and on
`Return` the literal string `ka` landed in the buffer — not a converted
かな. That means real kana/kanji conversion was not demonstrated; the
observed behavior is also consistent with the keys simply passing
through as plain ASCII without IBus/mozc ever engaging. Inconclusive
either way, cleaned up (undone, disk file unchanged). Still owed, same
as before, now with a documented real-engine attempt on record instead
of "no engine installed at all."

---

## Track 20 — LSP integration

**Phase 1 — server lifecycle + handshake.** Child-process management for
`jdtls`/`kotlin-language-server` (external-tool paths via Settings,
mirroring Track 5's own convention); `lsp-types` for protocol structs;
stdio JSON-RPC framing read/write loop on a background thread per server
(mirroring `PtySession`'s own background-reader-thread shape).
`initialize`/`initialized` handshake, no user-visible feature yet.

**Checkpoint 1:** done — `lsp_client::LspSession` (JSON-RPC-over-stdio
framing, a background reader thread routing responses/server-originated
messages, `initialize`/`initialized`) plus `lsp_state::LspState`, the
app-owned lifecycle deciding whether/when a session should exist at all —
at most one `jdtls`/`kotlin-language-server` process for the one open
project, strictly behind `LspSettings::enabled`, retried/retired as the
project's open documents or the settings themselves change. Live-verified
against a real `jdtls` 1.43.0 (Java 17) completing the full handshake
against a real single-file Java project with no build file.

**Phase 2 — diagnostics.** `textDocument/publishDiagnostics` feeds the
existing `Diagnostic`/squiggle pipeline as a second source.

**Checkpoint 2:** done — `textDocument/didOpen`/`didChange`/`didClose`
(full-text sync; `Document::lsp_version`/`lsp_sync_pending` track which
open document still needs a resend) and `textDocument/publishDiagnostics`
(UTF-16 position → byte range via `lsp_state::utf16_range_to_bytes`) feed
`Document::lsp_diagnostics`, chained into the existing squiggle-paint
pipeline (`widgets/editor/widget.rs`) right alongside `checkstyle_
diagnostics`/`pmd_diagnostics`. Live-verified: a real `jdtls` process
against code calling an undefined method (valid syntax, so this
codebase's own tree-sitter parser reports nothing) rendered a real
squiggle with genuine ECJ-produced hover text, not this codebase's own
diagnostics.

One real, generally-applicable gotcha this live-verify surfaced: nothing
requested a repaint after handing work to `LspState::sync`, so a real
server's own *unprompted* `publishDiagnostics` (`jdtls` validates and
reports right after `didOpen`, with no further client action to react to)
sat unread on the background channel until some unrelated input event
happened to repaint the window. Fixed by `LspState::wants_repaint` plus
`ui.ctx().request_repaint_after(...)` in `app.rs`'s update loop — the same
throttled-polling shape `pty_session`/`terminal_widget` already use for
background work; any future background-driven UI update needs the same
treatment.

**Phase 3 — hover docs.** `textDocument/hover` feeds a tooltip,
structurally mirroring the existing syntax-error hover.

**Checkpoint 3:** done — `cargo test -p foxgarden` green. Live-verified
headless (`Xvfb`+`xdotool`, same harness as Track 17/19/20's other
checkpoints; `~/.local/share/foxgarden/app.ron` backed up before pointing
it at a throwaway single-file `ArrayList` project, byte-for-byte restored
after) against a real jdtls 1.60.0: hovering `ArrayList` rendered the real
`java.util.ArrayList<String>` Javadoc, not this codebase's own diagnostics.
One real harness gotcha worth keeping: nothing repaints on a *stationary*
pointer (no cursor blink, no unrelated input) until `HoverState`'s own
request lands, so a single `xdotool mousemove` to the target and a plain
wait never crossed `HOVER_DELAY` — small in-place jitter (a couple of
pixels, still inside the same identifier's span so `stays` keeps matching)
every ~100ms for over 500ms was needed to keep frames flowing long enough
for the dwell timer to actually fire.

This live-verify also closed `TECHNICAL_DEBT.md` #22 (its own proposed
stripping-route fix was already implemented in `hover_text_from_response`/
`strip_markdown`, just never re-verified against a real reply): the real
`ArrayList` Javadoc surfaced jdtls' single-asterisk `*word*` emphasis,
distinct from the `**bold**` case #22's own fixture covered — `strip_
inline_markdown` gained a `.replace('*', "")` pass (after the existing
`**` one, so a bold marker's two leftover single asterisks are also
caught), degrading both italic emphasis and a leading `* ` bullet marker
to plain text. Re-verified live after the fix: the same tooltip renders
with no stray asterisks. See #22 for the closing note.

**Phase 4 — go-to-definition.** `textDocument/definition` reuses
`pending_navigation`'s existing cross-tab-jump primitive.

**Checkpoint 4:** done — full suite green. Live-verified headless (`Xvfb`
+ `xdotool`/screenshots, real desktop untouched — see Track 17's own
checkpoint for the harness) against a real `jdtls`: Ctrl+Click on a
same-project cross-file symbol (`new Helper()`) switched tabs and landed
the caret exactly on `Helper`'s class declaration; Ctrl+Click on
`ArrayList` (a JDK type, no file in the open project's own tree) opened a
new tab with `jdtls`' real decompiled `java.util.ArrayList` source
(genuine Javadoc included) and landed on its no-arg constructor — the
`jdt://` + `java/classFileContents` round-trip this checkpoint exists to
prove.

**Phase 5 — autocomplete.** `textDocument/completion` as a second
candidate source merged into the existing completion popup.

**Checkpoint 5:** done (Java side) — `LspState::request_completion`
(`lsp_state.rs`) sends `textDocument/completion`, flushing the document's
own pending edit first via `sync_one_document` (split out of
`sync_documents` so a single-document flush doesn't misdetect every
*other* open document of that language as closed) — necessary because
this frame's `sync()` already ran *before* the keystroke that both lands
the triggering `.` and calls `request_completion`, so without an explicit
flush the request would race ahead of its own `didChange` on the same
ordered stdin pipe. Decode/merge (`completion.rs`) derives a bare
insertable name and a real `has_params` signal straight from the server's
own `label` text (`bare_label_and_has_params`) rather than trusting
`insertText`/`textEdit`, since real servers (jdtls) routinely
signature-decorate `label` and may format `insertText` as an unsupported
`Snippet` — this sidesteps ever inserting broken `$1`/`${1:x}` syntax.
Two real bugs surfaced and fixed during live-verify: (1) a JDK-typed
receiver's popup opened empty and was then closed the same or next frame
by *two* separate "close if empty" checks that predate this phase,
neither of which knew an async LSP reply could still populate it — both
gained a `has_pending_lsp()` guard; (2) confirmed via a real `jdtls` that
`list.`/`array.` on `List<String>`/`int[]` show real JDK members with
clean names, not signature-decorated garbage. Kotlin side: a raw
JSON-RPC probe (bypassing FoxGarden entirely) confirmed `kotlin-
language-server` itself returns correct completions for
`MutableList<String>`, but the in-app GUI check was inconclusive —
recorded as `TECHNICAL_DEBT.md` #18 rather than assumed fixed (later
resolved — see Build status).

**Phase 6 — find-references.** `textDocument/references`, a results-list
UI (popup or panel depending on typical result count observed live).

**Checkpoint 6:** done — `lsp_state::LspState::request_references`
(`textDocument/references`, `include_declaration: true` — a symbol's own
declaration is itself a legitimate "place this is used", matching VS
Code/IntelliJ's own convention) plus `widgets::editor::references::
FindReferencesState`, a Shift+F12-triggered popup listing every hit as
`path:line  <source line preview>`; a row click jumps there via
`pending_navigation`, same cross-tab primitive Ctrl+Click already uses,
closing the popup on the way. Each `Location` is resolved into a real
byte offset (and its own preview line) once, right when the reply lands
— `decode_references`/`resolve_hit`, reading the focused tab's own live
buffer for a same-file hit and disk otherwise — so a row click needs no
further UTF-16 conversion, unlike `goto_definition::Target::File`'s own
still-UTF-16 `Range`. Full suite green (884 tests, `references.rs`'s own
decode/preview logic covered directly).

Live-verified headless (`Xvfb` + `xdotool`, same harness as Track 17/
Track 20 Phase 4's own checkpoints — real display/mouse/keyboard never
touched) against a real `jdtls`: Shift+F12 on `Helper.greet`'s own
declaration opened a "N references" popup, and clicking a row jumped
tabs and landed the caret exactly where the row said. One real harness
gotcha surfaced here, worth recording for any future headless-GUI
click-through: `xdotool key shift+F12` (the single-combo chord form)
silently dropped the Shift modifier against this app's `egui`/`winit`
event loop even though the same form worked fine for `alt+F12` and
`ctrl+a` earlier — `xdotool keydown shift` / `key F12` / `keyup shift`
as three separate calls carried the modifier correctly every time.
Separately, this scratch project's own lack of a `pom.xml`/build file
(deliberately minimal, matching Track 20 Phase 1's own single-file-
project precedent) meant `jdtls` itself answered `references` with a
mix of real hits and clearly-bogus ones (unrelated JDK-internal
`jdt://` locations, and two `Main.java` hits whose char ranges land on
`static`/`String`, not `greet`) — a real limitation of `jdtls`'s own
no-project fallback search, not a bug in this decode/render path, which
faithfully reproduced whatever the server sent either way. A real
Maven/Gradle-backed project (Track 21) would give `jdtls` proper
classpath/reference indexing and a materially better answer here.

**Phase 7 — rename-symbol.** `textDocument/rename`, applying a
`WorkspaceEdit` across every affected file (open or not).

**Checkpoint 7:** done — `lsp_state::LspState::request_rename`
(`textDocument/rename`) plus two new pieces split the same way Track 17/
Phase 6 already split their own UI-vs-apply halves: `widgets::editor::
rename::RenameBox` is F2's own inline "new name" box (pre-filled with the
identifier under the caret, Enter/Escape/click-outside), a per-tab widget
concern with no LSP access of its own; `rename::RenameState` (this
crate's root, alongside `goto_definition.rs`) is everything past Enter —
firing the request, decoding the reply's `WorkspaceEdit`
(`document_changes` preferred, `changes` the fallback, per the spec's own
stated preference), and rewriting every file it names: an open tab's own
live buffer (bumped `lsp_version`/`lsp_sync_pending`, a fresh parser —
`reload_tab_from_disk`'s own sequence, except `saved_buffer` is
deliberately left alone so the tab shows dirty, same as a real edit the
user typed) for a file that's open, straight to disk otherwise. Each
file's own edits are applied furthest-in-the-file-first so an earlier
edit's byte offsets never shift under a later one still waiting. Full
suite green (892 tests; `rename.rs`'s own edit-application math and
`document_changes`-vs-`changes`/`Operations`-skip decoding covered
directly, `RenameBox`'s own prefill covered in `widgets/editor`).

Live-verified headless (`Xvfb`/`xdotool`, same harness as the two
checkpoints above) against a real `jdtls`, renaming `Helper.greet` to a
new name from Main.java's own call site: the box opened pre-filled with
"greet", Enter applied the edit to **both** open tabs correctly — the
call site in `Main.java` and the declaration in `Helper.java` — each
showing the dirty-tab asterisk immediately, no auto-save. The "a file
that isn't open gets edited on disk" half of this checkpoint's own
wording did *not* get a clean live confirmation: a third file in the
same no-`pom.xml` scratch project, containing another real call to the
same method but never opened as a tab, was left completely untouched on
disk after the same rename. Given Track 20 Phase 6's own already-
recorded finding — this exact project's `jdtls` answering `references`
with an incomplete/noisy list, a known limitation of its no-build-file
fallback search, not a bug in this app's own decode/apply path — the
likely explanation is the same one: `jdtls` itself never named that
third file in its own `WorkspaceEdit` reply, so there was nothing here
to apply. Not confirmed by directly inspecting the raw reply (unlike
Phase 6's own investigation, which did capture and inspect it) — flagged
here rather than assumed, since the alternative (a real bug in `rename::
apply_reply`'s per-file loop silently stopping early on an unrelated
file's own error) hasn't been ruled out either. A real Maven/Gradle-
backed project (Track 21) removes the likelier cause; re-verify the
not-open-file path once one's available, and if it still fails there,
treat it as this app's own bug rather than `jdtls`'s.

---

## Track 21 — Maven/Gradle awareness

**Phase 1 — `pom.xml` parsing.** `MavenProject` struct from
`<dependencies>`/`<modules>`/`<properties>` via `quick-xml`/`roxmltree`
(verify current crate health before pinning).

**Checkpoint 1:** done — `crates/core/src/maven.rs` has `parse_pom`, a
pure/no-I/O `Event`-driven `quick-xml` walk (`quick-xml` — already a
`core` dependency, reused rather than adding a second XML crate) into
`MavenProject { group_id: Option<String>, artifact_id: String, version:
Option<String>, packaging: String, parent: Option<MavenParent>,
properties: HashMap<String, String>, modules: Vec<String>, dependencies:
Vec<MavenDependency> }`. Deliberately scoped to what Phase 1 actually asks
for — no property substitution (a `${foo.version}` placeholder is kept
verbatim, unresolved) and no reading of `<dependencyManagement>` at all;
real version *resolution* is Phase 3's job via `mvn dependency:
build-classpath`, sidestepping reimplementing Maven's own effective-POM/
BOM computation entirely.

Verified against real multi-module `pom.xml` files pulled from this
machine, not synthesized. Real complications the parser had to handle: a
child module's `pom.xml` routinely declares no `<version>` (and sometimes
no `<groupId>`) of its own, inheriting both from `<parent>`; most of a
child module's own `<dependency>` entries carry no `<version>` either,
resolved transitively via the parent's inherited BOM — both recorded as
`None` rather than guessed at. `<dependencyManagement>` wraps a second,
differently-scoped `<dependencies>`/`<dependency>` structure that looks
identical to the project's own real dependencies at the tag-name level; a
`<plugin>` can carry a *third* such block (compiler-plugin artifacts, not
project dependencies at all). Getting all three right needed tracking the
full element path from `<project>` down, not just matching on tag name.
Comments interspersed between `<properties>` children and multi-line
values also had to not corrupt neighboring properties — handled by a
clear-on-`Start`/read-and-clear-on-`End` accumulator, no special-casing
needed. Headless-only checkpoint — nothing in the UI reads a
`MavenProject` yet.

**Phase 2 — Gradle model extraction.** Validate the offline-init-script-
dump approach against a real multi-module Gradle project before
committing further; if it holds up, build the extraction against it
rather than attempting to parse Groovy/Kotlin DSL as text.

**Validation (this session, before writing any production code, per this
phase's own instruction):** confirmed against a real multi-module Kotlin/
Spring Gradle project on this machine (Spring Boot 4 +
`io.spring.dependency-management` + Kotlin JPA/Spring plugins, Kotlin DSL
build scripts). A hand-written Groovy init script registering a task via
`allprojects { tasks.register(...) { doLast { ... } } }`, run as `gradle
--offline --init-script <script> -q <task>`, successfully walked every
project's own `configurations`/`dependencies` and printed a JSON dump —
confirmed working via both the system `gradle` and the project's own
`./gradlew` wrapper, and confirmed `--offline` alone is sufficient (only
each configuration's *declared* dependency notation is read, never real
artifact resolution). This directly disproves needing to parse
Groovy/Kotlin DSL as text, the alternative this phase's own instruction
named.

Two real findings during validation: (1) `ProjectDependency.
dependencyProject` (the API an older/more-commonly-documented approach
uses to resolve a `project(":foo")` reference back to a `Project` object)
has been removed as of this Gradle version — `dependencyProject.path`
needed to become plain `dep.path` instead. (2) A naive first dump
surfaced a pile of purely internal tooling configurations
(`kotlinCompilerPluginClasspathMain`, every plain `*Classpath` resolvable
configuration duplicating what `implementation`/`testImplementation` etc.
already declare) that have nothing to do with a user's own `dependencies
{ }` block — an allow-list filter (`is_dependency_configuration`, kept as
a hand-synced Rust/Groovy pair) was needed before the dump was usable at
all.

**Checkpoint 2:** done — `crates/core/src/gradle.rs` has
`gradle_projects(project_root)`, which writes the validated init script
(`INIT_SCRIPT`) to a temp file, runs `<gradlew-or-gradle> --offline
--init-script <path> -q foxgardenGradleModelDump` in `project_root`,
deletes the temp script, and parses stdout into `Vec<GradleProject>` —
each with `path`/`name`/`group`/`version`/`project_dir` plus
`Vec<GradleDependency>` (an enum: `Project { configuration, path }` for
an inter-module `project(":x")` reference, or `Module { configuration,
group, artifact, version: Option<String> }` for an external coordinate,
`version: None` when unspecified — resolved elsewhere, e.g. via
`io.spring.dependency-management`'s inherited BOM, same explicit "don't
chase it down here" non-goal `maven.rs` established for `pom.xml`, Phase
3's job instead). `gradle_command` prefers a project's own
`<root>/gradlew` when present over a bare `gradle` on `PATH`. Each
project's dump is wrapped in `FOXGARDEN_JSON_BEGIN`/`FOXGARDEN_JSON_END`
text markers rather than assembled into one combined JSON document across
every project — Gradle's own configuration-phase logging (and any
plugin's own stray stdout) is guaranteed to land *outside* those markers,
which a single top-level JSON document would have no way to recover from
if any of it landed mid-document.

`cargo test --workspace` deliberately does **not** shell out to a real
`gradle`/`gradlew` process (unlike Track 9's `git`-based end-to-end
tests, since `git` is a safe universal assumption this codebase already
leans on, but a `gradle` install is not) — mirrors `static_analysis`'s
own convention of testing parsers unconditionally while leaving the
real-binary-invocation path to manual/live verification only.
Headless-only checkpoint.

**Real bug found and fixed while validating Phase 3** (surfaced by a
slower classpath-resolution probe, unlike Phase 2's near-instant
declared-dependency read): a project with `org.gradle.parallel=true` in
its own `gradle.properties` runs each project's `doLast` concurrently —
their `println` output interleaves **line-by-line** across projects, so
one project's own `FOXGARDEN_JSON_BEGIN`/`FOXGARDEN_JSON_END` markers can
land spliced in the middle of a different project's block, silently
corrupting `parse_dump_output`'s assumption that a project's block is
contiguous. This was a latent bug in the already-shipped Phase 2 code too
— it just hadn't manifested on a small/fast enough validation project for
the race to lose. Fixed by adding `--no-parallel` to `gradle_projects`'
own invocation (forces this one read-only metadata dump to run serially
regardless of the target project's own setting — harmless, since it isn't
a real build).

**Phase 3 — dependency-aware classpath resolution.** `mvn
dependency:build-classpath` / Gradle's own resolution task, parsed into a
resolved jar-file list.

**Checkpoint 3:** done.

Maven side: `crates/core/src/maven.rs` gained `maven_classpath(module_root)`,
shelling `mvn -q dependency:build-classpath -Dmdep.outputFile=<temp file>`
and splitting the written file's contents on the real OS classpath-list
separator (`:`/`;` — not `std::path::MAIN_SEPARATOR`, a different
character entirely, for the directory separator, not the classpath-list
one). Deliberately reuses Maven's own dependency-resolution machinery
rather than reimplementing effective-POM/BOM/version-conflict resolution
(the reason Phase 1 stayed scoped to declared-only data). Verified
against a real, cleanly-resolvable Maven project against the real local
`~/.m2/repository` — resolved paths existed on disk and matched the
expected jar names exactly.

Gradle side: `crates/core/src/gradle.rs` gained
`gradle_classpaths(project_root)` and its own `CLASSPATH_INIT_SCRIPT`/
`GradleClasspath { path, compile: Vec<PathBuf>, runtime: Vec<PathBuf> }`,
resolving each project's real `compileClasspath`/`runtimeClasspath`
configurations (`configuration.resolve()`, a real filesystem/network-
triggering call, unlike Phase 2's declared-only reads) rather than
reimplementing Gradle's own dependency graph. `write_init_script`/the
marker-splitting loop inside `parse_dump_output` were both generalized
(`write_temp_script`/`split_marked_blocks`) so this second init-script-
driven dump could reuse them instead of duplicating that plumbing. A
project with neither configuration at all (a non-JVM module) is simply
absent from the result rather than appearing with two empty lists, so a
caller can't mistake "not a JVM module" for "a JVM module with zero
dependencies." Verified against a real, minimal, single-module Gradle
project — both `compile` and `runtime` resolved to the same real,
existing jar path in the local Gradle module cache. `gradle_classpaths`
deliberately omits `--offline` (unlike `gradle_projects`), since real
resolution has to be allowed to actually download anything not yet
cached, the same way a real `gradle build` would.

The `--no-parallel` fix noted under Phase 2 above was actually discovered
by this phase's own classpath-resolution probe (slow enough for the
interleaving race to manifest), not Phase 2's faster declared-dependency
probe.

Headless-only checkpoint — nothing in the UI reads a
`MavenClasspathError`/`GradleClasspath` yet.

With Phase 3 done, Track 21 is complete: Track 12 (Spring config property
autocomplete), Track 20's own classpath feed into `jdtls`'s init config,
and Track 27 (DI/bean graph visualizer, alongside Track 20) are all now
unblocked.

---

## Track 22 — Build/run/test integration

Split into 3 phases along the track's own name — Build, Run, Test — each a
self-contained capability with its own checkpoint, rather than one
combined build+run pass followed by a matcher pass bolted on afterward.
Phase 1 lands the shared output-panel infra (reused by Track 14 if it
lands after this) since Phases 2/3 both need it; Phases 2/3 are otherwise
independent of each other and can land in either order.

**Phase 1 — Build.** Shared dockable output panel; a Build action invokes
the project's `RunConfig` via its build tool's wrapper script (`mvn
compile`/`gradle compileJava` shape), streaming stdout/stderr live.
Problem-matcher wiring lands here too, not as a separate phase — compiler
errors are exactly this phase's own output to parse: per-tool regex
patterns recognizing compiler-error line shapes (verified against real
Maven/Gradle-wrapped build output, not just bare `javac`'s own format),
clickable jump via `pending_navigation`.

**Checkpoint 1:** done — `fg_core::build_output` has `detect_build_tool`
(the same `pom.xml` vs `build.gradle[.kts]` file-presence check
`spring_config::scan_project` already established, no other precedent
existed), `build_command` (assembles `mvn -B compile`/`gradle
--console=plain compileJava`, preferring a project's own `mvnw` the same
way `gradle_command` already prefers `gradlew` — that function's own
visibility widened to `pub(crate)` for this reuse), and `parse_build_
output_line`. One real, found-not-assumed asymmetry between the two
tools, verified against real `mvn 3.9.3`/`gradle 9.6.1` runs against a
deliberately broken two-error fixture: Maven's own `[ERROR] path:[line,
col] message` lines land on **stdout**; Gradle's `compileJava` prints
plain unwrapped javac output (`path:line: error: message`, no column at
all — a caret line points at it instead) on **stderr**. `crates/app/src/
panels/build_panel.rs`'s `BuildState` spawns the real child with both
streams piped, one reader thread per stream feeding a shared `mpsc`
channel — genuine live streaming (`BufReader::lines()`), not the
spawn-wait-`Command::output()` shape every earlier background job in this
codebase (`static_analysis`, `gradle_classpaths`, ...) already uses, since
none of those needed to show partial progress while still running.

Deliberate deviation from this phase's own text above: Build does **not**
go through `RunConfig` — a compile has no main-class/args/env to configure,
so there's nothing in `RunConfig` for it to read; `RunConfig` stays
reserved for Phase 2's Run action, which actually needs it. Run > Build
(a new `MenuBarOutcome::build_request`) auto-shows the new View > "Build
Output" bottom-docked panel (a new `build_panel_visible` bool, same
`terminal_panel_visible`-shaped persisted toggle) the same way `Ctrl+\``'s
terminal handler already auto-starts its own session on first open. A
clicked problem row hands back the compiler's own raw `(path, line,
column)` — not a byte offset — since unlike the Spring endpoint map (which
already has a byte offset at scan time), there's no live buffer to convert
against until `open_path` runs; `app.rs`'s own click-site opens the path
first, then reads the byte offset straight off that document's real
`Rope` (`line_to_char`/`char_to_byte`) before setting `pending_navigation`,
reusing that exact mechanism unchanged.

Live-verified end to end against a real broken two-error Maven fixture
(screenshots, not just described): Build Output panel opens automatically
on Run > Build, real `mvn -B compile` output streams in live (confirmed
mid-build, not just the finished log), both compiler-error lines render in
the error color while every surrounding line (`[INFO]`, `symbol:`,
`location:`) stays plain text, and clicking each one opens `Foo.java` with
the caret landing exactly on the offending identifier (`undefinedSymbol`
line 5, `anotherUndefined` line 6). Driven this once via `xdotool` at the
user's own explicit request despite `AGENTS.md`'s own standing "don't
automate clicks" guidance — and it surfaced a real, live reproduction of
`TECHNICAL_DEBT.md` #23 in the process: File > Open Folder's native picker
genuinely froze the whole window on this machine, confirming that entry
isn't hypothetical. Worked around by pre-seeding `eframe`'s own
`last_project` storage key under an isolated `XDG_DATA_HOME` instead of
driving that dialog — not a fix for #23 itself, still open.

**Phase 2 — Run.** A Run action invokes the project's configured main
class/application task (`mvn exec:java`/`gradle run` shape, reusing
`RunConfig`), streaming to the same output panel; a Stop control kills the
running process.

**Checkpoint 2:** done — deliberately **not** `mvn exec:java`/`gradle run`
(this phase's own literal text above): `gradle run` only exists when the
target project applies Gradle's own `application` plugin, which an
arbitrary real project — including this app's own scaffolded Gradle
template, Track 29 Phase 4 — has no guarantee of declaring. `fg_core::run`
(`run_command`) instead launches `java` directly against Track 21's own
already-resolved classpath (`maven_classpath`/`gradle_classpaths`),
prefixed with the tool's own default compiled-classes output directory
(`target/classes`/`build/classes/java/main`) — the same thing every
mainstream IDE's own "Run" already does under the hood, verified live end
to end (a real dependency on the classpath, `System.getenv`/`args` both
read correctly by the launched program) rather than assumed. `crates/app/
src/panels/build_panel.rs`'s `BuildState` (Phase 1's own single-shot
design) gained a `Stage` enum so Run's "compile, then — only on success —
launch" chains onto the *same* streamed process machinery Build already
uses, both stages appending to one continuous log; `Stop` needed a live
handle to whichever child is currently running, which Phase 1's original
"the completion thread owns the `Child` outright" shape didn't keep
around at all — `child` is now `Arc<Mutex<Option<Child>>>`, shared with
that thread (which locks it to call `wait()`), so `stop()` can reach in
and kill it from the UI thread at any time. Run > Run Project always uses
the *first* saved `RunConfig` for the project, not a user-picked one — a
deliberate scope limit (a real config-picker dropdown is a natural
follow-up, not what this checkpoint asked for).

Live-verified end to end (screenshots, not just described), same isolated-
profile `xdotool` approach Phase 1's own checkpoint used: a real two-
dependency Maven project's `Run Project` streamed `mvn -B compile`'s own
live output, then chained straight into the launched program's own output
(`MY_ENV=hello from run config`, `arg: foo`, `arg: bar`, then a real
`Thread.sleep`-paced `tick 1`/`tick 2`/... arriving roughly once a second,
proving genuine live streaming rather than a buffered dump at the end).
Stop clicked mid-run: output froze at `tick 3` and never resumed, the
"Parar" button disappeared (Rust-side confirmation the process transitioned
out of "running"), and no further ticks appeared even several seconds
later — a real kill, not a detach that would have kept ticking invisibly.

**Phase 3 — Test.** A Test action invokes the project's test task (`mvn
test`/`gradle test` shape), parsing the tool's own test-report output
(Maven Surefire XML / Gradle's test XML — same "parse the real report
format, not console text" approach `static_analysis`'s Checkstyle/PMD
integration already established) into a pass/fail summary; a failing
test's entry jumps to its source location the same way a build error's
does.

**Checkpoint 3:** done — `fg_core::test_report` (`parse_junit_xml`) reads
either tool's own report with the same code: compared side by side against
real `mvn -B test`/`gradle --console=plain test` runs (a two-test JUnit 5
fixture, one deliberately failing), Maven's `target/surefire-reports/
TEST-<FQCN>.xml` and Gradle's `build/test-results/test/TEST-<FQCN>.xml`
turned out to be the *same* JUnit-XML schema at the structural level both
tools happen to have converged on, not two formats needing two parsers.
Two real, found-not-assumed differences handled: Maven wraps a `<failure>`/
`<error>`'s own text in `<![CDATA[...]]>`, Gradle's is plain element text —
`quick_xml` surfaces these as `Event::CData` vs. `Event::Text`, both read
into the same `detail` field; Gradle's own `<testcase name="...">` keeps
JUnit 5's `()` suffix on a parameterless test method's name, Maven's
strips it — `parse_junit_xml` strips a trailing `"()"` unconditionally so
a case's `name` reads identically regardless of which tool produced the
report. `test_command` is `mvn -B test`/`gradle --console=plain test`
directly (unlike Run, no separate compile step to chain — both tools'
`test` task/phase already compiles everything it needs on its own).

Jump-to-location doesn't come from the XML report itself (a JUnit report
carries no file/line, only `classname`/`name`) — `test_source_file` maps
`classname` onto the standard `src/test/java/<package/path>/<ClassName>.
java` layout both tools' own scaffolding already assumes (a `None` for a
nonstandard layout just means no click-to-jump for that one row, not a
failure), and `failure_line` searches the failure/error's own captured
stack trace text for the first frame naming that class's own simple name
(`"(CalcTest.java:14)"` in `... at com.example.CalcTest.addIsBroken
(CalcTest.java:14)"` — the *first* such frame, since every frame above it
belongs to the assertion framework's own internals, not the test itself;
verified against both tools' real traces, which turned out to share this
exact frame shape since both come from the same JUnit 5 assertion
machinery regardless of which build tool ran it).

`crates/app/src/panels/build_panel.rs`'s `Stage` enum gained a `Test`
variant: `start_test` spawns `test_command` through the exact same
streamed-process machinery Build/Run already established, and its own
`Finished` handling (`append_test_summary`) scans the report and appends a
plain summary line plus one clickable row per failing/errored test —
reusing `BuildProblem`/`BuildRow` unchanged rather than growing a second,
parallel row type just for this (`column: 1`, the same convention Gradle's
own column-less compiler errors already established in Phase 1). This also
meant `app.rs`'s click handler — previously Build/Run-only — now serves
Test's own rows for free, no new plumbing needed there at all. One
incidental cleanup made in passing: that click handler's line/column-to-
byte-offset conversion was hand-rolled `ropey` calls with no CRLF handling;
`static_analysis::line_col_to_byte` (Checkstyle/PMD's own existing helper,
which does handle CRLF) is now exported and reused there instead, for both
Build/Run's and Test's own click-to-jump alike.

Live-verified end to end (screenshots, same isolated-profile `xdotool`
approach the earlier two checkpoints used): a real two-test Maven project
(one passing, one deliberately failing) — Run > Run Tests streamed real
`mvn -B test` output, then appended `Tests: 2 total, 1 passed, 1 failed, 0
errored, 0 skipped` and a red `FAILED  com.example.CalcTest.addIsBroken`
row; clicking it opened `CalcTest.java` with the caret landing exactly on
line 14, the failing `assertEquals` call itself.

**Track 22 — Build/run/test integration — all three phases done.**

---

## Track 23 — Debugger

Depends on Track 22 (`Build/run/test integration`) for process launch.

**Phase 1 — DAP client + launch. Done, live-verified.** `dap_client.rs`
reuses `lsp_client::read_message`/`write_message` (`Content-Length`-framed
JSON is identical between LSP and DAP; the message *shape* on top isn't, so
`classify`/`IncomingMessage` are new) over a `TcpStream`, not a child
process: `java-debug` (`com.microsoft.java.debug.plugin`) runs *inside* the
already-running jdt.ls JVM as a loaded OSGi bundle, started via a
`workspace/executeCommand` call for `vscode.java.startDebugSession`
(`lsp_state::LspState::request_start_debug_session`) that hands back a TCP
port — verified concretely against the real `vscjava.vscode-java-debug`
extension's own bundled client code (its `.vsix`, downloaded from Open VSX
and sha256-checked against its own published hash), not assumed from
documentation. The plugin jar itself isn't published anywhere as a build
artifact (`microsoft/java-debug`'s own GitHub Releases are empty, same
absent-releases situation `lsp_manager`'s own header already found for
jdt.ls) — extracted from that same verified `.vsix` and vendored at
`vendor/lsp-servers/java-debug-plugin-0.53.2.jar`, materialized to a real
file on disk at Java-session-start time (`lsp_manager::
ensure_debug_plugin_jar`, best-effort exactly like `ensure_kotlin_stdlib_
override_for`) and passed to jdt.ls via `initializationOptions.bundles`.
`debug_state.rs` drives the real DAP `initialize`/`launch`/
`configurationDone` handshake as a small non-blocking phase machine, polled
once a frame like every other background op here. `SPEC.md`'s own "verify
Kotlin support concretely" caution: confirmed real — the extension's
`readme.md`/`changelog.md`/`Configuration.md` mention Kotlin nowhere at
all, so Phase 1 targets Java only; Kotlin debugging stays an explicitly
open question, not a "probably fine, same bytecode" assumption.

Two real, live-server findings corrected code that looked reasonable but
wasn't: `vscode.java.startDebugSession`'s response body is a bare JSON
number (`33267`), not the quoted string its own TypeScript types implied;
and a DAP request with no real arguments needs `{}`, not `null` — real
java-debug's own server-side Gson deserializer rejects `null` outright
(`Expected a JsonObject but was JsonNull`). Neither was guessable from
documentation alone, which is exactly why this phase's own checkpoint asks
for a live-verify rather than a green test suite alone.

**Checkpoint 1 — done.** `cargo test --workspace` green (929 passed, 4
ignored — one of them this phase's own new real-server test). Live-verified
end to end, not mocked: a real fixture Maven project (`pom.xml` + a `Main`
that sleeps so there's something to observe), compiled with a real `mvn`,
opened by a real jdtls 1.60.0 session under a real JDK 21 with the vendored
debug bundle loaded, a real `vscode.java.startDebugSession` call, a real
socket connection, and the real `initialize`/`launch`/`configurationDone`
sequence reaching `Attached` — jdt.ls' own log confirms it: `"Launching
debuggee VM succeeded."` A full GUI click-through (Run > Debug Project) was
attempted under Xvfb but abandoned partway: the virtual display turned out
to be shared with a real, already-running FoxGarden session (a different
project, a long-lived real jdtls process) rather than exclusively available
for this session's own isolated testing — continuing risked a stray
`xdotool` click landing on that real window instead of the intended one,
exactly the failure mode `AGENTS.md`'s own xdotool section warns about, so
this stopped rather than guessing. The menu/UI wiring itself (`Run > Debug
Project`) is mechanically identical to the already-proven Run/Test/Coverage
menu code (same `MenuBarOutcome` pattern, same `any_running`-gated button
shape) — low risk, but not itself live-verified through a real click; worth
a real click-through next time an exclusive display is available.

**Phase 2 — breakpoints + stepping. Done, live-verified against the real
protocol; GUI click-through still owed.** `Document` gained `breakpoints:
HashSet<usize>` (0-indexed, same session-only membership-set shape
`folded_lines` already established). Breakpoints reach the adapter two
ways: a snapshot taken at `start` time (`DebugState::start`'s new
`initial_breakpoints` parameter) is sent the instant the adapter's own
`initialized` event first arrives — deliberately *before*
`configurationDone`, since java-debug may resume the JVM as soon as that
lands, so an early-line breakpoint would otherwise race the debuggee
actually reaching it; and live toggles while attached go through
`DebugState::sync_breakpoints`, called once a frame from `app.rs`, which
diffs every open document's current set against `last_sent_breakpoints`
(pure `breakpoints_to_resend` helper, unit-tested headlessly) and re-sends
`setBreakpoints` only for files that actually changed.

`Phase::Attached` grew `paused: Option<PausedFrame>` and an in-flight
`stack_trace_rx` — a real `"stopped"` event's `threadId` (`parse_stopped_
thread_id`) triggers a `stackTrace` request, and its top frame (`parse_
top_stack_frame`, `- 1` to undo `linesStartAt1`) becomes the paused
location `DebugState::paused_location()` exposes; a `"continued"` event
clears it. `continue_`/`step_over`/`step_into`/`step_out` send `"continue"`/
`"next"`/`"stepIn"`/`"stepOut"` with the paused thread id and clear `paused`
optimistically (the highlight disappears the instant the button is
clicked, not after a round-trip) — all four are harmless no-ops outside a
paused session, same contract `stop` already holds itself to.

`crates/app/src/widgets/editor/breakpoint_gutter.rs` is a new gutter
column, leftmost in the gutter (ahead of the code-action/fold columns,
which shifted right by its width) — deliberately **not** built like
`folding::show_fold_gutter` (which only makes a row interactive if it
already has a fold marker): every currently-shaped row gets a click
target here, since the whole point is letting the user create the first
breakpoint on a line that has none yet. Reserved whenever `doc.language ==
Some(Language::Java)`, regardless of whether a breakpoint already exists —
unlike every other gutter column here, which only reserves its width once
there's something to show. Kotlin is excluded: Phase 1's own research
found no evidence java-debug supports it. The current-line highlight
(`painting::paint_paused_line_highlight`, new — no existing whole-row
painter to reuse; `paint_occurrence_highlights` is span-width and
`paint_blame_annotation` draws past the line, neither fills a full row)
paints a translucent amber band across the *full editor content width*
(`out.response.rect`, not a glyph-derived width) on `debug_state.paused_
location()`'s own line, only in the document it's actually paused in.

`crates/app/src/panels/debug_toolbar.rs` is a new, non-dockable panel
(`egui::Panel::top`, shown exactly while `DebugState::is_running()` — not
a `View`-menu-toggled dock like the build/terminal panels, since its
whole purpose is 1:1 tied to a live session existing) with Continue/Step
Over/Step Into/Step Out (enabled only while `DebugState::is_paused()`) and
Stop (always enabled).

**Checkpoint 2 — done, including the GUI click-through.**
`cargo build --workspace`/`cargo test --workspace`/`cargo clippy
--workspace --all-targets` all green (938 passed, 4 ignored — one of them
this phase's own extended real-server test; two unrelated tests
— `app::e2e_test::navigation_test::ctrl_shift_e_lists_the_projects_
spring_endpoints_and_jumps_to_one` and `jdk_registry::tests::detect_and_
add_records_the_real_detected_version` — were observed flaking only under
the default parallel test run, both passing reliably under `--test-
threads=1`; a pre-existing test-isolation issue, not caused by this
track, not investigated further here). The existing Phase 1 real-server
test (`lsp_state::tests::java_debug_launch_against_a_real_server_
attaches_to_a_real_process`) was extended, not duplicated: it now sets a
real breakpoint on the fixture's own `println` line via `initial_
breakpoints`, asserts the session actually pauses there (`paused_
location()` resolving to the exact file and 0-indexed line 3), sends a
real `step_over` and asserts it lands on the very next source line, then
a real `continue_` and asserts the debuggee runs to real completion
(`Thread.sleep(5000)` included, not skipped) — live-verified against a
real jdtls 1.60.0 + java-debug 0.53.2 + JDK 21, not mocked. Every new pure
helper (`parse_stopped_thread_id`, `parse_top_stack_frame`, `breakpoints_
to_resend`) also has its own fast, no-I/O unit tests.

The GUI half's owed click-through (flagged twice before — Phase 1's own
checkpoint and this phase's first pass — both times because the shared
Xvfb `:99` turned out to be running someone else's real FoxGarden
session) closed this session on a dedicated Xvfb instance confirmed
exclusive first (`ps aux`/`xdotool getactivewindow` checked clean before
touching it). A real persistent fixture Maven project (same shape as the
real-server test's own throwaway one) was opened, a breakpoint toggled
via a real gutter click on `System.out.println("debug fixture
running")`, a run configuration created through Executar > Editar
Configurações, then Executar > Depurar Projeto: the pause highlight
landed on the exact breakpointed line, Passar Por Cima (Step Over) landed
on the very next line, and Continuar ran the debuggee to completion with
the toolbar cleanly disappearing afterward — every button in the
toolbar's real button row exercised, not just asserted to exist.

One real, found-not-assumed environment gotcha, worth keeping for future
GUI testing sessions here: **FoxGarden's native folder-open dialog (via
`rfd`) does not honor the child process's `DISPLAY` — it opened on the
real physical desktop instead of the dedicated Xvfb display**, almost
certainly because `rfd`'s Linux backend goes through an XDG desktop
portal (a D-Bus service scoped to the user's actual active session)
rather than a plain GTK file chooser that would have respected `DISPLAY`
directly. This means File > Open Folder specifically cannot be driven
from an isolated virtual display the way every other menu/dialog in this
app can. Worked around this session by editing `~/.local/share/foxgarden/
app.ron`'s `last_project` key directly (the same `eframe::Storage` file
`restore_session`/`persist_session` already read/write) rather than
clicking through the picker — real session data was backed up first and
restored afterward. Any future click-through that needs to switch
projects should reach for the same storage-file edit rather than the
Open Folder menu item.

**Phase 3 — variable/call-stack panel. Done, live-verified against the real
protocol; GUI click-through still owed.** `DebugState`'s `PausedFrame` grew
`frame_id` (the top frame's own DAP `id`, needed by `scopes`), `stack: Vec<
StackFrameSummary>` (the full call stack — the `stackTrace` request's own
`levels` argument is no longer sent at all, since an absent/zero `levels`
is DAP's own "every remaining frame from `startFrame`", where Phase 2 only
ever asked for the top one), and `variables: Vec<VariableGroup>`. A second,
later in-flight chain (`VarFetch`, alongside `stack_trace_rx`) starts once
the top frame itself resolves — it needs that frame's own `id`, unusable
until the first `stackTrace` response has already landed — and drives a
real `scopes` request followed by one `variables` request per non-
`expensive` scope with a nonzero `variablesReference` (`parse_scopes`
drops the `expensive: true` "Static"/synthetic scope java-debug reports,
matching the real `vscode-java-debug` extension's own Locals-panel
behavior, and drops any scope with `variablesReference: 0` — DAP's own "no
variables here" marker). `PausedFrame::variables_fetched` (not `VarFetch::
Idle`, which is also true right after the chain finishes) is what tells
"never started" apart from "finished with zero groups". Both the top-frame
chain and the older `stack_trace_rx` are reset on every fresh `"stopped"`/
`"continued"` event, so a new pause always re-fetches instead of showing
stale values from the pause before it.

`crates/app/src/panels/debug_panel.rs` is a new panel — shown as an
`egui::Panel::right`, exactly while `DebugState::is_running()`, the same
"tied 1:1 to a live session, not a `View`-menu-toggled dock" choice
`debug_toolbar` (Phase 2) already made, since there's nothing useful to
show once no session exists. Renders the call stack (each frame with a
real file a click-to-jump row, resolved through `app.rs`'s own
`pending_navigation` the same way `build_panel`'s clickable rows already
are) and the variables grouped by scope (`egui::CollapsingHeader` per
group).

**Checkpoint 3 — real-protocol half done and live-verified; GUI
click-through owed.** `cargo build --workspace`/`cargo test --workspace`/
`cargo clippy --workspace --all-targets` all green (945 passed, 4 ignored
— one of them this phase's own extended real-server test; the same two
pre-existing tests flagged in Phase 2's own checkpoint were observed
flaking again only under the default parallel run, not investigated
further here, not caused by this phase). The existing Phase 1/2 real-
server test (`lsp_state::tests::java_debug_launch_against_a_real_server_
attaches_to_a_real_process`) was extended again, not duplicated: after the
real breakpoint pause it now asserts a real, non-empty call stack whose
top frame is the fixture's own `Main.main(String[])` in the exact fixture
file, waits out the real `scopes`/`variables` chain, and asserts `main(String[]
args)`'s own `args` parameter shows up among the real fetched variables —
run live against a real jdtls 1.60.0 + java-debug 0.53.2 + JDK 21 this
session (not mocked), including one real, found-not-assumed correction a
first guess got wrong: a stack frame's own `name` is java-debug's full
`"Main.main(String[])"` (declaring class + signature), not the bare method
name the DAP spec's own field name alone would suggest. Every new pure
helper (`parse_call_stack`, `parse_scopes`, `parse_variables`) also has its
own fast, no-I/O unit tests. The GUI half (opening the panel, clicking a
call-stack frame to jump, watching real values update across a step) is
not yet click-through-verified — worth doing next time an exclusive Xvfb
display is available, same caveat Phase 1/2 already flagged for their own
GUI halves.

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

## Track 28 — Language Server settings modal + jdtls/kotlin-language-server installer

**Landed, revised twice since.** Both halves shipped: the inline "Settings
> Language Server" menu-bar submenu became a modal (`crates/app/src/panels/
lsp_servers.rs`), and a self-service installer/updater landed alongside it
(`crates/app/src/lsp_manager.rs`), mirroring the Checkstyle/PMD/SpotBugs
installer (`crates/app/src/tool_manager.rs` via `static_analysis.rs`'s
"External Tools…" modal).

**Revision 2 (current): both servers are vendored, not fetched at all.**
Revision 1 (below) built jdtls from source on demand; in practice that
build resolves its Tycho target-platform dependencies straight off Maven
Central, and behind a corporate mirror that doesn't proxy every artifact it
touches (e.g. `com.jetbrains.intellij.java:java-decompiler-engine`), it
fails outright. The fix: vendor both servers' real release archives directly into
`vendor/lsp-servers/` (tracked via Git LFS — plain git history would
otherwise carry their ~135 MB combined weight forever) and `include_bytes!`
them into the FoxGarden binary itself (`lsp_manager.rs`'s own header). This
is exactly revision 1's own Phase 1 research below, just landed rather than
deviated from: jdtls' official prebuilt milestone tarball from
`download.eclipse.org/jdtls/milestones/<version>/`, `bin/jdtls` and all.
Installing now means extracting embedded bytes — no network, no `git`, no
Maven, no Python — for either server. `Server::recommended_version` is now
also the *only* installable version (whatever's vendored); `check_latest`
still pings each project's real upstream as an FYI, but "Update to X" no
longer offers a version this app doesn't have the bytes for.

`recommended_version` for jdtls stays pinned at **1.60.0** (not the
**1.44.0** revision 1's research flagged as the last milestone runnable
under Java 17 — see that finding below) — a deliberate choice to require
Java 21 rather than pin to an older runtime, since jdt.ls 1.60.0 is the
version actually vendored and Java 21 is what it needs to run.
`lsp_manager::resolve_jdtls_java` resolves and verifies that JVM, and
`lsp_state::start_session` pins every jdt.ls spawn to it via `bin/jdtls`'s
own `--java-executable` flag — rather than trusting whatever `java`
`JAVA_HOME`/`PATH` happens to resolve to at that moment, which is Java 17 on
this project's own dev machine. `LspSettings::jdtls_java_home` (Settings >
Language Servers…'s new "Java Home" field, empty = auto-detect) exists
precisely for that case: pointing jdt.ls at a specific JDK 21 install (e.g.
an sdkman-managed one) without touching the system default `java`.

**Revision 1 (superseded): built jdtls from its own GitHub repository** —
shallow-clone the release tag, run the project's bundled `./mvnw clean
verify -DskipTests=true`, take `org.eclipse.jdt.ls.product/target/
repository/bin/jdtls`. Both of that approach's own open follow-ups
(pinning `recommended_version` to a Java-17-runnable milestone; the
source build never having run end-to-end under this machine's Java 17)
are moot now — there's no source build left to run.

The original research below (still accurate — it's what revision 2 above
actually implements) follows, unchanged.

**Phase 1 — the installer + modal.** Real research already done this
session (downloads/diffs actually run, not assumed — same discipline
`tool_manager.rs`'s own header comment already holds itself to):

- `eclipse-jdtls/eclipse.jdt.ls` has **no GitHub Releases** (`gh api
  repos/eclipse-jdtls/eclipse.jdt.ls/releases/latest` → 404). It publishes
  milestone builds to `https://download.eclipse.org/jdtls/milestones/
  <version>/` instead — a date-stamped `.tar.gz` whose exact filename
  isn't derivable from the version alone, but each version directory also
  has a `latest.txt` naming it. Verified by actually downloading and
  extracting milestone `1.60.0`: the tar.gz is flat (no versioned
  top-level folder, unlike PMD/SpotBugs's own zips) — `bin/`,
  `config_<os>/`, `features/`, `plugins/` directly at the root — and
  `bin/jdtls`/`bin/jdtls.py` (the official Python launcher wrapper) is
  genuinely part of the upstream release, not a third-party addition from
  the `pulsar-ide-java` packaging this session's own live-verify used (a
  byte-diff confirmed they're the same file, just different versions).
  The installer can extract-and-point at `bin/jdtls`, same shape as PMD/
  SpotBugs — no custom launcher generation needed.
- **Real, verified version-compatibility finding, the reason not to just
  install "latest":** `jdtls`'s own Python wrapper hard-checks the running
  JVM's major version and refuses to start below its own minimum.
  Diffing `org.eclipse.jdt.ls.product/scripts/jdtls.py` across tags (`gh
  api repos/.../contents/...?ref=<tag>`) found the exact boundary:
  **`v1.44.0` is the last milestone requiring only Java 17; `v1.45.0`
  bumped the hard-coded minimum to Java 21.** This machine's (and this
  project's own already-verified, per `tool_manager.rs`'s Checkstyle pin)
  baseline JVM is Java 17. Confirmed live: downloaded, extracted, and
  launched `1.44.0`'s real `bin/jdtls` under this machine's real Java
  17.0.4 — clean OSGi bootstrap logs, no version-gate exception (the same
  failure mode Checkstyle's own 13.x line hit under Java 17, which is
  exactly why that tool is pinned too). **`1.44.0` is the version to pin**
  as `recommended_version` — `check_latest` can still separately report
  whatever the real newest milestone is (`1.60.0` as of this session).
- `fwcd/kotlin-language-server` **does** have normal GitHub Releases and
  fits the existing `Tool` pattern almost verbatim: latest tag `1.3.13`,
  one asset `server.zip`, extracts to `server/bin/kotlin-language-server`
  — verified by downloading and listing the real zip. Pin to `1.3.13`
  explicitly (already what's verified working this session), even though
  it happens to equal GitHub's own latest right now.
- Installing/updating kotlin-language-server via this tool will **not**
  fix `TECHNICAL_DEBT.md` #17 (the stdlib-version-mismatch false
  diagnostics) — that's caused by *this machine's* installed Kotlin SDK
  vs. kotlin-language-server's own bundled analysis compiler, baked into
  every 1.3.13 build regardless of when it's (re)installed. Don't assume
  this phase fixes #17.
- New Cargo dependencies needed: `tar` (absent from the workspace
  entirely) + `flate2` (currently only a transitive dep via `ureq`/`zip`,
  needs promoting to direct, pinned to whatever version is already
  transitively resolved in `Cargo.lock` so promoting it doesn't silently
  bump anything else).

Design shape (full detail in the approved plan from this session, not
reproduced here — re-derive from this section plus a fresh read of
`tool_manager.rs`/`static_analysis.rs` if that plan file is gone by the
time this is picked up): extend `tool_manager::Tool` with `Jdtls`/
`KotlinLanguageServer` variants (`KotlinLanguageServer` fits the existing
generic GitHub-releases method table directly; `Jdtls` needs its own
`install_sync`/`check_latest_sync` branch for the two-step `latest.txt`-
then-tarball fetch and the Eclipse download-server directory-listing
parse, since neither fits the existing per-tool method shape). `LspSettings`
gains `jdtls_installed_version`/`kotlin_language_server_installed_version`
fields + matching storage keys. `FoxGardenApp` gains its own
`lsp_tool_manager: ToolManagerState` (a second, independent instance of
the already-generic type — keep it separate from `StaticAnalysisState`'s
own instance, different concern) and an `LspSettings::apply_installed`
mirroring `ExternalToolPaths::apply_installed`. The modal itself
(`lsp_settings.rs` gains a `show_settings`) mirrors `static_analysis.rs`'s
`show_settings`/`show_install_row` almost exactly, plus a help line
covering jdtls's own extra runtime requirement (a `python3` on PATH,
beyond the JVM every other tool here already needs).

**Checkpoint 1:** `cargo test --workspace` green (new tests: `Tool::
KotlinLanguageServer`'s URL construction; a tar.gz-extraction test
mirroring the existing zip one, built against jdtls's real flat layout;
a pure parser test for "highest `X.Y.Z` in a milestones directory
listing"); live-verify the modal renders, Install actually lands a
working binary in the cache dir and auto-populates the path field for
both servers, and a real project still gets working diagnostics/
completions against the freshly-*installed* (not just the pre-existing
`pulsar-ide-java`) jdtls afterward.

---

## Track 29 — Java-version-aware editing + new-project scaffolding

**Reconciliation note:** this track's Phase 0 finding and Phase 2 below
were written without knowledge of "Detect the JDK and the project's Java
release for jdt.ls" (`f5c4931`, landed on `origin/ide-henshin` before
this track was started locally) — which already ships almost exactly
what Phase 2 describes: `fg_core::java_release::detect` reads a project's
own
`pom.xml`/`build.gradle(.kts)`/`.java-version`/`.sdkmanrc` for its real
compiler release, and `lsp_state.rs` already sends jdt.ls
`java.configuration.runtimes` built from every JDK `lsp_manager::
java_home_candidates` finds on the machine, release marked default. Phase
0's own finding (jdt.ls enforces a *managed* project's declared compliance
level with zero client-side help) still stands and is now doubly
confirmed — Phase 2 as scoped below is very likely already done in
substance; the next session picking this track up should read
`java_release.rs`/`lsp_state.rs`'s current shape first and treat Phase 2
as a verification-and-close pass, not a from-scratch implementation. See
TECHNICAL_DEBT.md #24 for the resulting duplicate-JDK-detection debt
(Track 29 Phase 1's own `JdkRegistry` vs. `java_home_candidates`) that
Phase 2's close-out should probably also resolve.

**Phase 0 done (live-verified); Phases 1+ not started.** `jdt.ls` needs a
JDK 21+ **host** runtime just to execute
(`LspSettings::jdtls_java_home`/`resolve_jdtls_java`, `lsp_manager.rs` —
already correct, unrelated to this track). Separately, Eclipse JDT's own
compiler (`ecj`) can *target* any older Java language level regardless of
the host JVM, the same way `javac --release 8` works fine under a JDK 21.
Today FoxGarden sends jdt.ls no `java.configuration.*` settings at all
(`lsp_state.rs`'s `initialize_params`), relying entirely on its own native
Maven/Gradle project import — never live-verified in this codebase for
anything beyond the default (Java 21) case (see TECHNICAL_DEBT.md #20).

Two capabilities, both explicitly in scope: (1) open and correctly analyze
an *existing* project targeting any Java level, and (2) create a *new*
Java/Kotlin/Maven/Gradle project from FoxGarden's own UI, picking a target
Java version. Actually compiling/running either kind of project is **not**
in scope — that's Track 22 (Build/run/test integration, not started). This
track ends with a project a real `mvn`/`gradle` on the user's own machine
can build, analyzed by jdt.ls at the right language level.

**Phase 0 — Live-verify jdt.ls's actual multi-version behavior (no code).
Done — finding recorded below, confirmed via a real jdtls 1.60.0 process.**
Method: a raw JSON-RPC probe (bypassing FoxGarden entirely, same technique
TECHNICAL_DEBT.md #17/#18 already used) against the real vendored jdtls
1.60.0, run under a JDK 21, sending exactly what `lsp_state.rs`'s real
`initialize_params` sends today (no `java.configuration.*` at all).

- **Managed case (a real `pom.xml` with `<maven.compiler.release>8`
  `</maven.compiler.release>`):** a fixture with a `record` (Java 16+) and
  `var` (Java 10+) opened with zero client-side configuration got back
  real `publishDiagnostics` correctly rejecting both — `"'record' is not a
  valid type name; it is a restricted identifier and not allowed as a type
  identifier in Java 1.8"`, plus `var`/`Point` reported unresolvable.
  jdt.ls's own native Maven import already reads and enforces the
  project's declared compliance level with **zero help needed** from this
  client. Gradle's own Buildship-based import is the same well-established
  mechanism (reading `sourceCompatibility`/toolchain instead of a POM
  property) — not independently re-probed, low risk given how decisive the
  Maven result was.
- **Unmanaged case (the identical fixture, no `pom.xml`/`build.gradle` at
  all):** the same `record`+`var` file opened with no build file present
  produced **zero diagnostics** — jdt.ls's own "unmanaged folder" default
  accepts both, i.e. it defaults to a modern compliance level (consistent
  with the host JVM), not something a user can steer without help.

**Finding:** outcome (a) for the managed case (confirmed, no speculation)
— Phase 2 below is now scoped down accordingly: `MavenProject::
java_release()` still lands (cheap, useful for UI/Phase 3), but the
`java.configuration.runtimes` wiring is needed **only** for the genuinely
narrower unmanaged/loose-file case, not for any project a scaffolded or
opened pom.xml/build.gradle already describes.

**Phase 1 — Registered JDKs inventory.** Both capabilities need "which
JDKs exist on this machine," independent of Phase 0's outcome. New
`crates/app/src/jdk.rs`: lift `java_command`/`java_major_version` out of
`lsp_manager.rs` as `pub fn`s, add `pub fn detect_major_version(java_home:
&str) -> Result<u32, String>` as `check_java`'s generic core (jdt.ls's own
wording moves into a thin wrapper on top, `resolve_jdtls_java` unchanged
from its caller's side). New `crates/app/src/jdk_registry.rs`:
`RegisteredJdk { label, home, major_version }`, `JdkRegistry { jdks:
Vec<RegisteredJdk> }` with `detect_and_add`/`closest_for(release: u32)` —
a JDK install is a fact about the machine, not the project (same reasoning
`jdtls_java_home` already rests on), so this is a global registry via
`eframe::Storage`, not a per-project `.foxgarden/` file. New
`crates/app/src/panels/jdk_registry.rs`: a flat settings form (add via
folder picker + auto-detect, list with remove) under its own new
"Settings > JDKs…" entry — deliberately separate from the Language Servers
modal, since `jdtls_java_home` is "which JVM runs jdt.ls" (always 21+) and
this is "which JDKs exist to target" (any version).
**Checkpoint 1 — done.** `cargo test --workspace` green (including
`detect_and_add`/`closest_for`/`add_known` against fake `java_home`
dirs). Live-verify of the Settings > JDKs… dialog surfaced and closed
TECHNICAL_DEBT.md #23: `pick_folder()` was called synchronously on the UI
thread with no timeout, freezing the whole app if the portal didn't
answer promptly; now backgrounded on a thread and polled like every other
slow op in this codebase (`git_stage.rs`'s own `spawn`/`poll` shape). The
"add a JDK, restart the app, confirm it's still there" step wasn't
confirmable through the native folder picker in this sandboxed
environment, so it was instead confirmed via the Auto-detect button
(closing TECHNICAL_DEBT.md #24) exercising the identical add → persist →
reload path without a native dialog — live-verified through a real quit
and relaunch, finding this machine's own SDKMAN-managed JDK 17 both
times.

**Phase 2 — done, shipped independently as `f5c4931` before this track was
even started locally (see this track's own "Reconciliation note" above).**
Landed shape differs from what was originally scoped here but delivers
the same capability: `fg_core::java_release::detect` (not
`MavenProject::java_release` — lives in its own module, also covers
Gradle/`.java-version`/`.sdkmanrc`, not just Maven) reads the project's
real declared release; `lsp_state.rs`'s `jdtls_runtimes`/
`initialization_options` send jdt.ls `java.configuration.runtimes`
**unconditionally** (every JDK `lsp_manager::java_home_candidates` finds,
release marked default) rather than only for the unmanaged case Phase 0
scoped this down to — simpler than the conditional this phase originally
planned, and a superset of it (still correct for the managed case, since
jdt.ls's own import already agreed with the marked default there).
`SessionConfig` already carries `java_release`/`runtimes` and restarts the
session on either changing, same mechanism `java_home` uses.
**Checkpoint 2 — done.** `f5c4931`'s own commit message records it as
live-verified at the time ("A Java 8 codebase is now linted as Java 8 —
var and records are errors again — while jdt.ls itself keeps running on
21"), not merely implemented. Not re-verified freshly in this session —
TECHNICAL_DEBT.md's own entries are the place a regression here would
show up if the claim turns out stale.

**Phase 3 — Capability B, part 1: New Project wizard + Maven+Java.** New
`crates/core/src/scaffold.rs` (pure generation, mirrors `gradle.rs::
INIT_SCRIPT`'s "Rust string constant, values substituted in" shape):
`ScaffoldSpec { group_id, artifact_id, java_release, build_tool, language
}`, `scaffold_files(&ScaffoldSpec) -> Vec<(PathBuf, String)>`,
`write_scaffold(project_root, files)` — refuses if `project_root` exists
and is non-empty. Maven+Java output: `pom.xml` (a bare
`<maven.compiler.release>` property, no compiler-plugin config needed),
`src/main/java/<package path>/Main.java`, `.gitignore`. New
`crates/core/src/project_config.rs`: per-project `ProjectConfig {
java_release, jdk_home }` in `.foxgarden/project.json`, sibling of
`run_config.rs`'s own `.foxgarden/run_configs.json` convention (same
`#[serde(default)]`, malformed/missing → default not error). Written once
at scaffold time; an existing project opened normally just has none, and
Phase 2's logic falls back to `java_release()`/jdt.ls's native import. New
`crates/app/src/panels/new_project.rs`: `NewProjectWizardState`, built on
`widgets::modal::show_modal` (`run_configs.rs`'s own template — the right
one here since this wizard has a real terminal "Create" action). On
Create: `scaffold::write_scaffold` → `project_config::save_project_config`
→ `EditorState::open_project` (the exact function "Open Folder…" already
calls). `MenuBarOutcome` gains `open_new_project_wizard_request`; File
menu gets "New Project…" right after "Open Folder…".
**Checkpoint 3 — done.** `cargo test --workspace` green
(`scaffold_files`/`write_scaffold` exact-output and
non-empty-directory-refusal tests, `project_config.rs` round-trip tests
mirroring `run_config.rs`'s own suite, `new_project.rs`'s own
`create_and_open` tests). Live-verified under a fresh isolated FoxGarden
instance: filled the wizard (`com.example.demo`/`demo-app`, Java 8),
clicked Create, watched it open into the side panel with the exact
generated tree. Confirmed on disk: `pom.xml` states
`<maven.compiler.release>8</maven.compiler.release>` and a real `mvn -q
compile` against it outside FoxGarden succeeds, producing
`target/classes/com/example/demo/Main.class`. Did **not** separately
re-verify jdt.ls actually diagnosing this specific scaffolded project at
Java 8 — Phase 2's own wiring (`java_release::detect` reading this exact
`pom.xml` shape, already unit-tested against it) is the same code path
already live-verified generically by `f5c4931`; nothing in this wizard
introduces a new one. One real, reusable testing gotcha found along the
way, not a FoxGarden bug: `xdotool type` needs `xdotool windowfocus`
first under a bare Xvfb with no window manager — a mouse click alone
focuses the widget inside egui but not the X11 window itself, so
keystrokes silently go nowhere without it.

**Phase 4 — Capability B, part 2: Gradle (Kotlin DSL) + Java.**
`scaffold.rs` gains `BuildTool::Gradle`: `settings.gradle.kts`,
`build.gradle.kts` (`java { toolchain { languageVersion = ... } }`), same
source/`.gitignore` shape. No Gradle wrapper generated (needs a real
network fetch or vendoring — the same trade-off `lsp_manager.rs`'s own
header already reasons through for jdt.ls, out of scope per this track's
own non-goal) — stated explicitly in the wizard's help text, not silently.
**Checkpoint 4:** `cargo test --workspace` green; live-verify: create a
real Gradle+Java project targeting Java 17, confirm a real `gradle
compileJava` (system Gradle) succeeds outside FoxGarden, confirm FoxGarden
opens it and jdt.ls treats it as Java 17.

**Phase 5 (stretch, optional) — Kotlin scaffolding.** Maven+Kotlin/
Gradle+Kotlin added to `scaffold.rs`'s `ProjectLanguage` enum once Phases
3-4 are solid. Explicitly deferrable: `kotlin-language-server` has two
open, unresolved gaps (TECHNICAL_DEBT.md #17/#18) that make a fresh
Kotlin project's actual in-app analysis experience uncertain regardless of
how correct the generated skeleton is.

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
- [x] Track 5 — Static analysis integration (all 3 phases shipped and
      live-verified: Checkstyle and PMD verified earlier, including the
      in-app install/update addition for all three tools' binaries;
      Phase 3/SpotBugs's own live click-through — Tools > Run SpotBugs
      against a real built Maven project, with the fixture's
      `InputStreamReader`-leak/default-encoding bug landing as a real
      diagnostic squiggle on the right line — done this session under an
      isolated `XDG_DATA_HOME`/`Xvfb` instance so it couldn't touch the
      real session's persisted project. Along the way, confirmed
      `run_spotbugs_process`'s `fb` launcher choice
      (`tool_manager.rs`'s `launcher_script_name`) is correct — SpotBugs'
      own `bin/spotbugs` script launches the GUI driver instead and
      silently exits 0 without writing `-output`, which only surfaced
      because the first click-through attempt was pointed at it by
      mistake rather than through the in-app Installer)
- [x] Track 6 — Auto-save (both phases shipped and live-verified)
- [x] Track 7 — Rectangular (block) paste (all 3 phases shipped and
      live-verified)

### Substantial tier

- [x] Track 9 — Git diff gutter, inline blame, commit/stage/push UI (all 4
      phases shipped and live-verified: diff gutter, inline blame, stage/
      commit panel, and hunk-level staging + push)
- [x] Track 10 — Code folding (Java: fold-range computation, gutter
      marker, fold state/toggle all pre-existing and already live-verified
      in an earlier session; Kotlin's own class/method/control-flow/
      block-comment folding — the one real gap `PLAN.md`'s Phase 1
      determination found this session — closed and live-verified this
      session)
- [ ] Track 11 — Multi-window / split-pane editing
- [x] Track 12 — Spring config property autocomplete (unblocked by Track
      21 this session; Phase 1 shipped and live-verified against a real
      Gradle/Kotlin/Spring Boot project, both `.properties` and `.yml`;
      Addendum — Spring annotation completion + auto-import — shipped and
      live-verified)
- [x] Track 13 — Code coverage overlay (Phase 1, Maven-only — Gradle
      deferred, no clean CLI-only JaCoCo injection point without a
      `build.gradle` edit; shipped and live-verified: "Run with Coverage"
      invokes `jacoco-maven-plugin`'s own `prepare-agent`+`test`+`report`
      goals as bare plugin coordinates on one `mvn` command — no `pom.xml`
      edits, no standalone jar downloads, `tool_manager.rs` untouched,
      confirmed by decompiling the real `org.jacoco.jacoco-maven-plugin`/
      `org.jacoco.core` jars already cached on this machine. Real-jar-
      verified line-status algorithm (`mi=0` alone does *not* mean fully
      covered — a split branch on an otherwise fully-covered line is still
      `Partial`). Live-verified end to end against a real Maven project:
      the covered/partial/missed gutter marks landed on the exact expected
      lines, the build-panel summary line appeared, the Gradle-rejection
      toast fired on a Gradle project with no process spawned, and the
      build-failure path correctly skipped reading a nonexistent
      `jacoco.xml`.)
- [ ] Track 14 — Docker/container run integration
- [x] Track 15 — Quick-fix intention actions (Phase 1 shipped: `cargo
      test -p foxgarden` green, 905 passed — checklist here was stale,
      corrected this session; see that track's own Checkpoint 1)
- [x] Track 17 — Peek definition (Phase 1 shipped and live-verified:
      Alt+F12 on a cross-file call opened the inline panel with the
      target line highlighted, active tab unchanged, Escape restored
      state)
- [x] Track 18 — Inline diff viewer widget (shipped and live-verified,
      including abridged-context and in-window hunk-staging follow-ups)

### Major tier

- [ ] Track 19 — Large file handling — full viewport virtualization
      (Phase 1 shipped and live-verified: word-wrap row-count computation
      no longer shapes every line up front — see that phase's own
      checkpoint for the measured 19-lines-shaped/14.865ms number against
      a real 200,000-line file. Phases 2-4 — hand-built widget replacing
      `egui::TextEdit`, drag-select, IME — turn out to have already
      shipped ahead of this Track's own numbering (`show_interactive`,
      2026-07-25); this session re-verified click-to-position live at
      real huge-file scale (instant, exact) and confirmed drag-select/IME
      both have real, non-mocked test coverage, but couldn't complete a
      live mouse-drag or CJK-IME check in this sandbox — see Phases 3/4's
      own checkpoints for exactly what's still owed and why.)
- [x] Track 20 — LSP integration (all 7 phases shipped and verified
      against real servers: Phase 4 go-to-definition — Ctrl+Click, both
      same-project and JDK decompiled source, tab-switch/no-switch
      confirmed; Phase 6 find-references — Shift+F12 popup, row click
      jumps tabs correctly; Phase 7 rename-symbol — F2 inline box,
      WorkspaceEdit applied across open tabs correctly, not-open-file
      case unconfirmed pending a real Maven/Gradle project, see that
      phase's own checkpoint for detail.) Phase 3's own
      Checkpoint 3 is satisfied: hover returns real documentation for
      both a project-owned symbol and a JDK type, proven by a permanent
      `#[ignore]`d real-jdtls test (`lsp_state::tests::java_hover_
      against_a_real_server_documents_a_project_owned_symbol`) rather
      than a one-off manual check — closing `TECHNICAL_DEBT.md` #20. One
      defect that verification surfaced stays open as #22: jdtls answers
      in Markdown regardless of the client's declared `PlainText`
      preference, and the tooltip is a plain `ui.label`. Kotlin-side
      Phase 5 now has its own equivalent real-server test
      (`kotlin_completion_against_a_real_server_returns_the_receivers_
      own_members`), which closes #18 — the earlier degraded GUI result
      is attributed to #17's server/SDK mismatch, itself since resolved
      and re-probed. A Kotlin GUI click-through is the one thing still
      owed there, both tests being headless by design.
- [x] Track 21 — Maven/Gradle awareness (all 3 phases done: `pom.xml`
      parsing verified against 5 real files; Gradle model extraction
      verified against a real multi-module Kotlin/Spring project, including
      a real `--no-parallel`-race bug found and fixed; classpath resolution
      for both build tools verified end-to-end against real, resolvable
      projects with real jars landing on disk. Unblocks Track 12, Track
      20's classpath feed, and Track 27.)
- [x] Track 22 — Build/run/test integration (all 3 phases shipped and
      live-verified — real `mvn`/`gradle` compile streamed live with
      clickable error rows; Run chains a real `java` launch (direct
      classpath, not `mvn exec:java`/`gradle run`) after a successful
      compile, with a working Stop that genuinely kills the process; Test
      parses either tool's own JUnit-XML report — the same schema for
      both, verified side by side — into a pass/fail summary with a
      clickable row per failing test, jumping to the exact assertion line
      via its own stack trace)
- [ ] Track 23 — Debugger (Phase 1 — DAP client + launch — shipped and
      live-verified: a real jdtls + vendored `java-debug` bundle + real
      Maven project reached a real DAP `Attached` state, `"Launching
      debuggee VM succeeded"` in jdt.ls' own log; Phase 2 — breakpoints +
      stepping — shipped and fully live-verified, both the real-server
      protocol test (a real breakpoint paused a real fixture at the exact
      line, Step Over landed on the next line, Continue ran it to
      completion) and the owed GUI click-through (gutter toggle, Debug
      Project, pause highlight, every toolbar button) closed this session
      on a dedicated Xvfb instance; Phase 3 — variable/call-stack panel —
      shipped and live-verified against the real protocol (real call stack
      + real fetched `args` variable off a real jdtls + java-debug pause);
      its own GUI click-through still owed, same as Phase 1's)
- [ ] Track 26 — Profiler integration
- [x] Track 28 — Language Server settings modal + jdtls/kotlin-language-
      server installer (landed; both servers are now vendored directly
      into the binary rather than built or downloaded on demand — see
      that track for the revision history)
- [ ] Track 29 — Java-version-aware editing + new-project scaffolding
      (Phase 0 live-verified: jdt.ls already enforces a Maven project's own
      declared compliance level with zero client-side help; Phase 1 — JDK
      registry, Settings > JDKs…, plus an Auto-detect button reusing
      lsp_manager's own machine scan — shipped and fully live-verified,
      including persistence across a real restart, also fixing
      TECHNICAL_DEBT.md #23 (UI-thread-blocking folder picker) and #24
      (duplicate JDK-detection code paths) found along the way; Phase 2 —
      correct analysis across Java levels — already shipped and
      live-verified independently as `f5c4931`; Phase 3 — New Project wizard,
      Maven+Java scaffolding — shipped and live-verified: a real
      `com.example.demo`/`demo-app` Java-8 project created through the
      wizard, opened correctly, and a real `mvn -q compile` against it
      outside FoxGarden succeeded; Phase 4 — Gradle (Kotlin DSL) + Java
      scaffolding — shipped (`crates/core/src/scaffold.rs`'s
      `BuildTool::Gradle` arm, `c80ad7d`; checklist here was stale,
      corrected this session — Checkpoint 4's live-verify not re-confirmed
      in this pass); Phase 5 (Kotlin scaffolding, stretch/optional) not
      started)
