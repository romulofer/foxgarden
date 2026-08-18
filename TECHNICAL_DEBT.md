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
| 25 | `[OPEN]` | No extension/plugin architecture — every language, panel, and LSP integration is hardcoded into the app crate itself |
| 23 | `[OPEN]` | `rfd::FileDialog::pick_folder()` blocks the whole UI thread with no timeout — fixed for Settings > JDKs…, three other call sites still do it |
| 22 | `[OPEN]` | Hover tooltips paint jdtls' Markdown as literal punctuation — declaring a `PlainText` preference didn't stop it |
| 21 | `[OPEN]` | `bundled_archives_extract_with_the_launcher_at_its_documented_path` fails on any clone without Git LFS, because `include_bytes!` happily embeds the pointer file |
| 15 | `[OPEN]` | Spring endpoint map jump-to-handler doesn't land the cursor correctly |
| 3 | `[OPEN]` | Kotlin's reference `highlights.scm` targets a different grammar than the one vendored here |
| 9 | `[OPEN]` | Cross-class dot-completion offers a field regardless of its visibility, unlike methods — Java and Kotlin both |
| 10 | `[OPEN]` | `fields_in_type` can never find an interface's own constants — a second, more severe instance of #9's shape |
| 11 | `[OPEN]` | Opening a project tree aborts entirely on the first unreadable file/directory, anywhere in the tree |
| 12 | `[OPEN]` | Dot-completion's local-variable scan doesn't respect declaration order relative to the cursor |
| 5 | `[OPEN]` | Splitting `widget.rs` further |
| 6 | `[OPEN]` | `widget.rs`'s `open_fixture` test helper wraps `test_support::temp_document` instead of being replaced by it |

---

# Open

## 25. [OPEN] No extension/plugin architecture — every language, panel, and LSP integration is hardcoded into the app crate itself

**Where:** `fg_core::Language` (`crates/core/src/language.rs`, a fixed
`enum` — Java, Kotlin, Properties, Yaml, Xml, Dockerfile), `syntax`'s own
grammar registration (tree-sitter grammars compiled into the `syntax`
crate, `highlights_*.scm` files bundled as `include_str!`), `lsp_manager::
ServerKind`/`Server` (`crates/app/src/lsp_manager.rs`, an `enum` naming
exactly jdtls and kotlin-language-server, each with its own hardcoded
install/launch logic), and every panel (`crates/app/src/panels/*.rs`,
compiled Rust modules with no dynamic loading of any kind).

**Status:** Open — recorded per the user's own request while other work
was in progress; not investigated or scoped beyond this survey of where
the hardcoding actually lives.

### What was found

Nothing here is pluggable. Adding support for a new language today means
touching the `Language` enum, adding/vendoring a tree-sitter grammar and
its own `highlights_*.scm` inside the `syntax` crate, teaching
`lsp_manager` a new `ServerKind` variant with its own install/launch
shape, and wiring any language-specific panel behavior (codegen,
boilerplate, Spring-config awareness) by hand into each relevant module.
Same story for a wholly new *kind* of feature (a new panel, a new static-
analysis tool beyond Checkstyle/PMD/SpotBugs, a new external-tool
integration) — every one of those is first-party code in this repo, not
something a third party (or a future version of this project itself)
could add without a FoxGarden release. `CLAUDE.md`'s own project overview
already frames the current Java/Kotlin-only scope as a deliberate first
checkpoint toward "a full fledged... Spring Boot IDE," which makes this
gap load-bearing for that stated direction, not a hypothetical.

### Why it wasn't fixed on the spot

Not a bug — a real architecture question with several credible shapes and
no clearly-right answer yet, exactly the kind of decision that shouldn't
get picked unilaterally mid an unrelated feature session:

- **Config-driven extensibility, no dynamic loading at all.** Let a
  `.foxgarden`-style file describe an LSP server (command, args,
  install source) and a tree-sitter grammar (a `.wasm` grammar file
  tree-sitter can already load at runtime, plus a `highlights.scm`) for a
  new language without a Rust code change — cheapest to build, covers the
  two most-requested extension points (a language, an LSP), doesn't cover
  a genuinely new panel/feature.
