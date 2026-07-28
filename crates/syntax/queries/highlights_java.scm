; Forked from the tree-sitter-java crate's bundled highlights.scm (checked
; into this repo as queries/highlights_java.scm, same as Kotlin's) to fix
; three scopes the bundled query maps to captures the fixed theme doesn't
; render at all: `@attribute` (annotations), `@variable.builtin` (`this`),
; and `@constant.builtin` (`true`/`false`/`null`) have no corresponding
; `Scope` in highlight.rs's `scope_for_capture`, so those tokens rendered as
; plain text. `@function.builtin` on `(super)` did resolve — to `Function`,
; via the shared "starts_with(\"function\")" prefix match — but that's an
; accident of prefix collision, not an intentional choice to color `super`
; as a function.
;
; Also extended past the bundled query with coverage it lacked outright
; (checked against `../references/java`, Zed's own extension, which pins the
; identical `tree-sitter/tree-sitter-java` grammar so its highlights.scm is a
; valid diff target — unlike Kotlin's reference, which targets a different
; grammar crate than the one vendored here): `record_declaration` and
; `annotation_type_declaration` names weren't captured at all, so a record's
; or a custom annotation's own name rendered as plain text even though
; class/interface/enum names were; and `"@interface"` (the keyword
; introducing an annotation type declaration) was missing from the keyword
; list. Both are user-visible fixes (`@type`/`@keyword` both have a `Scope`).
;
; `binary_integer_literal` (e.g. `0b1010`) was also added to the `@number`
; literal list, alongside `hex`/`decimal`/`octal_integer_literal` — this is
; NOT currently user-visible: `scope_for_capture` in highlight.rs has no
; "number"-prefixed branch, so every numeric-literal capture in this file,
; not just this one, is inert today. Added anyway for consistency with the
; other three integer literal kinds already listed, so the four are complete
; as a set whenever `Scope::Number` (or equivalent) is added.

; Variables

(identifier) @variable

; Parameters: `formal_parameter`'s own `name` field is unambiguous, distinct
; from a local variable declaration (which stays the generic `@variable`
; capture above, `scope_for_capture` has no branch for it and it renders as
; plain text) — `PLAN.md` Track 2, `SPEC.md` §2.
(formal_parameter
  name: (identifier) @variable.parameter)

; Methods

(method_declaration
  name: (identifier) @function.method)
(method_invocation
  name: (identifier) @function.method)

; Annotations: routed to @type, same as Kotlin routes its annotations
; through the generic user_type capture — an annotation name is a type
; reference, and `Scope::Type` is the closest fit among the fixed theme's
; six scopes.
(annotation
  name: (identifier) @type)
(marker_annotation
  name: (identifier) @type)

"@" @operator

; Operators: binary/unary/assignment expressions each declare their own
; operator token under an `operator` field (verified against real
; `tree-sitter-java` grammar.js source, not assumed) — `update_expression`'s
; `++`/`--` have no field to key off instead, so they're matched by their
; own literal token text, same shape `"@"` above already uses.
(binary_expression operator: _ @operator)
(unary_expression operator: _ @operator)
(assignment_expression operator: _ @operator)
["++" "--"] @operator

; Labels: `labeled_statement`'s own grammar is `identifier ':' statement`
; with no field names — the identifier is unambiguously the label, since
; the wrapped `statement` child is never itself literally node type
; `identifier` (verified against grammar.js, not assumed).
(labeled_statement
  (identifier) @label)

; Types

(type_identifier) @type

(interface_declaration
  name: (identifier) @type)
(class_declaration
  name: (identifier) @type)
(enum_declaration
  name: (identifier) @type)
(record_declaration
  name: (identifier) @type)
(annotation_type_declaration
  name: (identifier) @type)

((field_access
  object: (identifier) @type)
 (#match? @type "^[A-Z]"))
((scoped_identifier
  scope: (identifier) @type)
 (#match? @type "^[A-Z]"))
((method_invocation
  object: (identifier) @type)
 (#match? @type "^[A-Z]"))
((method_reference
  . (identifier) @type)
 (#match? @type "^[A-Z]"))

(constructor_declaration
  name: (identifier) @type)

[
  (boolean_type)
  (integral_type)
  (floating_point_type)
  (floating_point_type)
  (void_type)
] @type.builtin

; Fields: a field's own declaration site and any `object.field`-style
; access (ported from `../references/java`'s highlights.scm) — visually
; distinct from a local variable/parameter, which stay the generic
; `@variable` capture above since nothing in the syntax tree alone marks
; an identifier as "resolves to a local" (that needs semantic analysis,
; not just a query). Placed *before* the Constants section below, not
; after: a `static final` ALL-CAPS field (`MAX_LENGTH`) is also a field
; declarator, and `highlight_spans` resolves a same-range capture
; conflict in favor of whichever pattern is declared *later* in this file
; — Constants needs to win there, so it has to stay the later pattern.
(field_declaration
  declarator: (variable_declarator
    name: (identifier) @property))
(field_access
  field: (identifier) @property)

; Constants

((identifier) @constant
 (#match? @constant "^_*[A-Z][A-Z\\d_]+$"))

; An enum constant explicitly, not just via the ALL-CAPS regex above —
; same reasoning TECHNICAL_DEBT.md #3 already applied to Kotlin's enum
; entries: `enum Suit { Hearts, Diamonds, ... }`-style constants that
; don't follow the ALL_CAPS convention are still constants, and the
; regex alone would otherwise leave them as plain `@variable`.
(enum_constant
  name: (identifier) @constant)

; Builtins: `this`/`super` treated as keywords, matching how Kotlin's
; highlights_kotlin.scm keywords its `this`/`super`/`this@`/`super@`.
[(this) (super)] @keyword

; Literals

[
  (hex_integer_literal)
  (decimal_integer_literal)
  (octal_integer_literal)
  (binary_integer_literal)
  (decimal_floating_point_literal)
  (hex_floating_point_literal)
] @number

[
  (character_literal)
  (string_literal)
] @string
(escape_sequence) @string.escape

; Same treatment as Kotlin's `true`/`false`/`null` (there matched by text
; since that grammar has no dedicated literal nodes for them; here they're
; real node types, so a plain capture suffices).
[
  (true)
  (false)
  (null_literal)
] @keyword

[
  (line_comment)
  (block_comment)
] @comment

; Doc comments: a `/** ... */` block comment, distinguished from a plain
; `/* ... */` one purely by its own leading text — tree-sitter-java has no
; separate node type for a doc comment, unlike Kotlin's KDoc. Declared
; *after* the generic `@comment` capture above so it wins the same-range
; conflict `highlight_spans` resolves in favor of the later pattern (same
; "Constants must stay the later pattern" reasoning the Fields section
; above already documents).
((block_comment) @comment.doc
 (#match? @comment.doc "^/\\*\\*"))

; Keywords

[
  "@interface"
  "abstract"
  "assert"
  "break"
  "case"
  "catch"
  "class"
  "continue"
  "default"
  "do"
  "else"
  "enum"
  "exports"
  "extends"
  "final"
  "finally"
  "for"
  "if"
  "implements"
  "import"
  "instanceof"
  "interface"
  "module"
  "native"
  "new"
  "non-sealed"
  "open"
  "opens"
  "package"
  "permits"
  "private"
  "protected"
  "provides"
  "public"
  "requires"
  "record"
  "return"
  "sealed"
  "static"
  "strictfp"
  "switch"
  "synchronized"
  "throw"
  "throws"
  "to"
  "transient"
  "transitive"
  "try"
  "uses"
  "volatile"
  "when"
  "while"
  "with"
  "yield"
] @keyword
