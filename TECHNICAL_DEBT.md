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