- **WASM plugins** (`wasmtime`/`extism`-style) for real code, sandboxed,
  cross-platform, no `dlopen` ABI-stability problem — the heaviest lift,
  and this project's own stated performance bar (Zed-class startup/frame
  cost) makes the runtime cost of a WASM boundary a real design constraint
  to measure, not assume away.
- **Native Rust plugin crates** loaded via `dlopen`/`abi_stable` — fastest
  at runtime, but Rust's lack of a stable ABI makes this brittle across
  compiler versions in a way the other two options aren't; probably only
  viable if plugins are always built from source against the exact
  FoxGarden version they target.

### Proposed fix

Start with the config-driven route for languages/LSP servers specifically
— it's the smallest change that unblocks the most common real request
("FoxGarden doesn't support my language yet") without committing to a
full plugin runtime before this project has any plugins to learn from.
Revisit WASM/native plugins once there's a concrete second or third
feature (beyond language support) someone actually wants to add from
outside this repo — designing a general plugin API against zero real
consumers tends to guess wrong.

### Trigger condition

The first real request (from the user or elsewhere) to support a language
beyond Java/Kotlin, or to build a feature that doesn't obviously belong
as first-party code in this repo.

---

## 23. [OPEN] `rfd::FileDialog::pick_folder()` blocks the whole UI thread with no timeout — fixed for Settings > JDKs…, three other call sites still do it

