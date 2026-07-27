# SPEC.md

Design spec for the **Spring endpoint map**: a searchable popup listing
every Spring MVC endpoint (`@GetMapping`/`@PostMapping`/`@RequestMapping`/…)
found across the currently open project's Java *and* Kotlin controllers,
jump-to-handler on pick. Fully replaces whatever this file covered before
(the previous code-completion pass — see git history/`TECHNICAL_DEBT.md`
if any of its items need to survive; nothing here continues that work).
Local-only planning doc (gitignored on `main`, tracked on `ide-henshin` for
the IDE pivot, same as `PLAN.md`) — a design record, not a commitment to
exact code.

`FEATURES.md` listed this feature as `[SKIP]`, "layered on top of Maven/
Gradle awareness (Major tier)" — that framing is deliberately dropped here.
Every cross-file feature this codebase already ships (Override Method,
dot-completion's cross-project lookup) works by walking the *currently
open project's file tree* and parsing whatever `.java`/`.kt` it finds,
with no real classpath or build-file awareness at all — a Maven multi-
module layout, a Gradle subproject, and a single flat folder all look
identical to that walk. This spec holds the endpoint map to the same
honest, already-proven-sufficient bar rather than waiting on Maven/Gradle
awareness (still unimplemented, still `[SKIP]`) to exist first.

---

## 0. Shape and scope

**Where:** a new `Ctrl+Shift+E`-triggered popup (verify that chord is
still free before wiring it — `menu_bar.rs`'s existing shortcuts are
`Ctrl+B/E/J/N/S`, `Ctrl+Shift+G/L/T/U`, `Ctrl+/`, none of which collide,
but re-check at Phase 3 rather than trusting this list to still be
current), modeled directly on `go_to_file.rs`'s existing shape: type to
fuzzy-filter, arrow keys + Enter or a click to pick, Escape to dismiss.
Deliberately *not* a persistent docked panel — this app has no panel-
docking infrastructure at all today (the side panel is the file tree,
full stop), and inventing one is a much bigger, riskier undertaking than
this feature needs. A searchable popup is how users already navigate a
large list here (`Ctrl+P` file search, `Ctrl+E` recent files); this is a
third instance of the same interaction, not a new one.

**Non-goals**, named up front so partial coverage reads as an honest
smaller feature rather than a broken bigger one (`SPEC.md`'s own
established convention — see §7 for the full list, mirrored on the
completion feature's own §6/§7):
- No real classpath/meta-annotation resolution — a class's own custom
  `@GetMapping`-named annotation (unrelated to Spring) would false-
  positive; simple-name matching only, same limitation `superclass_name`/
  `type_of_identifier_java` already accept for type names.
- No query-param/`consumes`/`produces` detail, no multi-value `method =
  {GET, POST}` (first value only, same "don't try to distinguish, just
  take the first one" scope limit `kotlin_superclass_name` already
  established for `delegation_specifiers`).
- No WebFlux functional routing (`RouterFunction`/`route { }`), no JAX-RS
  (`@GET`/`@Path`) — annotation-based Spring MVC only.
- No live re-scan on every keystroke — this is a whole-project walk, not
  a per-keystroke operation like completion; re-scanned when the popup
  opens (§4).

---

## 1. Data model

**Where:** new `crates/syntax/src/spring_endpoints.rs`.

```rust
/// One discovered Spring MVC endpoint — enough to render a popup row and
/// jump to its handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    /// `"GET"`/`"POST"`/`"PUT"`/`"DELETE"`/`"PATCH"`, or `"ANY"` for a bare
    /// `@RequestMapping` with no `method =` element (Spring's own default:
    /// matches every HTTP method).
    pub http_method: String,
    /// Class-level base path + method-level path, joined (§2's rule) —
    /// e.g. `"/api/users/{id}"`. Never empty; a mapping with no path
    /// anywhere resolves to `"/"`.
    pub path: String,
    pub controller_name: String,
    pub handler_name: String,
    /// Byte offset of the handler method/function's own name — where a
    /// jump should land the cursor (§6), not the annotation or the
    /// enclosing method's start.
    pub handler_byte: usize,
}
```

Reused as-is by both languages, same "one output struct, two producers"
shape `FieldInfo`/`MethodSignature` already established for Java/Kotlin
member extraction — no separate Java/Kotlin variant needed.

---

## 2. Extraction — Java

**Where:** `spring_endpoints.rs`, `java_endpoints_in_file(tree, source) ->
Vec<EndpointInfo>`.

**Grammar shapes** (verified fresh against `tree-sitter-java-0.23.5`'s
actual parse output, not assumed — same discipline
`TECHNICAL_DEBT.md` #3 established for Kotlin, applied here to Java too
since this is new grammar territory this codebase hasn't walked before):
a `class_declaration`'s (and, per the existing `fields.rs`/`methods.rs`
precedent of not missing nested classes, any nested `class_declaration`'s)
`modifiers` child holds zero or more `annotation`/`marker_annotation`
nodes alongside the `public`/etc. keyword tokens — both shapes appear as
direct children of `modifiers`, not wrapped further. Likewise a
`method_declaration`'s own `modifiers` child. An `annotation`'s `name`
field is the simple identifier (`"GetMapping"`) — ignore the rarer
`scoped_identifier` case (a fully-qualified `@org.springframework....
GetMapping`), same "match the simple name, not the qualified path"
limitation §0 already accepts. Its `arguments` field
(`annotation_argument_list`) holds either one bare value (a `string_literal`
for `@RequestMapping("/x")`) or `element_value_pair`s (`key = value`, e.g.
`value = "/x"`, `method = RequestMethod.GET`). A `string_literal`'s actual
text lives in its `string_fragment` child, not its own span (which
includes the quotes).

**Recognized annotations** and what each contributes:
- `@GetMapping`/`@PostMapping`/`@PutMapping`/`@DeleteMapping`/
  `@PatchMapping` — `http_method` fixed by name (`GET`/`POST`/…); `path`
  from the bare value or a `value =`/`path =` pair (Spring accepts either
  key as a synonym), or `""` if the annotation is a bare
  `marker_annotation` (no `()` at all).
- `@RequestMapping` — `path` the same way; `http_method` from a `method =`
  pair's value (a `field_access` — `RequestMethod` `.` `GET`; take the
  identifier after the dot) if present, else `"ANY"`.
- Anything else on the same `modifiers` node (`@Override`,
  `@Transactional`, …) is ignored — not an error, just not one of the
  recognized names.

**Path joining:** class-level base path (from the *class's own*
`@RequestMapping`, if any — `@RestController`/`@Controller` alone
contribute no path) plus the method-level path: strip a trailing `/` off
the base, ensure a leading `/` on the method path (add one if it's
non-empty and missing), concatenate; an empty result (both sides empty)
is `"/"`.

**A method only becomes an `EndpointInfo` if it carries one of the five
recognized method-level annotations** — independent of whether the
enclosing class carries `@Controller`/`@RestController` at all. Simpler,
and avoids missing an endpoint whose class annotation was written via a
custom composed/meta-annotation this syntactic pass can't see through
anyway (§0).

**Tests:** table tests per case above — positional-string path, `value =`,
`path =`, `method =` combined with a class-level base path, a bare marker
annotation with no args, a class with no base path at all, a nested class,
multiple unrelated annotations on one method (only the recognized one
counts), and a class/method with no recognized annotation at all
(contributes nothing, not an error).

---

## 3. Extraction — Kotlin

**Where:** `spring_endpoints.rs`, `kotlin_endpoints_in_file(tree, source)
-> Vec<EndpointInfo>`.

**Grammar shapes** (verified fresh against `tree-sitter-kotlin-ng` 1.1.0's
actual parse output — mandatory per `TECHNICAL_DEBT.md` #3's own
established discipline, re-verify rather than trusting this spec):
Kotlin's `class_body`'s `modifiers` child wraps an `annotation` node whose
own child is a `constructor_invocation` (`@GetMapping("/x")`) or a bare
`user_type` (a marker annotation with no args — verify this bare shape
directly too, the same "don't assume, check" rule, since it wasn't
exercised in this pass's own grammar dump). Either way the annotation's
*name* is the `user_type`'s own `identifier` child. A `constructor_
invocation`'s `value_arguments` holds `value_argument` nodes: a bare one
(`string_literal`, positional) or a named one (`identifier "=" value`).
Array-valued elements (Kotlin's own `method = [RequestMethod.DELETE]`,
`path = ["/x"]` — Kotlin requires an array literal where Java accepts a
bare value for a single-element case) wrap a `collection_literal`; take
its first element, same one-value scope limit §2 already accepts for
Java's own `method =`. A `string_literal`'s text lives in its
`string_content` child (Kotlin's own name for what Java calls
`string_fragment` — different node kind, same role, verify before
assuming the name transfers). A `navigation_expression` (Kotlin's
`RequestMethod.DELETE`) is Java's `field_access` equivalent — same "take
the identifier after the dot" extraction.

**Everything else — recognized annotation list, path-joining rule,
"method-level annotation is what makes it an endpoint" rule, ignoring
other annotations — is identical to §2**, just walking `class_body`'s
`function_declaration` children (mirroring `kotlin_members.rs`'s own
`class_body` traversal) instead of Java's `class_body`'s
`method_declaration` children.

**Tests:** the same table as §2, translated to Kotlin syntax, plus the
Kotlin-specific array-literal-unwrapping case for `method =`/`path =`
explicitly.

**Shared entry point:** `pub fn endpoints_in_file(language: fg_core::
Language, tree: &Tree, source: &str) -> Vec<EndpointInfo>` dispatches to
whichever of the two functions above matches `language`, empty `Vec` for
every other language — the one function callers outside this module
actually call, mirroring `type_of_identifier`'s own dispatcher shape.

---

## 4. Whole-project scan

**Where:** new `crates/app/src/widgets/editor/codegen.rs` function (or a
sibling module, if `codegen.rs` is already large enough that adding this
would blur its own "getter/setter/constructor/toString generation, plus
the shared file-finder" focus — check its current size before deciding),
`pub fn scan_project_endpoints(root: &FileNode) -> Vec<(PathBuf,
EndpointInfo)>` (the file path travels alongside each entry — §6's jump
needs to know which file to open, and `EndpointInfo` itself has no
per-file identity of its own).

Walks every `.java`/`.kt` `FileNode` in the tree (same recursive-walk
shape `find_source_file_by_stem`/`all_files` in `go_to_file.rs` already
use), reads + throwaway-parses each with a fresh `IncrementalParser` (the
same "read + throwaway-parse" sequence Override Method/dot-completion's
cross-project lookup already do for a *single* file, just looped over
every file here instead of one resolved-by-name target), and calls
`endpoints_in_file` on each. A file that fails to read (permission error,
race with a delete — `TECHNICAL_DEBT.md` #11's own open concern) is
skipped silently, same "don't fail the whole operation over one bad
entry" reasoning that entry already argues for, not a new decision this
spec invents.

**Cost:** a whole-project walk + parse of every source file — not free,
but only run when the popup opens (§0's own scope decision), not on a
timer or on every keystroke, so it doesn't compete with the actual typing/
completion hot path `AGENTS.md`'s performance principle is concerned with.
No caching across popup opens in this first pass — simpler, and correct
by construction (nothing can go stale between a project's file changing
and the next time the popup is opened, since it always re-scans fresh).
Revisit only if a real large-project open feels slow *and is actually
measured*, per `AGENTS.md`'s own "measure before fixing" testing
convention — not preemptively.

**Tests:** a small two/three-file project fixture (mirroring dot-
completion's own cross-project test shape) asserting the aggregated list
contains exactly the expected `EndpointInfo`s with the right paths; a file
that isn't `.java`/`.kt` is ignored; an empty project returns `[]`.

---

## 5. UI — the popup

**Where:** new `crates/app/src/panels/spring_endpoints.rs`, structurally a
near-twin of `go_to_file.rs`: `SpringEndpointsState { open: bool, query:
String, selected: usize }` with the same `toggle()`; `show(ui, state:
&EditorState, popup: &mut SpringEndpointsState) -> Option<(PathBuf,
usize)>` (path + the picked entry's `handler_byte`, for §6 to act on).

Re-scans via §4's `scan_project_endpoints` once per `toggle()` into
`open` (not on every frame the popup is shown — same "recomputed on
open" reasoning `CompletionState::open` already uses for its own
candidate list), then filters/ranks rows against `query` using
`go_to_file.rs`'s own `fuzzy_score` (reused directly if it's `pub(crate)`-
reachable from a sibling `panels` module already, or promoted from
private to `pub(crate)` if not — a one-line visibility change, not a
duplicate implementation) matched against a row's rendered text (`"GET
/api/users/{id} — UserController#getUser"`), so typing either a path
fragment or a method/controller name narrows the list.

**Tests:** since this mirrors `go_to_file.rs`, which itself has no direct
render-level test (its own popup interaction is exercised live, not
headlessly, going by this crate's existing test coverage), match that
same split rather than inventing a new testing shape for this one popup:
`scan_project_endpoints`/`endpoints_in_file` get real unit tests (§2-§4);
the popup's own open/filter/pick wiring is verified live, per `AGENTS.md`'s
testing-conventions section on GUI click-through (`cargo build`/`test`/
`clippy` green, then hand the user exact numbered steps and wait for them
to report back — not a click-automation tool, which that same section
documents as unreliable here).

---

## 6. Jump-to-handler

**Where:** `app.rs`, alongside `open_path`.

**The real gap this phase closes:** nothing in this app today opens a
file *at a specific position* — `go_to_file`/`quick_switcher` both return
a bare `PathBuf`, and `open_path` just opens/focuses the tab. `text_area::
set_caret(ctx, id, caret)` already exists and is exactly the primitive
needed (its own doc comment even names "a generated getter/setter jumping
to it" as a precedent use, though checking that precedent's real call
site shows the *only* current caller is `widget.rs`'s own end-of-`show`
`manual_caret` application, all within one already-focused frame — not
actually a cross-tab-switch jump yet, despite what the comment implies;
verify this before assuming a ready-made cross-tab pattern exists) — the
new work is wiring it across a tab switch, which nothing does yet.

**Design:** a new `pending_navigation: Option<(PathBuf, usize)>` (byte
offset) field on `FoxGardenApp`, alongside `pending_editor_input`/
`cached_clipboard_text`. When §5's popup returns `Some((path, byte))`:
call `open_path` as today, then set `pending_navigation`. On the *next*
frame (or the same frame, if `set_caret` genuinely can be called ahead of
a widget's first `show` the way its doc comment claims — **verify this
directly** rather than assuming an extra frame of delay is needed, same
"check, don't guess" discipline this whole spec already applies to every
grammar claim): convert the byte offset to a char offset via the now-
open `Document`'s buffer, call `text_area::set_caret(ctx, egui::Id::new
(path.to_string_lossy()...), Caret::at(char_offset))` (the exact same id
computation `widget.rs`'s own `show` and every test helper already use),
then clear `pending_navigation`.

**Open question, to resolve in this phase, not before:** does setting the
shell's persisted caret alone cause the surrounding `egui::ScrollArea`
(if the editor is scrollable — check `text_area.rs`/`painting.rs` for
how/whether one wraps the text widget) to actually scroll the new
position into view, or does this need an explicit `ui.scroll_to_rect`/
equivalent call alongside it? A jump that moves the cursor to the right
byte but leaves the viewport scrolled somewhere else entirely would be a
half-working feature — check the real behavior live before calling this
phase done, not just via the automated test suite.

**Tests:** a `pending_navigation` round-trip test (mirroring
`app/tests.rs`'s existing `FakeStorage`-based shape) confirming the byte-
to-char conversion and that `set_caret` is called with the right `Id`/
`Caret` once the target document exists; the actual visible-scroll
behavior is a live-verification item per the open question above.

---

## 7. Known gaps — not full parity with a real IDE's endpoint explorer

Worth naming explicitly rather than letting "endpoint map shipped" imply
more than it does, same convention the completion feature's own §6/§7
established:

- **No re-scan on file change** — if a project's controllers change while
  the popup stays closed, the *next* open re-scans fresh (§4), but nothing
  watches for changes proactively. Consistent with this feature never
  running on a timer at all (§0's own scope decision), not a regression
  from some richer behavior this pass almost had.
- **Simple-name annotation matching, no real classpath** — a project's own
  unrelated `@GetMapping` (not Spring's) would false-positive; Spring's
  own annotation resolved via a different import wouldn't be
  distinguishable from it either. Same category of gap `type_of_
  identifier_java`'s JDK-type handling already accepts.
- **First-match-only for multi-value `method =`/path variables/query
  params/`consumes`/`produces`** — not shown at all; only the bare path
  and (single) HTTP method.
- **No WebFlux, no JAX-RS** — annotation-based Spring MVC only, per §0.
- **No multi-module path-prefix awareness** — a Gradle subproject's own
  servlet context-path or a reverse-proxy prefix isn't known to this tool
  at all (needs real build-file awareness, still `[SKIP]`); paths shown
  are exactly what the `@...Mapping` annotations say, nothing more.

---

# 8. Terminal window tabs

A second, independent feature added to this same design pass — an
in-app, PTY-backed terminal that opens as its **own tab in the existing
tab strip**, alongside file tabs, rather than a docked panel or a modal.
Unrelated to §0-§7 above (no shared code, no shared dependency in either
direction); `PLAN.md`'s phases for it are a separate track for the same
reason. Reuses the tab bar UI users already understand instead of
inventing a new one — the same "reuse an existing interaction rather than
invent a new one" call §0 already made for the endpoint map's own popup.

Genuinely bigger and riskier than the endpoint map: real process
management, a byte-level terminal protocol, and a restructuring of a tab
model that's been file-only (`Vec<Document>`) since this app's first
checkpoint. `FEATURES.md` previously listed "Integrated terminal panel"
as Substantial-tier, below the Major-tier undertakings — treat that as
optimistic once the tab-model rework (§8.2) and full keyboard-protocol
translation (§8.5) are actually accounted for; this is closer to Major
tier in practice, just without a second external process/protocol server
the way LSP integration would need.

**Non-goals:** no terminal multiplexing (splits/panes within the tab —
one shell process per tab, full stop); no terminal-specific color
scheme or font distinct from the editor's own settings; no session
persistence across restarts (a dead shell process has no scrollback/state
worth resuming — `restore_session`/`persist_session` keep covering file
tabs only, unchanged); no SSH/remote shell — a local process only, via
whatever shell is installed.

## 8.1 Dependencies

- **`portable-pty`** — spawns a shell with a real pseudo-terminal. A
  plain pipe isn't enough: line editing, job control, and a colored
  prompt all expect a *real* pty (the shell checks `isatty()` and behaves
  differently otherwise), not just captured stdout/stdin.
- **`vt100`** — parses the raw byte stream into a `Screen` (a grid of
  cells, each with its own foreground/background/bold/etc.), which
  FoxGarden then paints itself via egui, rather than embedding a full
  terminal emulator's own rendering (e.g. `alacritty_terminal`, much
  heavier). Matches `AGENTS.md`'s "lightweight AND functional" design
  principle: hand-rolling VT100 *parsing* from scratch is not worth
  doing (the real escape-sequence surface a shell/editor/`htop` actually
  emits is large), but *rendering* an already-parsed grid is squarely the
  same kind of custom-painting work this app already does for the
  editor's own text/squiggles/brackets — no reason to pull in a second
  renderer for that half.

## 8.2 Tab model — the real architectural fork this feature needs

`EditorState.open_tabs: Vec<Document>` / `active_tab: Option<usize>` is
used everywhere in this app as if every tab is a file — `app.rs`'s own
`parsers: Vec<Option<IncrementalParser>>` is kept *index-aligned* with
`open_tabs` (an already-documented `AGENTS.md` gotcha), and every
tab-bar/save/close/reopen code path indexes `open_tabs` directly. A
terminal tab isn't a `Document` at all: no path, nothing to save, no
diagnostics.

Proposed shape (resolve the exact fields with the real code in hand
during Phase 5, not locked in here — same "verify against the actual
current shape before writing the two call sites" discipline the
completion feature's own Phase 3b already modeled for a comparable real
design point):
- Leave `open_tabs`/`parsers` exactly as they are today — file tabs only,
  no change to their own type or indexing.
- Add `terminal_tabs: Vec<TerminalTab>` (new struct — the pty child
  handle, its writer half, the shared `vt100::Parser`/`Screen`, a
  display title) alongside `open_tabs` on `EditorState`.
- Add one ordered `Vec<TabKind>` (`TabKind::File(usize)` /
  `TabKind::Terminal(usize)`) that *is* the tab order the strip renders
  left-to-right, replacing `open_tabs`'s own implicit ordering as the
  single source of truth for tab position — a file tab and a terminal tab
  can end up interleaved in whatever order the user created them, and the
  strip needs one list to walk, not two.
- `active_tab` becomes "which `TabKind`," not a bare file index — every
  existing call site that reads `state.active_tab` needs auditing, not
  assumed unaffected.
- Closing/reordering/middle-click-close reuse whatever mechanism file
  tabs already have (`EditorState::close_tab`'s push-onto-`closed_tabs`
  shape), dispatched by `TabKind` instead of assumed to always be a
  `Document` — a closed terminal tab does *not* get pushed onto
  `closed_tabs`/get a `Ctrl+Shift+T` reopen (§8's own non-goal: nothing
  to meaningfully reopen).

## 8.3 Spawning and lifecycle

- **"New Terminal Tab"** — a new toolbar button/menu item/shortcut,
  additive alongside the existing external "Open Terminal" button
  (`terminal.rs`), not a replacement. Some users want their own terminal
  emulator's own ergonomics (copy/paste, tmux, saved profiles); this is a
  second, complementary option.
- Spawns the user's default shell via `portable-pty`: `$SHELL` on Unix
  (falling back to `/bin/sh` if unset), `%COMSPEC%` on Windows (falling
  back to `cmd.exe`) — verify the exact fallback `terminal.rs`'s own
  external-terminal spawn already uses for "no project open" before
  assuming this feature should match it, rather than inventing a second,
  possibly-inconsistent default.
- A background thread does a blocking read of the pty's output
  continuously (fine off the UI thread) and feeds bytes into the
  `vt100::Parser` behind a `Mutex`/channel the UI thread drains once per
  frame; calls `egui::Context::request_repaint()` whenever new output
  arrives, since a long-running command's own output (unlike the user's
  own typing) has no other event to trigger a repaint on.
- Closing a terminal tab kills its child process — no "still running,
  are you sure" confirmation for a first pass (a dirty *file* tab's
  close-confirmation modal doesn't translate; a terminal has no "unsaved"
  concept). Matches most terminal-tab UIs' own default.

## 8.4 Rendering

New `crates/app/src/widgets/terminal.rs`. Reads the current `vt100::
Screen`'s cells once per frame — only while the tab is actually the
active one (same "don't do per-frame work for a tab that isn't shown"
discipline the editor's own tab rendering already follows) — and paints
each cell as a monospace glyph using the *editor's own* configured font/
size (`EditorFont`, `font_size`) rather than a second, separate terminal
font setting. `vt100`'s own cell attributes (foreground/background/
bold/etc.) map onto the current theme's own color table (`theme.rs`),
not the raw ANSI 16-color palette verbatim — a terminal tab that ignores
light/dark mode would visually clash with the rest of the app. Cursor
renders as a blinking block/bar; reuse the editor's own blink-timing
logic if it's factored generically enough to share rather than a second
timer.

## 8.5 Input

While a terminal tab is focused, keyboard `Event`s translate to the byte
sequences a real terminal sends and get written to the pty's writer
half: printable characters as UTF-8 bytes, Enter → `\r`, Backspace/Tab
→ their own single control bytes, arrow keys/Home/End/Page Up/Down/
function keys → their standard ANSI escape sequences, Ctrl+letter → the
matching control byte (Ctrl+C → `0x03`, Ctrl+D → `0x04`, …). This is a
real, nontrivial translation table — a full terminal keyboard protocol,
not a one-line `match` — budget a dedicated phase for it and table-test
the byte sequences for at least arrows/Enter/Tab/Backspace/Ctrl+C/Ctrl+D
against known-correct values, not asserted correct by inspection alone.
Paste writes the clipboard text's raw bytes the same way, no different
handling from typing it.

## 8.6 Resizing

On a size change (font-size setting, side-panel width drag, window
resize), compute the new rows/cols from the available rect and the
font's own glyph metrics (the same measurement the editor's own layout
already needs — reuse it rather than re-deriving) and call the pty's
`resize()`, so the shell's own `$LINES`/`$COLUMNS` — and any full-screen
program running inside it (`vim`, `htop`, `less`) — redraw correctly.
Getting this wrong doesn't crash anything, but a TUI program in an
unresized pty renders garbled; treat a resize as a real, testable event
(construct a known rect, assert the computed rows/cols), not an
afterthought discovered only by eyeballing a live resize.

## 8.7 Known gaps

- No multiplexing, no terminal-specific theming, no session persistence,
  no remote/SSH shells — §8's own stated non-goals, repeated here for the
  same "don't let partial scope read as a bug" reason §7 gives for the
  endpoint map.
- **Windows support is the one real platform risk** — `portable-pty`'s
  ConPTY backend is a different code path from its Unix pty
  implementation; verify it actually works end-to-end in this app's own
  build during Phase 6/7 rather than assuming parity with the Unix path,
  mirroring `terminal.rs`'s own existing per-OS branching for the
  external-terminal case (proof that this app already can't assume one
  code path covers every platform for anything terminal-adjacent).
- No attempt at terminal-inside-terminal edge cases (a shell running
  another shell, `screen`/`tmux` launched from inside this tab) beyond
  whatever `vt100` itself already handles — not a scenario this feature
  tests for deliberately.
