# ISSUES

Findings from a full-repo code review (2026-08-21), covering all Rust source under `crates/` (~48k lines). Not tied to a specific diff — these are pre-existing issues in the current state of the codebase, split by severity. File:line references are relative to repo root and may drift as the code changes.

## High severity

### 1. Path traversal in "New File" creation — FIXED (2026-08-21)
`crates/app/src/panels/side_panel.rs:260` (`show_new_file_row`)

`dir.join(trimmed)` joins raw, unsanitized user input from the New File dialog onto the target directory. `Path::join` replaces the whole path when the argument is absolute, and a relative argument can still contain `..`. `create_file_with_parents` (lines 299-304) will `create_dir_all` any missing parent directories to make it succeed.

Failure scenario: typing `/home/<user>/.bashrc` (or `../../../../tmp/x`) into the New File box writes/overwrites a file entirely outside the open project.

### 2. Path traversal in rename — FIXED (2026-08-21)
`crates/app/src/panels/side_panel.rs:384-396` (`apply_tree_actions`, rename branch)

`old_path.parent().map(|p| p.join(new_name))` uses the raw rename text field with no rejection of `/`, `..`, or a leading path separator/drive letter. Only an `exists()` check guards the resulting `std::fs::rename`.

Failure scenario: renaming a file to `../../outside.txt` or an absolute path moves it out of the project directory.

### 3. LSP server-to-client requests misrouted as notifications — FIXED (2026-08-21)
`crates/app/src/lsp_client.rs:62-78` (`classify`)

Checks `method` before `id`, so a server-to-client *request* (has both `method` and `id`, e.g. `workspace/configuration`, `client/registerCapability`) is routed as fire-and-forget and its `id` is dropped — never replied to.

Failure scenario: jdt.ls sends `workspace/configuration` right after `initialized`; with no reply, a real jdtls session can stall/hang.

### 4. Panic on stale byte offset in Spring endpoint navigation — FIXED (2026-08-21)
`crates/app/src/app.rs:445` (`resolve_pending_navigation`)

`doc.buffer.byte_to_char(byte)` has no bounds check; `ropey::Rope::byte_to_char` panics if the offset exceeds the rope's current length. Contrast with `build_click` (app.rs:1489), which clamps via `saturating_sub`/`min`.

Failure scenario: open the Spring endpoint map (Ctrl+Shift+E), edit/shorten the target buffer or let it externally reload smaller, then click the (now stale) navigation result — app crashes.

### 5. Undo/redo bypasses `read_only` — FIXED (2026-08-21)
`crates/app/src/widgets/editor/text_area/shell.rs:714-745`

The `Ctrl+Z` / `Ctrl+Shift+Z` / `Ctrl+Y` match arms sit *before* the `_ if read_only => {}` catch-all (line 747), so they execute even when `read_only` is true.

Failure scenario: a document editable earlier in the session (populating `History`) later becomes read-only (large-file guard, external-change lock) while the same widget/`ShellState` persists; pressing Ctrl+Z mutates the supposedly read-only buffer.

## Medium severity

### 6. Concurrent temp-file name collisions
`crates/core/src/gradle.rs:334-338`, `crates/core/src/maven.rs:251`, `crates/core/src/static_analysis.rs:424`

`write_temp_script`, `maven_classpath`, and `run_spotbugs_process` name their scratch file using only `std::process::id()` plus a fixed label. Two concurrent calls to the *same* function within one process collide on the identical path — the module's doc comment only reasons about different labels, not two overlapping calls with the same one.

Failure scenario: a Run-panel launch calling `gradle_classpaths`/`maven_classpath` while a background Spring-config scan calls the same function concurrently — one invocation's output file is truncated/overwritten/removed out from under the other, producing a corrupted or missing classpath.

### 7. Zombie processes from terminal/pty handling
- `crates/app/src/pty_session.rs:178-184` — `Drop for PtySession` only calls `kill()`, never `wait()`, leaving a zombie process-table entry for every closed terminal tab until FoxGarden exits.
- `crates/app/src/pty_session.rs:101-102` — if `try_clone_reader`/`take_writer` fails after the shell is spawned, the already-spawned `child` is dropped without being killed, leaking an orphan process.
- `crates/app/src/terminal.rs:84-93` — `open()` spawns an external terminal emulator and drops the `Child` handle immediately with no `wait()`; each "Open Terminal" leaks a zombie entry.

