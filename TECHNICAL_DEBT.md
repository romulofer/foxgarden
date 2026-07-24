# TECHNICAL_DEBT.md

Known-and-deferred issues in this codebase — things a `/simplify`-style
review found and a human or agent explicitly chose *not* to fix on the
spot, with the reasoning preserved. This file is git-tracked (unlike
`FEATURES.md`/`PLAN.md`/`SPEC.md`, which are local-only) because deferred
debt is exactly the kind of context that's expensive to reconstruct and
cheap to lose.

**Format note for whoever (human or agent) picks one of these up:** each
entry gives the current code shape, the proposed fix with a code sketch,
why it wasn't done immediately, and a concrete trigger condition for when
it becomes worth doing. Line numbers are a snapshot and will drift — the
function names and file paths are the stable anchor. Verify the "current
shape" section still matches reality before trusting the rest of the entry;
if it doesn't, the entry is stale and should be rewritten or removed, not
blindly executed.

Entry numbers are stable IDs assigned in discovery order, not a priority
ranking or a sequential count within a section — a cross-reference like
"see #2" always means the same entry regardless of which section (Open or
Resolved) it currently lives in.

---

# Open

## 3. Kotlin's reference `highlights.scm` targets a different grammar than the one vendored here

**Where:** `crates/syntax/queries/highlights_kotlin.scm` vs.
`../references/kotlin/languages/kotlin/highlights.scm`.

