# FEATURES.md

Roadmap for FoxGarden becoming a daily-driver editor, then the full Maven/
Kotlin/Java Spring Boot IDE `CLAUDE.md` describes. Local-only, gitignored —
not project documentation; `README.md` is the git-tracked, user-facing
feature list (the Shipped section below should stay a superset of it).

**Schema** — every line item starts with a status tag:

- `[DONE]` — shipped and working.
- `[WIP]` — partially shipped; the remainder is scoped in its own tier
  entry below (the entry it's tagged on tells you which one).
- `[TODO]` — wanted, not started.
- `[SKIP]` — deliberately deprioritized, not "impossible." A reason is
  given so a future pass doesn't re-litigate the same call from scratch.

`[DONE]`/`[WIP]`-stub entries live under **Shipped**, grouped by area —
their build cost no longer matters once shipped. Everything still to build
(`[TODO]`/`[SKIP]`, plus the full remaining-scope detail for any `[WIP]`)
lives under **Not yet done**, grouped by effort tier instead: how contained
the change is, how much new architecture/research it needs, how much
verification back-and-forth it'd take — ordered easiest to hardest, _not_
by user-importance. Detail for any given feature lives in exactly one
place: full detail in its tier entry if there's remaining work, a one-line
stub under Shipped otherwise.

Related docs: `SPEC.md`/`PLAN.md` (also gitignored — working design +
execution-order docs for whatever's currently in flight), `TECHNICAL_DEBT.md`
(git-tracked, durable log of known-deferred issues).

## Shipped

### Project, tabs, sessions

- `[DONE]` Project tree — create/rename/delete, dirs-before-files ordering,
  skip list for VCS/build/dependency noise (`.git`, `target`,
  `node_modules`, ...) so opening a large project doesn't walk everything.
- `[DONE]` Directory rename/delete — recursive; closes/repoints every open
  tab under a deleted/renamed directory.
- `[DONE]` Cut/Copy/Paste on tree nodes — recursive for directories;
  same-filesystem-rename fast path for Cut.
- `[DONE]` Tabbed editing — dirty-state tracking, save-prompt-on-close,
  middle-click-to-close, `Ctrl+Shift+T` reopens the last closed tab.
- `[DONE]` Per-tab read-only toggle (tab context menu or Tools menu) —
  blocks every edit path (typing, paste, `Ctrl+J`, `Ctrl+Shift+G`,
  case-conversion, comment-toggle); navigation/selection/copy untouched.
- `[DONE]` `Ctrl+E` — jump back to a recently open/closed file.
- `[DONE]` `Ctrl+P` — fuzzy "Go to File" across the whole project tree
  (distinct from `Ctrl+E`'s open/recent-tabs-only scope).
- `[DONE]` Session persistence — last project folder, open tabs + focused
  tab, font/theme/indentation settings, all restored across restarts.
- `[DONE]` File watching — an external change reloads transparently (no
  local edits) or shows a "changed on disk" banner with Reload/Keep Mine
  (local edits exist); content-compared, not time-debounced, so this app's
  own saves are never mistaken for an external change.
- `[DONE]` Per-project Application Run Configurations (Run > Edit
  Configurations…) — stored in the project's own
  `.foxgarden/run_configs.json` (travels with the project, unlike
  `eframe::Storage`). Storage/editing only — _running_ one needs
  `Build/run/test integration` (Major tier, below).

### Syntax & diagnostics

- `[DONE]` Java/Kotlin/YAML/XML/`.properties` syntax highlighting via
  tree-sitter, each with its own tree icon. YAML/XML/`.properties` color
  mapping keys/tag names via `Scope::Property`/`Scope::Tag`; Java ALL-CAPS
  constants and Kotlin enum entries get `Scope::Constant`.
- `[DONE]` Dockerfile syntax highlighting — `tree-sitter-containerfile`;
  matched by bare filename (`Dockerfile`, `Dockerfile.dev`,
  `dev.Dockerfile`), since it has no extension to key off.
- `[DONE]` Live syntax-error squiggles with hover tooltip — generic over
  whichever tree-sitter grammar is active, works for all five languages,
  not just Java/Kotlin.
- `[DONE]` Incremental re-parsing.
- `[WIP]` Richer Java/Kotlin syntax highlighting — `Scope::Constant`/
  `Scope::Property` shipped for Java; ongoing, one distinction at a time by
  design. See Moderate-effort tier below for exactly what's left.

### Editing

- `[DONE]` Auto-closing brackets/quotes, with skip-over-existing-closer
  behavior.
- `[DONE]` Wrap-selection — typing a pairable character while a selection
  is active wraps it instead of replacing it.
- `[DONE]` Auto-indent — matches the previous line, +1 level after `{`;
  indentation style (spaces/tabs) and width configurable (Settings >
  Indentation), also respected by Tab-with-no-selection.
- `[DONE]` Tab/Shift+Tab indent/dedent — over an active selection, every
  touched line is indented/dedented by one level without deleting the
  selected text (unlike egui's own default Tab handling, which replaces
  the whole selection); Tab with a collapsed cursor and no selection
  inserts one indent unit; Shift+Tab with a collapsed cursor dedents just
  the current line.
- `[DONE]` `Ctrl+D`/`Ctrl+Shift+D` — multi-cursor "select next occurrence"
  (case-insensitive/case-sensitive).
- `[DONE]` Alt+Click — drops an additional bare cursor without disturbing
  the primary one.
- `[DONE]` Passive occurrence highlight — every other occurrence of the
  word under the cursor, when nothing is selected.
- `[DONE]` `Ctrl+W`/`Ctrl+Shift+W` — expand/shrink selection by syntax node
  (Java/Kotlin only; starts from the word under a bare cursor).
- `[DONE]` Bracket-pair highlighting — outline around the matched pair
  whenever the cursor sits on or beside a bracket.
- `[DONE]` Whitespace rendering, word-wrap toggle, indentation guides
  (Settings/View).
- `[DONE]` `Ctrl+J` — joins the current line with the next, trimming the
  next line's leading indentation to one separating space (or none, for a
  blank-line join or a line already ending in whitespace).
- `[DONE]` `Ctrl+/` — toggles `//` line comments (current line, or every
  line a selection touches).
- `[DONE]` `Alt+↑`/`↓` moves the current line up/down; `Alt+Shift+↑`/`↓`
  duplicates it.
- `[DONE]` Tools > Sort Lines / Unique Lines — over the selection, or the
  cursor's line if nothing's selected.
- `[DONE]` `Ctrl+Shift+U`/`L` (or Tools > Convert to
  UPPERCASE/lowercase/Title Case) — selection case conversion.
- `[DONE]` Smart Home — toggles the cursor between column 0 and the line's
  first non-whitespace character.
- `[DONE]` Live templates — a trigger word + Tab expands it; Java and
  Kotlin each have a built-in set; Help > Live Templates… adds/edits/
  removes custom ones (a custom trigger overrides a built-in of the same
  name; custom templates persist across restarts).
- `[DONE]` Code completion (`SPEC.md`/`PLAN.md`) — a popup opens while
  typing (word-completion, keyword/live-template candidates) or right after
  a `.` (dot-completion: `this.`/`super.`/a local variable typed as an
  in-project Java or Kotlin class, one supertype level, silent fallback for
  anything unresolvable — a JDK/stdlib type, Kotlin's non-constructor-call
  inference gaps, a chained/call-expression receiver). Method candidates
  insert `()` with the cursor placed between the parens (has-args) or right
  after (zero-args).
- `[DONE]` Auto-save (Settings > Auto-save) — off by default; "on focus
  loss" or "after N seconds idle," both reusing `Document::save` unchanged;
  suppressed for any tab currently showing the "changed on disk" conflict
  banner, resuming once Reload/Keep Mine resolves it.
- `[WIP]` Rectangular (column/block) selection — `Alt`+drag; its own
  selection mode, independent of the normal single-range caret.
  Typing/Backspace/Delete over an active block selection already edit
  every spanned row identically. See Moderate-effort tier below for
  exactly what's left (block paste itself).

### Java code generation

- `[DONE]` Boilerplate generation for new Java/Kotlin files.
- `[DONE]` Getter/setter generation from a class's fields at the cursor —
  `Ctrl+Shift+G` for both, or Tools > Generate Getters/Generate Setters for
  one; non-applicable cases (non-Java file, fieldless class) surface why
  through the same error modal other failures use.
- `[DONE]` Constructor / `toString()` / `equals()`+`hashCode()` generation
  — Tools menu.
- `[DONE]` "Override Method" quick action — looks up a class's superclass/
  interface _within the open project only_ (no JDK/library lookup — needs
  the Maven/Gradle dependency-aware classpath, Major tier below), offers
  its not-already-overridden methods as `@Override` stubs.

### Look & feel

- `[DONE]` Light/dark theming; selectable editor font + font size.
- `[DONE]` Sticky scroll (View > Sticky Scroll) — pins the enclosing
  class/method signature to the top of the editor while scrolling through
  its body (Java; persisted).
- `[DONE]` Zen Mode (`F11`, or View > Zen Mode) — hides the menu bar and
  side panel down to just the tab bar and editor; `F11` always works
  regardless of which panels are currently shown.
- `[DONE]` Menu bar with a Tools menu.
- `[DONE]` Code folding (collapse a class/method/control-flow body or a
  run of consecutive imports, Java and Kotlin) — a gutter fold arrow per
  foldable region, click to collapse/expand, Tools/View "Fold All"/"Expand
  All"; built against the existing (non-virtualized) editor widget, not
  gated on `Large file handling`'s virtualization work below as this
  section previously guessed.

### Session & performance

- `[WIP]` Large file handling — the tab-switch cost is fixed (a persistent
  per-tab galley cache survives switching away from and back to a tab, so
  that alone doesn't force a full text reshape); full viewport
  virtualization is left. See Major tier below.
- `[DONE]` `[profile.dev.package."*"]` opt-level override — dependency
  code (egui/epaint's text shaping especially: ~750ms unoptimized vs.
  ~25ms optimized for a glyph-heavy file) runs near-release speed in debug
  builds without slowing down recompiles of our own code.
- `[DONE]` Trailing whitespace stripped from every line on save (before
  the write and in the buffer itself, so the tab doesn't immediately show
  dirty again).

## Not yet done, by effort tier

Ordered easiest → hardest by rough AI-assisted implementation cost, not by
user-importance.

### Quick wins

One subsystem, no new dependencies, narrow scope.

- `[TODO]` **Javadoc stub generation** — not present today; checked
  against this file and added here on request. What *is* already
  present/planned isn't the same thing: `/** */` doc comments get their
  own distinct highlight color (`Scope::DocComment`, Moderate-effort
  entry below), and real rendered Javadoc as hover documentation is
  planned under LSP integration (Major tier, Phase 3 — "a JDK type's own
  Javadoc," via `jdtls`/`kotlin-language-server`, needs that whole
  subsystem first). Neither generates a Javadoc *stub*. This entry is
  that: a Tools-menu action (natural sibling to the already-shipped
  Getters/Setters/Constructor/`toString`/`equals`+`hashCode` generation
  in `widgets::editor::codegen.rs`) that inserts a `/** ... */` skeleton
  above the class/method at the cursor, with `@param` per parameter,
  `@return` if non-`void`, and `@throws` per declared checked exception —
  same "find the enclosing declaration via the tree-sitter tree, build
  the text, insert it" shape that code already uses, no new dependency.

Otherwise, none currently identified — worth a fresh pass next time
there's an opening for a batch of small, contained changes.

### Moderate effort

Contained to a subsystem or two; needs some new state, a new UI element,
or an external crate.

- `[SKIP]` **Multi-select in the tree** (for batch delete) — extends the
  side panel's existing selection/action patterns (new file, rename,
  delete) rather than a new interaction model. (Cut/copy/paste for files,
  the other half of this, shipped — see above.)
- `[WIP]` **Richer Java/Kotlin syntax highlighting** — ongoing, one
  distinction at a time by design (`SPEC.md`'s own note). Shipped:
  `Scope::Constant` (Java ALL-CAPS + explicit `enum_constant`, Kotlin
  enum-entry-as-constant — `TECHNICAL_DEBT.md` #2/#3), `Scope::Property`
  for Java fields (declaration site + `object.field`-style access). Left,
  per a diff against Zed's Java extension (`../references/java`, same
  `tree-sitter-java` grammar): parameters vs. local variables, operators,
  punctuation/brackets, labels, `/** */` doc comments distinct from
  regular ones — each needs its own `Scope` variant plus a color in both
  of `theme.rs`'s light/dark tables, a real design decision, not just a
  query change. Kotlin hasn't had a further pass at all:
  `../references/kotlin`'s reference query targets a different grammar
  than the vendored `tree-sitter-kotlin-ng`, so any improvement has to be
  re-derived from its own `node-types.json` (`TECHNICAL_DEBT.md` #3 has
  the worked example and a candidate list: richer modifier-keyword
  coverage, regex-literal detection, `@variable.builtin` for `it`/`field`).
- `[SKIP]` **Command palette** — a fuzzy-searchable action list; needs a
  registry of actions to search over (most don't exist as decoupled,
  nameable actions yet) plus the search UI itself.
- `[SKIP]` **Local (non-git) file history** — snapshot a file's content
  into a hidden per-project history folder on every save, with a simple
  diff/revert UI. Useful independent of whether the project uses git.
- `[TODO]` **Integrated terminal panel** — spawn a real shell (e.g.
  `portable-pty`), render its scrollback. Distinct from the dedicated
  build/run output panel (Major tier, below) and from the existing "open
  an external terminal" button (shells out to the OS's own terminal).
- `[SKIP]` **Static analysis integration** (Checkstyle/PMD/SpotBugs) —
  shell out to the tool, parse its report format, surface through the
  existing `Diagnostic`/squiggle pipeline rather than inventing a new one.
- `[WIP]` **Rectangular (block) paste** — column/block selection
  (`Alt`+drag) and block-scoped Typing/Backspace/Delete (`PLAN.md` Track 7
  Phases 1-2) both shipped and live-verified; remaining scope is Phase 3,
  block paste itself: clipboard text split on `\n`, one line per row.

### Substantial

Real new subsystems, several files touched, more design decisions to get
right before writing much code.

- `[SKIP]` **Customizable keybindings** — needs a config format and
  rebinding infrastructure threaded through every hardcoded shortcut check
  (currently just `Ctrl+S`, but every future shortcut adds to this).
- `[SKIP]` **Git diff gutter, inline blame, commit/stage/push UI** — more
  git integration than the tree's status indicators; realistically still
  shells out to the `git` CLI rather than embedding `libgit2`, but the UI
  surface (diff rendering, staging flow) is real work.
- `[SKIP]` **Multi-window / split-pane editing** — one tab visible at a
  time, one OS window total today; a real change to the app's state
  model, not just a new widget.
- `[SKIP]` **Spring endpoint map** — a panel listing every
  `@RequestMapping`/`@GetMapping`/etc. found via tree-sitter across the
  project, click to jump to its handler. Layered on top of Maven/Gradle
  awareness (Major tier, below).
- `[SKIP]` **Spring config property autocomplete** — completing keys in
  `application.properties`/`.yml` against each dependency jar's bundled
  `spring-configuration-metadata.json`. Depends on Maven/Gradle
  dependency-aware classpath resolution (Major tier, below) existing
  first.
- `[SKIP]` **Code coverage overlay** — run tests with instrumentation
  (JaCoCo for Maven), parse its report, paint covered/uncovered gutter
  marks. Depends on `Build/run/test integration` (Major tier, below)
  existing first.
- `[TODO]` **Docker/container run integration** — build and run the
  project's Dockerfile/compose stack, stream container logs into the
  output panel. Distinct from just editing those files (Dockerfile
  highlighting, shipped).
- `[SKIP]` **Quick-fix intention actions** (a lightbulb offering an
  auto-import, a suggested fix, etc.) — depends on `LSP integration`
  (Major tier, below) actually supplying structured `CodeAction` data; the
  UI (lightbulb + apply) is comparatively small once that exists.
- `[SKIP]` **Minimap** — a scaled-down whole-file overview beside the
  scrollbar with a viewport indicator and click-to-jump. Its own
  miniature rendering pass over the buffer, separate from the main
  editor's layout.
- `[SKIP]` **Peek definition** (an inline preview of a symbol's definition
  without switching tabs) — consumes the LSP go-to-definition below; the
  peek overlay itself is the new editor-side work.
- `[SKIP]` **Inline diff viewer widget** — a reusable side-by-side/inline
  diff renderer, needed by both the git diff gutter and local file history
  above. Worth building once and sharing between them.

### Major undertakings

Architecture-level work: new protocols, external processes, or a rewrite
of a core piece of the editor.

- `[WIP]` **Large file handling — full viewport virtualization** — Code
  folding (now shipped, see Shipped > Look & feel) turned out **not** to
  need this as a prerequisite after all, contrary to this entry's own
  earlier guess — it already works against the existing (non-virtualized)
  editor widget via a plain hidden-line-range map. Spec'd in `SPEC.md`
  alongside Code folding and Sticky scroll (now shipped). The tab-switch
  half is already fixed
  (see Shipped > Session & performance: persistent per-tab galley cache).
  What's left is the cost of laying out a huge file's _entire_ buffer on
  first open and on every keystroke, since `egui::TextEdit` has no concept
  of "only the visible lines" — it lays out the whole document every time
  the content changes, full stop. Fixing that means rendering only the
  visible line range, which in practice means replacing `TextEdit` with a
  hand-built widget and reimplementing cursor movement, click-to-position,
  drag-select, and IME on top of it — everything `TextEdit` currently
  provides for free.
- `[SKIP]` **LSP integration** — autocomplete, real (semantic) diagnostics,
  go-to-definition, find-references, rename-symbol, hover docs. Needs a
  JSON-RPC/LSP client, per-language server process management (`jdtls`
  for Java, `kotlin-language-server` for Kotlin), and dedicated UI for
  each capability. The single biggest gap for this to read as a "real"
  code editor.
- `[SKIP]` **Maven/Gradle awareness** — parsing `pom.xml`/`build.gradle`, a
  dependency-aware classpath, multi-module project understanding. Today
  only the boilerplate generator's package inference knows Maven/Gradle
  conventions at all, and it's a path-string heuristic, not real
  project-model awareness.
- `[SKIP]` **Build/run/test integration** — process management, an output
  panel, problem-matcher wiring from compiler output back to file/line.
- `[SKIP]` **Debugger** — DAP protocol integration, breakpoints, stepping,
  variable inspection.
- `[SKIP]` **Plugin/extension model** — an actual extensibility API, with
  the sandboxing and loading-mechanism design that implies. Reasonable to
  skip entirely at this stage; a real editor's feature set is usually
  extended by users, not just by us, but that's a "someday" concern, not a
  gap in the current architecture.
- `[SKIP]` **Extension marketplace** — installable/discoverable plugins on
  top of the plugin/extension model above. The distribution and
  marketplace layer implies that underlying API already exists, making
  this an even bigger scope than the API alone.
- `[SKIP]` **Profiler integration** (CPU/heap profiling of a running JVM
  process, flame graphs) — JVM instrumentation/agent attachment plus a
  nontrivial visualization.
- `[SKIP]` **Dependency-injection / bean graph visualizer** — needs real
  semantic understanding of the whole classpath and its annotations, not
  just syntax; effectively gated on the same depth of analysis LSP/
  Maven-Gradle awareness above would need to provide.