### 8. No integrity verification on downloaded tool binaries
`crates/app/src/tool_manager.rs:182-191, 226-249`

Checkstyle/PMD/SpotBugs/jdtls/kotlin-language-server archives are downloaded over HTTPS and extracted/executed with no checksum or signature check against the GitHub release. A compromised asset or MITM'd download is silently installed and later run as a jar/binary.

### 9. Synchronous filesystem read on the render thread
`crates/app/src/app.rs:591` (`process_file_events`)

`std::fs::read_to_string(path)` runs inline inside the per-frame `ui()` call for every matching file-watch event — unlike every other I/O path in this file (git, diff, LSP, static analysis), which is backgrounded.

Failure scenario: a slow disk or a burst of watch events (e.g. branch switch touching many open tabs) stalls frame rendering.

### 10. `git_show_head` breaks on Windows
`crates/core/src/diff.rs:207-215`

Builds the git ref as `format!("HEAD:./{}", path.display())`. On Windows, `Path::display()` renders backslashes; `git show` requires forward slashes in a tree-ish path — silently returns empty output on every Windows invocation instead of the file's committed content, breaking diff-view there. Every other cross-platform concern in this crate is explicitly `cfg`-gated; this one isn't.

### 11. Unbounded recursion walking trees
`crates/core/src/project.rs:36-84` (`FileNode::build`) and mirrored in `crates/syntax`: `diagnostics.rs:4`, `fields.rs:83,116`, `methods.rs:44,155`, `spring_endpoints.rs:198,437`, `completion.rs:61,222`, `kotlin_members.rs:12`.

No depth cap on directory/AST tree recursion (only *width* is guarded via `SKIPPED_DIR_NAMES`). An unusually deep source tree, generated code, or deeply nested-but-valid expressions (chained builder calls, nested lambdas) can drive a stack overflow — crashing the app on project open or during live diagnostics (`syntax_errors` runs on every edit).

### 12. `run_command` breaks on quoted arguments
`crates/core/src/run.rs:85,89`

`vm_args`/`program_args` are split via `split_whitespace()`, naively tokenizing on any whitespace. A VM arg or program arg containing an intentionally quoted space (e.g. `-Dmy.prop="hello world"`) gets split into two separate `Command::arg` calls, silently changing the launched process's argv.

### 13. Paste-target guard doesn't canonicalize paths
`crates/app/src/panels/side_panel.rs:475-477` (`is_invalid_paste_target`)

Compares `target_dir == source || target_dir.starts_with(source)` on raw, non-canonicalized paths. A paste through a symlinked subdirectory (or differing `.`/`..` segments) can defeat the "don't paste into your own subtree" guard, letting `copy_recursive` recurse into a directory it's still writing.

## Scalability — large / multi-module projects (LSP)

Found live against a real ~30-module Maven/Kotlin monorepo (`~/bridge/pec`)
on 2026-08-23, not a synthetic case: the user opened the project, did a
couple of Ctrl+Click go-to-definition attempts, and within a few minutes
the system fan spun up loudly. Root-caused via the real `jdtls`/`kotlin-
language-server` process list and jdtls' own `.metadata/.log`, not
guessed. FoxGarden's stated performance bar is Zed-class; today's real
behavior on a large project is the opposite. Distinct from the editor's
own per-frame costs below — this is the LSP integration (`PLAN.md` Track
20) not degrading gracefully at project scale.

### 14. LSP sessions are rooted at the whole opened project, with no per-module scoping or resource ceiling

**Where:** `crates/app/src/lsp_state.rs` (`initialize_params`, `config.root`
— derived from whichever folder is open as the FoxGarden project, `app.rs`'s
`last_project`) and `crates/app/src/lsp_manager.rs` (server spawn — no
memory/CPU limit passed to either child).

Opening a monorepo root hands that same root to both `jdtls` and `kotlin-
language-server` as their own workspace/`rootUri`, with no way to scope a
session to just the module actually being edited, no lazy/on-demand
per-module import, and no CPU/memory ceiling on either child process.
Observed live: `kotlin-language-server` climbed to **618% CPU / 8.8 GB
RSS** within a few minutes against the real 30-module project, system RAM
dropped from 22 GB free to 853 MB free with swap in use, and load average
reached 7.94 on this machine — all with zero indication in FoxGarden's own
UI that indexing was even happening, let alone how far along it was or how
to cancel it. This is consistent with `kotlin-language-server`'s own known
eager-full-classpath-and-source-indexing behavior, just never previously
exercised against a project this large.

