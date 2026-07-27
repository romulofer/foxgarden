# TECHNICAL_DEBT.md

Known-and-deferred issues in this codebase — things a `/simplify`-style
review (or ordinary feature work) found and a human or agent explicitly
chose *not* to fix on the spot, with the reasoning preserved. Git-tracked
(unlike `FEATURES.md`/`PLAN.md`/`SPEC.md`, which are local-only) because
deferred debt is exactly the kind of context that's expensive to
reconstruct and cheap to lose.

**Schema** — every entry's heading carries one status tag:
- `[OPEN]` — unresolved: either a real fix still to do, or a question
  that's been considered and argued against for now (reasoning in the
  entry itself) but isn't fixed code, so it stays open rather than
  closed — a future pass can re-open the question if circumstances
  change instead of re-litigating it from scratch.
- `[RESOLVED]` — fixed. Kept as historical record of what the bug/gap was
  and how it was actually fixed, in case the same shape resurfaces
  elsewhere.

Grouped by tag into two sections (Open / Resolved) below, but
**entry numbers are stable IDs assigned in discovery order** — not a
priority ranking, not a per-section count — so "see #2" always means the
same entry no matter which section it currently lives in. An entry moves
sections as its status changes; the number never changes.

Each entry gives: the current code shape (`Where`/`What was found`), the
reasoning for not fixing it immediately or at all (`Why it wasn't fixed`/
`Why it doesn't apply`), a concrete fix sketch (`Proposed fix`), and a
trigger condition for when it's worth revisiting. **Line numbers are a
snapshot and will drift** — function names and file paths are the stable
anchor. Verify the "current shape" still matches reality before trusting
the rest of an entry; if it doesn't, the entry is stale and should be
rewritten or removed, not blindly executed.

## Index

| # | Tag | Entry |
|---|-----|-------|
| 3 | `[OPEN]` | Kotlin's reference `highlights.scm` targets a different grammar than the one vendored here |
| 9 | `[OPEN]` | Cross-class dot-completion offers a field regardless of its visibility, unlike methods — Java and Kotlin both |
| 10 | `[OPEN]` | `fields_in_type` can never find an interface's own constants — a second, more severe instance of #9's shape |
| 11 | `[OPEN]` | Opening a project tree aborts entirely on the first unreadable file/directory, anywhere in the tree |
| 12 | `[OPEN]` | Dot-completion's local-variable scan doesn't respect declaration order relative to the cursor |
| 5 | `[OPEN]` | Splitting `widget.rs` further |
| 6 | `[OPEN]` | `widget.rs`'s `open_fixture` test helper wraps `test_support::temp_document` instead of being replaced by it |
| 1 | `[RESOLVED]` | Moving `display_path` computation into the `Err` arm |
| 2 | `[RESOLVED]` | `highlights_java.scm`'s `@constant` capture was dead — no `Scope` rendered it |
| 4 | `[RESOLVED]` | Context menu's Paste item created a new OS clipboard connection every frame the menu was open |
| 7 | `[RESOLVED]` | `widget::show`/`tabs::show`/`menu_bar::show` were missing a `too_many_arguments` allowance a sibling function's comment already claimed they had |
| 8 | `[RESOLVED]` | A stray `cargo fmt` run reformatted every file touched during the Phase 2–4 virtualized-editor work to rustfmt's defaults |
| 13 | `[RESOLVED]` | A still-open word-completion popup blocked dot-completion's own trigger on the exact keystroke that should have opened it |
| 14 | `[RESOLVED]` | Shift+Tab with no selection was a silent no-op — "left to egui's own no-selection handling," which egui never actually implemented |

---

# Open

## 3. [OPEN] Kotlin's reference `highlights.scm` targets a different grammar than the one vendored here

**Where:** `crates/syntax/queries/highlights_kotlin.scm` vs.
`../references/kotlin/languages/kotlin/highlights.scm`.

