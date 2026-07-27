# PLAN.md

Execution plan for `SPEC.md`'s Spring endpoint map: a `Ctrl+Shift+E`
popup listing every Spring MVC endpoint found across the open project's
Java and Kotlin controllers, jump-to-handler on pick. Fully replaces
whatever this file covered before (the previous code-completion pass —
see git history/`TECHNICAL_DEBT.md` if any of its items need to survive;
nothing here continues that work). Local-only planning doc (tracked on
`ide-henshin`, same as `SPEC.md`) — a commitment to an *order*, not a
timeline.

Dependency graph:

```
Phase 0  Java endpoint extraction    ─┐   [crates/syntax, headless]
Phase 1  Kotlin endpoint extraction  ─┤
                                       │  (0/1 don't depend on each other —
                                       │   either order, or genuinely
                                       │   parallel across two sessions)
                                       └─► Phase 2  Whole-project scan
                                               [crates/app, new module]
                                               │
                                               └─► Phase 3  Popup UI shell
                                                       (list/filter/select
                                                       a real scanned list;
                                                       picking a row
                                                       doesn't navigate yet)
                                                       │
                                                       └─► Phase 4  Jump-to-
                                                               handler (the
                                                               new cross-
                                                               tab-switch
                                                               `set_caret`
                                                               wiring)
```

