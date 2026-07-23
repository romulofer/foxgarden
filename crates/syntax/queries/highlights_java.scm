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

; Variables

(identifier) @variable

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

; Types

(type_identifier) @type

(interface_declaration
  name: (identifier) @type)
(class_declaration
  name: (identifier) @type)
(enum_declaration
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

; Constants

((identifier) @constant
 (#match? @constant "^_*[A-Z][A-Z\\d_]+$"))

; Builtins: `this`/`super` treated as keywords, matching how Kotlin's
; highlights_kotlin.scm keywords its `this`/`super`/`this@`/`super@`.
[(this) (super)] @keyword

; Literals

[
  (hex_integer_literal)
  (decimal_integer_literal)
  (octal_integer_literal)
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

; Keywords

[
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
