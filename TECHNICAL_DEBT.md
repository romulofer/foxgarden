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

---

## 1. Wrap-selection and multi-cursor use two different input-interception mechanisms that could be one

**Where:** `crates/app/src/widgets/editor/widget.rs`, inside `pub fn show(...)`.

**Status:** Open. Flagged by the altitude-review agent during a `/simplify`
pass; deliberately not fixed in that pass because doing it properly means
touching the already-tested multi-cursor mechanism, not just the new code.

### Current shape

Two features both need to "steal an input event from egui before
`TextEdit` sees it, then apply the resulting edit manually" — but they do
it with different mechanics, coordinated only by inverting the same flag
(`multi_cursor_active_at_start`):

**Multi-cursor** (pre-existing, unchanged; `widget.rs` around line 127 for
the flag, line 408 for the predicate, line 305 for the repaint call):
1. Strip *all* matching events out of `ui.input_mut` up front via a named
   predicate (`is_multi_edit_event`) + `Vec::retain`.
2. Let `TextEdit::show()` run anyway — with those events gone, it renders
   the *unmodified* text this frame.
3. *After* `show()` returns, read `output.cursor_range` (reliably still
   the pre-edit selection, precisely because the events were stripped) and
   apply the edit manually.
4. Accept one frame of stale paint and call `ui.ctx().request_repaint()`
   — the comment at the call site explicitly owns this tradeoff.

**Wrap-selection** (added this session; `widget.rs` around line 157,
`fn single_pairable_char` around line 399):
1. Cheap-check the event queue for a single pairable character first
   (`single_pairable_char`), only then load the *persisted* `TextEditState`
   directly (`egui::text_edit::TextEditState::load(ui.ctx(), widget_id)`)
   to read the prior selection.
2. Remove *one* matching event via `i.events.iter().position(...)` +
   `i.events.remove(index)` — not `retain`, since at most one keystroke
   should ever be intercepted this way.
3. Apply the edit and mutate `old_text`/`text` *before* `TextEdit::show()`
   runs, so this same frame already renders the wrapped result — no stale
   frame, no `request_repaint()` needed.

### Why this is debt, not just a style difference

The wrap-selection timing (mutate before `show()`) is arguably *strictly
better* than multi-cursor's (mutate after, eat a stale frame, request a
repaint) — but that improvement was special-cased into the new feature
instead of being fed back into the shared mechanism. Two independently-
invented ways to solve the identical problem, sitting side by side in the
same function, is exactly the kind of divergence that makes the next
similar feature (there will be one — anything that needs "intercept before
TextEdit, decide based on selection state" fits this shape) default to
inventing a *third* variant instead of reusing either.

### Proposed fix

Factor one shared "pending-edit interception" primitive at the top of
`show()`:

```rust
/// Removes and returns the first event matching `predicate`, if any.
fn take_event(ui: &egui::Ui, predicate: impl Fn(&Event) -> bool) -> Option<Event> {
    ui.input_mut(|i| {
        let index = i.events.iter().position(|e| predicate(e))?;
        Some(i.events.remove(index))
    })
}
```

then:
- Have multi-cursor's interception use the *pre-apply* timing
  wrap-selection introduced (mutate `old_text`/`text` before `TextEdit::
  show()`, eliminating the `request_repaint()` workaround entirely) —
  this is the harder, higher-payoff half of the fix, since it changes
  multi-cursor's actual behavior (in a good way) and needs the full
  multi-cursor test suite (`widgets::editor::widget::tests::multi_cursor_*`)
  re-verified green, not just re-read.
- Have wrap-selection's single-event removal go through the same
  `take_event` helper multi-cursor's `retain`-based bulk removal would be
  rebuilt on, rather than the ad hoc `position`/`remove` it uses today.

### Why it was skipped in the `/simplify` pass

- The higher-payoff half (retiming multi-cursor) touches a mechanism with
  its own dedicated, already-green test suite (`multi_cursor_typed_edit_
  applies_at_every_active_cursor`, `arrow_key_collapses_extra_selections`,
  `non_intercepted_mutating_key_collapses_extra_selections_via_safety_net`)
  that a "cleanup" pass on freshly-written code has no business risking.
- It's a real design change (removing a documented, deliberate tradeoff —
  the stale-frame comment at the `request_repaint()` call site), not a
  mechanical extraction. That calls for its own reviewed change, not a
  drive-by inside an unrelated feature's cleanup.