**Status:** Partially addressed. The whole-file incompatibility below is
still real — a direct line-by-line port remains off the table — but two
concrete constructs (enum-entry-as-constant, richer modifier-keyword
coverage) have now been ported by following this entry's own proposed
methodology. Recorded below as worked examples for whichever construct gets
picked up next.

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
`Scope::Constant` (added for #2) as where it now renders.
Verified via `cargo test -p syntax`: an `enum class Level { LOW, MEDIUM,
HIGH }` fixture in `valid.kt` plus `has_scope_over("LOW"/"MEDIUM"/"HIGH",
Scope::Constant)` assertions in `kotlin_highlight_query_compiles_and_covers_expected_ranges`.

Also ported richer modifier-keyword coverage (PLAN.md Track K), the second
item on this entry's own example list. Every modifier keyword Zed's file
covers via grouped `fwcd`-grammar node types (`class_modifier`,
`function_modifier`, `visibility_modifier`, etc.) turned out to already be
present in `highlights_kotlin.scm`'s flat `@keyword` literal list here — one
exception: `"reified"`, which this file's own header comment had
deliberately excluded (it doesn't compile as a bare literal, same as
`"break"`/`"continue"`). Checked `tree-sitter-kotlin-ng`'s `grammar.js`:
unlike `"break"`/`"continue"`, `"reified"` isn't a bare string in the
grammar — it's wrapped in its own `reification_modifier` rule
(`reification_modifier: _ => 'reified'`), and `(reification_modifier)`
*does* compile (bisected via `tree_sitter::Query::new`, same methodology as
this entry's own). Added `(reification_modifier) @keyword` to
`highlights_kotlin.scm`. Verified via `cargo test -p syntax`: an
`inline fun <reified T> isInstance(value: Any): Boolean = value is T`
fixture in `valid.kt` plus a `has_scope_over("reified", Scope::Keyword)`
assertion in `kotlin_highlight_query_compiles_and_covers_expected_ranges`.

Remaining candidates from Zed's file (regex-literal detection,
`@variable.builtin` for `it`/`field`) are each still their own future pass,
following the same three-step process.

### Trigger condition

Next time Kotlin highlighting is revisited — this entry just saves that
future pass from re-discovering the grammar mismatch from scratch, and now
also has one worked example of the fix process to follow.

---

## 9. [OPEN] Cross-class dot-completion offers a field regardless of its visibility, unlike methods — Java and Kotlin both

**Where:** `crates/syntax/src/fields.rs` (`fields_in_class_body`,
`fields_in_type`), used by `crates/app/src/widgets/editor/widget.rs`
(`java_members_as_items`); `crates/syntax/src/kotlin_members.rs`
(`kotlin_properties_in_class_body`, `kotlin_properties_in_type`), used by
`kotlin_members_as_items` — for the code-completion popup's field/property
candidates (`SPEC.md`/`PLAN.md` Phases 3b/3c).

**Status:** Open, now confirmed on both languages. Found on the Java side
while wiring Phase 3b; not fixed on the spot since `SPEC.md` §4 never asked
for it and `fields_in_class_body` already had this shape before Phase 3b
touched it. Phase 3c's Kotlin wiring reproduced the identical shape rather
than fixing it, for the same reason: `SPEC.md` §4's Kotlin section only
asks for visibility filtering on `kotlin_functions_in_type` (mirroring
`methods_in_type`'s complement rule), never on properties — no `SPEC.md`
ask, no fix, same as the Java side.

### What was found

`fields_in_class_body`'s only visibility-adjacent check is `include_static`
gating whether `static` fields are skipped — it never checks
`private`/`protected`/package-private the way `methods_in_type`'s sibling
`method_signature` does (excludes `static`/`private`/`final` for the
default, "what can an external caller see" listing). Concretely: `Bar b =
new Bar(); b.` correctly excludes `Bar`'s private *methods* (verified by
`a_local_variable_typed_as_another_project_class_offers_that_classs_public_members`
in `widget/tests/completion.rs`), but would incorrectly still offer any of
`Bar`'s private *fields* — nothing filters those out for a cross-class
receiver. `this.`/`super.` are unaffected by this gap (they're supposed to
see every member regardless of visibility already); only the
external-receiver path is wrong.

`kotlin_properties_in_class_body`/`kotlin_properties_in_type` (added for
Phase 3c) have the exact same shape: no modifier check at all, unlike
`kotlin_function_signature`'s deliberate `contains("private")` check for
functions.
`kotlin_a_local_variable_typed_as_another_project_class_offers_that_classs_public_members`
in `widget/tests/completion.rs` proves the function side is filtered
correctly, same as Java's equivalent test, but there's no companion test
with a `private val`/`private var` on the fixture class — because there's
no filtering to test yet.

### Why it wasn't fixed immediately

`SPEC.md` §4's own field-related ask was narrower — add `include_static` so
completion could offer a class's constants — and never mentions field
privacy filtering at all, unlike its explicit "`this.`/`super.` unfiltered
vs. `methods_in_type`'s existing filtering" call for methods. Adding
privacy filtering unprompted would mean guessing at a shape `SPEC.md`
doesn't specify (mirror `method_signature`'s modifier-text-search exactly?
thread a `Visibility` enum through instead of a bool?) rather than
following an existing design decision, and risked scope creep on a phase
already large enough (Phase 3b's Java wiring plus its own "does
`this.`/`super.` need an unfiltered variant" design question).

### Proposed fix

Give `fields_in_class_body`/`fields_in_type` the same modifier-text-search
`private` check `method_signature` already does
(`modifiers_text.contains("private")`), gated the same way
`unfiltered`/`include_static` already is: skip a private field for the
external, filtered listing; `this.`/`super.`'s unfiltered call keeps
seeing it regardless. Add a regression test mirroring
`a_local_variable_typed_as_another_project_class_offers_that_classs_public_members`
but with a private *field* on `Bar` instead of a private method.

Same fix, same shape, on the Kotlin side: give
`kotlin_properties_in_class_body` the `contains("private")` check over a
property's own `child_by_kind(member, "modifiers")` span (the same node
`kotlin_function_signature` already checks for functions), gated the same
way — an `unfiltered` parameter threaded through
`kotlin_properties_in_class_body`/`kotlin_properties_in_type`, mirroring
`kotlin_functions_in_type`'s own `unfiltered` parameter exactly. Add a
regression test mirroring
`kotlin_a_local_variable_typed_as_another_project_class_offers_that_classs_public_members`
but with a `private val`/`private var` on `Bar` instead of a private `fun`.

