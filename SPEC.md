# SPEC.md

Design spec for **every feature `FEATURES.md` lists as not yet fully
shipped** — its `[TODO]`/`[SKIP]` entries, plus the full remaining-scope
detail for its `[WIP]` ones — grouped into the same three effort tiers
`FEATURES.md` itself uses (Moderate, Substantial, Major), in the same
easiest-to-hardest order. Local-only planning doc (gitignored on `main`,
tracked on `ide-henshin`, same as `PLAN.md`) — a design record, not a
commitment to exact code. Replaces this file's previous single-feature
scope (the Spring endpoint map + terminal panel pass); both of those are
now shipped — see `PLAN.md`'s own build-status history and
`FEATURES.md`'s Shipped section — and this doc's job is what's left, not
what already landed.

**`[SKIP]` is deliberately included, not dropped.** `FEATURES.md`'s own
schema treats `[SKIP]` as "deliberately deprioritized, not impossible" —
this pass writes a real design for those too, at the same depth as the
`[TODO]` ones, so a future session that decides to pick one up isn't
starting from nothing. Deprioritization is a scheduling decision, not a
design decision; nothing here overrides `FEATURES.md`'s own priority
ordering, and picking any one of these up for real implementation should
still start with a fresh look at whether it's still deprioritized for the
same reasons.

**How to read a section below:** each names its `FEATURES.md` tag and
one-line description verbatim, then works through shape/scope, data
model, the concrete files/modules it touches, its own non-goals, and
(where a section leans on unfamiliar territory — an external protocol, a
new crate, a build-tool file format) what needs verifying against real
data before implementation starts, mirroring the "check the actual parser
output/API surface yourself, don't trust this doc's own guess" discipline
`TECHNICAL_DEBT.md` #3 established and every section of this doc's
previous pass (Spring endpoint map, terminal panel) already applied.
Cross-feature dependencies are named explicitly where one exists (e.g.
Spring config autocomplete needs Maven/Gradle awareness first) — those
orderings also drive `PLAN.md`'s own track sequencing.

---

## Contents

Section numbers are stable ids matching `PLAN.md`'s own track numbers,
not a position — §3 (Command palette), §8 (Customizable keybindings), §16
(Minimap), §24 (Plugin/extension model), and §25 (Extension marketplace)
were cut from scope; their numbers are retired, not reassigned, rather
than renumbering every section after them.

**Moderate tier** — contained to a subsystem or two:
1. Multi-select in the tree
2. Richer Java/Kotlin syntax highlighting (remaining scope)
4. Local (non-git) file history
5. Static analysis integration
6. Auto-save
7. Rectangular (block) paste

**Substantial tier** — real new subsystems, several files touched:
9. Git diff gutter, inline blame, commit/stage/push UI
10. Code folding
11. Multi-window / split-pane editing
12. Spring config property autocomplete
13. Code coverage overlay
14. Docker/container run integration
15. Quick-fix intention actions
17. Peek definition
18. Inline diff viewer widget

**Major tier** — architecture-level, external processes, or a rewrite of
a core piece:
19. Large file handling — full viewport virtualization
20. LSP integration
21. Maven/Gradle awareness
22. Build/run/test integration
23. Debugger
26. Profiler integration
27. Dependency-injection / bean graph visualizer

---

# Moderate tier

## 1. Multi-select in the tree

`FEATURES.md`: `[SKIP]` — "extends the side panel's existing selection/
action patterns (new file, rename, delete) rather than a new interaction
model." Batch delete is the only action multi-select unlocks that
single-select can't already do one file at a time; cut/copy/paste
(single-node) already shipped.

**Shape:** `side_panel.rs`'s `SidePanelState` gains `selected: HashSet<PathBuf>`
alongside whatever single-focus field it already tracks. Ctrl+Click toggles
a node in/out of the set; Shift+Click selects the contiguous visual range
between the last-clicked node and the new one (same "anchor + shift-extends"
model most tree/list widgets use, not a new invention). A plain click clears
the set back to a single selection — multi-select is an explicit gesture,
never the default.

**Actions on a multi-selection:** Delete (confirm once, "Delete 4 items?",
not once per file) and the existing Cut/Copy (extended to serialize every
selected path, not just one) are the only two that make sense over a set;
Rename and "New File" stay single-node-only (both are inherently
one-target operations) and are disabled (or simply not shown) in the
context menu when more than one node is selected.

**Non-goals:** no drag-multi-select (a click-drag gesture across rows) —
Ctrl/Shift-click covers the real use case (batch delete) without a second
pointer-gesture to build and test; no cross-directory paste-of-many beyond
what single-node Paste already does per path.

---

## 2. Richer Java/Kotlin syntax highlighting (remaining scope)

`FEATURES.md`: `[WIP]` — shipped: `Scope::Constant` (Java ALL-CAPS +
`enum_constant`, Kotlin enum-entry-as-constant, `TECHNICAL_DEBT.md` #2/#3),
`Scope::Property` for Java fields. Ongoing "one distinction at a time by
design" — this section specs the next distinctions, not a single big
rewrite.

**Remaining for Java** (per a diff against Zed's own Java extension,
`../references/java`, same `tree-sitter-java` grammar this codebase already
vendors):
- **Parameters vs. local variables** — a new `Scope::Parameter` (or reusing
  `Scope::Property` if the two should render identically; a real design
  call to make at implementation time, not assumed here) for
  `formal_parameter`'s own `identifier` child, distinct from a
  `local_variable_declaration`'s.
- **Operators/punctuation** — a `Scope::Operator` for `+`/`-`/`==`/etc.
  token nodes; currently unhighlighted (falls through to `default_text`).
- **Labels** — `labeled_statement`'s own label identifier (rare in
  practice, but distinct enough visually in other editors to be worth the
  one `Scope` variant).
- **Doc comments distinct from regular ones** — `/** */` (`block_comment`
  whose text starts `/**`) vs. a plain `/* */`; needs a text-prefix check
  alongside the existing `block_comment`/`line_comment` node-kind match,
  not a new grammar node (tree-sitter-java doesn't distinguish these as
  separate node kinds).