**Failure scenario:** any multi-module project past some real (unmeasured)
size threshold turns on the language servers and the machine becomes
unusable for several minutes with no feedback, no cancel button, and no
warning before it happens.

### 15. A failed LSP project import is silently swallowed — no error surfaced to the user

**Where:** `crates/app/src/lsp_state.rs` (wherever `initialize`'s response /
the server's own post-init `!MESSAGE Initialization failed` equivalent is
handled — currently nowhere; `LspState` has no code path that inspects
project-import outcome at all, only the handshake itself).

Real jdtls log line, unprompted, from the same session: `Maven` import
failed for one module (`Error processing changed links in project
description file` / `No file system is defined for scheme:
org.jetbrains.kotlin.core.filesystem`, caused by a stale `.project` left
over from a previous real Eclipse+Kotlin-plugin install of the same
project — not a FoxGarden-caused file, but FoxGarden's jdtls session reads
it as part of the same directory tree regardless). jdtls itself logs
`Initialization failed` and carries on in a degraded state; FoxGarden
never inspects for this and shows nothing — go-to-definition/hover just
silently fail to resolve, with no toast, no log panel entry, nothing to
tell the user *why*. The user's own report ("não funcionou") had no signal
to go on beyond "it didn't work."

**Failure scenario:** any project with metadata jdtls can't import (stale
Eclipse project files, an unreadable `pom.xml`, a missing parent POM, …)
degrades every LSP feature silently, with the user given no way to
distinguish "still indexing" from "permanently broken" from "not a
real project at all."

### 16. Child LSP processes are orphaned (not killed) when the app is terminated externally

**Where:** `crates/app/src/lsp_client.rs:351-365` (`Drop for LspSession`).

`Drop::drop` is the *only* place a session's child is killed+`wait()`ed
(its own doc comment is explicit that this is deliberate, to avoid
zombies). That only runs on a normal Rust unwind/return — an external
`SIGTERM` (a `kill`, a desktop session logout, a window manager force-
quit) terminates the process immediately without running `Drop` at all.
Reproduced live this session: sending `SIGTERM` to the FoxGarden process
left both its `jdtls` and `kotlin-language-server` children running as
orphans, still consuming their full CPU/RAM, until killed separately by
PID. Related to, but distinct from, #7 above (which covers `PtySession`/
terminal children specifically) — no `signal-hook`-style handler exists
anywhere in `app.rs`/`main.rs` to run cleanup on external termination for
*any* child process this app owns.

**Failure scenario:** a user (or a script, or `killall`, or a desktop
session ending) terminates FoxGarden while a language server is running —
the language server(s) keep running and consuming resources indefinitely,
compounding #14 above (nothing bounds their resource use either) until
someone notices and kills them by hand.

### Proposed direction (not designed yet — flagging scope, not a plan)

Worth investigating together, roughly in order of leverage: (a) let the
user pick/override the LSP project root independently of the opened
folder, so a monorepo can be scoped to one module; (b) surface `jdtls`'
own post-init failure/degraded state as a real, visible error/status
rather than silence; (c) install a `SIGTERM`/`SIGINT` handler (or
`ctrlc`-style hook) that runs the same child-kill cleanup `Drop` already
does, so external termination doesn't orphan servers; (d) a background-
work status indicator (indexing progress, cancel) — `kotlin-language-
server` does emit `window/workDoneProgress` LSP notifications during
indexing that nothing in this client currently reads.

---

## Performance (per-frame / hot-path costs)

These aren't correctness bugs but work against the project's stated Zed-level performance bar.