### Trigger condition

Next time Phase 4 (now landed) or any later phase touches `fields.rs`/
`java_members_as_items`/`kotlin_members.rs`/`kotlin_members_as_items` for
an unrelated reason — same files, low incremental cost to fix alongside it
— or sooner if a user notices a private field/property leaking into the
dot-completion popup, on either language.

---

## 10. [OPEN] `fields_in_type` can never find an interface's own constants — a second, more severe instance of #9's shape

**Where:** `crates/syntax/src/fields.rs` (`collect_fields_in_type:116`,
`fields_in_class_body:37`), contrasted with `crates/syntax/src/methods.rs`
(`collect_methods:156`, which explicitly covers both `class_declaration`
and `interface_declaration`).

**Status:** Open. Found via a dedicated debt-hunting pass after #9 landed
— grew directly out of the same code #9 already flags, so recorded
alongside it rather than as a fully independent surprise.

### What was found

`collect_fields_in_type` only ever matches `node.kind() == "class_declaration"`:

```rust
fn collect_fields_in_type(node: Node, source: &str, type_name: &str, include_static: bool, out: &mut Vec<FieldInfo>) {
    if node.kind() == "class_declaration"
```

`collect_methods`, its direct sibling, explicitly covers both:

```rust
let is_target = matches!(node.kind(), "class_declaration" | "interface_declaration")
```

— and has a dedicated regression test, `methods_in_type_finds_interface_methods`,
that `fields.rs`'s test module has no equivalent of.

It's a two-layer gap, not one: even with the node-kind check added,
`fields_in_class_body` only matches children of kind exactly
`"field_declaration"` — but per `tree-sitter-java-0.23.5`'s own
`node-types.json`, an interface's constants are a structurally-identical,
differently-named node kind, `"constant_declaration"` (same
`declarator`/`type`/`modifiers` fields as `field_declaration`, just its own
tag; `interface_body`'s possible children list `constant_declaration`, not
`field_declaration`, at all). Both the node-kind filter *and* the
declaration-kind filter need a second case, or a class implementing
`interface Constants { int MAX = 10; void run(); }` and typed as a
dot-completion receiver offers `run()` (methods correctly walk interfaces)
but never `MAX` (fields never do) — interface constants are an idiomatic,
common Java pattern, not an edge case.