### Trigger condition

Do this the *next* time a third feature needs the same "intercept before
TextEdit sees it" shape (block/rectangular paste and quick-fix application
are both candidates already in `FEATURES.md`). At that point the pattern
has three independent implementations instead of two, which is a much
stronger forcing function than two, and the unification pays for itself
across three call sites instead of speculatively across two.

---

## 2. Four `show_*` modal functions share an identical skeleton with no shared helper

**Where:**
- `crates/app/src/app.rs:176` — `fn show_open_error`
- `crates/app/src/panels/menu_bar.rs:117` — `fn show_about`
- `crates/app/src/panels/tabs.rs:191` — `fn show_close_confirm`
- `crates/app/src/panels/side_panel.rs:268` — `fn show_delete_confirm`

**Status:** Open. Flagged by the reuse-review agent; `show_open_error` is
the newest of the four (added this session) and is the one that
introduced the fourth copy of the pattern, but the first three already
existed and were themselves never consolidated.

### Current shape

All four follow the identical skeleton:

```rust
fn show_whatever(ui: &egui::Ui, /* state */) {
    let Some(data) = /* guard: is there anything to show? */ else { return };
    let ctx = ui.ctx().clone();
    egui::Modal::new(egui::Id::new("some_id")).show(&ctx, |ui| {
        ui.label(/* message, built from `data` */);
        // one or more buttons, each clearing/mutating state to dismiss
    });
}
```

They differ only in: the guard condition, the id string, the label
content, and how many buttons there are (1 for about/open-error, 2-3 for
close/delete confirm).

### Proposed fix

A shared low-level helper that owns the "clone ctx, open a `Modal` with
this id" part, leaving the label/buttons to a closure:

```rust
/// Shows a modal with `id` if `guard` is `Some`, calling `body` with the
/// guard's contents to draw the label/buttons. Returns whatever `body`
/// returns, so callers can signal "dismissed" back out without needing
/// interior mutability inside the closure.
fn show_modal<T, R>(ui: &egui::Ui, id: &str, guard: Option<T>, body: impl FnOnce(&mut egui::Ui, &T) -> R) -> Option<R> {
    let data = guard?;
    let ctx = ui.ctx().clone();
    Some(egui::Modal::new(egui::Id::new(id)).show(&ctx, |ui| body(ui, &data)).inner)
}
```

Each of the four call sites would shrink to a guard clause + one closure
with just its own label/buttons — no change in behavior, less boilerplate
per site, and one place (not four) that owns "how do we present a modal in
this app" if that ever needs to change (styling, animation, a consistent
OK-button position, etc).

### Why it was skipped in the `/simplify` pass

Three of the four call sites (`show_about`, `show_close_confirm`,
`show_delete_confirm`) are pre-existing code, untouched by this session's
diff. Refactoring them to adopt a new shared helper is a real change to
already-shipped, already-tested files that falls outside "clean up the
code this diff just added" — the `/simplify` skill's explicit scope is the
diff under review, not a general-purpose refactor invitation. Pulling in
three unrelated files to fix a duplication pattern that predates this
session's work needs its own deliberate pass, not a rider on this one.

### Trigger condition

Do this the next time a *fifth* modal is added (the DAP/debugger and
build/run output panel work in `FEATURES.md` will likely want at least
one), or the next time any of the four existing ones needs a behavior
change that should apply to all of them (e.g. Escape-to-dismiss, which
none of the four currently support).

---

## 3. Considered and rejected: moving `display_path` computation into the `Err` arm

**Where:** `crates/app/src/app.rs`, inside `impl eframe::App for
FoxGardenApp { fn ui(...) }`, the `if let Some(path) = outcome.open { ... }`
block (around line 220).

**Status:** Not actual debt — recorded here so a future agent doesn't
re-flag it and waste time re-deriving why it's not applicable.

### What was flagged

An efficiency-review agent noted that `let display_path =
path.display().to_string();` runs on every file-open attempt (success or
failure) but is only used in the `Err` arm, and suggested moving it inside
that arm so the successful-open path skips the allocation.

### Why it doesn't apply

