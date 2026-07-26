# SPEC.md

Implementation spec for **code completion**: a suggestion popup that opens
while typing and offers identifiers to insert, most importantly the
IntelliJ/VSCode-standard case the user actually asked for — type
`someObject.` and get a list of that object's members, not just "every word
seen in the buffer." This file is local-only (gitignored, same as
`FEATURES.md`/`PLAN.md`) — a working design doc, not project documentation,
and it fully replaces whatever this file covered before (see `PLAN.md`'s own
note on the same). `TECHNICAL_DEBT.md` stays the durable, git-tracked log of
deferred issues; nothing here is meant to persist past this feature landing
except by being promoted there explicitly.

**Scope relative to `FEATURES.md`:** `FEATURES.md`'s "Major undertakings"
tier lists **LSP integration** — real semantic autocomplete, go-to-definition,
find-references, hover docs, backed by `jdtls`/`kotlin-language-server` — as
an explicit `SKIP`, because it needs a JSON-RPC client and per-language
server process management this codebase doesn't have yet. This spec is
**not** that. It's the same kind of no-LSP, tree-sitter-only, project-local
heuristic analysis this codebase already ships for "Override Method"
(`syntax::enclosing_class`/`superclass_name` + `codegen::find_java_file_by_stem`
— cross-file lookup restricted to `.java` sources already inside the open
project tree, explicitly *not* resolving JDK/library types) and Java
getters/setters generation (`syntax::fields::java_classes_with_fields`).
Dot-completion here reuses and extends that exact mechanism rather than
building a second one: resolve a receiver's *declared* type only when that
type has a source file somewhere in the project, walk one supertype level
the same way Override Method already does, and degrade gracefully (fall back
to word-completion, never an error) for anything a classpath would be needed
for. Real cross-classpath semantic completion is still gated on the
Maven/Gradle awareness + LSP integration entries in `FEATURES.md`'s
major-undertakings tier — this spec doesn't shortcut that, it just makes the
in-project-only slice usable today, the same tradeoff Override Method already
made and shipped.

Every code citation below was traced to an actual file:line as of this
writing — verify before trusting a specific detail long after this was
written, same caution every other doc in this repo's set already asks for.

**Java and Kotlin are both first-class in this pass.** An earlier draft of
this file scoped Kotlin dot-completion out entirely as a follow-on; that's
been reconsidered — `CLAUDE.md`'s own project overview names both languages
equally, and completion that only worked for one of them would be a real
gap, not a reasonable first cut. §3/§4 below give Java and Kotlin parallel
(not identical — the grammars differ too much for that, see below)
subsections. Word-completion (§1) and the popup shell (§0) were already
fully language-agnostic and need no change for this. Kotlin's own grammar
node shapes were checked directly against
`tree-sitter-kotlin-ng-1.1.0`'s own `node-types.json`
(`~/.cargo/registry/src/index.crates.io-*/tree-sitter-kotlin-ng-1.1.0/src/node-types.json`)
for this revision, same "never assume a node name carries over" discipline
`TECHNICAL_DEBT.md` #3 already established the hard way for Kotlin
highlighting — cited node names below (`property_declaration`,
`variable_declaration`, `class_parameter`, `navigation_expression`, etc.)
are real, verified node kinds in that grammar, not guesses or ported-over
Java/fwcd-grammar names.

---

## 0. Cross-cutting shared building block: the floating completion popup

**Why shared:** every trigger path below (§1 word-completion, §3/§4
dot-completion) ends at the same UI — a small floating list anchored under
the caret, filtered as more characters are typed, navigable with arrow
keys, closed on Escape/click-elsewhere/losing its trigger condition,
accepted with Enter/Tab/click. Building this once and feeding it different
candidate sources keeps the two completion kinds from drifting into two
different popups with subtly different keybindings.

