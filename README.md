# FoxGarden

A light, native code editor for `.java` and `.kt` files, built entirely in Rust.
This is checkpoint 1 of a longer-term goal: a full-featured Spring Boot IDE for
Maven/Kotlin/Java projects. This checkpoint proves out the editor core —
project tree, tabs, dirty-state tracking, syntax highlighting, and syntax-error
squiggles — that later checkpoints (Maven awareness, LSP, build/run) will
layer on top of.

## Features

- Menu bar: File (New File, Open Folder, Save, Close Tab, Exit), Settings
  (Light/Dark theme, editor font), Help (About)
- Side panel with the open project's file tree, with icons per entry
  (folders, `.java`, `.kt`); remembers and reopens the last project folder
  on restart
- Create, rename, and delete files from the side panel — via the "New File…"
  button or right-click on a file/folder (delete asks for confirmation first)
- Files open in tabs; clicking an already-open file focuses its tab instead
  of duplicating it
- Dirty files show an asterisk (`*name.kt`) until saved (`Ctrl+S`)
- Closing a dirty tab prompts to save, discard, or cancel
- Syntax highlighting for Java and Kotlin (tree-sitter based)
- Red squiggly underlines on syntax errors, updated live as you type; hover
  one to see the error message as a tooltip
- Multi-cursor editing: `Ctrl+D` selects the word under the cursor, then each
  further press adds the next occurrence as another cursor so you can type
  and edit all of them at once (`Ctrl+D` matches case-insensitively,
  `Ctrl+Shift+D` case-sensitively). Arrow keys, click, or `Escape` collapse
  back to a single cursor.
- Auto-closing brackets and quotes (`{`, `(`, `[`, `"`, `'`)
- Auto-indent on Enter (matches the previous line, plus one level after `{`)
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

Automated tests cover `core` (data model, dirty-state, tab lifecycle) and
`syntax` (parsing, incremental reparse, highlighting, error detection)
headlessly. The `app` crate has a handful of headless widget tests (via
egui's own test harness) that exercise highlighting and error-squiggle
rendering without needing a window, including some that drive a real,
reused `egui::Context` with simulated keyboard events and an explicitly
focused widget to exercise multi-cursor editing end to end. Full end-to-end
GUI interaction (opening folders, clicking files, editing) is verified
manually.

## Non-goals (checkpoint 1)

Maven awareness, build/run, LSP integration, deprecation warnings, git
integration, and autosave/crash-recovery are explicitly out of scope for this
checkpoint.

## License

MIT — see [LICENSE](LICENSE).