`path: PathBuf` is moved into `self.state.open_tab(path)` — the `match`
scrutinee itself — before either arm's body runs. By the time the `Err`
arm executes, `path` has already been consumed by the `open_tab` call;
it's not available to compute `display_path` from inside that arm at all.
Moving the computation there without also cloning `path` first (which just
relocates the same cost rather than avoiding it, since `PathBuf::clone`
and `.display().to_string()` are both real allocations) doesn't compile as
suggested. The current placement — before the `match`, so both `path` and
its display string are available where needed — is the correct shape given
`open_tab`'s ownership signature, not an oversight. The cost itself (one
`format!`-free `.display().to_string()` per user click on a file in the
tree) is genuinely trivial and not worth restructuring around regardless.

---

## 4. `Scope::Property` conflates two different lexical concepts

**Where:** `crates/syntax/src/highlight.rs`, `fn scope_for_capture`, and
the `Scope` enum's doc comment.

**Status:** Open, but deliberate — recorded for awareness, not urgency.

### Current shape

```rust
} else if name.starts_with("property") || name.starts_with("tag") {
    Some(Scope::Property)
```

YAML/properties mapping keys (`@property` captures) and XML element names
(`@tag` captures) both map to the same `Scope::Property`, purely because
they happen to want the same color in the current theme. This is already
called out as a deliberate tradeoff in the `Scope` enum's own doc comment
in `highlight.rs` — it's not hidden, and it stays inside the existing
generic prefix-matching + exhaustive-match machinery (`theme::
color_for_scope` forces a match arm for every `Scope` variant, so nothing
about a future change here would silently go unhandled).

### When this becomes real debt

If a future language integration wants tag-like styling *visually distinct*
from property-key styling (plausible for, say, HTML if that's ever added
alongside XML), this conflation stops being free and needs either:
- a new `Scope` variant (touching every `match` on `Scope` —
  `scope_for_capture`, both `theme::color_for_scope` arms), or
- per-language capture-name remapping before the shared `Scope` enum
  (e.g. a `language`-aware `scope_for_capture` rather than the current
  language-agnostic one), which is a bigger, more structural change.

Until then, leave it — splitting the variant speculatively, before a
second language actually needs the distinction, is exactly the kind of
premature generalization this codebase's own conventions (see `AGENTS.md`)
argue against.

---

## 5. `EditorState::closed_tabs` grows without bound for the life of the process

**Where:** `crates/core/src/editor_state.rs` — the `closed_tabs: Vec<Document>`
field (line 13), pushed to by `close_tab` (line 69), popped by
`reopen_last_closed_tab` (line 78).

**Status:** Open. Found during a whole-project re-check, not tied to any
specific session's diff.

### Current shape

Every tab close pushes the *full* `Document` — including its entire
`Rope` buffer, i.e. the whole file's contents — onto `closed_tabs`, purely
so `Ctrl+Shift+T` can restore it later:

```rust
pub fn close_tab(&mut self, index: usize) {
    let document = self.open_tabs.remove(index);
    // ...
    self.closed_tabs.push(document);
}
```

Nothing ever removes entries from `closed_tabs` except `reopen_last_closed_tab`
popping the single most recent one. There's no cap on its length, no LRU
eviction, and — checked directly — no other reference to `closed_tabs`
anywhere in the codebase that would bound or clear it, *including*
`open_project` (switching to an entirely different project keeps every
closed tab from the *previous* project sitting in memory indefinitely).

### Why this matters

A session that opens and closes many files over time (browsing through a
large project, reviewing file after file) accumulates full copies of every
closed file's content for as long as the process runs. For small
Java/Kotlin source files this is a slow leak, not an acute problem — but
it's unbounded, and it compounds across project switches rather than
resetting. A user who works a full day without restarting the app, moving
between several projects, is the realistic case this eventually bites.

### Proposed fix

Cap `closed_tabs` at a small fixed size (an actual "recently closed" list,
which is the only thing it's used for — `Ctrl+Shift+T` walks it, repeat
presses go further back):

```rust
const MAX_CLOSED_TABS: usize = 20;

pub fn close_tab(&mut self, index: usize) {
    // ... existing logic ...
    self.closed_tabs.push(document);
    if self.closed_tabs.len() > MAX_CLOSED_TABS {
        self.closed_tabs.remove(0); // drop the oldest
    }
}
```