### Why it wasn't fixed immediately

Discovered during a debt-hunting pass, not while touching this file for a
feature — fixing it now would mean editing `fields.rs` outside of any
concrete phase that needs it, same "don't expand scope beyond what's
already in flight" reasoning #9 gives, just one step further removed.

### Proposed fix

In `collect_fields_in_type`: match `"class_declaration" | "interface_declaration"`,
same as `collect_methods`. In `fields_in_class_body` (or a variant of it):
also accept `"constant_declaration"` bodies, mapping them into `FieldInfo`
the same way `field_declaration` already is (same field names on the node,
so the extraction logic itself doesn't need to change, only the node-kind
gate). Add a `fields_in_type_finds_interface_constants` test mirroring
`methods_in_type_finds_interface_methods`.

### Trigger condition

Same as #9 — next time Phase 3c/4 (or any other work) touches
`fields.rs`/`java_members_as_items`, or sooner if a user notices interface
constants missing from dot-completion on a class that implements an
interface.

---

## 11. [OPEN] Opening a project tree aborts entirely on the first unreadable file/directory, anywhere in the tree

**Where:** `crates/core/src/project.rs` (`FileNode::build:36-83`).

**Status:** Open. Found via a dedicated debt-hunting pass; not observed to
have actually misfired for a real user yet, which is why this is recorded
rather than fixed speculatively.

### What was found

```rust
for entry in std::fs::read_dir(path)? {
    let entry = entry?;
    let is_dir = entry.file_type()?.is_dir();
    ...
}
...
let children = entries.iter().map(|(_, child_path)| FileNode::build(child_path))
    .collect::<std::io::Result<Vec<_>>>()?;
```

Every `?` here propagates through the recursive `FileNode::build` calls all
the way up to `Project::open`, and from there to `EditorState::open_project`,
which only assigns `self.project` if `Project::open` succeeds outright. A
single `read_dir`/`file_type` failure *anywhere* in an arbitrarily large,
arbitrarily deep tree — a permission-denied subdirectory, or a file
deleted between `read_dir` listing it and the recursive `build` call
reaching it — fails the *entire* "Open Folder" (or the post-rename/delete
tree refresh in `side_panel.rs`) with an error banner and no tree at all,
discarding every other file/directory that was already walked
successfully. The failure mode is total, not per-node.

### Why it wasn't fixed immediately

Not observed to have actually caused a problem — `?`-propagation is the
path of least resistance in Rust, and every existing test in this module
only covers the happy path plus the `SKIPPED_DIR_NAMES` skip-list, not a
read error mid-walk. Worth recording rather than fixing blind, since the
right degrade-gracefully shape (skip the bad entry with a placeholder
node? surface a non-fatal warning alongside the otherwise-complete tree?)
is a real design decision, not a one-line change, and doesn't have a
concrete trigger yet.

### Proposed fix

Change `FileNode::build`'s recursive-directory case to not fail the whole
walk on one bad child: catch each child's `std::io::Result` individually
(instead of `.collect::<Result<Vec<_>>>()?`) and either skip that entry
silently (mirroring how `SKIPPED_DIR_NAMES` already treats some entries as
not worth including) or represent it as a placeholder `FileNode` the tree
UI can render distinctly (e.g. greyed out, non-expandable). Add a test that
makes one subdirectory unreadable (`std::fs::set_permissions`, Unix-only —
check how existing tests in this module handle platform-specific
setup, if at all) and asserts the rest of the tree still builds.

### Trigger condition

A user reports "Open Folder" failing on a real project, or `side_panel.rs`'s
tree-refresh path is touched for an unrelated reason and this is easy to
fix alongside it.

---

## 12. [OPEN] Dot-completion's local-variable scan doesn't respect declaration order relative to the cursor

