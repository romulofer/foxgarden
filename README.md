# FoxGarden

A light, native code editor for `.java` and `.kt` files, built entirely in Rust.
This is checkpoint 1 of a longer-term goal: a full-featured Spring Boot IDE for
Maven/Kotlin/Java projects. This checkpoint proves out the editor core —
project tree, tabs, dirty-state tracking, syntax highlighting, and syntax-error
squiggles — that later checkpoints (Maven awareness, LSP, build/run) will
layer on top of.

Speed is a hard requirement, not a nice-to-have — [Zed](https://zed.dev) is
the bar FoxGarden holds itself to for startup time, input latency, and idle
resource use. The later Maven/LSP/build-tooling checkpoints look to Zed's own
[Java](https://github.com/zed-extensions/java) and
[Kotlin](https://github.com/zed-extensions/kotlin) extensions as prior art.

## Features

- Menu bar: File (New File, Open Folder, Save, Close Tab, Reopen Closed Tab,
  Exit), Settings (Light/Dark theme, editor font), View (Zen Mode), Help
  (About)
- `F11` toggles Zen Mode — hides the menu bar and side panel, leaving just
  the tab bar and editor; `F11` again brings them back
- Side panel with the open project's file tree, with icons per entry
  (folders, `.java`, `.kt`); folders are always listed before files, each
  group sorted alphabetically, and VCS/build/dependency noise (`.git`,
  `target`, `node_modules`, `build`, `.idea`, `dist`, `out`, `.svn`, `.hg`)
  is skipped so it never clutters the tree or slows down opening a large
  real-world project
- Files with no recognized extension still open and edit as plain text (no
  highlighting or diagnostics); actual binaries (images, class files, jars,
  fonts) are detected and refused rather than read in full
- Remembers and restores your session on restart: the last project folder,
  every tab that was open, and which one was focused
- Create, rename, and delete files from the side panel — via the "New File…"
  button or right-click on a file/folder (delete asks for confirmation
  first); `Enter` confirms a new-file/rename input, `Escape` cancels it. A
  new file's name can include `/` (e.g. `controllers/UserController.java`)
  to create it inside a not-yet-existing subdirectory in one step
- "Open Folder…" starts at the currently open project's own folder, if
  there is one, instead of wherever the OS defaults to
- Files open in tabs; clicking an already-open file focuses its tab instead
  of duplicating it
- Dirty files show an asterisk (`*name.kt`) until saved (`Ctrl+S`)
- Closing a dirty tab prompts to save, discard, or cancel; close a tab via
  its `x` button or by middle-clicking it
- `Ctrl+Shift+T` reopens the most recently closed tab (repeatable — each
  press walks further back through recently closed tabs), restoring it
  instead of duplicating it if you already reopened the same file manually
  in the meantime
- Line numbers gutter, right-aligned, scrolling in sync with the text
- Syntax highlighting for Java, Kotlin, YAML, XML, and `.properties`
  (tree-sitter based), each with its own icon in the file tree; error
  squiggles work for all five, not just Java/Kotlin
- Red squiggly underlines on syntax errors, updated live as you type; hover
  one to see the error message as a tooltip
- Multi-cursor editing: `Ctrl+D` selects the word under the cursor, then each
  further press adds the next occurrence as another cursor so you can type
  and edit all of them at once (`Ctrl+D` matches case-insensitively,
  `Ctrl+Shift+D` case-sensitively). Arrow keys, click, or `Escape` collapse
  back to a single cursor.
- Auto-closing brackets and quotes (`{`, `(`, `[`, `"`, `'`)
- Auto-indent on Enter (matches the previous line, plus one level after `{`)
- `Ctrl+J` joins the current line with the next one, trimming the next
  line's leading indentation down to a single separating space (or none, if
  the current line already ends in whitespace or either line is blank)
- New Java/Kotlin files get boilerplate: a class declaration named after the
  file, plus a `package` line inferred from a Maven/Gradle-style
  `src/main/java|kotlin` (or `src/test/...`) path
- Editor font is selectable (Settings > Font); defaults to bundled JetBrains
  Mono (SIL OFL 1.1 — license included alongside the font under
  `crates/app/assets/fonts/`)
- Light theme background uses raylib's `RAYWHITE` (245, 245, 245); syntax
  colors are adapted per-theme so both light and dark stay readable

## Tech stack

| Layer | Choice |
|---|---|
| Language | Rust (backend and frontend) |
| GUI | [`egui`](https://github.com/emilk/egui) + [`eframe`](https://github.com/emilk/egui) |
| Text buffer | [`ropey`](https://github.com/cessen/ropey) |
| Parsing | [`tree-sitter`](https://github.com/tree-sitter/tree-sitter) (Java + Kotlin grammars) |
| File dialogs | [`rfd`](https://github.com/PolyMeilex/rfd) |

## Project structure

```
foxgarden/
  crates/
    core/     # Document, Project, EditorState, Diagnostic — no GUI dependency
    syntax/   # tree-sitter integration: parsing, highlighting, error extraction
    app/      # eframe/egui application: side panel, tabs, editor widget
```

`core` has no dependency on the other two crates and is fully unit-testable
headless. `syntax` depends on `core` for its `Document`/`Diagnostic` types.
`app` is a thin rendering layer over both.

## Building and running

Requires a stable Rust toolchain (install via [rustup](https://rustup.rs) if
you don't have one).

```sh
cargo build --workspace
cargo run -p app
```

## Testing

```sh
cargo test --workspace
```

Automated tests cover `core` (data model, dirty-state, tab lifecycle,
project-tree ordering/skip-list, binary-file detection) and `syntax`
(parsing, incremental reparse, highlighting, error detection) headlessly.
The `app` crate has a handful of headless widget tests (via egui's own test
harness) that exercise highlighting and error-squiggle rendering without
needing a window, including some that drive a real, reused `egui::Context`
with simulated keyboard events and an explicitly focused widget to exercise
multi-cursor editing end to end, plus session-persistence tests against a
hand-rolled fake `eframe::Storage`. Full end-to-end GUI interaction (opening
folders, clicking files, editing) is verified manually.

## Non-goals (checkpoint 1)

Maven awareness, build/run, LSP integration, deprecation warnings, git
integration, and autosave/crash-recovery are explicitly out of scope for this
checkpoint.

## License

MIT — see [LICENSE](LICENSE).