Each phase ends at a **green checkpoint**: `cargo build --workspace`,
`cargo test --workspace`, `cargo clippy --workspace --all-targets` all
pass. Phases 2-4 additionally need a live click-through in `cargo run -p
foxgarden` for anything the phase touches, per `AGENTS.md`'s testing-
conventions section: build the change, get the automated checkpoint
green, then hand the user exact numbered steps and wait for them to
report back what actually happened — never claim a click-through passed
without that. **Unlike the completion feature's own plan, there is no
early "useful on its own" stopping point here** — Phase 3's popup with no
working jump is a complete mechanism proven live, but not a usable
feature by itself (a list you can't act on). If this doesn't land in one
sitting, Phase 2 (a real, tested whole-project scan with no UI yet) is
the most defensible pause point, not Phase 3.

---

## Phase 0 — Java endpoint extraction

New `crates/syntax/src/spring_endpoints.rs`. `EndpointInfo` (`SPEC.md`
§1) and `java_endpoints_in_file(tree: &Tree, source: &str) ->
Vec<EndpointInfo>` (`SPEC.md` §2): walks every `class_declaration`
(top-level and nested, same "don't miss nested classes" precedent
`fields.rs`/`methods.rs` already set), reading each one's own
`@RequestMapping` (if any) as a base path, then every `method_declaration`
in its body carrying one of the five recognized mapping annotations.

- Recognized-annotation dispatch table (`GetMapping`→`GET`, …,
  `RequestMapping`→ from `method =` or `"ANY"`) and the path-joining rule
  — both exactly as `SPEC.md` §2 specifies.
- `annotation`/`marker_annotation` extraction from a `modifiers` node's
  children, `annotation_argument_list`'s bare-value vs. `element_value_
  pair` shapes, `string_literal`'s `string_fragment` child for the actual
  text — verify all of this against `tree-sitter-java-0.23.5`'s real
  parse output fresh (this spec's own dump is a starting point, not a
  substitute for checking) before writing the extraction, same "check
  `node-types.json`/real parser output yourself" discipline
  `TECHNICAL_DEBT.md` #3 established for Kotlin, applied here to a piece
  of Java's own grammar this codebase hasn't walked before.
- Table tests per `SPEC.md` §2's list: positional-string path, `value =`,
  `path =`, `method =` combined with a class-level base path, a bare
  marker annotation, no class-level base path, a nested class, multiple
  unrelated annotations on one method, no recognized annotation at all.

**Checkpoint 0:** `cargo test -p syntax` green. No live click-through
needed yet — nothing in the running app calls this module until Phase 2.

---

## Phase 1 — Kotlin endpoint extraction

`spring_endpoints.rs`, same file (mirrors `kotlin_members.rs` living
alongside `fields.rs`/`methods.rs` rather than as a separate module,
since both languages' extraction is one concern: "what endpoints does
this file declare").

- **First**, independent of any code: confirm every node kind/shape
  `SPEC.md` §3 cites (`annotation`, `constructor_invocation`,
  `value_argument`, `collection_literal`, `string_content`, `navigation_
  expression`, and the bare-marker-annotation shape §3 explicitly flags
  as unverified) against `tree-sitter-kotlin-ng`'s own `node-types.json`
  *and* a real parse dump fresh — re-verify rather than trusting `SPEC.md`
  already did this correctly, same discipline Phase 0 just applied to
  Java.
- `kotlin_endpoints_in_file(tree: &Tree, source: &str) ->
  Vec<EndpointInfo>` — same recognized-annotation table, same path-join
  rule, same "method-level annotation makes it an endpoint regardless of
  class-level `@Controller`" rule as Phase 0, walking `class_body`'s
  `function_declaration` children (mirroring `kotlin_members.rs`'s own
  traversal) instead of Java's `method_declaration` children.
- Shared `endpoints_in_file(language, tree, source) -> Vec<EndpointInfo>`
  dispatcher (`SPEC.md` §3, "Shared entry point") — the one function
  outside this module actually calls.
- Table tests per `SPEC.md` §3's list, translated to Kotlin syntax, plus
  the Kotlin-specific array-literal-unwrapping case for `method =`/
  `path =` explicitly (Kotlin requires `[RequestMethod.DELETE]` where
  Java accepts a bare `RequestMethod.DELETE`).

**Checkpoint 1:** `cargo test -p syntax` green for both sub-phases; no
live click-through needed yet, same reasoning as Checkpoint 0.

---

## Phase 2 — Whole-project scan

New `crates/app/src/widgets/editor/spring_scan.rs` — **not**
`codegen.rs`: that file is already ~720 lines covering getter/setter/
constructor/`toString`/`equals`+`hashCode` generation plus the shared
file-finder, a cohesive "code generation" concern this scan doesn't
belong in (it doesn't generate anything, and doesn't share logic with
those functions beyond the same recursive-tree-walk shape
`find_source_file_by_stem`/`go_to_file.rs`'s `all_files` already use
independently of each other).

- `pub fn scan_project_endpoints(root: &FileNode) ->
  Vec<(PathBuf, EndpointInfo)>` (`SPEC.md` §4): walks every `.java`/`.kt`
  file in the tree, reads + throwaway-parses each with a fresh
  `IncrementalParser`, calls `syntax::endpoints_in_file` on each, and
  collects the results with their originating path attached. A file that
  fails to read is skipped silently (`TECHNICAL_DEBT.md` #11's own "don't
  fail the whole operation over one bad entry" reasoning, not a new
  decision this phase invents).
- Tests: a small multi-file project fixture (mirroring dot-completion's
  own cross-project fixture shape — a temp dir with 2-3 `.java`/`.kt`
  files, at least one controller with a class-level base path and
  multiple mapped methods, one plain non-controller file) asserting the
  aggregated list contains exactly the expected entries with the right
  paths attached; a non-source file in the tree is ignored; an empty
  project returns `[]`.

**Checkpoint 2:** `cargo test -p app` (or workspace-wide) green. No live
click-through yet — still no UI calls this function.

---

## Phase 3 — Popup UI shell

New `crates/app/src/panels/spring_endpoints.rs`, structurally a near-twin
of `go_to_file.rs` (`SPEC.md` §5): `SpringEndpointsState { open, query,
selected }` + `toggle()`, `show(ui, state: &EditorState, popup: &mut
SpringEndpointsState) -> Option<(PathBuf, usize)>`.

- Re-scan via Phase 2's `scan_project_endpoints` once per `toggle()`
  (open), not every frame — mirrors `CompletionState::open`'s own
  "recomputed on open" reasoning.
- Reuse `go_to_file.rs`'s `fuzzy_score` for filtering against each row's
  rendered text (`"GET /api/users/{id} — UserController#getUser"`) —
  promote it from private to `pub(crate)` if it isn't already reachable
  from a sibling `panels` module, rather than writing a second fuzzy
  matcher.
- Wire into `app.rs`: a `spring_endpoints: SpringEndpointsState` field on
  `FoxGardenApp`, a `Ctrl+Shift+E` shortcut (**verify it's still unused**
  against `menu_bar.rs`'s current shortcuts before wiring it — this plan
  believes it's free as of Phase 0/1's own writing, but re-check fresh,
  same "verify, don't trust an older note" discipline this whole feature
  already applies to grammar claims) or a Tools/View menu item calling
  `toggle()`, and a `spring_endpoints::show(...)` call in the same place
  `quick_switcher::show`/`go_to_file::show` are called today. Its
  `Some((path, byte))` return is captured but not acted on yet — Phase 4's
  job.

**Checkpoint 3:** full suite green; live-verify in a real multi-controller
project: the shortcut/menu item opens the popup with every real endpoint
listed, typing narrows the list by path *and* by controller/method name,
arrow keys + Enter or a click selects and closes the popup (confirm
nothing crashes or navigates yet — that's expected, not a bug, until
Phase 4 lands), Escape dismisses without picking.

---

## Phase 4 — Jump-to-handler

`app.rs`, alongside `open_path` (`SPEC.md` §6) — the phase that makes
picking a row actually do something, and the one genuinely new piece of
cross-cutting infrastructure this feature needs (nothing in this app
today opens a file *at a position*; `go_to_file`/`quick_switcher` only
ever return a bare `PathBuf`).

- Read `text_area::shell.rs`'s `set_caret`/`peek_caret` and every current
  call site fresh before assuming a ready-made cross-tab-switch pattern
  exists — its own doc comment names "a generated getter/setter jumping
  to it" as precedent, but the actual only caller today is `widget.rs`'s
  own end-of-`show` `manual_caret` application, entirely within one
  already-focused frame. Confirm concretely whether `set_caret` can be
  called *ahead of* a widget's first `show` after a fresh tab-open (no
  extra frame of delay needed) or whether it needs to wait one frame,
  rather than assuming either shape.
- `pending_navigation: Option<(PathBuf, usize)>` (byte offset) on
  `FoxGardenApp`, alongside `pending_editor_input`/`cached_clipboard_
  text`. Phase 3's `Some((path, byte))` triggers `open_path` (as today)
  plus setting this field; once the target `Document` exists, convert the
  byte offset to a char offset via its buffer and call `text_area::
  set_caret(ctx, egui::Id::new(path.to_string_lossy()...), Caret::at
  (char_offset))` — the exact same `Id` computation `widget.rs`'s `show`/
  every existing test helper already use — then clear the field.
- **Resolve, don't assume:** does setting the shell's persisted caret
  alone scroll the new position into view, or does this need an explicit
  scroll call alongside it? Check live before calling this phase done —
  a cursor that jumps to the right byte while the viewport stays scrolled
  elsewhere is a half-working feature, not a finished one.
- Tests: a `pending_navigation` round-trip test mirroring `app/tests.rs`'s
  existing `FakeStorage`-based shape, confirming the byte-to-char
  conversion and that `set_caret` receives the right `Id`/`Caret` once
  the target document exists. The actual visible-scroll behavior is a
  live-verification item, not something the automated test proves either
  way.

**Checkpoint 4:** full suite green; live-verify picking an endpoint from
the popup switches to (or opens) the right file, lands the cursor on the
handler method's own name, and the viewport is actually scrolled to show
it — check once for a handler already in an open tab and once for a
handler in a file that isn't open yet, since those are genuinely
different code paths through `open_path`.

---

## Ordering notes

- **Phases 0 and 1 don't depend on each other** — either order, or
  genuinely parallel across two sessions, same as the completion
  feature's own Java/Kotlin sub-phases. Both are hard prerequisites for
  Phase 2, which needs `endpoints_in_file` to cover whatever languages a
  real project actually uses.
- **There is no safe-to-ship-partial point before Phase 4** — unlike the
  completion feature (where word-completion alone was a complete, useful
  feature), a popup that lists endpoints but can't jump to them is a
  demo, not a shipped feature. Don't report this feature as done at
  Phase 3.
- **Phase 2's scan is the most defensible pause point** if this doesn't
  land in one sitting — real, tested, and independently verifiable
  (`cargo test`) without needing any UI decision made yet.

---

## Build status (live)

- [ ] Phase 0 — Java endpoint extraction
- [ ] Phase 1 — Kotlin endpoint extraction
- [ ] Phase 2 — whole-project scan
- [ ] Phase 3 — popup UI shell
- [ ] Phase 4 — jump-to-handler