**Status:** Partially addressed. The whole-file incompatibility below is
still real — a direct line-by-line port remains off the table — but one
concrete construct (enum-entry-as-constant) has now been ported by
following this entry's own proposed methodology, now that `Scope::Constant`
exists (added alongside TECHNICAL_DEBT.md #2) to route it into. Recorded
below as a worked example for whichever construct gets picked up next.

### What was found

`crates/syntax/Cargo.toml` pins `tree-sitter-kotlin-ng = "1.1.0"`. Zed's own
Kotlin extension (`../references/kotlin/extension.toml`) pins a grammar from
`https://github.com/fwcd/tree-sitter-kotlin` instead — a different grammar
project. Checked directly against both grammars' `node-types.json`: the
fwcd grammar (which Zed's `highlights.scm` is written against) has
`simple_identifier`, `type_identifier`, and `navigation_suffix` node types
that `tree-sitter-kotlin-ng` simply doesn't define — it uses a single
`identifier` node type for everything the fwcd grammar splits across
`identifier`/`simple_identifier`. A query built against Zed's file would
fail to compile (`Query::new` errors on any node type name the loaded
grammar doesn't define) rather than silently under-match, so this isn't a
subtle bug — it's a hard incompatibility.

This is exactly the situation `highlights_kotlin.scm`'s own header comment
already warns about ("check node-types.json ... instead of trusting the
source") for individual keyword tokens; the same caution applies at the
whole-file level to this specific reference.

By contrast, `../references/java`'s grammar pin
(`tree-sitter/tree-sitter-java`) is byte-for-byte the same crate
`highlights_java.scm` is built against, which is why that file *was* a
valid diff target (see the fixes made alongside this entry: record/
annotation-type declaration names, `"@interface"`, `binary_integer_literal`).

### Proposed fix

Not a mechanical port. Any future Kotlin highlighting improvement needs to:
1. Identify the *construct* worth adding from Zed's file (e.g. richer
   modifier-keyword coverage, enum-entry-as-constant, regex-literal
   detection) independent of its exact node names.
2. Look up the equivalent node shape in `tree-sitter-kotlin-ng`'s own
   `node-types.json` (bundled in the crate at
   `~/.cargo/registry/src/.../tree-sitter-kotlin-ng-1.1.0/src/node-types.json`)
   — don't assume node names carry over.
3. Verify the resulting query compiles against this project's actual
   grammar version (`cargo test -p syntax`) before trusting it, same as the
   existing header comment's bisection approach for keyword tokens.

### What was done

Ported enum-entry-as-constant, the first item on this entry's own example
list. Zed's file captures it as `(enum_entry (simple_identifier)
@constant)`; per `tree-sitter-kotlin-ng`'s `node-types.json`, `enum_entry`
has no `simple_identifier` at all — its name child is a plain `identifier`
(the same collapsing this entry already documented for `identifier` vs.
`simple_identifier` generally). `enum_entry`'s only other possible direct
children are `modifiers`, `value_arguments`, and `class_body` — all
distinct node types — so `(enum_entry (identifier) @constant)` unambiguously
matches just the entry's own name, not an identifier buried inside a
constructor-argument list. Added to `highlights_kotlin.scm`, with
`Scope::Constant` (added for TECHNICAL_DEBT.md #2) as where it now renders.
Verified via `cargo test -p syntax`: an `enum class Level { LOW, MEDIUM,
HIGH }` fixture in `valid.kt` plus `has_scope_over("LOW"/"MEDIUM"/"HIGH",
Scope::Constant)` assertions in `kotlin_highlight_query_compiles_and_covers_expected_ranges`.

Remaining candidates from Zed's file (richer modifier-keyword coverage,
regex-literal detection, `@variable.builtin` for `it`/`field`) are each
still their own future pass, following the same three-step process.

### Trigger condition

Next time Kotlin highlighting is revisited — this entry just saves that
future pass from re-discovering the grammar mismatch from scratch, and now
also has one worked example of the fix process to follow.

---

## 5. Considered and rejected: splitting `widget.rs` further

**Where:** `crates/app/src/widgets/editor/widget.rs` (~2200 lines, 2214 as
of this recheck).

**Status:** Not actual debt — recorded so a future review doesn't re-flag
file size alone and fragment a file that's already been evaluated for
exactly that. Re-checked while working through this file's other entries
(#1-#4, three of which touched this exact file): still holds. Those
changes only threaded one more plumbing parameter
(`cached_clipboard_text`) through `show()` and its call sites — no new
self-contained feature landed inside `show()` the way getters/setters
generation once did, so the "Why it doesn't apply" reasoning below is
unchanged and no split was made.

### What was flagged

A code-organization review this session already extracted two genuinely
separable concerns out of this file — the right-click context menu (now
`context_menu.rs`) and the getters/setters picker's rendering (moved into
`codegen.rs`, next to the data/logic it already owned) — after `widget.rs`
had grown to 2397 lines doing several distinct things. Even after that
split the file is still large (dominated by the `show()` function and its
colocated test module) and could get flagged again for size alone.

### Why it doesn't apply

What's left in `show()` is genuinely one thing: orchestrating a single
frame of the `TextEdit` widget. Its interception blocks (Tab, Alt+Arrow,
Home, `Ctrl+/`, wrap-selection, multi-cursor, the generate-request
dispatch, the context-menu call, case conversion) all share the same
`text`/`old_text`/`manual_cursor_range` locals, threaded sequentially
through the function specifically because `TextEditState::store` can only
be called once per frame (see that gotcha in `AGENTS.md`). Splitting
further would mean either passing that whole bundle of state across a new
function boundary for every remaining block (worse than the length it
"fixes"), or bundling it into a struct — a bigger, riskier change than the
size problem it would solve. `AGENTS.md`'s own architecture principle
already covers this: a file earns a split once it's doing more than one
identifiable thing, and this one, post-split, is back down to exactly one.

### Proposed fix

None right now. If a *specific* self-contained block within `show()` grows
into its own feature the way getters/setters generation did — not just
"the file is long" — extract that block specifically, the same way
`codegen.rs`/`context_menu.rs` were.

### Trigger condition

A new feature that's genuinely separable (its own state, its own
rendering, minimal interaction with the shared `text`/`old_text`/
`manual_cursor_range` threading) lands inside `show()` and grows past a
few dozen lines — extract that feature, not the file in general.

---

## 6. `widget.rs`'s `open_fixture` test helper wraps `test_support::temp_document` instead of being replaced by it directly

**Where:** `crates/app/src/widgets/editor/widget.rs`'s test module, `fn
open_fixture`.

**Status:** Not real debt — recorded so a future cleanup pass doesn't
"simplify" this into ~50 error-prone call-site edits for no real benefit.
Re-checked alongside #5: `temp_document`'s argument order is unchanged, and
today's ~23 mechanical edits to this same test module (appending one
plumbing argument to each `show(...)` call, for #4) never touched
`open_fixture` call sites or required reading each one's filename/content
argument order, so it doesn't count as the "already touching most of this
test module" trigger this entry calls for. Still not worth doing on its
own.

### What was found

`crates/test-support`'s `temp_document(name, contents)` takes its
arguments in the opposite order from this file's own
`open_fixture(contents, filename)`, which is called roughly 50 times
throughout this module's tests. `open_fixture` now just delegates to
`temp_document` with the arguments swapped, rather than being deleted
outright with every call site updated to match the shared crate's order.

### Why it doesn't apply

This was a deliberate choice, not an oversight: mechanically swapping two
string-literal arguments across ~50 call sites by hand (or via a
regex that has to distinguish which of two `&str` arguments is which at
each site) is exactly the kind of change likely to silently swap a *test's
own* filename and content at one or two sites without anyone noticing,
since both are usually plausible-looking string literals. The one-line
wrapper already achieves the actual goal — the file-writing logic isn't
duplicated anymore — without that risk.

### Proposed fix

If `test_support::temp_document`'s argument order ever changes for an
unrelated reason, or this test module gets touched heavily enough that a
careful pass through all ~50 sites is already happening anyway, fold
`open_fixture` away and call `test_support::temp_document` directly at
each site under that same pass.

### Trigger condition

Only opportunistically, alongside other work that already touches most of
this test module — not worth a dedicated pass on its own.

---

# Resolved

## 1. ~~Considered and rejected: moving `display_path` computation into the `Err` arm~~ — Resolved

**Where:** `crates/core/src/document.rs` (`OpenDocumentError`, `Document::open`)
and `crates/app/src/app.rs` (`open_path`).

**Status:** Fixed. The originally-flagged shape genuinely didn't compile as
suggested (see below for why), but the underlying goal — not paying for a
path-display allocation on the success path — was still achievable by
changing what the error type carries, so it's been done that way instead.

### What was flagged

An efficiency-review agent noted that `let display_path =
path.display().to_string();` ran on every file-open attempt (success or
failure) in `open_path`, but was only used in the `Err` arm, and suggested
moving it inside that arm so the successful-open path skips the allocation.

### Why the original suggestion didn't apply

`path: PathBuf` was moved into `state.open_tab(path)` — the `match`
scrutinee itself — before either arm's body ran. By the time the `Err` arm
executed, `path` had already been consumed by the `open_tab` call; it
wasn't available to compute `display_path` from inside that arm at all.
Moving the computation there without also cloning `path` first (which just
relocates the same cost rather than avoiding it, since `PathBuf::clone`
and `.display().to_string()` are both real allocations) wouldn't have
compiled as suggested.

### What was actually done

`OpenDocumentError::Io` didn't carry a `PathBuf` (only `Binary` did), which
is *why* `open_path` had to pre-compute `display_path` before the match in
the first place — it was the only place both arms had `path` in scope.
Giving `Io` its own `PathBuf` (`Io(PathBuf, std::io::Error)`) removes that
constraint: `Document::open` already owns `path` at both fallible call
sites (`looks_binary(&path)?` and `read_to_string(&path)?` only *borrow*
it), so it can move `path` into the error on failure with no clone. That
makes `path.display().to_string()` computable straight from `err` inside
`open_path`'s `Err` arm, and the pre-match allocation is gone.

The cost being eliminated was genuinely trivial (one `.display().to_string()`
per file-open click, hardly ever on a hot path) — this was worth doing
because the fix was free and mechanical (a small, self-contained type
change with no clone introduced anywhere), not because the perf delta
matters.

---

## 2. ~~`highlights_java.scm`'s `@constant` capture is dead — no `Scope` renders it~~ — Resolved

**Where:** `crates/syntax/queries/highlights_java.scm:94-95`, and
`crates/syntax/src/highlight.rs:25-41` (`fn scope_for_capture`).

**Status:** Fixed. Found while diffing `highlights_java.scm` against
`../references/java` (Zed's Java extension, same `tree-sitter-java` grammar)
for syntax-highlighting improvements.

### What was done

Added a dedicated `Scope::Constant` (per the "proposed fix" below — none of
the existing seven scopes fit well enough to share, `Property` in
particular reads as a YAML/properties-key concept, not a Java
`static final` one) touching:
- `crates/syntax/src/highlight.rs`: the `Scope` enum and `scope_for_capture`
  (`"constant"`-prefixed captures now map to it)
- `crates/app/src/style/theme.rs`: a color for `Scope::Constant` in both the
  dark and light tables (`d19a66`/`c18401`, the Atom One Dark/Light
  constant-and-number color in each palette)
- `crates/syntax/tests/fixtures/valid.java`: added a `MAX_LENGTH` constant
  field
- `crates/syntax/tests/syntax_tests.rs`: a `has_scope_over("MAX_LENGTH",
  Scope::Constant)` regression assertion in
  `highlight_spans_cover_expected_keyword_string_comment_ranges`, as this
  entry's own "proposed fix" section called for

### Current shape

```scheme
((identifier) @constant
 (#match? @constant "^_*[A-Z][A-Z\\d_]+$"))
```

This pattern exists specifically to color ALL-CAPS identifiers (the
idiomatic Java convention for `static final` constants and enum constants)
— but `scope_for_capture` only recognizes `keyword`/`string`/`comment`/
`type`/`function`/`property`/`tag`-prefixed capture names:

```rust
fn scope_for_capture(name: &str) -> Option<Scope> {
    if name.starts_with("keyword") { ... }
    else if name.starts_with("string") { ... }
    else if name.starts_with("comment") { ... }
    else if name.starts_with("type") { ... }
    else if name.starts_with("function") { ... }
    else if name.starts_with("property") { ... }
    else if name.starts_with("tag") { ... }
    else { None }
}
```

`"constant"` matches none of those prefixes, so every `@constant` capture
resolves to `None` and the token renders as plain, uncolored text —
identical to having no rule at all. This is the same category of bug the
file's own header comment says it already fixed for `@attribute`/
`@variable.builtin`/`@constant.builtin` (bundled-query captures with no
matching `Scope`) — this one capture was missed, and it's homegrown, not
inherited from the bundled query, so it's not covered by that header's
"forked to fix three scopes" framing at all.

### Why this wasn't fixed on the spot

Unlike the `record_declaration`/`annotation_type_declaration`/`"@interface"`
fixes made alongside this entry (which route cleanly into the existing
`Type`/`Keyword` scopes, matching an established in-file pattern), there's
no obviously-correct existing `Scope` for "a constant." Guessing one (e.g.
reusing `Type` again, or `Property`) risks visually conflating constants
with an unrelated token category rather than fixing the bug — this needs a
deliberate call, not a drive-by.

### Proposed fix

Pick one:
- Route `@constant` into an existing `Scope` if one reads as a reasonable
  visual fit once seen rendered (candidates: `Property`, on the theory that
  a constant is "a named value," similar in kind to a config key).
- Add a dedicated `Scope::Constant` (touches the enum, both `theme.rs`
  color-table arms, and `scope_for_capture`) if none of the existing seven
  scopes fit well enough to share.

Either way, add the regression coverage this bug's absence shows was
missing: a `has_scope_over("SOME_CONSTANT", Scope::X)` assertion in
`syntax_tests.rs`, mirroring the existing `this`/`true` regression tests in
`highlight_spans_cover_expected_keyword_string_comment_ranges`.

### Trigger condition

Next time `highlight.rs`'s `Scope` enum or `highlights_java.scm` is touched
for any reason — low-risk, self-contained, and the fixture/test scaffolding
for adding a `Scope`-mapped constant is already in place.

---

## 4. ~~Context menu's Paste item creates a new OS clipboard connection every frame the menu is open~~ — Resolved

**Where:** `crates/app/src/widgets/editor/context_menu.rs`, the
`arboard::Clipboard::new()` call inside `show_context_menu`'s
`context_menu` closure (used to decide whether "Paste" should be enabled).

**Status:** Fixed. Was deliberately deferred as low-priority/speculative
(see "Why this wasn't fixed on the spot" below), but was picked up anyway
alongside the rest of this file's entries — the trigger condition below was
never met on its own, this was just done opportunistically.

### What was found

Determining whether "Paste" should be enabled reads the OS clipboard via
`arboard::Clipboard::new().and_then(|mut cb| cb.get_text())`. Because this
line lives inside the `context_menu` closure, it re-runs every single frame
the menu is rendered, not just once when it opens — opening a clipboard
connection isn't free (an X11/Wayland round-trip under the hood on Linux),
and a user hovering the menu while deciding what to click could keep it
open for many frames in a row.

### Why this wasn't fixed on the spot

Low severity: the context menu is a rare, deliberately user-initiated,
short-lived interaction, not the typing/scrolling hot path `AGENTS.md`'s
performance principle is actually concerned with. A clean fix means real
new state — caching the clipboard read across frames, keyed to the
open/closed transition — for a benefit that's unlikely to ever be
perceptible. Not worth the complexity until there's evidence it matters.

### Proposed fix

If this ever shows up in profiling or a user-reported stutter: read the
clipboard once on the frame the menu actually opens (detectable via
`Response::secondary_clicked()`, or by diffing egui's own popup-open memory
state across frames) and cache the result in a small piece of state
threaded alongside `pending_input`, instead of on every frame the popup
renders.

### What was done

Exactly the proposed fix. `show_context_menu` gained a
`cached_clipboard_text: &mut Option<String>` parameter; the `arboard` read
now runs once, right before `response.context_menu(...)` is called, gated
on `response.secondary_clicked()` (the same condition egui's own
`context_menu` checks internally to decide whether to open the popup, so
it's true on exactly the frame it opens — no new detection mechanism
needed). The Paste item's closure reads the cached value instead of
re-reading the clipboard itself.

The new state lives on `FoxGardenApp` as `cached_clipboard_text: Option<String>`
(not per-tab) — threaded through `tabs::show` and `widgets::editor::show`
alongside `pending_editor_input`, since only the active tab's editor (and so
only one context menu) is ever shown at a time, same reasoning already
applied to `pending_editor_input` itself.

### Trigger condition

~~Only if actually observed to matter — speculative caching without a
measured need is exactly the kind of premature complexity the "lightweight"
principle in `AGENTS.md` warns against.~~ Superseded — done opportunistically
instead of waiting for that evidence.

---

## 7. ~~`widget::show`/`tabs::show`/`menu_bar::show` were missing the `too_many_arguments` allowance a sibling function's comment already claimed they had~~ — Resolved

**Where:** `crates/app/src/widgets/editor/widget.rs` (`show`),
`crates/app/src/panels/tabs.rs` (`show`), `crates/app/src/panels/menu_bar.rs`
(`show`).

**Status:** Fixed. Found during a full re-read of the project for missed
technical debt (prompted by working through #1-#4 above); fixed on the
spot as a one-line, well-precedented, zero-risk change per function rather
than recorded as deferred.

### What was found

`crates/app/src/widgets/editor/context_menu.rs`'s `show_context_menu`
carries `#[expect(clippy::too_many_arguments, reason = "... — see
widget::show's own too-many-arguments allowance for the same shape")]` — a
comment asserting `widget::show` has a matching suppression of its own. It
didn't. `widget::show`, `tabs::show`, and `menu_bar::show` all had more
parameters than clippy's default `too_many_arguments` threshold (7) — 12,
13, and 12 respectively as of this fix — with no `#[allow]`/`#[expect]`
anywhere, so `cargo clippy --workspace --all-targets` was silently emitting
three unaddressed warnings for exactly the kind of function #5 above (the
"splitting `widget.rs` further" entry) already argues is deliberately
many-argument: each parameter is independently-threaded per-frame state,
not a bundle worth turning into a struct.

### Why this wasn't caught earlier

Both `tabs::show` and `widget::show` crossed the 7-argument threshold
gradually, one new parameter at a time across several unrelated features
(most recently `cached_clipboard_text`, added for #4 above) — there was
never one single change that visibly introduced the warning, so nothing
flagged it in the moment. `context_menu.rs`'s comment reads as though it
was written assuming `widget::show` already carried (or would shortly
carry) the same suppression; that half of the change evidently never
landed.

### What was done

Added a matching `#[expect(clippy::too_many_arguments, reason = "...")]`
to all three functions, each citing the same "independently-threaded
frame state, not a bundle" reasoning already established for
`context_menu::show_context_menu` and spelled out at length in #5's "Why
it doesn't apply" section. `cargo clippy --workspace --all-targets` is
clean of `too_many_arguments` warnings as of this fix.

### Trigger condition

N/A — already fixed.
