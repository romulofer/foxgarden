# PLAN.md

Execution plan for `SPEC.md`'s code-completion feature: a popup that opens
while typing and offers suggestions, with dot-triggered member completion
(`someObject.` → that object's fields/methods) as the headline case, on top
of an always-on word-completion fallback. Fully replaces whatever this file
covered before (the previous optimization-review pass — see git history/
`TECHNICAL_DEBT.md` if any of its items need to survive; nothing here is a
continuation of that work). Local-only planning doc (gitignored, same as
`FEATURES.md`/`SPEC.md`) — a commitment to an *order*, not a timeline.

**Heads up, not part of this plan:** the previous `PLAN.md`'s Build status
showed **Phase 1 (§1 undo-history clone, §2 double stringification) still
unchecked** when this file was overwritten for this new feature. That item
isn't tracked in `TECHNICAL_DEBT.md` and isn't repeated here — if it still
matters, log it there first (or note it back into a future `SPEC.md` pass)
before it's lost for good, since this file's own header says it's fully
disposable.

`SPEC.md` treats Java and Kotlin as equally in-scope (see its own header
note — an earlier draft deferred Kotlin dot-completion as a follow-on; that
call's been reversed). This plan reflects that: Phases 2 and 3 each carry a
Java sub-phase and a Kotlin sub-phase side by side, not a separate deferred
track. Java is sequenced first within each only because its groundwork
(`enclosing_class`, `superclass_name`, `find_java_file_by_stem`,
`fields_in_class_body`, `methods_in_type`) already exists and is proven by
"Override Method" — not because Kotlin matters less. Kotlin's own sub-phases
follow immediately, reusing the same shape once it's proven out on Java,
plus the grammar-verification step `TECHNICAL_DEBT.md` #3 already
established as mandatory for any new Kotlin tree-sitter work.

Dependency graph:

```
Phase 0  ── popup shell (Area, position, keyboard nav, lifecycle)
   │        [language-agnostic]
   │
   ├─► Phase 1  Word-completion end to end (ships a usable feature alone)
   │        [language-agnostic — already covers .java and .kt]
   │
   ├─► Phase 2  Receiver-type resolution
   │       ├─ 2a Java    (this./super./local/param/field)
   │       └─ 2b Kotlin  (this./super./local/param/property, incl. the
   │                      val-type-inference heuristic and grammar
   │                      verification against node-types.json)
   │       │
   │       └─► Phase 3  Cross-project member lookup + wiring
   │               ├─ 3a generalize the file-finder (shared by both)
   │               ├─ 3b Java wiring
   │               └─ 3c Kotlin wiring (new member-extraction functions —
   │                                    no existing fields.rs/methods.rs
   │                                    equivalent to build on)
   │               │
   │               └─► Phase 4  Method-aware insertion ( () / cursor placement )
   │                       [language-agnostic — operates on CompletionItem,
   │                        not on which language produced it]
```

Each phase ends at a **green checkpoint**: `cargo build --workspace`,
`cargo test --workspace`, `cargo clippy --workspace --all-targets` all pass,
and — since this feature lives entirely on `widget.rs`'s per-frame input
path — a live click-through in `cargo run -p foxgarden` for anything the phase
touches, per `AGENTS.md`'s "frame-driven code is proven live, not merged
blind" testing convention. Phase 1 in particular ships something real (word
completion, keyboard-navigable, working in every file type) and is a
reasonable place to pause if the rest doesn't happen in one sitting.

---

## Phase 0 — Popup shell

Nothing about candidate sources yet — just the reusable mechanism `SPEC.md`
§0 describes, built and tested against a hand-fed fixed candidate list so
it's provably correct before Phase 1 wires a real source into it.

- **0a.** `CompletionState`/`CompletionItem`/`CompletionKind` (`SPEC.md` §0)
  — new module, likely `crates/app/src/widgets/editor/completion.rs`
  (sibling to `templates.rs`/`multi_cursor.rs`, same "one file per
  identifiable concern" rule `AGENTS.md`'s architecture section already
  states).
- **0b.** The `egui::Area` positioning against `TextAreaOutput::char_rect`,
  anchored one row below the caret. Verify it renders in the right place at
  the start, middle, and end of a wrapped line, and doesn't run off the
  right/bottom edge of the editor pane (clamp position if needed — check
  whether `go_to_file.rs`/`quick_switcher.rs` already had to solve an
  off-screen case worth mirroring, or whether this is new).
- **0c.** Lifecycle: open/filter/close/accept exactly as `SPEC.md` §0
  specifies, plus the "consume Enter/Tab/arrows before `text_area::
  show_interactive`" interception, same shape as the existing Tab/
  live-template block (`widget.rs:586-610`).
- **0d.** `filter_and_rank` (`SPEC.md` §2) against the hand-fed fixture list
  — table-tested standalone before anything real feeds it.

**Checkpoint 0:** a debug-only "always populate with 3 dummy candidates on
`Ctrl+Space`" hook (deleted before Phase 1, or left as an internal-only test
seam if convenient) proves open/filter/navigate/accept/dismiss all work
live, before any real candidate source exists to blame for a bug in the
shell itself.

---

## Phase 1 — Word-completion end to end

Ships the first genuinely usable version of this feature: works in every
file type (`.java`, `.kt`, `.properties`, YAML, XML, plain text — same
"language-agnostic" scope multi-cursor and auto-pair already have), no
tree-sitter dependency, no project-tree dependency.

- **1a.** `identifiers_in(text: &str) -> Vec<String>` (`SPEC.md` §1) — pure
  function, table-tested standalone.
- **1b.** Wire the `.`-free trigger: 2+ identifier chars typed, or
  `Ctrl+Space` forced — into `widget.rs`'s interception chain, opening
  `CompletionState` with `identifiers_in(&text)` (deduped, current run
  excluded) as candidates.
- **1c.** Fold in live-template triggers and the current language's keyword
  list as additional candidates (`SPEC.md` §1, "Also includes") —
  `CompletionKind::Template` acceptance calls `templates::expand` exactly as
  the existing Tab-trigger path already does; this phase doesn't touch that
  function, only adds a second caller.
- **1d.** `insert_completion` (`SPEC.md` §5) for the `Word`/`Keyword`
  candidate kinds only — plain prefix-replace, no paren-insertion logic yet
  (that's Phase 4, gated on method candidates existing at all).

**Checkpoint 1:** full suite green; live-verify in a large-ish real file:
typing 2+ letters opens the popup with plausible candidates, arrow keys +
Enter accepts and replaces the right span, Escape/moving the cursor away
closes it, `Ctrl+Space` force-opens on a 1-character or already-typed word,
a live template still expands correctly both via Tab (unchanged path) and
via picking it from the popup (new path).

---

## Phase 2 — Receiver-type resolution

Purely additive to `crates/syntax` — no `widget.rs`/UI wiring yet, so this
whole phase (both sub-phases) is provable in complete isolation via `cargo
test -p syntax` before anything user-visible depends on it.

### 2a. Java

- New `crates/syntax/src/completion.rs`. `type_of_identifier_java`
  (`SPEC.md` §3a) — parameter lookup, then local-variable lookup (whole
  enclosing method, the documented first-cut simplification, not a real
  scope-ordered scan), then falls through to the enclosing class's own
  fields via `fields_in_class_body`.
- `this.`/`super.` handling — reuses `syntax::enclosing_class`
  (`methods.rs:21-33`) and `syntax::superclass_name` (`methods.rs:68-80`)
  directly, no new resolution logic, just the routing `SPEC.md` §3
  describes.
- Table tests per `SPEC.md` §3a's list: parameter resolves, local resolves,
  field resolves, a local shadowing a same-named field resolves to the
  local, an unresolvable name (JDK-typed local, undeclared name) returns
  `None`.

### 2b. Kotlin

- **First**, independent of any code: confirm every node kind/shape
  `SPEC.md` §3b cites (`property_declaration`, `variable_declaration`,
  `class_parameter`, `parameter`, `delegation_specifiers`) against
  `tree-sitter-kotlin-ng`'s own `node-types.json` fresh — `SPEC.md` already
  did this once while writing the spec, but re-verify rather than trusting
  a doc, same "check node-types.json yourself" discipline
  `TECHNICAL_DEBT.md` #3 established the hard way.
- `type_of_identifier_kotlin` (`SPEC.md` §3b) — parameter lookup (function
  parameters only, always explicitly typed), then primary-constructor
  `class_parameter` lookup, then local `property_declaration` lookup, then
  class-body `property_declaration` lookup. At each of the two
  `property_declaration`-based steps: explicit `type` child first, then the
  constructor-call-heuristic fallback for an inferred type — implement and
  test these as two clearly separate branches, not one merged heuristic, so
  the explicit-type case can never accidentally go through the "looks like
  a constructor call" guess.
- `this.`/`super.` handling — same `enclosing_class`-shaped walk-up as
  Java, adapted to `class_declaration`'s own shape; `super.` resolves via
  `delegation_specifiers`' first entry (§3b), a genuinely different lookup
  from Java's `superclass`/`interfaces` field split, not a reusable one.
- Table tests per `SPEC.md` §3b's list — including the two inferred-type
  cases (constructor-call heuristic resolves; anything else returns
  `None`, not a wrong guess) and the constructor-promoted `class_parameter`
  case.
- Shared `type_of_identifier(language, ...)` dispatcher (`SPEC.md` §3,
  "Shared entry point") — the only entry point `widget.rs` actually calls.

**Checkpoint 2:** `cargo test -p syntax` green for both sub-phases; no live
click-through needed yet (nothing in the running app calls this module
until Phase 3).

---

## Phase 3 — Cross-project member lookup + wiring

Connects Phase 2's resolved type name to an actual candidate list and into
the running popup.

### 3a. Generalize the file finder (do first, shared by both languages)

- Generalize `codegen::find_java_file_by_stem` into
  `find_source_file_by_stem(node, stem, extension)` (`SPEC.md` §4);
  `find_java_file_by_stem` becomes a one-line wrapper so Override Method's
  existing call site doesn't change behavior at all. Re-run the existing
  Override Method tests to confirm that wrapper is truly behavior-identical
  before building anything new on top of it.

### 3b. Java wiring

The phase most likely to surface a real design question (`SPEC.md` §4's
flagged uncertainty about `methods_in_type`'s current visibility
filtering) rather than being purely mechanical.

- Read `methods.rs`'s current `methods_in_type` filtering logic fresh
  (don't assume `SPEC.md` §4's description of it is still exactly right)
  and decide, concretely: does `this.`/`super.` need an unfiltered variant,
  or does the existing filtering already happen to be permissive enough?
  Resolve this before writing two call sites that might not need to differ
  after all.
- `fields.rs`: add the static-inclusive variant `SPEC.md` §4 describes (new
  function or an `include_static: bool` parameter — pick whichever is the
  smaller diff once looking at the one existing call site), so completion
  can offer a class's constants.
- Wire `widget.rs`'s `.`-typed trigger for Java: resolve the receiver via
  Phase 2a's `type_of_identifier_java` (or `this`/`super`'s direct
  routing), then — for a resolved non-`this`/`super` type —
  `find_source_file_by_stem` + read + throwaway-parse + member extraction,
  the exact sequence already proven at `widget.rs:1272-1306` for Override
  Method, called with the resolved type name instead of a supertype name.
  One supertype level included via the same `superclass_name` +
  `find_source_file_by_stem` chain.
- Every unresolvable case (§3's case 3: JDK type, multi-hop chain, anything
  else) is a silent no-op — dot-completion just doesn't open, and
  word-completion's own trigger (Phase 1) still applies once enough
  identifier characters follow. Verify this explicitly (type `s.` where `s`
  is a `String` local) rather than assuming it falls through cleanly.

**Checkpoint 3b:** full suite green; live-verify with a small two-class
Java project fixture (mirroring `SPEC.md` §4's test description): `this.`
inside a class offers its own fields/methods, a local variable typed as
another in-project class offers that class's members, a `String`-typed
local produces no dot-completion popup (falls through to nothing, not an
error), inherited members from a one-level supertype are included.

### 3c. Kotlin wiring

No existing `fields.rs`/`methods.rs` equivalent to lean on — this is new
extraction logic, not just a new call site.

- `kotlin_properties_in_class_body`/`kotlin_functions_in_type` (`SPEC.md`
  §4, "Member extraction — Kotlin") — walking `class_body`'s
  `class_member_declaration` children, plus the primary constructor's
  `class_parameters` for constructor-promoted `val`/`var` properties.
  Producing the same `FieldInfo`/`MethodSignature` structs Java's side
  already uses — no new candidate-item shape needed downstream.
- Visibility filtering for `kotlin_functions_in_type`, mirroring whatever
  3b settled on for `methods_in_type` — verify Kotlin's modifier-token
  shape against `node-types.json` rather than assuming it matches Java's
  anonymous-token pattern exactly.
- Wire `widget.rs`'s `.`-typed trigger for `.kt` files: same shape as 3b,
  `type_of_identifier_kotlin` in, `find_source_file_by_stem(node, stem,
  "kt")` + read + throwaway-parse + the new Kotlin member-extraction
  functions out.
- Every unresolvable case (§3b's inference gaps, JDK/stdlib types, `data
  class` synthesized members — `SPEC.md` §6) is a silent no-op, same as
  Java — verify explicitly rather than assuming the fallback path is
  reached cleanly.

**Checkpoint 3c:** full suite green; live-verify with a small two-file
Kotlin project fixture: `this.` inside a class offers its own
properties/functions, a local `val x: Foo = ...` (explicit type) and a
`val x = Foo()` (inferred via the constructor-call heuristic) both offer
`Foo`'s members, a `val x = someFunction()` (non-constructor-call
initializer) produces no dot-completion popup, a constructor-promoted
`class Foo(val x: Int)` parameter shows up as a member when accessed via
`this.`, and a Kotlin stdlib-typed local (`val s: String = ...`) produces
no dot-completion popup either (falls through to word-completion, not an
error) — the same non-goal `SPEC.md` §7 states for Java's JDK types,
confirmed on the Kotlin side too.

---

## Phase 4 — Method-aware insertion

Small, final polish phase — only meaningful once Phase 3 can actually
produce `CompletionKind::Method` candidates. Shared by both languages: it
operates on `CompletionItem`/`CompletionKind`, which don't carry which
language produced them — no Java/Kotlin split needed here at all.

- **4a.** Extend `insert_completion` (`SPEC.md` §5) with the `()`-insertion
  and cursor-placement rule for `Method` candidates — zero-arg vs.
  has-args cursor position, mirroring `templates::expand`'s marker
  convention rather than a new mechanism.
- **4b.** Table tests: zero-arg method places cursor after `()`, a
  parameterized method places it between the parens.

**Checkpoint 4:** full suite green; live-verify picking a method candidate
from the popup lands the cursor where a follow-up keystroke would expect
it (able to immediately start typing the first argument) — check this once
against a Java-resolved method candidate and once against a Kotlin-resolved
one, since Phase 3 is the first point their candidates actually flow
through the same code.

---

## Ordering notes

- **Phase 0 is a hard prerequisite for everything else** — there is exactly
  one popup shell, built once, fed different candidate sources afterward.
- **Phase 1 is the right place to stop if only shipping a subset** — it's a
  complete, usable feature on its own (real editors without a language
  server ship exactly this), already covers both `.java` and `.kt` files,
  and de-risks the popup-UI plumbing before Phases 2–3 layer semantic
  resolution on top of it.
- **Phases 2 and 3 are sequential, not parallel, within each language** —
  Phase 3's cross-project lookup needs Phase 2's resolved type name to look
  up in the first place. Java and Kotlin's own sub-phases (2a/2b, 3b/3c)
  don't depend on each other, though, and could run in either order or
  genuinely in parallel across two sessions if that's more convenient —
  they only share Phase 3a's generalized file-finder as a common
  prerequisite.
- **Java is sequenced first within Phases 2/3 for reuse, not priority** —
  its groundwork (`enclosing_class`, `superclass_name`,
  `find_java_file_by_stem`, `fields_in_class_body`, `methods_in_type`)
  already exists and is proven; Kotlin needs the grammar-verification step
  first (2b's opening bullet), which is genuinely new work regardless of
  order. If Kotlin parity matters more for a given session, do 2b/3c first
  — nothing about this plan requires Java to land before Kotlin starts.
- **If only shipping one language's dot-completion, that's still a
  complete, honest feature** — §6/§7 of `SPEC.md` already document exactly
  where each language's coverage stops (JDK/stdlib types, Kotlin's
  inference gaps and synthesized members) and everything degrades to
  word-completion rather than erroring, so partial-language coverage isn't
  a broken half-feature, just a smaller one.
- **Phase 4 is the one place it's fine to stop with a slightly rough edge**
  — without it, picking a method candidate just inserts its bare name with
  no parens, functional but not polished; not worth blocking the rest of
  the feature on.

---

## Build status (live)

- [x] Phase 0 — popup shell
- [ ] Phase 1 — word-completion end to end
- [ ] Phase 2a — Java receiver-type resolution
- [ ] Phase 2b — Kotlin receiver-type resolution
- [ ] Phase 3a — generalized file-finder
- [ ] Phase 3b — Java cross-project member lookup + wiring
- [ ] Phase 3c — Kotlin cross-project member lookup + wiring
- [ ] Phase 4 — method-aware insertion
