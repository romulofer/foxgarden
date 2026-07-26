; Minimal highlight query for checkpoint 1's fixed theme:
; keyword / string / comment / type / function.

; "break" and "continue" are deliberately excluded: they exist as literal
; strings in the grammar's grammar.js source, but don't survive as matchable
; node types in the compiled parser this crate ships (verified by bisecting
; each keyword through `tree_sitter::Query::new` individually — those two are
; the only ones left that fail with "Invalid node type"). A grammar.js
; literal isn't a reliable signal that a query can match it; check
; node-types.json (or bisect like this) instead of trusting the source.
;
; "reified" used to be on this exclusion list for the same reason — bare
; `"reified" @keyword` fails to compile — but grammar.js wraps it in its own
; `reification_modifier` rule (`reification_modifier: _ => 'reified'`), and
; unlike the bare literal, `(reification_modifier)` *does* compile and
; matches the same token (TECHNICAL_DEBT.md #3's "richer modifier-keyword
; coverage" candidate, ported via this grammar's node-type wrapper rather
; than Zed's `fwcd`-grammar node names, same methodology as the enum-entry
; fix below).
[
  "abstract" "actual" "annotation" "as" "as?" "by" "catch" "class"
  "companion" "const" "constructor" "crossinline" "data" "delegate" "do"
  "dynamic" "else" "enum" "expect" "external" "field" "final" "finally" "for" "fun"
  "get" "if" "import" "in" "!in" "infix" "init" "inline" "inner" "interface"
  "internal" "is" "!is" "lateinit" "noinline" "object" "open" "operator" "out"
  "override" "package" "param" "private" "property" "protected" "public"
  "receiver" "return" "return@" "sealed" "set" "setparam" "suspend" "tailrec"
  "this" "this@" "super" "super@" "throw" "try" "typealias" "val" "value"
  "var" "vararg" "when" "where" "while"
] @keyword

(reification_modifier) @keyword

; `true`, `false`, and `null` aren't distinct literal node types in this
; grammar (unlike Java's `(true)`/`(false)`/`(null_literal)`) — they parse as
; plain `identifier` nodes, so the only way to single them out is by text.
((identifier) @keyword
 (#any-of? @keyword "true" "false" "null"))

(line_comment) @comment
(block_comment) @comment

(string_literal) @string
(multiline_string_literal) @string
(character_literal) @string

(class_declaration name: (identifier) @type)
(object_declaration name: (identifier) @type)
(companion_object name: (identifier) @type)
(user_type (identifier) @type)

; Enum entries, e.g. `LOW` in `enum class Level { LOW, MEDIUM, HIGH }` —
; ported from Zed's reference (`(enum_entry (simple_identifier) @constant)`),
; but that capture name is the one node type this grammar doesn't split out:
; per `tree-sitter-kotlin-ng`'s own node-types.json, `enum_entry`'s name
; child is a plain `identifier`, not a `simple_identifier` (verified by
; checking node-types.json directly, per this file's and
; TECHNICAL_DEBT.md#3's own caution against assuming node names carry over
; from the fwcd grammar Zed's file targets). `(identifier)` as a direct
; child of `enum_entry` is unambiguously the entry's own name — the other
; possible direct children (`modifiers`, `value_arguments`, `class_body`)
; are distinct node types, so this can't accidentally match an identifier
; nested inside a constructor-argument list instead.
(enum_entry (identifier) @constant)

(function_declaration name: (identifier) @function)

; Call expressions: `foo()` (callee is a bare identifier, anchored first so
; this doesn't also match identifiers appearing later among the arguments)
; and `obj.method()` (callee is a navigation_expression; anchored last so
; this captures the method name, not the receiver `obj`).
(call_expression . (identifier) @function)
(call_expression (navigation_expression (identifier) @function .))