- **`crates/app/src/widgets/editor/widget.rs:427`** — `show()` calls `doc.buffer.to_string()`, materializing the entire rope into a fresh `String` every single frame the editor is visible (not just on edits). Every other expensive per-frame computation in this file (highlight spans, folds, occurrences) has a content-hash-keyed cache; this one doesn't.
- **`crates/app/src/widgets/editor/widget.rs:1961-1970`** — `all_diagnostics` chains and clones five diagnostic `Vec`s into a new `Vec` every frame unconditionally, with no cache guard, unlike the file's other cached per-frame computations.
- **`crates/app/src/widgets/editor/auto_edit.rs:676-711,727-753`** — `apply_auto_pair`/`apply_auto_pair_delete` unconditionally do `.chars().count()` on the whole buffer and `text.to_string()` (full clone) on the common non-pairable fallback path — every non-bracket/quote keystroke pays this. The sibling `apply_auto_indent` was explicitly rewritten to avoid the same cost via `Cow` + O(1) length pre-check; these two never got the fix.
- **`crates/app/src/widgets/editor/text_area.rs:181,435`** — `indent_selected_lines`/`toggle_line_comments` do an O(m) `touched.contains(&pos)` scan per line, making the loop O(n·m). Select-All + Tab/Ctrl+/ on a 10k+ line file goes quadratic.
- **`crates/app/src/widgets/editor/text_area/history.rs:70-78`** — `History::checkpoint` does `past.remove(0)` once past the 500-entry cap, an O(cap) memmove of full-buffer `Snapshot`s on every checkpoint past the limit. A `VecDeque` would make this O(1).
- **`crates/app/src/lsp_state.rs:564-591,734-749`** — `file_uri` calls `Path::canonicalize()` (a syscall) per document on every `publishDiagnostics`, via a linear scan; should cache each document's URI.
- **`crates/syntax/src/document_parser.rs:108-118`** — `diff_edit` calls `byte_to_point` up to three times, each an O(n) linear scan from the start; could be combined into one pass. Runs on every keystroke's incremental reparse.
- **`crates/syntax/src/highlight.rs:138-149`** — `highlight_spans` allocates a fresh `HashMap`/`Vec` and re-sorts every call, invoked from the editor's layouter closure every frame the editor is shown — the query itself is cached (per the doc comment at 89-92) but this per-frame collection work isn't.

## Low severity / reuse & simplification

- **`crates/app/src/panels/`** — the same "spawn thread, stash `Receiver`, `try_recv`/clear-on-`Disconnected`" pattern is hand-duplicated across `git_stage.rs:37-60`, `static_analysis.rs:103-135`, `jdk_registry.rs:55-91,142-154`, `new_project.rs:60-73,134-139`. A shared generic `poll<T>(&mut Option<Receiver<T>>) -> Option<T>` helper would remove ~60-80 duplicated lines.
- **`crates/app/src/widgets/editor/widget/widget_test/common_test.rs`** and siblings (`click_test.rs`, `codegen_test.rs`, `painting_test.rs`, `selection_test.rs`, `context_menu_test.rs`) — a ~25-positional-argument `show(...)` test call is copy-pasted 47 times with unlabeled args (e.g. runs of four bare `false` in a row). A builder/`ShowArgs` struct would collapse this to one definition and remove the risk of silently mis-threading a positional arg on future signature changes.
- **`crates/app/src/lsp_client.rs:174-249`** — `pending` request map has no timeout/eviction; a hung (not crashed) server leaks `Sender`s indefinitely.
- **`crates/app/src/lsp_state.rs:483-500`** — early return on a failed `didClose` notification skips removing that path from `open_documents`; harmless since the session is marked `Failed` shortly after.
- **`crates/app/src/lsp_manager.rs`** — redundant concurrent JDK scans between background snapshot refresh and direct `detect_java_home()` (harmless, idempotent).
- **`crates/app/src/auto_save.rs:60`** — `AfterIdle` mode's idle clock isn't reset on fire, causing redundant re-checks each idle frame; no functional bug since the consumer is a no-op once clean.
- **`crates/app/src/widgets/editor/widget.rs:1541,1569,1633,1663,1688,1707,1735,1787`** — several action handlers (Ctrl+D, Ctrl+J, generate accessors/method/override dialogs) each independently call `doc.buffer.to_string()` instead of reusing the `old_text` already computed earlier in the same frame. Only fires on the frame the action triggers, so low severity.
- **`crates/app/src/widgets/editor/widget.rs:275`** — `apply_edit` does `parser.tree().expect("just reparsed")` right after `reparse`; if `reparse` ever leaves the tree unset (pathological/huge input, internal parse failure), this panics instead of degrading gracefully. Not confirmed reachable — worth checking against `IncrementalParser::reparse`'s actual guarantees.
- **`crates/app/src/widgets/editor/text_area/input.rs:488-495`** — `clamp_out_of_hidden`: if a hidden/folded range starts at line 0, the "move out of range" marker line computation can itself land back inside the folded region. Likely unreachable (folds normally start after a visible header line) but worth a guard.