**Where:** `crates/syntax/src/completion.rs` (`local_variable_type:57-80`,
Java; `local_property_type`, Kotlin's equivalent, has the same shape).

**Status:** Open, but low severity and already reasoned through inline —
recorded per this pass's own "a comment documents a known simplification
that was never logged" criterion, not because anyone missed it.

### What was found

The function's own doc comment already states the limitation:

```rust
/// A simpler "anywhere in the enclosing method" scan rather than a precise
/// backward-from-cursor one (`SPEC.md` §3a's own documented first-cut
/// simplification) — a variable declared *after* the cursor being wrongly
/// offered is a rare, low-consequence shadowing edge case.
fn local_variable_type(node: Node, source: &str, name: &str) -> Option<String> {
```

Confirmed by reading the body: it recurses the whole enclosing method/
constructor subtree for a name match with no byte-position comparison
against the cursor at all. A variable declared *later* in the same method
is offered as if already in scope, and in a shadowing case (two same-named
locals at different points in one method) the wrong one's type can win.

### Why it wasn't fixed immediately

Already a deliberate, `SPEC.md`-sanctioned tradeoff (§3a explicitly called
this an acceptable first cut) — genuinely rare in practice (declaring two
locals with the same name in one method is itself unusual style), and a
precise backward-from-cursor scan is more code for a low-consequence edge
case. Recorded here mainly so it's discoverable from `TECHNICAL_DEBT.md`
directly rather than only from a source comment.

### Proposed fix

If ever prioritized: track byte position during the recursive walk and
only accept a `local_variable_declaration`/`property_declaration` whose
own start byte is `<=` the resolution's `cursor_byte`, keeping the
*nearest preceding* declaration on a name collision instead of the first
one found in tree order.

### Trigger condition

Only if a real shadowing case is reported as confusing in practice —
`SPEC.md` already accepted this tradeoff once; revisit only with new
evidence it matters, not preemptively.

---

## 5. [OPEN] Splitting `widget.rs` further

**Where:** `crates/app/src/widgets/editor/widget.rs` (~1360 lines since its
colocated test module was extracted to the sibling `widget/tests.rs`; ~2214
before that extraction).

**Status:** Not actual debt — recorded so a future review doesn't re-flag
file size alone and fragment a file that's already been evaluated for
exactly that. Re-checked while working through this file's other entries
(#1-#4, three of which touched this exact file): still holds. Those
changes only threaded one more plumbing parameter
(`cached_clipboard_text`) through `show()` and its call sites — no new
self-contained feature landed inside `show()` the way getters/setters
generation once did, so the "Why it doesn't apply" reasoning below is
unchanged and no split was made. Re-checked again while landing
dot-completion (Phase 2-3b): the trigger/candidate-builder functions added
to `widget.rs` for that feature are new *functions*, not new logic mixed
into `show()` itself, so this still doesn't apply.

### What was flagged

A code-organization review this session already extracted two genuinely
separable concerns out of this file — the right-click context menu (now
`context_menu.rs`) and the getters/setters picker's rendering (moved into
`codegen.rs`, next to the data/logic it already owned) — after `widget.rs`
had grown to 2397 lines doing several distinct things. The colocated test
module has since been extracted to a sibling `widget/tests.rs` (an orthogonal
split — it moves ~2100 lines of tests out without touching `show()`'s
frame-orchestration at all), leaving the module file dominated by the
`show()` function itself. It could still get flagged again for size alone.

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

## 6. [OPEN] `widget.rs`'s `open_fixture` test helper wraps `test_support::temp_document` instead of being replaced by it

**Where:** `crates/app/src/widgets/editor/widget/tests.rs` (the extracted test
module), `fn open_fixture`.

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

## 1. [RESOLVED] ~~Considered and rejected: moving `display_path` computation into the `Err` arm~~

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

## 2. [RESOLVED] ~~`highlights_java.scm`'s `@constant` capture is dead — no `Scope` renders it~~

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

## 4. [RESOLVED] ~~Context menu's Paste item creates a new OS clipboard connection every frame the menu is open~~

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

## 7. [RESOLVED] ~~`widget::show`/`tabs::show`/`menu_bar::show` were missing the `too_many_arguments` allowance a sibling function's comment already claimed they had~~

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

---

## 8. [RESOLVED] ~~A `cargo fmt` run (no project `rustfmt.toml`) reformatted every file touched during the Phase 2–4 virtualized-editor work to rustfmt's defaults~~

**Where:** Every file touched while landing PLAN.md Phase 2 (the
`egui::TextEdit` → `text_area` swap), Phase 3 (code folding), and Phase 4
(word-wrap) — `widget.rs`, `widget/tests.rs`, `painting.rs`,
`context_menu.rs`, `folding.rs`, everything under `text_area/`,
`menu_bar.rs`, `tabs.rs`, `app.rs`, `widgets/editor.rs`.

**Status:** Resolved (PLAN.md Phase 7 / SPEC.md §12) — see "What was done"
below. The rest of this entry (through "Proposed fix") is kept as the
historical record of why the partial reformatting happened in the first
place.

### What was found

Mid-session, a mechanical `sed`/Python edit across `shell.rs`/`painting.rs`/
`folding.rs` left a few lines mis-indented, and `cargo fmt -p app` was run
to clean it up. This repo has no `rustfmt.toml` at any level, so that ran
with rustfmt's *defaults* — roughly 90-column wrapping, and (for imports
specifically) alphabetized `use` braces — against a codebase that
consistently hand-formats much wider (single-line function signatures and
`use` blocks well past 100 columns are the norm throughout, e.g. `widget.rs`
`show`'s own 20-parameter signature) and doesn't alphabetize within `use`
braces. The result: every file touched from that point on in the session
got a large, purely-cosmetic reformatting pass layered on top of the real
logic changes, on top of an already-substantial diff (the `TextEdit` swap
alone touched ~15 files). ~17 files that had been touched by an earlier,
unrelated `cargo fmt` invocation but carried no intentional edits were
caught and reverted to `HEAD` in the same session (pure noise, zero risk);
the files listed above still carry the reformatting mixed into real changes
and were not reverted.

### Why it wasn't fixed immediately

No clean pre-`fmt` snapshot existed to diff against (all of Phase 2–4 was
uncommitted working-tree state, not a commit), so separating "my intentional
edit" from "rustfmt's rewrap" line-by-line across ~15 files would have meant
either a slow, error-prone manual pass (real risk of reintroducing a bug
while doing so) or re-deriving each file's content from scratch. Neither
was worth the risk this deep into an already-large, already-tested change,
and the reformatting itself is purely cosmetic — verified functionally
inert (`cargo build`, `cargo test --workspace`, `cargo clippy --all-targets`
all stayed green across the revert). Flagged here instead, per explicit
direction to keep going and track it as debt rather than block on it.

### Proposed fix

Add a `rustfmt.toml` at the workspace root that actually matches this
project's established style (`max_width` well past rustfmt's 100 default —
look at `widget.rs`'s existing signatures/`use` blocks for a real target
number; `imports_granularity`/import-sorting settings that stop
alphabetizing within a `use` brace), then run `cargo fmt --all` **once**,
deliberately, as its own commit — so the whole workspace converges to one
consistent, chosen style in a single reviewable diff instead of the
accidental partial one this entry describes. Until that lands, avoid running
bare `cargo fmt`/`cargo fmt -p <crate>` again on this repo; fix any stray
indentation by hand or with a narrowly-scoped editor action instead.

