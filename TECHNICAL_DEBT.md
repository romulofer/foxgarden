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

## 1. Considered and rejected: moving `display_path` computation into the `Err` arm

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

## 2. `highlights_java.scm`'s `@constant` capture is dead — no `Scope` renders it

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

## 3. Kotlin's reference `highlights.scm` targets a different grammar than the one vendored here

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
