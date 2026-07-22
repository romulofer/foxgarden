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
  "abstract" "actual" "annotation" "as" "by" "catch" "class"
  "companion" "const" "constructor" "crossinline" "data" "do"
  "dynamic" "else" "enum" "expect" "external" "final" "finally" "for" "fun"
  "get" "if" "import" "in" "infix" "init" "inline" "inner" "interface"
  "internal" "is" "lateinit" "noinline" "object" "open" "operator" "out"
  "override" "package" "private" "protected" "public" "return"
  "sealed" "set" "suspend" "tailrec" "this" "super" "throw" "try"
  "typealias" "val" "var" "vararg" "when" "where" "while"
] @keyword

(line_comment) @comment
(block_comment) @comment

(string_literal) @string
(multiline_string_literal) @string

(class_declaration name: (identifier) @type)
(user_type (identifier) @type)

(function_declaration name: (identifier) @function)