### Trigger condition

Whenever there's appetite for a dedicated formatting-normalization pass —
not urgent (no functional impact), but the longer it's deferred the bigger
that one-time diff gets as more files accumulate hand-formatting drift from
whatever rustfmt's defaults would produce.

### What was done

Added a workspace-root `rustfmt.toml` with `max_width = 120`, chosen
empirically rather than guessed: ran `cargo fmt --all` at a few candidate
widths on this codebase and compared the diffs. 100 (the default) is exactly
the problem this entry describes. 120 fixed the over-wrapping while staying
readable. 150 went too far the other way — several `use` blocks and match
arms landed at 145-149 columns, hard to scan even in a wide editor pane.

No `imports_granularity`/import-sorting override, contrary to this entry's
own "Proposed fix" above: `imports_granularity` and `group_imports` turned
out to be nightly-only options on this project's rustfmt version (stable
1.9.0) — setting either just prints a warning and is ignored. Turns out
none was needed anyway: stable rustfmt's *default* behavior already leaves
each `use { ... }` block's item order exactly as written, only rewrapping
line breaks to fit `max_width` — verified by running the real `cargo fmt
--all` and diffing every touched `use` block for reordering, not just
inspecting a few by eye. This entry's original "doesn't alphabetize within a
`use` brace" framing was itself imprecise: the hand-written blocks that
looked alphabetized were coincidentally so (short lists a human would
naturally write in a sensible order), not evidence rustfmt would reorder
them — there was never actually a setting to fix here.