**Not `egui::Modal`:** every existing picker in this codebase
(`go_to_file.rs`'s `egui::Modal::new(...)`, `quick_switcher.rs`,
`widgets/modal.rs`'s `show_modal`) is a centered, input-blocking dialog —
right for "stop and choose a file," wrong for a completion popup, which has
to sit anchored at the caret while the editor underneath keeps receiving
keystrokes (typing must keep landing in the buffer and re-filtering the
list, not get eaten by a modal). Use a plain `egui::Area::new(id)
.fixed_pos(pos).order(egui::Order::Foreground)`, positioned from
`TextAreaOutput::char_rect` (`crates/app/src/widgets/editor/text_area/render.rs:80`)
at the caret's *current* char offset — the same rect `painting.rs`'s
diagnostic squiggles and hover tooltips already read off for anchoring their
own overlays (`crates/app/src/widgets/editor/painting.rs:76-93`), one row
below instead of at it.

**New state, owned alongside the other per-tab editor state** (same
pattern `override_method_dialog: &mut Option<OverrideMethodDialog>` and
`generate_dialog` already use — threaded into `widgets::editor::show` as
another parameter, per-tab, not global):

```rust
pub struct CompletionState {
    /// Byte offset of the trigger point (right after the `.`, or right
    /// after the word-start for a bare word-completion) — candidates are
    /// filtered against the buffer slice from here to the current cursor.
    anchor_byte: usize,
    candidates: Vec<CompletionItem>,
    selected: usize,
}

pub struct CompletionItem {
    pub label: String,       // what's shown and what gets inserted
    pub kind: CompletionKind, // Word | Field | Method | Keyword | Template
    pub detail: Option<String>, // e.g. "int" for a field, "(String) -> void" for a method
}
```

`CompletionState` lives as `Option<CompletionState>` — `None` means closed,
same `Option<Dialog>` convention every other picker in this codebase
(`GenerateAccessorsDialog`, `GenerateMethodDialog`, `OverrideMethodDialog`)
already uses, for the same reason: a picker that's rarely open doesn't need
a permanently-allocated field, and "is it open" and "what's its state"
collapse into one check (`is_some()`) instead of two.

**Lifecycle, driven from inside `widgets::editor::show` (`widget.rs`)**,
checked every frame the popup is open (this is the one part of this feature
that *does* run every frame while active — bounded to "popup is open,"
which is rare and short-lived, same cost class already accepted for
`context_menu.rs`'s per-frame-while-open clipboard read before its own
fix, not a per-keystroke or per-idle-frame cost):

- **Opens** on the trigger conditions in §1/§3.
- **Filters**: recomputed whenever the buffer changes while open, by
  re-slicing `text[anchor_byte..cursor_byte]` and keeping only candidates
  whose `label` starts with that slice (case-insensitive prefix; see §5 for
  the exact ranking).
- **Closes** (sets state back to `None`) on: `Escape`; the cursor moving
  outside `[anchor_byte, cursor_byte]` (e.g. an arrow key, a click
  elsewhere) — same "moved off the thing being edited" signal the passive
  occurrence-highlight feature already uses; the filtered candidate list
  becoming empty after a keystroke (nothing left to suggest); or the popup
  losing whatever triggered it (backspacing past the `.` for dot-completion,
  covered in §3).
- **Accepts** (applies §6's insertion, then closes) on: `Enter`, `Tab`, or a
  click on a row. `ArrowUp`/`ArrowDown` move `selected` (clamped, same
  `saturating_sub`/`min(len - 1)` shape `go_to_file.rs:134-139` already
  uses) instead of leaving the editor.
- While open, `Enter`/`Tab`/`ArrowUp`/`ArrowDown`/`Escape` must be consumed
  before `text_area::show_interactive`'s own handling sees them (same
  "intercept before the widget's own default" shape `is_mutating_event`/
  `strip_mutating_events` already establish for read-only mode, and the
  live-template Tab interception at `widget.rs:586-610` already establish
  for Tab specifically) — otherwise Enter would insert a newline *and*
  accept a completion in the same keystroke.

**Tests:** a `widget/tests.rs` case per lifecycle transition (open on `.`,
filter narrows on further typing, Escape closes, moving the cursor off the
anchor closes, Enter both inserts and closes) — same `egui::__run_test_ui`+
`focused_frame` headless-frame pattern the rest of this module's tests
already use (`AGENTS.md`'s testing-conventions section), not a real
window/click-automation test.

---

## 1. Trigger: word-completion (always-on fallback, language-agnostic)

**Where:** new interception block in `widget.rs`, same family as the
existing Tab/live-template block (`widget.rs:586-610`), keyed off
`Event::Text` insertions rather than `Key::Tab`.

**Trigger:** after any alphanumeric/`_` character is typed such that the run
of identifier characters ending at the cursor is at least 2 characters long
(1 character would open on every single keystroke with almost no filtering
value yet — the same reasoning `go_to_file.rs`'s empty-query-matches-
everything case exists for, just inverted: don't bother showing a list
that's indistinguishable from "everything"). Reuses
`templates::word_before_cursor` (`crates/app/src/widgets/editor/templates.rs:52-60`)
verbatim to find the run — it already does exactly "the maximal identifier
run ending at the cursor," which is also the anchor/prefix this feature
needs. Also opens explicitly on `Ctrl+Space` regardless of run length (the
conventional "force it open" IDE shortcut, for when a user wants suggestions
after typing just one character or after moving the cursor back into an
existing word).

**Candidate source:** every distinct identifier-shaped token in the current
buffer (`text.split(|c: char| !c.is_alphanumeric() && c != '_')`, matching
the same character class `word_before_cursor` already uses, deduplicated),
excluding the run currently being typed itself. This is the "any editor
without a language server" baseline (Vim's `Ctrl+N`, VSCode's word-based
suggestions) — cheap, always available, and a real fallback for the cases
§3/§4 explicitly can't resolve (JDK types, Kotlin, non-Java/Kotlin files).
Recomputed on open, not cached across keystrokes — a single
`text.split(...)` pass over one already-open buffer is not in the same cost
class as the per-frame recomputation `SPEC.md`'s (superseded) optimization
pass was concerned with; only runs when the popup is actually open/being
retyped into, not every idle frame.

**Also includes:** the current language's live-template triggers
(`templates::JAVA_TEMPLATES`/`KOTLIN_TEMPLATES`) as `CompletionKind::
Template` candidates, and (Java/Kotlin only) the language's own keyword
list as `CompletionKind::Keyword` candidates — folding "type `sout`, see it
offered in the same list" and "type `pub`, see `public` offered" into one
mechanism instead of Tab-trigger-expansion being a second, disconnected way
to get similar value. Accepting a `Template` candidate from the popup calls
`templates::expand` exactly as the existing Tab-trigger path does
(`widget.rs:594-610`) — this spec doesn't change that function, just adds a
second caller.

**Tests:** a `templates.rs`-adjacent pure function,
`identifiers_in(text: &str) -> Vec<String>`, table-tested the same way
`word_before_cursor` already is (`templates.rs:96-114`) — empty buffer,
single word, punctuation-separated words, no duplicates in output.

---

## 2. Ranking and filtering as more characters are typed

**Where:** shared by every candidate source — one function, `filter_and_rank
(candidates: &[CompletionItem], prefix: &str) -> Vec<&CompletionItem>`, new
in whichever module owns `CompletionState` (§0).

**Rule:** case-insensitive prefix match only, not full fuzzy matching —
deliberately narrower than `go_to_file.rs`'s `fuzzy_score`
(`crates/app/src/panels/go_to_file.rs:57-83`). A completion popup's prefix
*is* what's already been typed right where the cursor sits, unlike a
go-to-file query typed fresh into a search box — fuzzy subsequence matching
here would surface distant, surprising candidates (`fuzzy_score`'s own
scattered-subsequence example, `"contuser"` matching a path with `User`
buried in it, is a feature there and would be a confusing one here). Ties
(same prefix match) break by: exact case match before case-insensitive
match, then shorter labels before longer ones (a closer match to what's
likely wanted), then alphabetical, for a stable, predictable order — no
frequency/recency scoring in this first cut (that's the kind of thing worth
adding *after* shipping the basic popup, not before, per this project's
usual "ship the plain version, refine later" pattern already visible in
`FEATURES.md`'s own tiering).

**Tests:** table tests over a fixed candidate list — prefix narrows the set,
case-insensitive matching, tie-break ordering, empty prefix returns
everything unfiltered (mirroring `fuzzy_score`'s own
`empty_query_matches_everything` test at `go_to_file.rs:196-198`, same
edge case, simpler rule).

---

## 3. Receiver-type resolution: what is `x` in `x.`?

**Where:** new `crates/syntax/src/completion.rs` — a sibling to
`fields.rs`/`methods.rs`, same "pure `tree_sitter`-in, plain-data-out,
no `egui`" shape (`AGENTS.md`'s architecture section already documents this
as the established pattern for this kind of file). One file, `Language`-
dispatched (same shape `crates/syntax/src/language.rs` already uses to keep
`highlight_spans`/`syntax_errors` generic over language while each
language's actual query/logic differs) — not two separate modules, since
the *shared* pieces (the public `type_of_identifier` entry point, the
`this`/`super` routing concept, the simple-name normalization) are real and
worth keeping in one place even though the Java and Kotlin bodies
underneath differ substantially.

**Trigger (in `widget.rs`), language-agnostic:** typing `.` (an
`Event::Text(".")`) with a non-empty identifier run immediately preceding
it (`templates::word_before_cursor` again, applied to the text *before* the
dot) opens the popup anchored right after the dot, with `anchor_byte` at
the cursor (nothing typed yet, so the initial filter is empty — every
member shows). Backspacing the `.` itself closes the popup (per §0's
lifecycle rule: the cursor is no longer inside `[anchor_byte, cursor_byte]`
once the anchor byte itself is deleted). Identical for both languages —
only what happens *after* the trigger fires (resolving `x`'s type) differs.

**Resolution, in order (first match wins), general shape shared by both
languages — only the tree-sitter node kinds underneath differ:**

1. **`this.` / `super.`:** the receiver is the enclosing class itself
   (`this`) or its supertype (`super`).
2. **A bare identifier `x.`:** walk up from the cursor's tree-sitter node
   looking for a declaration of `x` in scope, nearest first: a parameter of
   the innermost enclosing function/method/constructor, then a local
   variable declared in that same body, then a field/property of the
   enclosing class.
3. **Anything else** (a JDK/stdlib-typed local, a method-call receiver like
   `getFoo().`, a chained access like `a.b.`, or — Kotlin only — a locally
   inferred type this pass can't work out, see 3b below): no resolution —
   the popup simply doesn't open for dot-completion, and typing continues
   to fall through to word-completion's own trigger (§1) the moment enough
   identifier characters follow. This is a deliberate, silent no-op, not an
   error — same "degrade to the next-best thing" choice
   `find_java_file_by_stem`'s own doc comment already makes explicit for
   the JDK-type case one layer up (`codegen.rs:534-539`).

### 3a. Java

Everything this section needs already exists and is proven by "Override
Method": `syntax::enclosing_class` (`crates/syntax/src/methods.rs:21-33`)
finds the enclosing class for `this.`; `syntax::superclass_name`
(`methods.rs:68-80`) resolves `super.` on top of that, same chain
`widget.rs:1261-1265` already uses.

For a bare identifier `x.`, walk up from
`tree.root_node().named_descendant_for_byte_range(cursor_byte,
cursor_byte)` (the same starting point `enclosing_class` already uses),
checking in order:

- a `formal_parameter` of the innermost enclosing `method_declaration`/
  `constructor_declaration` whose `name` child's text equals `x` — its
  `type` child is the answer;
- a `local_variable_declaration` anywhere between the enclosing method's
  opening `{` and `cursor_byte` whose `declarator`'s `name` equals `x` —
  a simpler "anywhere in the enclosing method" scan (not a precise
  backward-from-cursor one) is an acceptable first cut, same reasoning as
  before: a variable declared *after* the cursor being wrongly offered is a
  rare, low-consequence shadowing edge case, not a common one — call this
  out explicitly as a known simplification rather than silently getting it
  slightly wrong;
- a field of the enclosing class itself, via `syntax::fields::
  fields_in_class_body` on the same class `enclosing_class` found — but see
  §4's note on that function's static-field filtering not fitting
  completion's needs unmodified.

```rust
pub fn type_of_identifier_java(tree: &Tree, source: &str, cursor_byte: usize, name: &str) -> Option<String>;
```

(Simple name, generics/array brackets stripped — same `simple_name`
normalization `methods.rs:39-42` already applies.)

### 3b. Kotlin

**Grammar shapes** (verified against `tree-sitter-kotlin-ng-1.1.0`'s own
`node-types.json`, not assumed):

- A local `val`/`var` and a class-level `val`/`var` are **the same node
  kind**, `property_declaration` — unlike Java, which has distinct
  `local_variable_declaration` and `field_declaration` kinds. A
  `property_declaration`'s name/type live one level down, in its
  `variable_declaration` child (fields: none — access by child *kind*, not
  `child_by_field_name`, same as Java's `formal_parameter`/`local_variable_
  declaration` pattern already established): `variable_declaration` has an
  `identifier` child (the name) and an *optional* `type` child. A
  `property_declaration` also (optionally) has its own `expression` child
  directly — the initializer, e.g. the `Foo()` in `val x = Foo()`.
- A primary-constructor parameter declared with `val`/`var`
  (`class Foo(val x: Int)`, Kotlin's idiom for "constructor parameter that's
  also a class property") is a distinct node kind, `class_parameter` —
  children `identifier` + `type` (type is **required** here, unlike
  `variable_declaration`'s optional one) + optional `modifiers`/default
  `expression`. Found under the class's `primary_constructor` →
  `class_parameters` children, not inside `class_body` at all.
- A function/lambda parameter is `parameter` (inside a `function_
  declaration`'s `function_value_parameters`) — children `identifier` +
  `type`, both **required** (Kotlin never infers a function parameter's
  type), so this case needs no inference fallback at all, same as Java's
  `formal_parameter`.
- `x.` itself parses as a `navigation_expression` (children: `expression`
  — the receiver — and `identifier` — the member) once the tree fully
  reflects the completed access; resolving `x`'s type doesn't need to wait
  for that node to exist, though, since it only needs `x`'s *own*
  declaration, findable from the cursor position the same way `enclosing_
  class` walks up from a byte offset in Java.
- A class's supertype is one of `delegation_specifiers` (a
  `class_declaration` child) rather than Java's single `superclass`/
  `interfaces` field split — Kotlin doesn't distinguish "extends" from
  "implements" syntactically, a class simply lists every supertype (class
  or interface) after `:`. `superclass_name`'s Kotlin equivalent reads the
  first entry in `delegation_specifiers`, same "only the first, no
  multi-interface merging" scope limitation Java's version already has
  (`methods.rs:61-67`'s own doc comment).

**The one real new problem Java didn't have: `val x = Foo()` has no
`type` child at all** — Kotlin infers it, and this pass has no type
checker. Handle only the single common case worth handling without one:
if `variable_declaration` has no explicit `type` child, look at the
`property_declaration`'s own `expression` child (the initializer); if
*that* is a `call_expression` whose own leading `expression` child is a
bare `identifier` starting with an uppercase letter (the idiomatic Kotlin
constructor-call shape, `Foo(...)`), use that identifier's text as the
inferred type name. Anything else as an initializer (a method call
returning some other type, a literal, a binary expression, a
already-existing variable) resolves to `None` — this is a deliberately
narrow, syntactic heuristic ("looks like a constructor call"), not real
type inference, and should say so in its own doc comment so a future
reader doesn't mistake it for one.

```rust
pub fn type_of_identifier_kotlin(tree: &Tree, source: &str, cursor_byte: usize, name: &str) -> Option<String>;
```

Same resolution order as 3a (parameter, then primary-constructor
`class_parameter`, then local `property_declaration`, then class-body
`property_declaration`), and the same explicit-type-first,
constructor-call-heuristic-second rule applied at whichever step actually
finds `x`'s declaration.

### Shared entry point

```rust
/// Dispatches to `type_of_identifier_java`/`_kotlin` based on `language` —
/// the one function `widget.rs` actually calls; callers never need to
/// branch on language themselves.
pub fn type_of_identifier(language: Language, tree: &Tree, source: &str, cursor_byte: usize, name: &str) -> Option<String>;
```

**Tests:** table tests in `completion.rs`, same fixture-source style
`fields.rs`/`methods.rs` already use (`IncrementalParser::new(Language::
Java)` + a literal source string), duplicated for both languages:
a parameter's type resolves, a local variable's type resolves, a field's
type resolves when no local/parameter shadows it, a local shadowing a
field of the same name resolves to the local (nearer scope wins), an
unresolvable name (a JDK/stdlib-typed local, a never-declared name)
returns `None`. Kotlin additionally needs: an explicit-type local
(`val x: Foo = ...`) resolves without touching the initializer at all; an
inferred-type local via a constructor call (`val x = Foo()`) resolves via
the heuristic; an inferred-type local via anything else (`val x =
someFunction()`, `val x = 5`) resolves to `None`, not a wrong guess; a
primary-constructor `val`/`var` parameter (`class Foo(val x: Int)`)
resolves the same as a class-body property would.

---

## 4. Cross-project member lookup: what can you call on that type?

**Where:** `widget.rs`, wired the same way the "Override Method" resolution
chain already reads a supertype's source off disk
(`widget.rs:1272-1306`) — this spec's dot-completion path is that exact
sequence, called with §3's resolved type name instead of a supertype name:
find the type's source file in the project → read + parse with a throwaway
`IncrementalParser` → extract members. Shared by both languages up to the
"extract members" step, which — like §3 — has to branch on language
underneath, since Java's and Kotlin's class-body shapes aren't alike enough
to share one traversal.

**File lookup, generalized for both languages:**
`codegen::find_java_file_by_stem` (`codegen.rs:540-552`) is hardcoded to
`.java`. Generalize it in place rather than duplicating the whole-tree walk
for Kotlin:

```rust
/// Same walk `find_java_file_by_stem` already did, generalized over which
/// extension counts as a match — `.java` for `Language::Java`, `.kt` for
/// `Language::Kotlin`. `find_java_file_by_stem` becomes a one-line wrapper
/// (`find_source_file_by_stem(node, stem, "java")`) so its one existing
/// call site (Override Method) doesn't need to change at all.
pub fn find_source_file_by_stem(node: &FileNode, stem: &str, extension: &str) -> Option<std::path::PathBuf>;
```

**Member extraction — Java:** `fields.rs`'s `fields_in_class_body` and
`methods.rs`'s `methods_in_type` both already exist and both already do
almost the right thing — with one mismatch each, worth fixing rather than
working around:

- `fields_in_class_body` **skips `static` fields outright**
  (`fields.rs:48-50`) — the right call for a getter/setter generator (static
  accessors are a much rarer, more deliberate ask), the wrong call for
  completion, which should offer a class's constants
  (`public static final int MAX = ...`) same as any real IDE's
  autocomplete would. Add a `pub fn all_fields_in_class_body(body: Node,
  source: &str) -> Vec<FieldInfo>` (or a `include_static: bool` parameter on
  the existing function, threaded through its one call site in
  `fields.rs`) that keeps everything `fields_in_class_body` currently
  finds, minus the static-skip — `FieldInfo::is_final` already exists and
  is enough to distinguish them in the popup if wanted (e.g. a different
  `detail` string), no new field needed on the struct itself.
- `methods_in_type` (signature not fully re-derived here — verify its
  current filtering directly in `methods.rs` before relying on it) is built
  for "what's overridable," so it's expected to already exclude
  `private`/`static`/`final` the same way `method_signature`
  (`methods.rs:82-90`ish) filters for Override Method's purposes. Completion
  wants the *complement* in one respect (private members of `this.`/`super.`
  *are* callable from inside the same class, and should be offered there)
  and the same restriction in another (a field/method of an unrelated
  class reached via a local variable can only ever see its `public`
  surface, same visibility rule `methods_in_type` already effectively
  encodes by being built for the "what does a subclass see" question).
  Concretely: `this.`/`super.` receivers should use an unfiltered member
  listing (every field/method declared on the class, regardless of
  visibility, since "inside the same class" sees all of it); a local-
  variable receiver of another project class should keep `methods_in_type`'s
  existing external-visibility filtering as-is. This is a real design
  point to verify against `methods.rs`'s actual current filter before
  writing the two call sites — don't assume the split above is exactly
  right without reading that function fresh.

**Member extraction — Kotlin:** no existing `fields.rs`/`methods.rs`
equivalent to build on — this is new. A `class_body`'s direct children are
`class_member_declaration` nodes (per `node-types.json`), each wrapping
either a `property_declaration` (a field, per §3b's shape) or a
`function_declaration` (a method) one level down. New functions, same
output structs as Java (`FieldInfo`/`MethodSignature` are already
language-agnostic in shape — no new struct needed, just a second producer
of them):

```rust
pub fn kotlin_properties_in_class_body(body: Node, source: &str) -> Vec<FieldInfo>;
pub fn kotlin_functions_in_type(tree: &Tree, source: &str, type_name: &str) -> Vec<MethodSignature>;
```

`FieldInfo::java_type` reads oddly named for a Kotlin field (Kotlin calls
it a *property*, not a field, and doesn't share Java's `final` keyword
concept) — reuse the struct as-is rather than forking it: `is_final` maps
to Kotlin's `val` (immutable) vs. `var` (mutable), same "can't be
reassigned" meaning the field's own doc comment already describes, just
spelled differently in the source language. Visibility filtering for
`kotlin_functions_in_type` should mirror whatever §4's Java "complement"
rule above settles on for `methods_in_type`, applied to Kotlin's own
`private`/`internal`/`protected` modifiers (checked the same
text-search-over-the-modifiers-span way `fields_in_class_body`/
`method_signature` already check Java's, since Kotlin's modifiers are
anonymous tokens in this grammar too, not their own field — verify that
against `node-types.json` before assuming it, the same discipline this
whole section already follows).

Constructor-promoted properties (`class Foo(val x: Int)`, §3b's
`class_parameter` case) count as class members too, findable via `.`
completion the same as a `class_body` property would — `kotlin_
properties_in_class_body`'s caller should also walk the class's
`primary_constructor` → `class_parameters` looking for `class_parameter`
nodes that actually carry the `val`/`var` modifier (a plain constructor
parameter with neither is a constructor-only argument, not a class
member, and shouldn't be offered).

**One supertype level:** same as Override Method, a resolved type's
inherited members (one level up via `superclass_name`/its Kotlin
equivalent + `find_source_file_by_stem`, not the whole inheritance chain)
are included — consistent scope limitation, not a new one this feature
invents. Kotlin's `delegation_specifiers` (§3b) can name a superclass *or*
an interface in the same slot, same "don't try to distinguish, just take
the first one" scope limit Java's version already accepts.

**Tests:** a `widget/tests.rs` (or `completion.rs`, if the wiring is pure
enough to live there instead — prefer that, same "push logic down to the
headless-testable crate" principle `AGENTS.md`'s testing conventions
already state) case per language, building a small two-file fixture (a
class with a field/property and a method/function, and another class with
a local variable of the first type), asserting the popup's candidate list
contains exactly the expected members — the same "two-file, real project
tree" fixture shape `find_java_file_by_stem`'s own tests (if any exist —
check `codegen.rs`'s test module) or Override Method's manual verification
already establish as necessary for this class of cross-file feature. The
Kotlin case additionally covers a constructor-promoted `class_parameter`
property showing up in the candidate list.

---

## 5. Insertion

**Where:** new `pub fn insert_completion(text: &str, anchor_char: usize,
cursor_char: usize, item: &CompletionItem) -> (String, usize)` — same
`(String, new_cursor)` return shape every other text-transform in this
codebase already uses (`templates::expand`, `auto_edit`'s functions,
`codegen::insert_generated`), for the same reason: one consistent contract
every call site in `widget.rs` already knows how to apply and feed into
`apply_edit` (`widget.rs:262-268`).

**Rule:** replace `text[anchor_char..cursor_char]` (whatever's been typed
since the popup opened — the partial prefix) with `item.label`. For
`CompletionKind::Method` specifically, append `()` and place the cursor
*between* the parens if the method takes at least one parameter, or *after*
the closing paren if it takes none — mirroring `templates::expand`'s own
`${cursor}`-marker convention (`templates.rs:76-89`) rather than inventing a
second cursor-placement mechanism. `CompletionKind::Template` doesn't go
through this function at all — it calls `templates::expand` directly, same
as the existing Tab-trigger path, since that function already handles
multi-line bodies and its own marker.

**Tests:** table tests mirroring `templates.rs`'s own `expand` tests
(`templates.rs:127-155`) — a field/keyword/word candidate inserts plain
text at the right position; a zero-arg method candidate places the cursor
after `()`; a method with parameters places it between them.

---

## 6. Kotlin: known rough edges, not full parity with Java

Word-completion (§1) and the popup shell (§0) are language-agnostic and
already work for `.kt` files exactly as they do for any other file.
Dot-completion (§3b/§4) is speced for Kotlin in this pass too, but two
honest gaps remain even once it ships, worth naming explicitly rather than
letting "Kotlin is supported" imply exact parity with Java:

- **Inferred-type locals (`val x = Foo()`) only resolve for the
  constructor-call shape** (§3b's heuristic) — `val x = someFactory()`,
  `val x = list.first()`, and every other inference case this pass has no
  type checker for simply don't offer dot-completion. Java has no
  equivalent gap (`local_variable_declaration` always carries an explicit
  type), so Kotlin's dot-completion will genuinely trigger less often on
  equivalent-looking code, and that's expected, not a bug to chase.
- **Kotlin's richer member surface (extension functions, data class
  `componentN()`/`copy()` synthesized members, delegated properties) isn't
  covered at all** — only members textually declared in the resolved
  type's own `class_body`/primary constructor are offered, one supertype
  level up. A `data class`'s auto-generated members won't appear in the
  popup even though they're callable. Real coverage of these needs either
  a Kotlin-specific synthesis step (detecting `data class` and
  hand-generating the same members the compiler would) or the same
  classpath/compiler-backed resolution `FEATURES.md`'s LSP-integration
  entry is already gated on — out of scope for this pass, flagged here so
  it isn't rediscovered as a surprise later.

Both are real, separate follow-on work, not blockers for shipping Kotlin
dot-completion as speced — the "constructor-call and explicit-type cases
work, everything else silently falls back to word-completion" behavior is
functional and honest about its own limits, the same tradeoff Java's
JDK-type gap (§7) already makes.

---

## 7. Explicit non-goals for this pass

Recorded so a future pass doesn't re-litigate these without new evidence,
same convention the (superseded) optimization-pass `SPEC.md` used for its
own §10:

- **JDK/library-typed receivers** (`String s; s.` offering `length()`/
  `substring()`/etc., or Kotlin's own stdlib types — `String`, `List`,
  `MutableList`, etc.) — needs a classpath, gated on the Maven/Gradle
  awareness `FEATURES.md` already lists as a major undertaking. Falls
  through to word-completion instead, never an error. Same limitation,
  same reason, in both languages.
- **Multi-hop chains** (`a.b.c.`, `getFoo().`) — only a single identifier
  (or `this`/`super`) immediately before the dot is resolved. A method-call
  or nested-access receiver simply doesn't open the dot-completion path.
- **Generics-aware member types** (`List<String> l; l.get(0)` knowing the
  result is a `String`) — no generic substitution, just the raw declared
  type's own simple name.
- **Auto-import** — nothing here writes an `import` statement; only
  same-package, no-import-needed access is assumed to work correctly once
  a member is inserted (consistent with every other project-local
  cross-file lookup in this codebase today).
- **Parameter-hint overlays** (highlighting which argument position the
  cursor is in in a signature-help popup) — a real, separate feature, not
  bundled into this one.
- **Frequency/recency-based ranking, fuzzy (non-prefix) matching, or a
  settings toggle to disable completion** — none of this codebase's other
  first-cut features (live templates, occurrence highlighting) shipped with
  a settings toggle either; add one only if it turns out to be needed in
  practice, not preemptively.
