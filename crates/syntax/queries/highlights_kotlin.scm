; Minimal highlight query for checkpoint 1's fixed theme:
; keyword / string / comment / type / function.

; "break", "continue", and "reified" are deliberately excluded: they exist as
; literal strings in the grammar's grammar.js source, but don't survive as
; matchable node types in the compiled parser this crate ships (verified by
; bisecting each keyword through `tree_sitter::Query::new` individually —
; those three are the only ones that fail with "Invalid node type"). A
; grammar.js literal isn't a reliable signal that a query can match it;
; check node-types.json (or bisect like this) instead of trusting the source.
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

(function_declaration name: (identifier) @function)

; Call expressions: `foo()` (callee is a bare identifier, anchored first so
; this doesn't also match identifiers appearing later among the arguments)
; and `obj.method()` (callee is a navigation_expression; anchored last so
; this captures the method name, not the receiver `obj`).
(call_expression . (identifier) @function)
(call_expression (navigation_expression (identifier) @function .))