(A `VecDeque` would make the eviction O(1) instead of O(n) shift, if this
ever shows up as a real cost — at `MAX_CLOSED_TABS` = 20 it won't.)
Separately, `open_project` should probably clear `closed_tabs` outright —
reopening a tab from a project you've since navigated away from is a
confusing "resurrection" even before memory is a concern.

### Why it wasn't fixed on the spot

Found during a documentation pass ("re-check the whole project"), not a
code-change pass — recording it rather than making an unreviewed behavior
change (even a small, obviously-correct one) outside the context of an
actual task that touches this file.

### Trigger condition

Worth fixing opportunistically the next time `editor_state.rs` or
`Ctrl+Shift+T` behavior is touched for any other reason — it's a small,
low-risk, self-contained change. Worth prioritizing sooner if anyone
reports memory growth over a long-running session.

---

## 6. `paint_diagnostics` redoes an O(file length) byte→char scan per diagnostic, every frame

**Where:** `crates/app/src/widgets/editor/painting.rs:9-45`, `fn
paint_diagnostics`, specifically lines 24-25.

**Status:** Open. Found during a whole-project re-check.

### Current shape

```rust
for diag in diagnostics {
    let start = diag.range.start.min(text.len());
    let end = diag.range.end.min(text.len()).max(start);
    // ...
    let char_start = text[..start].chars().count();
    let char_end = text[..end].chars().count().max(char_start + 1);
    // ...
}
```

`diag.range` is a *byte* range (it comes straight from tree-sitter's
`node.byte_range()`, see `crates/syntax/src/diagnostics.rs`), but egui's
`CCursor` — used a few lines later for `output.galley.pos_from_cursor(...)`
— needs a *char* index. The conversion, `text[..start].chars().count()`,
walks the string from byte 0 up to `start` counting characters: O(start)
work, redone from scratch for every diagnostic, every single frame the
editor with that diagnostic is visible — regardless of whether `text` or
`diagnostics` changed since the last frame. `paint_diagnostics` is called
unconditionally each frame in `widgets::editor::show` (no early-out, no
memoization) whenever `doc.diagnostics` is non-empty.

For one diagnostic near the *start* of a file this is negligible. For a
diagnostic near the *end* of a large file, or multiple diagnostics at once,
this is O(file length × diagnostic count) of redundant work, sustained for
as long as the syntax error persists — which, in practice, is exactly the
window where a user is actively looking at the file trying to fix it.

### Proposed fix

The layout cache this codebase already has for shaped galleys
(`LayoutCacheKey`/`CachedLayout` in `widgets/editor/widget.rs`) is the
right model: cache the byte→char conversion, invalidate it the same way
the galley cache already is (on content-hash change), and pass pre-computed
char positions into `paint_diagnostics` instead of raw byte ranges. Simplest
version: since `highlight_spans`'s layouter already walks the *whole*
source once per shape to build the `LayoutJob`, byte→char conversion for
every diagnostic could piggyback on that same single pass rather than
paying for it again, separately, per diagnostic, in `paint_diagnostics`.

A smaller, more local fix if a full cache feels like too much: sort
`diagnostics` by `range.start` once (cheap, and diagnostics rarely number
more than a handful) and walk `text` *once* computing all the char offsets
in a single forward pass, rather than restarting the scan from byte 0 for
each diagnostic independently. This turns O(file length × diagnostic
count) into O(file length + diagnostic count) — a real improvement with
much less structural change than a full cache.

### Why it wasn't fixed on the spot

Found during a documentation pass, not a code-change pass. Also: no
concrete evidence yet (no profiling, no user report) that this is
*actually* a perceptible problem at realistic file sizes/diagnostic
counts for this project's stated scope (single Java/Kotlin source files,
not multi-thousand-line generated code) — worth confirming it's real
before spending a review cycle on the fix.

### Trigger condition

Fix the "sort once, walk once" version the next time `painting.rs` is
touched for any reason. Escalate to the full layout-cache-integrated
version only if profiling (or a user report of editor lag on a file with
persistent syntax errors) actually shows this mattering — don't do the
bigger version speculatively.

---

## 7. Window icon fix is unconfirmed on a real desktop

**Where:** `crates/app/src/main.rs`'s `ICON_PNG`, and the corresponding
gotcha already recorded in `AGENTS.md` (search "window icon fix").

**Status:** Open, likely-but-unverified fix — cross-referenced here from
`AGENTS.md` because that file is context/history for a future agent to
*learn from*, not a queue of things to *act on*; this file is that queue.

### Current shape