Ran `cargo fmt --all` once, as the only change alongside `rustfmt.toml`
itself — no logic mixed in. Verified functionally inert the same way the
original stray run was: `cargo build --workspace`, `cargo test --workspace`
(same pass count before and after), and `cargo clippy --workspace
--all-targets` all stayed green across the formatting-only diff.

---

## 13. [RESOLVED] ~~A still-open word-completion popup blocked dot-completion's own trigger on the exact keystroke that should have opened it~~

**Where:** `crates/app/src/widgets/editor/widget.rs` (`show`) — the
word-completion trigger (`SPEC.md` §1b) and dot-completion trigger
(`SPEC.md` §3), both gated on `completion.is_none()`, and the popup's
post-frame "close if the filtered list is now empty" check.

**Status:** Fixed. Reported live: finishing "super" character by character
(word-completion open the whole time, offering the `super` keyword/
identifier) and then typing `.` didn't open dot-completion at all — only
erasing and retyping the `.` did. A parallel report ("Kotlin `b.` doesn't
work") turned out to be the same bug wearing a different receiver, not a
second one — see "What was done" below for how that was confirmed rather
than assumed.

### What was found

Both triggers require `completion.is_none()` before even checking whether
this frame's keystroke should open something — correct in isolation (don't
clobber an already-open popup), but the "is this popup now stale" check
that would clear it back to `None` only ran once, at the very end of
`show()`, right before painting. Sequence for "finish typing `super` then
type `.`":

1. Typing `s`/`u` opens word-completion (2+ identifier chars) — anchored
   at `s`, candidates include whatever's already in the file plus
   keywords/templates.
2. `p`/`e`/`r` extend the run; `completion` stays exactly the `Some` from
   step 1 the whole time (neither trigger re-runs once something's
   already open).
3. Typing `.`: `completion` is still `Some` at the top of this frame, so
   the dot-completion trigger's own `completion.is_none()` guard skips it
   entirely — no dot-completion resolution even attempted this frame. The
   `.` character itself still lands in the buffer normally (typing isn't
   blocked, only the *trigger check* is skipped). Only *after* that, at
   the very end of the same frame, the post-frame close check finally
   notices `"super."` matches no candidate and sets `completion = None`
   — one step too late to help the trigger that already ran.
4. Next frame, `completion` is `None`, but the `.` event from step 3 is
   gone (each frame's `ui.input().events` only holds *that* frame's
   input) — the window where both "no completion open" and "a `.` just
   arrived" hold at once has already passed. Erasing the `.` (Backspace)
   and retyping it works purely because it manufactures a fresh `.` event
   in a frame where step 3's late close has already landed.

### Why it wasn't caught by existing tests

`widget/tests/completion.rs`'s own header said so explicitly: it tests
`dot_completion_candidates`/`java_dot_completion_candidates`/
`kotlin_dot_completion_candidates` directly, "rather than through the full
`show` harness... The actual in-editor trigger (typing `.`) is verified
live in `cargo run -p foxgarden`, not here." That direct-call style proves
the *resolution* logic correct but is structurally blind to a bug that
lives entirely in *when* `show` decides to call it — exactly this bug.

### What was done

Moved the "close if the filtered list is now empty" check to run right
after this frame's edit lands, *before* either trigger block, instead of
only at the end near painting. A keystroke that both empties the current
popup's filter and should open a new one (typing `.` right after a
completed identifier) now sees `completion.is_none()` as true in time for
the dot-trigger to actually fire, in the same frame. The original
end-of-frame check was left in place too (harmless, and still the right
place to close a popup that goes stale from something between the early
check and painting, e.g. a context-menu paste).

