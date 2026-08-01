# IMPLEMENTED_FEATURES.md

A snapshot of what FoxGarden actually does today, as of this writing.
Local-only (gitignored, like `FEATURES.md`/`PLAN.md`/`SPEC.md`) — this is a
reference for the current state, not project documentation, and it will
drift out of date as the codebase evolves. Cross-check against the code
before trusting a specific detail long after this was written.

For what's *not* here yet, see `FEATURES.md`'s "Quick wins" /
"Moderate effort" / "Substantial" / "Major undertakings" roadmap.

## Supported languages

Java (`.java`), Kotlin (`.kt`), `.properties`, YAML (`.yml`/`.yaml`), XML
(`.xml`) — each gets real tree-sitter-backed syntax highlighting and a
distinct tree icon (☕ 🔷 ⚙️ 📜 🏷️). Any other file extension still opens
and edits normally as plain text (no highlighting or diagnostics, but
everything else — auto-pair, multi-cursor, live templates' language-gated
parts aside — still works).

## Project / file management

- Open a folder as a project; the side panel shows its file tree
  (directories before files, alphabetical within each), skipping VCS/build/
  dependency noise (`.git`, `target`, `node_modules`, etc.).
- Create, rename, and delete both files and directories from the tree
  (directory operations are recursive) — right-click context menu, the
  📄 "New File" toolbar button, File > New File…, or `Ctrl+N`.
- New Java/Kotlin files get boilerplate generated from their path (package
  declaration inferred from the Maven/Gradle-style source root).
- Deleting or renaming a directory closes/repoints every open tab whose
  file was inside it, not just a tab pointing at the directory itself.
- Recent-files quick switcher (`Ctrl+E`): a searchable popup listing open
  tabs, then recently-closed ones, most-recent first; type to filter,
  arrow keys + Enter or click to jump.
- Side panel toggle: `Ctrl+B`, View > Side Panel, the "◀"/"▶" button pinned
  to the right of the menu bar (always reachable, even while the panel is
  hidden), or the "◀" button in the panel's own toolbar. Width (drag to
  resize) and shown/hidden state both persist across restarts.
- 💻 "Open Terminal" toolbar button (beside "New File"): opens a system
  terminal at the project root. Platform-specific under the hood
  (`crates/app/src/terminal.rs`) — Terminal.app on macOS, `cmd` on
  Windows, the first installed terminal emulator from a fixed candidate
  list on Linux/other Unix.

## Tabs and sessions

- Tabbed editing with dirty-state tracking (`*` prefix on an unsaved tab's
  label) and a save-prompt-on-close.
- Middle-click a tab to close it; `Ctrl+Shift+T` reopens the most recently
  closed one (LIFO, walking further back on repeated presses).
- Session persistence across restarts: last project folder, which tabs
  were open (in order), which one was focused, and all Settings (font,
  font size, theme, indentation style/width, every View toggle, side panel
  width and shown/hidden state) — all via `eframe::Storage`.
- A persistent per-tab shaped-galley cache so switching between
  already-open tabs doesn't force a full text reshape.

## Editor core

- Live syntax-error squiggles (red wavy underline) with hover tooltips
  showing the error message — generic over whichever tree-sitter grammar
  is active, so it works for all five supported languages, not just
  Java/Kotlin.
- Incremental re-parsing on every edit (diff-based, not full reparse).
- Auto-closing brackets/quotes (`{ ( [ < " '`), with skip-over-existing-
  closer behavior (typing `)` right before an auto-inserted `)` moves past
  it instead of duplicating it).
- Auto-indent: Enter matches the previous line's indentation, plus one
  extra level after a line ending in `{`.
- Line-numbers gutter, right-aligned, sized to the buffer's current line
  count, painted from the same `Galley` rows the text uses (can't drift
  out of alignment with scrolling or font metrics).
- Trailing whitespace is stripped from every line on save.
- A selectable editor font (JetBrains Mono bundled, or the system default
  monospace) and an adjustable font size (Settings > Font… modal).