The app bundles a downscaled 128×128 copy of the window icon
(`crates/app/assets/icon/icon_128.png`) instead of the pristine 1024×1024
source, on the theory that winit's X11 backend was silently failing to
set a `_NET_WM_ICON` window property that large (`.ignore_error()` on the
egui-winit side swallows the failure with no panic, log, or visible
error). Per `AGENTS.md`'s own account, `xprop -id <window> _NET_WM_ICON`
came back *empty* both before and after the resize inside the sandbox
this was tested in — so the fix track record so far is "was seen working
on the reporting user's real Linux Mint/Cinnamon desktop before the
resize was even applied," not a controlled before/after confirmation.

A same-session attempt to shrink this further (128 -> 32, then 128 -> 64)
was tried and reverted at the user's request — back to 128×128 as the
bundled size. Whatever the eventual fix turns out to be, it isn't "keep
halving the PNG."

### Why this matters

If the size theory is wrong, the real cause (per `AGENTS.md`'s own
alternate hypothesis) might be that this sandbox's X server/WM doesn't
apply `_NET_WM_ICON` at all — in which case the 128px bundling is a no-op
fix that happens to coincide with the icon looking right for unrelated
reasons, and a *future* regression (someone reverting to the 1024px
source, or a different desktop environment with a stricter size limit)
would silently reintroduce the original bug with no test to catch it.

### Proposed fix

Not a code fix — a verification step: a human with access to a real
(non-sandboxed) Linux desktop needs to confirm the window icon actually
renders correctly with the current `icon_128.png`, ideally by checking
`xprop -id <window> _NET_WM_ICON` returns non-empty data matching the
bundled icon. If it's confirmed working, this entry can simply be deleted
(and the `AGENTS.md` gotcha updated from "likely, not confirmed" to
"confirmed"). If it's *not* working, the investigation needs to go past
the size theory to whether `_NET_WM_ICON` is being set at all on whatever
desktop is being tested — a `.desktop` file's `Icon=` entry, or whatever
icon-resolution convention the specific WM/DE in use actually follows, is
worth checking before trying yet another PNG size.

### Why it wasn't fixed on the spot

Not fixable from within this environment — there's no real (non-sandboxed)
X11 desktop available to test against here, which is exactly why the
original investigation left it unconfirmed in the first place.

### Trigger condition

Next time anyone is running FoxGarden on a real desktop anyway (not a
sandbox), it costs one `xprop` command to close this out either way. If
it's still broken at 128×128, that's the trigger to stop treating this as
a size problem and check what icon-resolution mechanism the actual WM/DE
uses instead.

---

## 8. `highlights_java.scm`'s `@constant` capture is dead — no `Scope` renders it

**Where:** `crates/syntax/queries/highlights_java.scm:94-95`, and
`crates/syntax/src/highlight.rs:25-41` (`fn scope_for_capture`).

**Status:** Open. Found while diffing `highlights_java.scm` against
`../references/java` (Zed's Java extension, same `tree-sitter-java` grammar)
for syntax-highlighting improvements.

### Current shape

```scheme
((identifier) @constant
 (#match? @constant "^_*[A-Z][A-Z\\d_]+$"))
```

This pattern exists specifically to color ALL-CAPS identifiers (the
idiomatic Java convention for `static final` constants and enum constants)
— but `scope_for_capture` only recognizes `keyword`/`string`/`comment`/
`type`/`function`/`property`-prefixed capture names:

```rust
fn scope_for_capture(name: &str) -> Option<Scope> {
    if name.starts_with("keyword") { ... }
    else if name.starts_with("string") { ... }
    else if name.starts_with("comment") { ... }
    else if name.starts_with("type") { ... }
    else if name.starts_with("function") { ... }
    else if name.starts_with("property") || name.starts_with("tag") { ... }
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
  color-table arms, and `scope_for_capture`) if none of the existing six
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

## 9. Kotlin's reference `highlights.scm` targets a different grammar than the one vendored here

**Where:** `crates/syntax/queries/highlights_kotlin.scm` vs.
`../references/kotlin/languages/kotlin/highlights.scm`.

**Status:** Not actionable as-is — recorded so a future agent doesn't
attempt a direct line-by-line port and introduce broken captures.

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

### Trigger condition

Next time Kotlin highlighting is revisited — this entry just saves that
future pass from re-discovering the grammar mismatch from scratch.