**Where:** `crates/app/src/panels/side_panel.rs:120` ("Open Folder" 📁
button), `crates/app/src/panels/run_configs.rs:139` ("Browse…" working-dir
picker), `crates/app/src/panels/menu_bar.rs:119` (File → "Open Folder…").
Fixed at the fourth site, `crates/app/src/panels/jdk_registry.rs` ("Add
JDK…", `PLAN.md` Track 29 Phase 1) — see "What was done" below.

**Status:** Open — the underlying pattern is now proven and one site is
fixed; the other three still call `rfd::FileDialog::new().pick_folder()`
synchronously inline.

### What was found

Found live while verifying Track 29 Phase 1's own Checkpoint 1 (adding a
real JDK through Settings > JDKs…) under a real, if unusually configured,
`xdg-desktop-portal`/`xdg-desktop-portal-gtk` pair (an isolated
`dbus-run-session` bound to a private Xvfb display, set up specifically to
give `rfd`'s Linux backend — `default = ["xdg-portal", "wayland"]` in
`rfd-0.17.2`'s own `Cargo.toml`, no `gtk3` fallback compiled in — a real
portal to talk to instead of failing instantly with no dialog at all).
Clicking "Add JDK…" froze the entire window — every widget, not just the
dialog — for 20+ seconds with zero recovery (confirmed via repeated
screenshots and a failed click on the modal's own "Close" button), because
`pick_folder()` is a blocking call made directly inside the button's
`.clicked()` handler, on the same thread that runs every other frame's
`egui::Context::run`. `ps`'s `wchan` showed the process parked in `poll()`
— consistent with `pick_folder()`'s internal `pollster::block_on` waiting
on a D-Bus reply that, in that portal configuration, never arrived. No
timeout exists anywhere in the call chain: a portal that's slow, hung, or
simply never answers (a first-run permission prompt stuck behind another
window, a portal backend that crashed, exactly this session's own
non-standard setup) freezes FoxGarden **completely**, with no way out
short of `kill -9`. Not confirmed whether a normal desktop portal ever
actually stalls this way in ordinary use — but the zero-timeout,
blocking-the-only-UI-thread shape is a real gap regardless of how often a
slow portal triggers it in practice.

All four `rfd::FileDialog` call sites in the codebase share the exact same
shape — none of them back it with a thread, unlike every other
slow/blocking operation in this codebase (`git push`, LSP process spawn,
static-analysis scans, …), which all already go through a `spawn`/`poll`
pair (`crates/app/src/panels/git_stage.rs:36-59` is the canonical
example: `std::thread::spawn` + `std::sync::mpsc::channel`, drained once a
frame from `FoxGardenApp::ui`, with `ui.ctx().request_repaint()` called
every frame an op is in flight since egui's reactive repaint mode
otherwise won't pick up a background result until an unrelated input event
happens to fire the next frame).

### Why it wasn't fixed everywhere on the spot

The JDK one was in scope (found live-verifying that track's own
checkpoint) and small enough to fix immediately. The other three sites are
unrelated features (`Open Folder` is the app's single most central
action) — fixing all four in the same pass was judged riskier than
fixing the one actually in scope and recording the rest here, matching
this file's own "known-and-deferred" purpose rather than silently
expanding an unrelated track's diff.

### What was done

`crates/app/src/panels/jdk_registry.rs`: `JdkRegistryState` gained a
`picker_rx: Option<Receiver<Option<PathBuf>>>` field and a `poll_picker`
method mirroring `git_stage.rs`'s `poll_op` shape exactly. The "Add JDK…"
button now spawns `rfd::FileDialog::new().pick_folder()` on a
`std::thread::spawn`, is disabled (`ui.add_enabled`) while a pick is in
flight, and `show_settings` polls once a frame and calls
`ui.ctx().request_repaint()` while running. Live-verified against the same
non-responding portal that produced the original hang: the button now
visibly disables on click and — critically — "Close" and every other
widget stay responsive immediately, even with the picker thread itself
still parked forever waiting on a portal that never replies. The stray
thread in that specific (still-not-understood) portal configuration never
returns and is never joined, but it's an isolated OS thread with no
handle back into the UI, so it costs nothing beyond the thread itself
sitting idle in `poll()`.

### Proposed fix

Apply the identical `spawn`/`poll` pair to `side_panel.rs`,
`run_configs.rs`, and `menu_bar.rs`'s own `pick_folder()`/`pick_file()`
call sites. Given all four sites need the exact same few lines, consider
factoring a tiny shared `spawn_folder_pick() -> Receiver<Option<PathBuf>>`
helper (and a `poll` twin) into a small module instead of copy-pasting the
pair a fourth time — a scope call for whoever picks this up, not required.

### Trigger condition

Next time any of the three remaining sites is touched for an unrelated
reason, or a user reports FoxGarden hanging on "Open Folder…"/"Browse…".

---

## 22. [OPEN] Hover tooltips paint jdtls' Markdown as literal punctuation — declaring a `PlainText` preference didn't stop it

**Where:** `crates/app/src/widgets/editor/hover.rs`
(`hover_text_from_response`, `HoverState::paint`) and
`crates/app/src/lsp_state.rs` (`initialize_params`' own
`hover.content_format`).

**Status:** Open. Found while closing #20 — the same real-jdtls run that
proved hover content resolves correctly also showed *what* that content
looks like.

### What was found

`initialize_params` declares `content_format: [PlainText, Markdown]`,
which the protocol defines as a client *preference*, not a constraint —
and a real jdtls 1.60.0 ignores it for the legacy `MarkedString` reply
shape. Hovering `String` came back as a two-element array: a
`{language: "java", value: "java.lang.String"}` code element plus a
second element whose text is unmistakably Markdown — backtick-quoted
identifiers, `>`-indented code blocks, `*  **Since:**` bullet lists, and
full `[Character](jdt://contents/java.base/java.lang/Character.class?=…)`
links whose URLs run to several hundred characters each.

`HoverState::paint` renders that through a plain `ui.label`, so every one
of those markers paints as literal text. For a JDK type the result is a
tooltip dominated by `jdt://` URLs rather than by the documentation the
user hovered for. A project-owned symbol's own short Javadoc (#20's
fixture) renders acceptably, which is why this didn't surface earlier.

### Why it wasn't fixed on the spot

Two credible fixes, and picking between them is a real design call rather
than an obvious cleanup: render the Markdown for real (a dependency —
`egui_commonmark` — against a project whose stated bar is Zed-class
startup and frame cost), or strip it to readable plain text in
`hover_text_from_response` (no dependency, but hand-rolling even a small
Markdown subset is exactly the kind of thing that grows). Both are
larger than the debt-closing pass that found this.