- The text caret blinks (on/off timing matches `egui::TextEdit`'s own),
  resetting to solid on click/keystroke/focus change; toggle via View >
  Blinking Cursor.
- The active editor pane is outlined — subdued when unfocused, highlighted
  (selection color) while focused — restoring the border `egui::TextEdit`
  used to paint before the editor moved to a hand-built virtualized text
  area; toggle via View > Editor Outline.

## Multi-cursor and selection

- `Ctrl+D`: select the word under the cursor, or extend to the next
  occurrence of the current selection on repeat presses (`Ctrl+Shift+D`
  variant is case-sensitive matching). Editing at one cursor applies
  identically at every active cursor.
- A non-intercepted mutating key (e.g. Tab) reaching a single cursor while
  multi-cursor is active implicitly collapses back to single-cursor mode
  rather than leaving stale extra selections.
- Tab/Shift+Tab over a selection indents/dedents every touched line
  in place, instead of egui's default of deleting the selection first.
- Passive highlight of every other occurrence of the word under (or
  touching) a collapsed cursor — read-only, distinct from `Ctrl+D`'s
  active editing; automatically suppressed while multi-cursor or a real
  selection is active.

## Editing commands

- `Ctrl+J`: join the current line with the next, trimming the joined
  line's leading whitespace and collapsing the separator to a single
  space (or none, joining onto/from a blank line or when the current line
  already ends in whitespace).
- `Ctrl+/`: toggle `//` line comments on the current line, or every line a
  selection touches. Uncomments only if every non-blank touched line is
  already commented; otherwise comments all of them (blank lines
  included).
- `Alt+↑`/`Alt+↓`: move the current line up/down, swapping with its
  neighbor.
- `Alt+Shift+↑`/`Alt+Shift+↓`: duplicate the current line (inserted
  below); Up keeps the cursor on the original, Down moves it to the copy.
- `Ctrl+Shift+U` / `Ctrl+Shift+L` (or Tools menu): convert the selected
  text to UPPERCASE / lowercase. Tools menu also offers Title Case
  (uppercase the first letter of every word, lowercase the rest).
- Smart Home key: `Home` toggles the cursor between the line's first
  non-whitespace character and true column 0; `Shift+Home` does the same
  while extending the selection.
- Wrap-selection: typing a pairable character (`{ ( [ < " '`) while a
  selection is active wraps the selection in that pair instead of
  replacing it.
- Live templates: type a trigger word, press Tab with no selection to
  expand it. Three built-in groups, all checked together (a custom trigger
  overrides a built-in one of the same name) —
  - Global (any file, regardless of language, including no recognized
    language at all): `pipe` → `|`.
  - Java: `sout`, `souf`, `serr` → `System.out`/`System.err` print calls,
    `psvm` → a `public static void main` stub, `fori`/`iter` → indexed/
    for-each loops, `ifn`/`inn` → null/non-null guards, `trycatch` → a
    try/catch block.
  - Kotlin: `sout`/`serr` → print calls, `main` → a `fun main()` stub,
    `fori` → a range-based for loop, `ifn`/`inn`/`trycatch` as above.
  Help > Live Templates… lists every built-in trigger (grouped Global/
  Java/Kotlin) and lets a user add/edit/remove their own per group;
  custom templates are persisted across restarts (`eframe::Storage`, one
  key per group) independently of any project.
  Tab with no selection otherwise inserts the configured indent unit
  (spaces, respecting width) or falls through to a literal tab in
  tabs-mode.

## Java code generation

- `Ctrl+Shift+G`: generate getters and setters for the whole file's
  classes — analyzes every class in the file (not just the one under the
  cursor). A single eligible class generates immediately for all its
  fields. More than one eligible class opens a picker: a class radio list,
  then a checkbox per field of whichever class is selected, with
  "Generate"/"Cancel" buttons.
- Tools menu → "Generate Getters" / "Generate Setters": the same
  generation, narrowed to just one kind.
- `final` fields get a getter only (never a setter). Static fields are
  skipped entirely.
- Every non-applicable case (non-Java file, no class in the file has
  fields, nothing checked in the picker, an all-`final` class for
  setters-only) surfaces a specific message through the shared error
  modal instead of silently doing nothing.

## Editor right-click menu

- Right-clicking inside the editor opens a context menu: Undo, Redo, Cut,
  Copy, Paste, Select All, Toggle Line Comment, Duplicate Line, Save.
- Cut/Copy/Paste/Toggle Line Comment/Duplicate Line/Save act immediately.
  Undo/Redo/Select All are queued as synthetic input events applied at the
  top of the next frame (see `widgets::editor::show`'s `pending_input`
  parameter) so they replay through egui's own `TextEdit` undo/redo
  history rather than a separate one that could drift from `Ctrl+Z`.
- Cut/Copy are disabled with no selection; Paste is disabled when the
  clipboard is empty; Save is disabled when the tab isn't dirty.

## Settings

- **Theme**: Light or Dark (Settings > Theme), persisted.
- **Font…** (Settings > Font…): a modal with the editor font (JetBrains Mono
  or system default) as radio buttons and font size as a numeric `DragValue`
  box together, persisted.
- **Indentation** (Settings > Indentation): Spaces or Tabs, plus a width
  control for Spaces mode (1–8). Drives auto-indent, Tab-over-selection
  block indent/dedent, and plain Tab-with-no-selection alike.
- **Zen Mode** (`F11`, or View > Zen Mode): hides the menu bar and side
  panel down to just the tab bar and editor; `F11` still works to exit
  even with the menu itself hidden.
- **Side Panel** (`Ctrl+B`, View > Side Panel, or either "◀"/"▶" toggle
  button): shows/hides the project tree independently of Zen Mode; width and
  visibility persisted.
- **Blinking Cursor** (View, persisted): toggles whether the text caret
  blinks or stays solid.
- **Editor Outline** (View, persisted): toggles the focus-aware border
  around the active editor pane.
- **Sticky Scroll** (View > Sticky Scroll, persisted): while scrolling
  through a long body, pins the enclosing class/method header line(s) to the
  top of the editor so the current context stays visible. Tree-sitter-driven,
  so Java only for now (a no-op in other languages); capped at the five
  outermost enclosing scopes.

## Error reporting

- A single shared "last error" modal surfaces user-facing failures from
  anywhere in the app (open/save/rename/delete/create a file, open or
  refresh a project, restore last session, code-generation requests that
  don't apply, etc.) rather than failing silently or only to a terminal
  the GUI user isn't watching.
- `Escape` closes whichever modal dialog is on top (error, delete/close
  confirmation, About, the getters/setters picker) — the same as clicking
  its Cancel/OK/Close button, never Save or Delete.

## Performance

- `[profile.dev.package."*"]` opt-level override so dependency code
  (egui/epaint's text shaping in particular) runs at near-release speed in
  debug builds without slowing down recompiles of the project's own code.
- Byte→char diagnostic-position conversion is batched into a single
  forward pass over the buffer per frame, rather than rescanning from byte
  0 for every diagnostic.
