# FoxGarden

A light, native code editor for `.java` and `.kt` files, built entirely in Rust.
This is checkpoint 1 of a longer-term goal: a full-featured Spring Boot IDE for
Maven/Kotlin/Java projects. Checkpoint 1 (released as `v0.2.0`) proves out the
editor core — project tree, tabs, dirty-state tracking, syntax highlighting,
syntax-error squiggles, code generation, and live templates. Work is now
moving on to checkpoint 2 (Maven awareness, LSP, build/run) on the
`ide-henshin` branch.

Speed is a hard requirement, not a nice-to-have — [Zed](https://zed.dev) is
the bar FoxGarden holds itself to for startup time, input latency, and idle
resource use. The later Maven/LSP/build-tooling checkpoints look to Zed's own
[Java](https://github.com/zed-extensions/java) and
[Kotlin](https://github.com/zed-extensions/kotlin) extensions as prior art.

Runs on Windows, macOS, and Linux. Everything is built on portable Rust
`std`/dependency APIs (`egui`/`eframe`, `rfd`, `ropey`, `tree-sitter`,
`arboard`); the "Open Terminal" button is the one feature with real
per-OS behavior (see `crates/app/src/terminal.rs`), since there's no
cross-platform "open a terminal here" API to call instead.

## Features

### Project, files, and sessions

- Menu bar: File (New File, Open Folder, Save, Close Tab, Reopen Closed Tab,
  Exit), Settings (Light/Dark theme, Font…, indentation), Tools (Java code
  generation, case conversion), View (Zen Mode, Side Panel, Blinking Cursor,
  Editor Outline, word wrap/whitespace/indent guides/sticky scroll), Help
  (Live Templates…, About). Every action with a keyboard shortcut shows it
  right beside the item, in both the menu bar and the editor's right-click
  context menu
- Help > About shows the app version, author, and a link to this repo
- `F11` toggles Zen Mode — hides the menu bar and side panel, leaving just
  the tab bar and editor; `F11` again brings them back
- Side panel with the open project's file tree, with icons per entry
  (folders, `.java`, `.kt`, `.properties`, `.yml`/`.yaml`, `.xml`); folders
  are always listed before files, each group sorted alphabetically, and
  VCS/build/dependency noise (`.git`, `target`, `node_modules`, `build`,
  `.idea`, `dist`, `out`, `.svn`, `.hg`) is skipped so it never clutters the
  tree or slows down opening a large real-world project
- `Ctrl+B` (or the "◀"/"▶" button pinned to the right of the menu bar, or the
  "◀" button in the side panel's own toolbar, or View > Side Panel) toggles
  the side panel; its width (drag the edge to resize) and shown/hidden state
  both persist across restarts
- Files with no recognized extension still open and edit as plain text (no
  highlighting or diagnostics); actual binaries (images, class files, jars,
  fonts) are detected and refused rather than read in full
- Remembers and restores your session on restart: the last project folder,
  every tab that was open, which one was focused, every Settings/View
  choice, and the side panel's width and shown/hidden state
- Create, rename, and delete files from the side panel — via the "New
  File…" button, `Ctrl+N`, File > New File…, or right-click on a
  file/folder (delete asks for confirmation first); `Enter` confirms a
  new-file/rename input, `Escape` cancels it. A new file's name can include
  `/` (e.g. `controllers/UserController.java`) to create it inside a
  not-yet-existing subdirectory in one step
- "Open Folder…" starts at the currently open project's own folder, if
  there is one, instead of wherever the OS defaults to
- A 💻 button beside "New File" opens a system terminal at the project
  root (Terminal.app on macOS, `cmd` on Windows, whichever common Linux
  terminal emulator is actually installed)
- `Ctrl+E`: a searchable recent-files popup listing open tabs, then
  recently-closed ones, most-recent first — type to filter, arrow keys +
  Enter or click to jump
- `Escape` closes whichever dialog is on top (error, delete/close
  confirmation, About, Live Templates, the getters/setters picker), the
  same as its Cancel/OK/Close button — never Save or Delete
- New Java/Kotlin files get boilerplate: a class declaration named after the
  file, plus a `package` line inferred from a Maven/Gradle-style
  `src/main/java|kotlin` (or `src/test/...`) path

### Tabs

- Files open in tabs; clicking an already-open file focuses its tab instead
  of duplicating it
- Dirty files show an asterisk (`*name.kt`) until saved (`Ctrl+S`)
- Closing a dirty tab prompts to save, discard, or cancel; close a tab via
  its `x` button or by middle-clicking it
- `Ctrl+Shift+T` reopens the most recently closed tab (repeatable — each
  press walks further back through recently closed tabs), restoring it
  instead of duplicating it if you already reopened the same file manually
  in the meantime

### Syntax and diagnostics

- Line numbers gutter, right-aligned, scrolling in sync with the text
- Syntax highlighting for Java, Kotlin, YAML, XML, and `.properties`
  (tree-sitter based), each with its own icon in the file tree; error
  squiggles work for all five, not just Java/Kotlin
- Red squiggly underlines on syntax errors, updated live as you type; hover
  one to see the error message as a tooltip
- Trailing whitespace is stripped from every line on save

### Editing

- Multi-cursor editing: `Ctrl+D` selects the word under the cursor, then each
  further press adds the next occurrence as another cursor so you can type
  and edit all of them at once (`Ctrl+D` matches case-insensitively,
  `Ctrl+Shift+D` case-sensitively). Arrow keys, click, or `Escape` collapse
  back to a single cursor.
- Auto-closing brackets and quotes (`{`, `(`, `[`, `"`, `'`), with
  skip-over-existing-closer behavior
- Wrap-selection: typing a pairable character while a selection is active
  wraps the selection in it instead of replacing it
- Auto-indent on Enter (matches the previous line, plus one level after `{`)
- `Ctrl+J` joins the current line with the next one, trimming the next
  line's leading indentation down to a single separating space (or none, if
  the current line already ends in whitespace or either line is blank)
- `Ctrl+/` toggles `//` line comments on the current line or every line a
  selection touches
- `Alt+↑`/`Alt+↓` move the current line up/down; `Alt+Shift+↑`/`Alt+Shift+↓`
  duplicate it
- `Ctrl+Shift+U`/`Ctrl+Shift+L` (or Tools menu) convert the selection to
  UPPERCASE/lowercase; Tools menu also offers Title Case
- Smart Home: `Home` toggles between the line's first non-whitespace
  character and column 0; `Shift+Home` does the same while selecting
- Live templates: type a trigger word, press Tab with no selection to
  expand it. Built-in triggers — Global (any file, language or not):
  `pipe`; Java: `sout`, `souf`, `serr`, `psvm`, `fori`, `iter`, `ifn`,
  `inn`, `trycatch`; Kotlin: `sout`, `serr`, `main`, `fori`, `ifn`, `inn`,
  `trycatch`. Help > Live Templates… lists every trigger and lets you add,
  edit, or remove your own (Global, Java, or Kotlin) — a custom trigger
  overrides a built-in one of the same name, and custom templates persist
  across restarts
- Right-click the editor for Undo, Redo, Cut, Copy, Paste, Select All,
  Toggle Line Comment, Duplicate Line, and Save — each with its keyboard
  shortcut shown beside it

### Java code generation

- `Ctrl+Shift+G` (or Tools > Generate Getters/Setters): generates accessors
  for the whole file's classes, not just whichever one the cursor happens
  to be in. A single eligible class generates immediately for every field;
  more than one opens a picker to choose the class, then which fields.
  `final` fields get a getter only; static fields are skipped.

### Look and feel

- Settings > Font… opens a modal with the editor font (bundled JetBrains
  Mono, SIL OFL 1.1 — license included alongside the font under
  `crates/app/assets/fonts/`, or the system default monospace) and a numeric
  font-size box together, instead of a separate submenu and menu row
- Light theme background uses raylib's `RAYWHITE` (245, 245, 245); syntax
  colors are adapted per-theme so both light and dark stay readable
- Sticky scroll (View > Sticky Scroll): pins the enclosing class/method
  signature to the top of the editor while scrolling through its body, so you
  never lose track of where you are (Java; persisted)
- The text caret blinks (View > Blinking Cursor to turn off), resetting to
  solid on every click, keystroke, or focus change
- The active editor pane is outlined — subdued when unfocused, highlighted
  while focused, same as a real `egui::TextEdit` — toggle via View > Editor
  Outline

## Tech stack

| Layer | Choice |
|---|---|
| Language | Rust (backend and frontend) |
| GUI | [`egui`](https://github.com/emilk/egui) + [`eframe`](https://github.com/emilk/egui) |
| Text buffer | [`ropey`](https://github.com/cessen/ropey) |
| Parsing | [`tree-sitter`](https://github.com/tree-sitter/tree-sitter) — one grammar crate each for Java, Kotlin (`tree-sitter-kotlin-ng`), YAML, XML, `.properties`, and Dockerfiles (`tree-sitter-containerfile`) |
| File dialogs | [`rfd`](https://github.com/PolyMeilex/rfd) |
| Clipboard | [`arboard`](https://github.com/1Password/arboard) |
| File-system watching | [`notify`](https://github.com/notify-rs/notify) (external-change detection/conflict banner) |
| Terminal panel | [`portable-pty`](https://github.com/wez/wezterm) + [`vt100`](https://github.com/doy/vt100-rust) |
| Config/report parsing | [`serde`](https://serde.rs)/`serde_json` (run configurations, live templates, GitHub release API responses), [`quick-xml`](https://github.com/tafia/quick-xml) (Checkstyle/PMD report parsing) |
| Tool downloader | [`ureq`](https://github.com/algesten/ureq) (HTTP), [`zip`](https://github.com/zip-rs/zip2) (archive extraction), [`directories`](https://github.com/dirs-dev/directories-rs) (per-OS cache directory) — see "External tools" below |
| Test fixtures | [`tempfile`](https://github.com/Stebalien/tempfile) (dev-dependency only) |

### External tools (optional)

Tools > Run Checkstyle / Run PMD shell out to [Checkstyle](https://checkstyle.org)/[PMD](https://pmd.github.io)
— neither is linked into the FoxGarden binary itself, unlike
`portable-pty`/`vt100` above (both are JVM tools; PMD's own release alone
is ~70MB, which would work directly against this project's own
lightweight/fast-startup goal). Settings > External Tools…'s own Install
button downloads each tool's official release into a per-user cache
directory on first use and fills in its binary/config fields — a JVM
still needs to already be on `PATH`, same as running any other Java tool.
"Install" always installs a specific version this app has verified works
(not necessarily whatever a tool's own GitHub release page currently
calls "latest" — Checkstyle's newest major line, for one, needs a newer
JDK than a plain Java 17 install has); "Check for Updates" shows what's
newer on GitHub without installing it automatically. None of this is
required for anything else in the app — only those two Tools-menu items
have an external-tool dependency at all. SpotBugs' binary is installable
the same way, but "Run SpotBugs" itself isn't wired up yet (it needs a
compiled-classes directory this app has no way to produce until build
integration lands — see `PLAN.md`'s own tracking of that gap).

## Project structure

```
foxgarden/
  crates/
    core/         # Document, Project, EditorState, Diagnostic — no GUI dependency
    syntax/       # tree-sitter integration: parsing, highlighting, error extraction
    app/          # eframe/egui application: side panel, tabs, editor widget
    test-support/ # shared test-fixture helpers (dev-dependency only)
```

`core` has no dependency on the other two crates and is fully unit-testable
headless. `syntax` depends on `core` for its `Document`/`Diagnostic` types.
`app` is a thin rendering layer over both. `test-support` sits outside that
chain as a dev-dependency of the others, so their tests share one place for
fixture helpers (a temp directory plus a file, or an opened `Document`)
instead of each reimplementing their own.

## Building and running

Requires a stable Rust toolchain (install via [rustup](https://rustup.rs) if
you don't have one).

```sh
cargo build --workspace
cargo run -p foxgarden
```

For a standalone binary (`target/release/foxgarden`), build with `--release`
instead:

```sh
cargo build --release --bin foxgarden
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
hand-rolled fake `eframe::Storage`.

End-to-end tests live in `crates/app/src/app/e2e_test/` and drive the *whole*
app — a real `FoxGardenApp` rendering real frames via
[`egui_kittest`](https://docs.rs/egui_kittest) against a temp project —
clicking, typing, and asserting on what the accessibility tree says is on
screen plus what ended up on disk:

```sh
cargo test -p foxgarden e2e::
```

They're split by user-facing area: `project_tree` (listing, collapsing,
icons, skip-list, creating files), `tabs` (opening, switching, closing,
reopening, read-only, binary-file errors), `editing` (dirty asterisk,
saving, the close-confirmation prompt), `editor_input` (undo, line
comment/join/move/duplicate, indent, auto-close, live templates,
multi-cursor), `completion`, `diagnostics` (squiggle sources in Java and
Kotlin), `file_operations` (rename/delete/copy/cut/paste and multi-select),
`menus` (every File/Settings/Tools/Run/View/Help item), `navigation`
(`Ctrl+P`/`Ctrl+E`/`Ctrl+Shift+E`), and `panels`.

Two things they can't drive through the UI: the native "Open Folder…"
dialog (`rfd` opens an OS window outside egui's event loop, so tests call
`EditorState::open_project` directly, exactly as the dialog's callback
does) and clicking in the editor to place the caret (it's custom-painted,
so it has no accessibility node — tests use `widgets::editor::jump_to`,
then send real keystrokes). Whatever an external process answers (LSP,
Checkstyle/PMD, `git`, the terminal's shell), the file watcher's
external-change paths, auto-save's timers, and session persistence are
covered by unit tests instead — from a frame loop those would assert on a
race. Anything mouse-driven beyond the above is still verified manually.

## Non-goals (checkpoint 1)

Maven awareness, build/run, LSP integration, deprecation warnings, git
integration, and autosave/crash-recovery are explicitly out of scope for this
checkpoint.

## License

MIT — see [LICENSE](LICENSE).
