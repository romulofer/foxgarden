; Forked from tree-sitter-containerfile's own bundled highlights.scm (same
; approach highlights_java.scm/highlights_kotlin.scm already take, unlike
; the YAML/XML/properties queries, which are used unmodified since they
; already fit this project's `Scope` vocabulary as-is) — trimmed to the
; captures this project's `Scope` enum (crates/syntax/src/highlight.rs) has
; a color for. The bundled query also has `@operator`, `@punctuation.special`,
; `@label`, and `@number` captures with no matching `Scope`; dropped here
; rather than left in, since a capture with no `Scope` just renders as
; plain text anyway — keeping them would only be misleading about what
; this file actually causes to be colored.

[
  "FROM"
  "AS"
  "RUN"
  "CMD"
  "LABEL"
  "EXPOSE"
  "ENV"
  "ADD"
  "COPY"
  "ENTRYPOINT"
  "VOLUME"
  "USER"
  "WORKDIR"
  "ARG"
  "ONBUILD"
  "STOPSIGNAL"
  "HEALTHCHECK"
  "SHELL"
  "MAINTAINER"
  "CROSS_BUILD"
] @keyword

(comment) @comment

[
  (double_quoted_string)
  (single_quoted_string)
  (json_string)
] @string

(heredoc_block) @string

(escape_sequence) @string.escape

; An ALL-CAPS `$VARIABLE`/`${VARIABLE}` reference — same convention as
; Java/Kotlin's own ALL-CAPS `@constant` capture.
((variable) @constant
 (#match? @constant "^[A-Z][A-Z_0-9]*$"))

; `ARG`/`ENV`/`LABEL` keys are this format's closest equivalent to a YAML/
; properties mapping key — same `Scope::Property` treatment for the same
; reason (see highlight.rs's own doc comment on why `Property` exists).
(arg_pair
  name: (unquoted_string) @property)
(env_pair
  name: (unquoted_string) @property)
(label_pair
  key: (_) @property)