### Proposed fix

Prefer the stripping route first, scoped tightly to what jdtls and
`kotlin-language-server` actually emit (confirmed above, not guessed):
unwrap inline code spans, drop link targets while keeping link text,
convert `>`-indented blocks and `*` bullets to plain indentation. Keep it
in `hover_text_from_response` so it's covered by that function's existing
unit-test shape, with the real captured jdtls reply as a fixture. Only
reach for a Markdown renderer if that proves insufficient in practice.

### Trigger condition

Next time hover docs are touched, or the first time a user reports
tooltips full of `jdt://` links.

---

## 21. [OPEN] `bundled_archives_extract_with_the_launcher_at_its_documented_path` fails on any clone without Git LFS, because `include_bytes!` happily embeds the pointer file

**Where:** `crates/app/src/lsp_manager.rs` (`JDTLS_ARCHIVE`/
`KOTLIN_LANGUAGE_SERVER_ARCHIVE`'s own `include_bytes!`, and the test of
the same name), plus `vendor/lsp-servers/` and `.gitattributes`.

**Status:** Open. Found as the one failing test in an otherwise-green
`cargo test --workspace` at the start of a later session.

### What was found

```
extracts: "failed to extract archive: failed to iterate over archive"
```

`vendor/lsp-servers/*.tar.gz` and `*.zip` are Git LFS-tracked. On a
machine without `git-lfs` installed (`git: 'lfs' is not a git command`),
checkout leaves the 133-byte pointer file in place — `version
https://git-lfs.github.com/spec/v1`, an `oid`, a `size` — and there is no
`.git/lfs` object store at all, so the real bytes aren't recoverable
locally either.

The failing test is the mild half of the problem. `include_bytes!` has no
idea it's embedding a pointer instead of an archive, so a release binary
built from such a clone ships with both bundled installers silently
broken; the failure only surfaces at the moment a user clicks Install,
as an extraction error naming neither LFS nor the pointer.

### Why it wasn't fixed on the spot