Added `typing_session` (`widget/tests/common.rs`) — a small extension of
the existing `focused_frame`/`focused_frame_with_selection` pattern that
reuses *one* `egui::Context` across a whole sequence of frames (with
`completion`/`project` threaded through, which the existing helpers
didn't expose), since this bug only reproduces across successive frames
of the same widget, not a single one-shot call. Four regression tests in
`widget/tests/completion.rs`:
- `java_typing_super_dot_one_character_at_a_time_opens_dot_completion_immediately`
  and its Kotlin counterpart — both reproduce the reported "super." bug
  directly (typing "super." one character per frame) and fail without the
  fix (verified by temporarily disabling the early check and re-running:
  both failed with the fix removed, confirming they weren't
  accidentally-passing tests).
- `kotlin_typing_a_single_char_receiver_then_dot_opens_dot_completion` and
  `java_typing_a_single_char_receiver_then_dot_opens_dot_completion` — a
  minimal `b.` case each (single-character receiver, so word-completion's
  own 2+-char trigger never opens while typing it), isolated specifically
  to check whether the reported `b.` failures were a *separate* bug. Both
  pass with or without the fix.

A follow-up live retest confirmed these two `b.`-shaped reports (and a
separate live "Kotlin `super.` doesn't work either" report) were never
this bug at all: the user had typed the receiver directly in the class
body, outside any method — invalid Java/Kotlin syntax for a bare
expression (confirmed by a live syntax-error squiggle under it), which
breaks the resolver's tree walk for an unrelated reason. Retyping the
exact same receivers *inside* a method body worked immediately, matching
what the passing `b.` tests above already covered. Recorded here so a
future reader doesn't wonder why two live bug reports resolved to "user
error" rather than a second fix — the four regression tests above are the
complete, real coverage this bug needed.

### Trigger condition

N/A — already fixed and covered by regression tests.

---

## 14. [RESOLVED] ~~Shift+Tab with no selection was a silent no-op — "left to egui's own no-selection handling," which egui never actually implemented~~

**Where:** `crates/app/src/widgets/editor/widget.rs` (`show`, the Tab/
Shift+Tab interception block) and `crates/app/src/widgets/editor/
auto_edit.rs` (`indent_selected_lines`, already collapsed-range-capable
and unchanged by this fix).

**Status:** Fixed. Reported live as "Shift+Tab to de-indent is not
working."

### What was found

The Tab/Shift+Tab interception block explicitly branched on
`range.is_empty()`: a real selection got full indent/dedent handling
(`indent_selected_lines`), but the no-selection branch only ever fired for
plain Tab (`!ui.input(|i| i.modifiers.shift)`) — live-template expansion or
a spaces-mode indent insert. Shift+Tab with a collapsed cursor matched
neither arm and fell through to `TextEdit::show_interactive` untouched.
The block's own comment claimed this was deliberate: "Shift+Tab is left to
egui's own no-selection handling either way, same as before either feature
existed" — but egui's `TextEdit` has no such handling to fall back to
outside its `lock_focus` literal-tab-insert path (that's Tab's default
behavior, not Shift+Tab's), so the fallback was actually a silent no-op,
not a deferral to real functionality. There was even a test asserting this
non-behavior as correct:
`shift_tab_with_no_selection_is_left_to_egui_regardless_of_indent_mode`
only checked that the buffer hadn't *grown* — it never checked Shift+Tab
actually did anything, because it didn't.

### Why it wasn't caught earlier

The comment reads confidently ("is (and remains) egui's own... handling"),
and the one test guarding this path was written to match that assumption
rather than to verify real dedent behavior — a case of a plausible-sounding
inline claim never actually being checked against egui's source.

### What was done

Added a third arm — `ui.input(|i| i.modifiers.shift)` with an empty
range — that dedents just the current line via
`indent_selected_lines(&old_text, range.start, range.start, true,
indent_settings)`, the exact collapsed-range case that function was
already built to handle (its own doc comment already covers a
degenerate/single-line touched-range; no changes needed there). Replaced
the old test with `shift_tab_with_no_selection_dedents_the_current_line`
(spaces mode) and `shift_tab_with_no_selection_respects_tabs_mode` (tabs
mode), both asserting the buffer actually loses its leading indentation,
not just that it didn't grow.

### Trigger condition

N/A — already fixed and covered by regression tests.