**Remaining for Kotlin** — hasn't had a further pass at all since the
initial `Scope::Constant` work; `TECHNICAL_DEBT.md` #3's own worked example
is the starting point:
- Richer modifier-keyword coverage (`suspend`, `inline`, `reified`, ...)
  beyond whatever subset already renders.
- Regex-literal detection (Kotlin's `Regex("...")` string-call convention
  has no dedicated grammar node — likely a semantic, not syntactic, call
  and probably out of scope for a tree-sitter-only pass; flag rather than
  attempt if the grammar confirms this).
- `@variable.builtin`-equivalent treatment for `it` (implicit lambda
  parameter) and `field` (property-accessor backing-field reference) — both
  need their own `Scope` (or reuse an existing one) plus a check that they
  only get it in the position where they're actually the implicit binding,
  not an unrelated identifier that happens to be named `it`.

**Every new `Scope` variant needs a color in both of `theme.rs`'s light and
dark tables** (`color_for_scope`) — a real design decision each time (see
`theme.rs`'s own accumulated palette for the established visual vocabulary:
keywords purple/magenta, strings green, types/constants amber, ...), not
just a query change. **Grammar shapes for every item above must be verified
against real `tree-sitter-java`/`tree-sitter-kotlin-ng` parse output before
writing the query** (`TECHNICAL_DEBT.md` #3's own established discipline)
— this section names what's missing, not the exact node shapes, since
those haven't been checked yet.

---

## 4. Local (non-git) file history

`FEATURES.md`: `[SKIP]` — "snapshot a file's content into a hidden
per-project history folder on every save, with a simple diff/revert UI.
Useful independent of whether the project uses git."

**Shape:** on every successful `Document::save`, additionally write the
pre-save buffer content into `<project_root>/.foxgarden/history/<relative
path>/<timestamp>.snapshot` (mirroring the existing `.foxgarden/
run_configs.json` precedent of "project-local state lives under
`.foxgarden/`, travels with the project" — not `eframe::Storage`, which is
this-machine-only). A cap per file (e.g. the most recent 50 snapshots,
oldest pruned) keeps this from growing unbounded across a long-lived
project, mirroring `closed_tabs`'s own `MAX_CLOSED_TABS` precedent for "a
convenience history, not a full audit log."

**UI:** a tab context-menu item ("File History…") opens a simple list of
past snapshots (timestamp + a one-line diff-stat, "+12 -3"), selecting one
shows a read-only diff against the current buffer (needs `Inline diff
viewer widget`, §18 — this feature is blocked on that one existing first,
or duplicates its own diff-rendering logic, which isn't worth doing twice)
with a "Revert to this version" action that replaces the live buffer
content (through the normal edit path, so it's undoable and dirty-tracked
like any other edit, not a special-cased file-replace).

**Non-goals:** no diff *between two arbitrary snapshots* — only "this
snapshot vs. the live buffer," the one comparison the revert flow actually
needs. No cross-machine sync of history (it's a local folder, like the rest
of `.foxgarden/`).

---

## 5. Static analysis integration

`FEATURES.md`: `[SKIP]` — "shell out to the tool, parse its report format,
surface through the existing `Diagnostic`/squiggle pipeline rather than
inventing a new one."

**Shape:** a new Tools > "Run Checkstyle"/"Run PMD"/"Run SpotBugs" menu
item (each tool a separate, explicit action — no auto-detection of which
tool a project uses, since that needs real build-file awareness this
codebase doesn't have, `Maven/Gradle awareness`, §21) shells out to the
configured tool binary (a Settings > External Tools text field for its
path, since none of these ship bundled — a real external dependency the
user must already have installed, unlike `portable-pty`/`vt100`, which are
linked into the binary itself) against the project root, parses its
report (Checkstyle/PMD: XML; SpotBugs: XML with its own schema — each
needs its own parser, not a shared one, the formats aren't related), and
converts each finding into the existing `Diagnostic` shape (file + byte
range + message + severity) already powering the live syntax-error
squiggle — same pipeline, a second *source* of diagnostics feeding it,
not a new rendering path.

**Non-goals:** no live/on-type analysis — these tools run as a whole-
project batch job on demand (Tools menu), not per-keystroke; no
auto-fix application (that's `Quick-fix intention actions`, §15, which
itself depends on `LSP integration`, §20, for a different diagnostic
source — this feature's own findings are read-only surfaces, no
`CodeAction` equivalent from these tools' report formats).

---

## 6. Auto-save

`FEATURES.md`: `[SKIP]` — "explicitly out of scope for checkpoint 1
(`README.md`'s non-goals), but a real gap once this sees daily use. Reuses
`Document::save`."

**Shape:** a new Settings > Auto-save toggle (off by default, preserving
today's explicit-save-only behavior for anyone who doesn't want it) with a
mode choice — "on focus loss" (save whenever the editor widget/window loses
focus while a tab is dirty) or "after N seconds idle" (a debounce timer
reset on every keystroke, firing `Document::save` once it elapses with no
further edits) — both call the *existing* `Document::save` path unchanged,
so trailing-whitespace-stripping/dirty-clearing/file-watcher-suppression
all keep working exactly as they do for an explicit `Ctrl+S` today. No new
save logic, only a new *trigger* for the one that exists.

**Interaction with the "changed on disk" conflict banner:** auto-save
firing while an external-change conflict banner is already showing for
that tab must not silently overwrite the disk version out from under the
user — auto-save is suppressed (falls back to "stays dirty, no save yet")
for any tab currently showing that banner, same as if the user just hadn't
pressed `Ctrl+S` yet; the banner's own Reload/Keep Mine resolution is what
re-enables auto-save for that tab again.

**Non-goals:** no auto-save *interval* configurability beyond the one
idle-timeout number — this isn't meant to grow a full scheduling UI.

---

## 7. Rectangular (block) paste

`FEATURES.md`: `[TODO]` — "pastes clipboard text into an existing block/
column selection, one line per row. Blocked on column/block selection
existing first, which itself has no spec yet."

**The real prerequisite this section has to spec first: column/block
selection itself doesn't exist yet.** Today's selection model
(`ShellState`'s caret/anchor) is a single linear range — no notion of "a
rectangular region spanning columns 4-10 across lines 12-18." Before block
*paste* can mean anything, block *selection* needs:
- A new selection mode, likely `Alt+drag` (common convention — VS Code,
  IntelliJ, Sublime all use it) producing a `BlockSelection { start_line,
  end_line, start_col, end_col }` instead of (or alongside) the existing
  linear `Range<usize>` — `text_area`'s painting/hit-testing both need a
  second code path for "highlight a rectangle of columns across several
  rows" distinct from "highlight a contiguous byte range."
- Typing/Backspace/Delete over an active block selection needs its own
  per-row-edit semantics (delete the same column range on every row the
  block spans) — a real, separate editing mode from today's single-range
  edit path, not a thin wrapper over it.

**Block paste itself**, once selection exists: clipboard text split on
`\n`, row *i* of the split inserted at `(start_line + i, start_col)` for
each row the block selection spans — if the clipboard has fewer lines than
the selection, remaining rows get nothing inserted (not repeated, not
cleared); if it has more, the surplus lines are dropped (a block paste
this codebase can be honest about not handling gracefully beyond the exact
same row-count case rather than inventing wrap-around behavior no one
asked for).

**Non-goals:** no block *copy* generalized beyond what block *selection*
naturally provides (copying a block selection's own text, joined with
`\n`, is what feeds this paste path in the first place — not a separate
feature). No column-selection-aware multi-cursor unification with the
*existing* multi-cursor model (`Ctrl+D`/Alt+Click) — block selection is its
own mode, not a reinterpretation of multi-cursor as "many single-column
selections."

---

# Substantial tier

## 9. Git diff gutter, inline blame, commit/stage/push UI

`FEATURES.md`: `[SKIP]` — "more git integration than the tree's status
indicators; realistically still shells out to the `git` CLI rather than
embedding `libgit2`, but the UI surface (diff rendering, staging flow) is
real work."

**Diff gutter:** on tab open/save/external-reload, shell out to `git diff
--no-color -U0 -- <path>` (unified diff, zero context lines — just the
changed-line ranges) against the file's own working-tree diff, parse the
`@@ -a,b +c,d @@` hunk headers into `added`/`removed`/`modified` line
ranges, paint a colored bar in the gutter (green add, red remove-marker at
the boundary line, blue/orange modified) alongside the existing line-number
column — same "per-tab computed state, refreshed on save" shape the syntax-
error squiggle pipeline already uses, a second gutter decoration source,
not a rendering rewrite.

**Inline blame:** `git blame --porcelain <path>` parsed per line, shown as
a dimmed inline annotation at the end of the current line (cursor-line
only, not every line at once — a full per-line blame gutter competes
visually with the diff gutter above it; a single "hover/cursor-line"
annotation, closer to what VS Code's own GitLens does by default, is the
better fit here) — author + relative date + first line of the commit
message.

**Commit/stage/push UI:** a new dockable panel (structurally similar to
the terminal panel, §8 of the *previous* SPEC.md pass — a bottom or side
dock, its own small UI) listing `git status --porcelain`'s output as a
checkbox tree (stage/unstage individual files or hunks — hunk-level
staging needs `git apply --cached` against a hand-built patch of just the
selected hunk, real parsing/patch-construction work, not a single CLI
call), a commit-message text box + "Commit" button (`git commit -F -`
piping the message in, avoiding shell-escaping the message directly), and
a Push button (`git push`, surfacing stdout/stderr through the existing
`last_error` modal on failure — auth failures, no upstream, rejected
non-fast-forward push, etc. all need to read as *that specific* failure,
not a generic "git failed").

**This is realistically the shell-out-to-CLI approach across the board**
(matches this feature's own `FEATURES.md` framing) — embedding `libgit2`
via the `git2` crate would avoid subprocess overhead and give structured
results instead of parsing CLI text output, but shelling out is simpler,
matches how the tree's *existing* status indicators already work (if they
already shell out — verify against the current status-indicator
implementation before assuming this consistency holds), and avoids adding
a large native-dependency crate for a feature whose main cost is UI
surface, not diffing performance.

**Non-goals:** no merge-conflict resolution UI, no interactive rebase, no
branch management beyond what's needed to commit/push on the current
branch — this is "the daily commit loop," not a full git client.

---

## 10. Code folding

`FEATURES.md`: `[TODO]` — "collapse a method/class body. Needs tree-
sitter-based fold-range computation; no clean home in `egui::TextEdit`'s
single-blob text model, so it likely piggybacks on `Large file handling`
(Major tier)."

**The real blocker, named honestly:** `egui::TextEdit`'s single-blob model
has no concept of "this byte range is collapsed, don't lay it out or paint
it, but keep it in the buffer" — the *existing* fold-map machinery this
codebase already has (`FoldMap`, used by import-block folding per the
`ide-henshin` branch's own recent work) proves the *rendering* half is
already solvable without full virtualization; the real question is
whether that same `FoldMap` shape (a set of hidden line ranges, visual-row
math done through it) generalizes to *arbitrary user-toggled* fold regions
(a method/class body) the same way it already handles *auto-computed*
import-block folding, or whether it needs its own second mechanism. **This
needs a direct look at the current `FoldMap`/import-folding
implementation before assuming either answer** — it may turn out this
feature doesn't need `Large file handling`'s full virtualization rewrite
as a hard prerequisite after all, only as `FEATURES.md`'s own
conservative worst case.

**Fold-range computation:** tree-sitter query per language identifying
"foldable" node kinds (Java: `class_body`, `method_declaration`'s block
body, `interface_body`; Kotlin: `class_body`, `function_declaration`'s
block body) — a fold range is `(open_brace_line, close_brace_line)`, and
the gutter renders a collapse/expand triangle at `open_brace_line` for
each one found (mirroring the *existing* import-block folding's own gutter
marker, if that's how it's currently rendered).

**State:** `Document` (or a per-tab side structure, mirroring `parsers`)
gains `folded_ranges: HashSet<usize>` (line numbers currently collapsed);
toggling one adds/removes its line from the set, and the fold-map used for
layout/painting is rebuilt from the union of auto-import-folding's own
existing hidden ranges plus this set.

**Non-goals:** no fold-state persistence across restarts in a first pass
(recomputed/reset to "everything expanded" on reopen) — revisit only if
that reads as a real gap once the base feature ships. No custom
user-defined fold regions (`// region`/`// endregion` comment markers) —
tree-sitter-node-based folding only.

---

## 11. Multi-window / split-pane editing

`FEATURES.md`: `[SKIP]` — "one tab visible at a time, one OS window total
today; a real change to the app's state model, not just a new widget."

**The real scope, named honestly:** `EditorState.active_tab: Option<usize>`
is a single focus index — the entire app assumes exactly one tab is "the"
active one at any moment. Split-pane editing (two or more tabs visible
side by side in the *same* window) needs `active_tab` to become
`Vec<Option<usize>>` (one per pane) or an equivalent multiple-focus model,
with every place that currently reads `state.active_tab` directly
(dozens of call sites across `app.rs`/`panels/tabs.rs`/the editor widget)
audited for "which pane is this for" — genuinely pervasive, not a
contained addition. Multi-*window* (a second OS window, a second
`eframe`/`egui::Context` instance or a second native window under one
process) is a different, likely *larger* problem: `eframe`'s single-window
assumption would need to be worked around or a multi-viewport approach
(`egui::Context::show_viewport_immediate`, if `eframe`'s current version
supports it — verify against the actual `eframe`/`egui` version this
project pins before assuming the API exists) adopted instead.

**Recommended scope if this is ever picked up:** split-pane first (same
window, same `eframe` app, "just" a state-model change), multi-window
only as a much later, separate follow-up — conflating the two into one
undertaking is how a "real change to the state model" (split-pane) turns
into "also re-architect the windowing model" (multi-window), which is a
different order of risk. This spec deliberately doesn't commit to
designing multi-window at all beyond naming it as out of split-pane's own
scope.

**Non-goals (for split-pane specifically):** no independent side-panel/
terminal-panel-per-pane — those stay single, shared chrome around
whichever panes exist, mirroring how VS Code's own side panel doesn't
duplicate per split either.

---

## 12. Spring config property autocomplete

`FEATURES.md`: `[SKIP]` — "completing keys in `application.properties`/
`.yml` against each dependency jar's bundled `spring-configuration-
metadata.json`. Depends on Maven/Gradle dependency-aware classpath
resolution (Major tier) existing first."

**Hard dependency on `Maven/Gradle awareness` (§21) — not workable
without it.** `spring-configuration-metadata.json` files live *inside*
each dependency's own jar (Spring Boot's annotation processor generates
one per module at build time, bundled into `META-INF/`) — there is no
"walk the open project's file tree" equivalent for this the way the
Spring endpoint map's own extraction got away with (that feature's own
`SPEC.md` entry explicitly noted it doesn't need real classpath
awareness; this one categorically does, since the data it completes
against isn't source the project's own tree contains at all). This
section is a placeholder for *how* it would work once that dependency
exists, not a feature this codebase can build today.

**Once classpath resolution exists:** on opening an `application.properties`/
`.yml` file, resolve the project's dependency jars (from `Maven/Gradle
awareness`'s own resolved classpath), scan each for a bundled
`META-INF/spring-configuration-metadata.json`, parse its `properties`
array (`name`, `type`, `description`) into a flat completion candidate
list, keyed by the property-key prefix already typed (`server.p` →
`server.port`, `server.port-header`, ...) — same completion-popup
mechanism the existing dot-completion already renders through
(`CompletionItem`), a new *candidate source* feeding it, not a new UI.

**Non-goals:** no live re-scan on dependency change (re-resolved when the
classpath itself is re-resolved, whatever cadence `Maven/Gradle awareness`
settles on for that); no completion for custom `@ConfigurationProperties`
classes the project defines itself (that needs semantic understanding of
the project's own annotated classes, arguably closer to what
`LSP integration`, §20, would eventually provide, not this feature's own
jar-scanning approach).

---

## 13. Code coverage overlay

`FEATURES.md`: `[SKIP]` — "run tests with instrumentation (JaCoCo for
Maven), parse its report, paint covered/uncovered gutter marks. Depends
on `Build/run/test integration` (Major tier) existing first."

**Hard dependency on `Build/run/test integration` (§22)** — this feature's
entire trigger ("run tests with instrumentation") needs that feature's own
test-running infrastructure to exist first; there's no standalone "run
JaCoCo" action this codebase can offer without it.

**Once test integration exists:** a "Run with Coverage" variant of the
existing test-run action invokes the build tool with JaCoCo's Maven/Gradle
plugin enabled (`mvn test jacoco:report` / the Gradle `jacoco` plugin's
own report task — exact invocation depends on which build tool `Maven/
Gradle awareness` (§21) determined the project uses), parses the
generated `jacoco.xml` report (an XML format with per-class,
per-line hit-count data), and paints a gutter mark per source line (green
hit, red miss, no mark for a non-executable line like a blank line or a
brace) — a third gutter-decoration source alongside the diff gutter (§9)
and the syntax-error squiggle, same shared-gutter-real-estate
consideration `theme.rs`'s existing gutter colors would need extending to
cover.

**Non-goals:** no branch-coverage-specific visualization (line-level
hit/miss only, matching what a gutter mark can show at a glance); no
coverage-trend-over-time tracking — this is "what does the *last* run
cover," not a historical dashboard.

---

## 14. Docker/container run integration

`FEATURES.md`: `[TODO]` — "build and run the project's Dockerfile/compose
stack, stream container logs into the output panel. Distinct from just
editing those files (Dockerfile highlighting, shipped)."

**Shape:** a Run > "Docker: Build & Run" (single `Dockerfile`) / "Docker
Compose: Up" (a `docker-compose.yml`/`compose.yaml` at the project root)
action shells out to `docker build`/`docker compose up`, streaming
stdout/stderr into an output panel — this needs the same "a dockable panel
showing live process output" infrastructure `Build/run/test integration`
(§22) also needs; **building that shared plumbing once, for whichever of
the two features lands first, and having the other reuse it** is the
right sequencing, rather than two independent output-streaming
implementations. If `Build/run/test integration` hasn't landed yet when
this is picked up, this feature's own output panel becomes that shared
piece's first real implementation, not a Docker-specific one-off.

**Container lifecycle:** a running container/compose stack shows as an
entry in the panel with a Stop button (`docker stop`/`docker compose
down`); closing the panel does *not* stop the container by default
(matching how closing a terminal-panel session behaves differently —
here, a long-running dev server the user wants to keep running in the
background across panel-visibility toggles is the common case, unlike a
terminal session with no meaningful "keep running invisibly" use case) —
only the explicit Stop button, or the app closing entirely (which does
stop every tracked container, so nothing leaks after the app exits).

**Non-goals:** no Dockerfile *linting* beyond existing syntax
highlighting (that's closer to `Static analysis integration`, §5's own
scope, if ever extended to Docker-specific tools like `hadolint`); no
container shell/exec UI (`docker exec -it`) — a user who wants a shell
inside a running container can already use the terminal panel's own
"external terminal" fallback or the in-app one, run `docker exec`
manually, without this feature needing a dedicated button for it.

---

## 15. Quick-fix intention actions

`FEATURES.md`: `[SKIP]` — "a lightbulb offering an auto-import, a
suggested fix, etc. Depends on `LSP integration` (Major tier) actually
supplying structured `CodeAction` data; the UI (lightbulb + apply) is
comparatively small once that exists."

**Hard dependency on `LSP integration` (§20)** — the LSP `textDocument/
codeAction` request is what actually supplies "here's a specific,
structured fix for this diagnostic" data (a `WorkspaceEdit`); without a
language server, there's no source of *real* code actions to offer (a
static-analysis-only fallback — e.g. always offering "suppress this
Checkstyle rule" for a `Static analysis integration`, §5, finding — is a
much narrower, lower-value version of this feature and arguably not worth
building on its own ahead of real LSP-backed actions).

**Once LSP integration exists:** a small lightbulb icon appears in the
gutter on any line with an active diagnostic that has one or more
associated `CodeAction`s (requested via `textDocument/codeAction` scoped
to that diagnostic's range); clicking it (or a keyboard shortcut with the
cursor on that line) shows a small popup listing each action's title,
picking one applies its `WorkspaceEdit` via the *same* text-editing
primitives the rest of the editor already uses (an edit is an edit,
regardless of whether a human typed it or a language server generated
it) — `Document`'s own edit-application path, not a new one.

**Non-goals:** no *editor-generated* quick fixes independent of LSP (e.g.
a hardcoded "add missing import" heuristic built directly into this
codebase, bypassing the language server) — that would duplicate semantic
work a real language server already does correctly, and is exactly the
kind of "syntax-only heuristic masquerading as semantic understanding"
this codebase's own completion feature already draws a hard line against
for anything beyond simple-name matching.

---

## 17. Peek definition

`FEATURES.md`: `[SKIP]` — "an inline preview of a symbol's definition
without switching tabs. Consumes the LSP go-to-definition below; the peek
overlay itself is the new editor-side work."

**Hard dependency on `LSP integration` (§20)** for go-to-definition
resolution — this codebase's own existing "jump to a symbol's
declaration" capabilities (Override Method's superclass lookup,
completion's own type resolution) are narrow, single-purpose lookups, not
a general "resolve this identifier to its declaration site" primitive a
peek feature needs across arbitrary code, which only a real language
server realistically provides.

**Once go-to-definition exists (LSP or otherwise):** triggering peek (a
keyboard shortcut or a gutter icon, distinct from an actual jump so the
current tab/scroll position isn't disturbed) opens an inline expandable
panel *within* the current editor's own layout, at the line below the
peek request — showing the target definition's surrounding lines
(read-only, syntax-highlighted the same as any other view) without
switching tabs or scrolling the main editor away from where the user was
reading. Dismissing it (Escape, or clicking outside it) collapses the
inline panel back to nothing, leaving the main editor exactly as it was.

**Non-goals:** no *editing* inside the peek view (read-only preview only
— "Go to Definition" proper, a real tab switch, is already what full
editing access requires); no multi-result peek UI (if a symbol resolves
to more than one definition — an interface with several implementations —
this shows the first/primary result only in a first pass, deferring a
disambiguation picker).

---

## 18. Inline diff viewer widget

`FEATURES.md`: `[SKIP]` — "a reusable side-by-side/inline diff renderer,
needed by both the git diff gutter and local file history above. Worth
building once and sharing between them."

**Shared prerequisite for `Git diff gutter` (§9)'s own full-diff view (if
it ever grows beyond the gutter-marks-only scope specced there) and
`Local file history` (§4)'s snapshot-vs-live comparison** — building this
once, as its own reusable widget, rather than each of those two features
growing its own bespoke diff renderer, is the entire point of this
section existing separately.

**Shape:** `pub fn show_diff(ui: &mut egui::Ui, old: &str, new: &str,
mode: DiffMode) -> DiffResponse` where `DiffMode` is `SideBySide` or
`Inline` (unified-style, `+`/`-` prefixed lines interleaved) — computed via
a line-level diff (the `similar` crate, a well-established, actively
maintained Rust diffing library — verify it's still the right choice by
checking its current crates.io status before pinning a version, rather
than assuming this doc's own naming stays accurate indefinitely) producing
an ordered list of `Equal`/`Delete`/`Insert`/`Replace` line ops, each
rendered as a colored row (green insert, red delete, a paired red/green
row for a replace) using the editor's own font/theme, same "reuse the
app's own visual language" principle every other new widget in this doc
follows.

**Non-goals:** no inline *editing* through the diff view (read-only
rendering; `Local file history`'s own revert action operates on the whole
snapshot, not a line-by-line accept/reject the way a merge-conflict UI
would need — that's out of scope here and for `Git diff gutter`, §9, which
explicitly excludes merge-conflict resolution too). No word-level diff
highlighting within a changed line (line-level granularity only, for a
first pass).

---

# Major tier

## 19. Large file handling — full viewport virtualization

`FEATURES.md`: `[WIP]` — the tab-switch cost is already fixed (a
persistent per-tab galley cache survives switching away from and back to
a tab). What's left: laying out a huge file's *entire* buffer on first
open and on every keystroke, since `egui::TextEdit` has no concept of
"only the visible lines."

**The real scope, unchanged from `FEATURES.md`'s own framing:** fixing
this means rendering only the visible line range, which in practice means
**replacing `egui::TextEdit` with a hand-built widget** and reimplementing
cursor movement, click-to-position, drag-select, and IME on top of it —
everything `TextEdit` currently provides for free. This is the single
largest undertaking in this entire doc measured by "how much of the
editor's existing behavior has to be reimplemented rather than reused,"
since the *entire* editor currently sits on `egui::TextEdit`'s own
internals for those four things.

**Recommended shape, if picked up:** keep the *data model*
(`Document`/`Rope` buffer, `ShellState` caret/anchor, `HighlightSpan`s)
entirely unchanged — this is a rendering/interaction-layer rewrite, not a
data-model one. The new widget:
- Computes `visible_rows` from scroll offset + viewport height + row
  height (exactly the math `text_area::render::visible_rows` already does
  for the *no-wrap* fast path today — the real new work is making the
  *word-wrap* path, `layout_visible_wrapped`'s `cached_row_counts`, stop
  needing every line's row-count computed up front, since that's the part
  that still scales with total file size even though painting itself
  already doesn't).
- Reimplements click-to-position (`layout_visible`'s existing
  `pos_from_cursor`/hit-testing logic already provides the *building
  blocks* — `char_rect`, `row_galleys` — this widget needs to keep using
  those exact same helpers, just called only for the visible slice, not
  reinvent hit-testing from scratch).
- Reimplements drag-select and IME composition — `egui::TextEdit`'s own
  source (vendored or referenced via `../references/zed` for how a
  production editor handles this without `TextEdit` at all — Zed's own
  editor never used `egui::TextEdit` to begin with, so its handling of
  these is the actual reference implementation to study, not egui's) is
  the concrete precedent to study before writing this from scratch.

**Code folding (§10) piggybacks on whatever `FoldMap`-generalization this
work produces** — see that section's own note that this dependency should
be re-verified, not assumed, before committing to it as a hard
prerequisite.

**Non-goals:** no change to the actual editing operations (auto-indent,
bracket-matching, multi-cursor, live templates, ...) — every one of those
operates on the `Document`/`ShellState` data model, which this rewrite
doesn't touch; they should keep working unchanged once the new widget
correctly reads/writes the same state the old `TextEdit`-based one did.

---

## 20. LSP integration

`FEATURES.md`: `[SKIP]` — "autocomplete, real (semantic) diagnostics,
go-to-definition, find-references, rename-symbol, hover docs. Needs a
JSON-RPC/LSP client, per-language server process management (`jdtls` for
Java, `kotlin-language-server` for Kotlin), and dedicated UI for each
capability. The single biggest gap for this to read as a 'real' code
editor."

**Client architecture:** a per-language-server child process (`jdtls` for
Java, `kotlin-language-server` for Kotlin — both real, existing binaries
the user must have installed/discoverable, a Settings > External Tools
path each, same pattern `Static analysis integration` (§5) already
establishes for externally-installed tool binaries), communicating over
stdio via LSP's JSON-RPC framing (`Content-Length: N\r\n\r\n<json>`).
`../references/java`/`../references/kotlin` (Zed's own extensions for
these exact two language servers) are the concrete reference for the
*server-specific* quirks (which capabilities `jdtls`/`kotlin-language-
server` actually implement well vs. poorly, non-standard initialization
options either expects) — read those before assuming the LSP spec alone
is enough to get a working integration; **every production editor's own
integration with these two servers carries workarounds the bare spec
doesn't hint at, and inventing an integration from the spec alone risks
rediscovering all of them the hard way.**

**Crate choice for the JSON-RPC/protocol-types layer:** `lsp-types`
(protocol type definitions — requests/responses/notifications as real
Rust structs, avoiding hand-rolled JSON schema matching) plus either a
hand-rolled stdio-framing read/write loop (a background thread per
server, mirroring `PtySession`'s own "background reader thread, channel
into the UI thread" shape this codebase already established for the
terminal panel) or an existing async LSP-client crate if one's current
maturity/maintenance state justifies the dependency over hand-rolling —
**verify the current state of the Rust LSP-client crate ecosystem before
committing to either path**, since this is exactly the kind of "check
real, current library state, don't assume" call this whole doc's own
discipline demands, and crate maturity here specifically is likely to
have shifted since this doc was written.

**Feature surface, roughly in the order that makes sense to land it
(each its own real phase, not a single undertaking):**
1. Server lifecycle + `initialize`/`initialized` handshake, no user-
   visible feature yet — the checkpoint-able foundation every capability
   below needs.
2. Diagnostics (`textDocument/publishDiagnostics`) — feeds the *existing*
   `Diagnostic`/squiggle pipeline (a second, semantic source alongside
   syntax errors and, if built, `Static analysis integration`'s findings)
   — the smallest real capability to prove the connection works
   end-to-end.
3. Hover docs (`textDocument/hover`) — a tooltip on hover, structurally
   similar to the *existing* syntax-error hover tooltip.
4. Go-to-definition (`textDocument/definition`) — the first capability
   needing the "open a file at a position" cross-tab primitive this
   codebase's own Spring-endpoint-map work already built
   (`pending_navigation`, `app.rs`) — reused directly, not reinvented.
5. Autocomplete (`textDocument/completion`) — a second candidate source
   feeding the *existing* completion popup (alongside word-completion/
   dot-completion), the highest-value capability and also the one with
   the most UI-merging subtlety (ranking/deduping LSP candidates against
   this codebase's own existing ones in one coherent list).
6. Find-references (`textDocument/references`) — needs a results-list UI
   (another `go_to_file.rs`-shaped popup, or a dockable panel if the
   result count regularly exceeds a popup's comfortable size).
7. Rename-symbol (`textDocument/rename`) — applies a `WorkspaceEdit`
   spanning potentially many files at once; needs a real "apply edits
   across every affected open-or-not-yet-open `Document`" path, more
   involved than the single-file edits every other capability produces.

**Non-goals:** no support for language servers beyond `jdtls`/`kotlin-
language-server` in a first pass (no generic "any LSP server" config,
though the client itself should be protocol-generic under the hood — the
UI/setup flow is scoped to exactly these two); no multi-root-workspace
LSP support (one server instance per open project, matching this
codebase's own existing single-project-open model everywhere else).

---

## 21. Maven/Gradle awareness

`FEATURES.md`: `[SKIP]` — "parsing `pom.xml`/`build.gradle`, a
dependency-aware classpath, multi-module project understanding. Today
only the boilerplate generator's package inference knows Maven/Gradle
conventions at all, and it's a path-string heuristic, not real
project-model awareness."

**Two genuinely separate sub-problems, worth naming as such rather than
one undertaking:**

**1. Build-file parsing (the contained, tractable half).** `pom.xml` is
plain XML (`quick-xml`/`roxmltree` — either an established, low-level XML
parser, verify current crate health before picking one) — parsing
`<dependencies>`/`<modules>`/`<properties>` into a `MavenProject` struct
is real but bounded work, similar in shape to this codebase's own
tree-sitter-based extraction work (a structured parse of a known,
documented file format). `build.gradle`/`build.gradle.kts` is the harder
half: Gradle build files are executable Groovy/Kotlin *code*, not
declarative data — a real parse means either (a) shelling out to Gradle's
own `--offline` + a custom init-script that dumps resolved project model
as JSON (the approach every serious Gradle-aware tool, including Gradle's
own official tooling API consumers, actually takes — parsing the Groovy/
Kotlin DSL *as* code with tree-sitter would only recover the *textual*
structure, not what it *resolves to* after Gradle's own conventions/
plugins apply), or (b) depending on Gradle's own Tooling API (a Java
library — would need a JVM subprocess bridge, a nontrivial integration
of its own). **(a) is very likely the pragmatic choice** — it reuses the
project's *own* already-installed Gradle wrapper rather than needing to
embed a JVM-facing API client, but this needs validating against a real
multi-module Gradle project before committing.

**2. Dependency-aware classpath resolution (the genuinely hard half).**
Even with `pom.xml`/Gradle-model data parsed, resolving the *actual jar
files* on disk means either invoking `mvn dependency:build-classpath`/
Gradle's own dependency-resolution task (shelling out again, reusing the
project's own build tool rather than reimplementing Maven Central/jar-
resolution logic from scratch — reimplementing that resolution logic
directly would be its own multi-month undertaking and isn't seriously in
scope here) and parsing the resulting classpath string/file list.

**What this unlocks, once both halves exist:** `Spring config property
autocomplete` (§12), a real "resolve this import to its actual JDK/
library source" for hover/go-to-definition beyond what `LSP integration`
(§20) alone would need a classpath *for* in the first place (a language
server like `jdtls` actually needs to be *told* the resolved classpath at
`initialize` time via its own configuration — meaning **this feature is
also a real prerequisite for `jdtls` working correctly on any non-trivial
project**, not just a nice-to-have alongside it), and better accuracy for
the existing Override Method/dot-completion features' own "look up a
class... within the open project only" limitation (extending their
search to resolved dependency jars, not just the open project's own
source tree).

**Non-goals:** no dependency *version resolution conflict* UI (accept
whatever the build tool itself resolves to, don't attempt to second-guess
or visualize its own conflict-resolution decisions); no support for
custom/private Maven repositories requiring auth beyond whatever the
user's own Maven/Gradle configuration already handles (this feature
shells out to the user's own already-configured tooling, it doesn't
reimplement repository auth).

---

## 22. Build/run/test integration

`FEATURES.md`: `[SKIP]` — "process management, an output panel, problem-
matcher wiring from compiler output back to file/line."

**Benefits from `Maven/Gradle awareness` (§21) existing first** (to know
which build tool a project uses and its module layout) but can start with
a narrower, still-useful scope even without it: a Run > "Run" / "Test"
action that just shells out to `mvn/./mvnw <goal>` or `gradle/./gradlew
<task>` at the project root (preferring a wrapper script if present, the
same convention every Maven/Gradle-aware tool follows, since it pins the
exact tool version the project expects rather than whatever's on the
user's global `PATH`) using whichever `RunConfig` the user has already
defined (`.foxgarden/run_configs.json` — this codebase already has
storage/editing for run configurations; this feature is what makes
*running* one actually do something, closing the gap `FEATURES.md`'s own
Shipped section explicitly names: "Storage/editing only — running one
needs `Build/run/test integration`").

**Output panel:** shares the same "dockable panel streaming live process
output" infrastructure `Docker/container run integration` (§14) also
needs — whichever of the two lands first builds it, the other reuses it
(see that section's own note). Streams stdout/stderr line-by-line,
`request_repaint` on new output, same background-thread-to-channel
shape `PtySession` already established for the terminal panel (though
this is a *plain* pipe, not a pty — a build/test process doesn't need
`isatty()` to behave correctly the way an interactive shell does, so
`portable-pty` isn't needed for this feature specifically, just
`std::process::Command` with piped stdout/stderr).

**Problem-matcher wiring (the genuinely new parsing work):** a per-tool
regex/pattern set recognizing compiler-error line shapes in stdout/stderr
(`javac`'s own `<path>:<line>: error: <message>` format at minimum; a
Maven/Gradle-wrapped build reformats or prefixes this differently
depending on the tool and plugin version — needs verifying against real
build output, not assumed from `javac`'s own bare format) and converting
each match into a clickable entry in the output panel that jumps to the
file/line via the same `pending_navigation` cross-tab-jump primitive the
Spring endpoint map's own jump-to-handler already built.

**Non-goals:** no test-result tree/reporting UI beyond the raw streamed
output plus problem-matcher jump links in a first pass (a structured
"N passed, M failed, click a failure to jump to its assertion" view is a
real, separate follow-up, not assumed here); no parallel/concurrent
multi-run-config execution (one run at a time, matching how most
single-window IDEs' own default Run behavior works too).

---

## 23. Debugger

`FEATURES.md`: `[SKIP]` — "DAP protocol integration, breakpoints,
stepping, variable inspection."

**Depends on `Build/run/test integration` (§22) existing first** — a
debugger needs to *launch* the target process (or attach to one already
running via that feature's own process-management plumbing) before
stepping/breakpoints mean anything; building debug launch independent of
that feature's own run infrastructure would duplicate it.

**Protocol:** DAP (Debug Adapter Protocol) — same JSON-RPC-over-stdio
shape as LSP (§20), a real *second* protocol client to build (DAP and LSP
are related in spirit but are two distinct protocols with their own
message schemas — no code reuse between the two beyond "we already know
how to frame/parse a JSON-RPC-over-stdio stream," which is genuinely
useful shared plumbing, not a false savings). Needs a Java debug adapter
(`java-debug`, the Eclipse/`vscode-java` project's own debug adapter
implementation, run as a child process alongside or launched by `jdtls`)
and, separately, a Kotlin one (Kotlin debugging typically runs through
the *same* JDI/JDWP-based tooling as Java, since Kotlin compiles to JVM
bytecode — likely the same debug adapter covers both languages, but
**verify this concretely against `java-debug`'s own documented Kotlin
support before assuming parity**, rather than treating "compiles to the
same bytecode" as proof the tooling already handles it well).

**UI surface:** breakpoint gutter markers (click a line number's margin to
toggle, mirroring how the diff/coverage gutters, §9/§13, already share
that same margin's real estate — a genuine "how many things want the
gutter" design tension worth resolving explicitly once several of these
land together, not before any of them exist); a debug toolbar (Continue/
Step Over/Step Into/Step Out/Stop); a variables/call-stack side panel
(structurally another dockable panel, alongside the side/terminal panels
this codebase already has); inline "current line" highlight while paused
at a breakpoint.

**Non-goals:** no remote debugging (attach to a JVM on a different
machine) — local process launch/attach only, matching every other
feature in this doc's own "local only" scope; no conditional-breakpoint
expression evaluation in a first pass (a breakpoint either fires or
doesn't — expression conditions are DAP-spec-supported but add real
complexity to the breakpoint UI itself, worth deferring past a first
working pass).

---

## 26. Profiler integration

`FEATURES.md`: `[SKIP]` — "CPU/heap profiling of a running JVM process,
flame graphs. JVM instrumentation/agent attachment plus a nontrivial
visualization."

**Depends on `Build/run/test integration` (§22) (to launch/attach to the
target JVM process) and benefits from `Debugger` (§23)'s own DAP/process-
management plumbing existing first**, though profiling and debugging are
functionally independent (a user might profile a process they're not
debugging at all) — the dependency here is on shared *process-launch*
infrastructure, not on the debugger's own stepping/breakpoint machinery
specifically.

**Approach:** JVM profiling realistically means attaching `async-profiler`
(the de facto standard low-overhead JVM profiler, widely used precisely
because it doesn't require special JVM flags at startup — it attaches via
the JVM's own Dynamic Attach mechanism to an already-running process) as a
native agent, either invoked as a CLI wrapper (shelling out to its own
`asprof`/`profiler.sh` script against the target PID, parsing its
generated output) or loaded via its own socket/file-based control
protocol if driving it more directly is worth the integration cost over
just shelling out — **verify `async-profiler`'s current invocation/output
format against its actual current documentation before committing to
either approach**, since profiler tooling specifics are exactly the kind
of "check real current state" item this doc's discipline applies to
everywhere else.

**Visualization:** flame graphs are `async-profiler`'s own well-
established output format (it can emit these directly, via Brendan
Gregg's original `FlameGraph` collapsed-stack format) — rendering one is
its own dedicated interactive-SVG-like widget (rectangles sized by sample
count, stacked by call depth, hover for the full symbol name, click to
zoom into a subtree) — real, nontrivial custom-painting work, though
narrower in scope than, say, the terminal panel's own full cell-grid
renderer, since a flame graph's own shape is simpler (static once
captured, no live per-frame update needed the way a running terminal
session has).

**Non-goals:** no heap-dump *analysis* UI (object retention graphs, etc.)
beyond raw heap-profiling sample capture — a full heap analyzer (in the
shape of, say, Eclipse MAT) is its own separate, large undertaking this
section doesn't attempt to scope; no continuous/production profiling —
local, on-demand profiling of a process this app itself launched or that
the user points it at, matching this doc's "local only" theme throughout.

---

## 27. Dependency-injection / bean graph visualizer

`FEATURES.md`: `[SKIP]` — "needs real semantic understanding of the whole
classpath and its annotations, not just syntax; effectively gated on the
same depth of analysis LSP/Maven-Gradle awareness above would need to
provide."

**Hard dependency on both `Maven/Gradle awareness` (§21) and, for real
accuracy, `LSP integration` (§20)** — a Spring bean graph (which
`@Component`/`@Service`/`@Repository`/`@Bean`-annotated classes exist,
which ones `@Autowired`/constructor-inject which others, which
`@Configuration` classes `@Import` which others) needs to resolve types
across the *entire* classpath, not just the open project's own source
tree the way the Spring endpoint map's own honest "no real classpath"
scope got away with — a bean's dependency might be an interface whose
single implementation lives in a *different* module or a third-party
library jar entirely, which only real classpath awareness can resolve.
Syntax-only extraction (walking `@Autowired` annotations the way the
endpoint map walks `@GetMapping` ones) would produce a graph with
silently-missing edges for exactly the cross-module cases that make a
real Spring application's bean graph interesting to visualize in the
first place — not an honest smaller feature the way the endpoint map's
own scope limits are, but a *misleading* one, since a bean graph that
quietly drops external-jar/other-module edges looks complete while
actually being wrong in a way a user can't easily tell from the picture
alone.

**Shape, once both dependencies exist:** a whole-project + whole-
classpath scan collecting every discoverable bean definition (annotated
class, `@Bean`-annotated method in a `@Configuration` class) and every
discoverable injection point (constructor parameter, `@Autowired` field,
`@Autowired` setter) on each, resolving each injection point's declared
type to the bean(s) that satisfy it (by type, or by `@Qualifier`
name if present) — a directed graph, rendered as an interactive node/
edge diagram (a new custom-painting widget: nodes as boxes, edges as
routed lines, pan/zoom, click a node to jump to its source) in its own
dockable panel.

**Non-goals:** no *runtime* bean-graph inspection (attaching to a running
Spring context via Actuator's own `/beans` endpoint or JMX, which would
show the *actual* resolved graph including profile-specific/conditional
beans a static analysis can't fully resolve) as a first pass — purely
static analysis, with the corresponding accuracy gap around
`@ConditionalOn*`/profile-specific beans named explicitly as a known gap,
not silently assumed away; no cycle-detection *diagnostics* beyond what
the visualization itself makes visually obvious (a real "detect and
report a circular dependency" check is a reasonable later addition, not
assumed here).