The session that found it was scoped to closing the LSP debts (#17/#18/
#20), and the fix isn't a one-liner: it's a choice between requiring
`git-lfs` as a documented build prerequisite (with a `build.rs` check
that fails the build with a readable message when the vendored file is a
pointer), dropping LFS in favor of downloading at install time again, or
not vendoring at all. That's a build/distribution decision, not a code
cleanup.

### Proposed fix

A `build.rs` check is the cheap, high-value half regardless of which
distribution route wins: a vendored archive that starts with `version
https://git-lfs.github.com/spec/v1` should fail the build with "run `git
lfs install && git lfs pull`", so the problem surfaces at build time with
its own remedy attached instead of as a broken Install button. Whether
LFS stays at all is the separate, larger question.

### Trigger condition

Any time `cargo test --workspace` is expected green on a fresh clone, or
before cutting any release binary that users will click Install in.

---


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

## 15. [OPEN] Spring endpoint map jump-to-handler doesn't land the cursor correctly

**Where:** `crates/app/src/widgets/editor/widget.rs` (`jump_to`),
`crates/app/src/panels/tabs.rs` (the `jump_to_char` scroll block in
`show`), `crates/app/src/app.rs` (`resolve_pending_navigation`,
`PLAN.md` Phase 4's whole call chain).

**Status:** Open. Reported live on a real project (`~/bridge/pec`) after
Phase 4 landed: picking an endpoint from the popup does *not* land the
cursor correctly. Two other bugs found in the same live-testing round
(the query box losing keyboard focus; the endpoint scan re-running in
full on every popup open) were root-caused and fixed in the same session
— see `PLAN.md`'s own Phase 4 entry and the commits landing it. This one
was deliberately not chased further in the same session; recorded here
per explicit direction to move on rather than keep debugging blind.

### What was found

Only that the symptom is real and survived both other fixes — "the
cursor still does not land correctly" was the exact report, with no
further detail captured yet (which file, which endpoint, off by how
much, whether it's the cursor's *position* or the viewport's *scroll*
that's wrong, whether word-wrap was on). Nothing below is confirmed root
cause; these are the candidates worth checking first, in rough order of
suspicion given what's already known about this code:

1. **The scroll-target math is a known, explicitly-flagged approximation.**
   `tabs.rs`'s `jump_to_char` block computes the target row as `doc.buffer.
   char_to_line(char_offset)` directly — i.e. it assumes one logical line
   equals one visual row, which is only exactly true with word-wrap off
   *and* nothing currently folded above the target line. `ViewSettings::
   word_wrap` defaults to `true` in this app (`crates/app/src/style/
   view.rs`), so the common case is exactly the one this approximation is
   weakest in. `widget.rs`'s own doc comment on this already calls it out
   as "close enough in the common case... cheap to verify live rather than
   assume needs the fuller treatment" — this report is that verification
   coming back negative. This would explain a *scroll* landing wrong (the
   handler's line off-screen or far from centered) but not a wrong *caret
   column/line* once you scroll to find it.
2. **Something in the caret's own byte→char conversion or the scanned
   `handler_byte` itself.** `resolve_pending_navigation` converts via
   `doc.buffer.byte_to_char(byte)` (ropey's own conversion, not hand-rolled)
   against the *currently open* document's buffer — if the popup's scan
   read the file at a different moment than the buffer the caret gets
   applied to (e.g. the file changed on disk between the scan and the
   jump, or the cached scan result is stale in some case
   `EndpointCache`'s mtime check doesn't actually catch), the byte offset
   could point at the wrong place in what's now a different buffer. Worth
   checking directly: does the reported file have unsaved edits, or did it
   change on disk recently?
3. **Focus/scroll timing interaction.** `jump_to` requests focus
   immediately (same frame, via `ctx.memory_mut`), but the scroll
   (`tabs.rs`) and the caret application itself (`text_area::set_caret`,
   inside `jump_to`) are on different effective timelines — the caret
   move takes effect next frame (same as every other `manual_caret` use
   in this file), the scroll's `ui.scroll_to_rect` call also only
   visually resolves next frame. If these two "next frame"s aren't
   actually the same frame for some reason, the caret could momentarily
   render in the pre-jump scroll position, or vice versa, and whichever
   was actually observed live might have been a transient rather than the
   final state.

### Why it wasn't fixed immediately

The report came with no reproduction detail, and this entry's own
candidate list shows the fix would be different depending on which of
(at least) three unrelated mechanisms is actually at fault — guessing
which one and patching it blind risks fixing a mechanism that was never
broken while leaving the real bug in place. Explicit direction was to
record this and move on (to Phase 5, a fully independent track) rather
than keep debugging without better information.

### Proposed fix

Next time this is picked up, get a precise repro first:
- Which file, which endpoint, and whether word-wrap was on or off for
  that live test.
- Whether the *cursor column/line* is wrong, the *scroll position* is
  wrong, or both — these point at different candidates above.
- If it's the scroll: temporarily hardcode `word_wrap` off and retest: if
  the jump lands correctly with wrap off, candidate 1 is confirmed, and
  the real fix is either accepting the approximation only for the no-wrap
  case (and finding some correct-enough fallback for wrapped mode) or
  building the fuller wrap-aware row lookup `widget.rs`'s own doc comment
  already gestures at as the alternative not yet built (reusing `render.
  rs`'s per-line row-count shaping, exposed for a one-off external query
  rather than only its own internal, visible-window-only cache).
- If it's the caret position itself even after confirming scroll is
  fine: add a temporary debug print of the byte offset `EndpointInfo`
  carries, the char offset `resolve_pending_navigation` computes, and
  what character is actually at that char offset in the live buffer —
  compare against where the cursor visually lands.

### Trigger condition

Next time the Spring endpoint map's jump-to-handler is revisited, or a
user provides a precise repro (file + endpoint + word-wrap setting) for
this exact symptom.
