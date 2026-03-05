# Story 10.5: Polish — Visual Vocabulary, Colors & Final Formatting

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer using BMAD Bot daily,
I want a polished, professional terminal output that is consistent across terminals and configurable,
So that the daemon feels like a production-quality tool.

## Acceptance Criteria

1. **Given** the `ConsoleRenderer` implementation **When** I review the visual output on different terminals **Then** the following visual vocabulary is consistently applied:
   - `●` (green) — completed action
   - `◉` (cyan, animated spinner) — in-progress action
   - `└` (dim/gray) — sub-detail / child event
   - `✗` (red) — error
   - `⚠` (yellow) — warning / escalation / retry
   - `→` (dim) — LLM request sent
   - `←` (dim) — LLM response received
   - Indentation: 2 spaces per nesting level
   - Elapsed time displayed on completed phases: `● Dev Session [47s]`

2. **Given** the `ui_mode` configuration in `bmad-bot.yaml` **When** `ui_mode` is set to `"fancy"` (default) **Then** `ConsoleRenderer` uses animated spinners and full ANSI colors **When** `ui_mode` is set to `"plain"` **Then** `ConsoleRenderer` disables colors (`console::set_colors_enabled(false)`) and uses static indicators instead of animated spinners (e.g., `...` instead of `◉`) **When** `ui_mode` is set to `"silent"` **Then** `NullRenderer` is used — no stdout output at all.

3. **Given** stdout is not a TTY (piped output, CI environment) **When** the daemon starts **Then** `NullRenderer` is automatically selected regardless of `ui_mode` setting **And** the stdout `tracing` layer is preserved for backward compatibility.

4. **Given** the `ConsoleRenderer` is active **When** a long-running phase completes **Then** the elapsed time is displayed in human-readable format:
   - Under 60s: `[47s]`
   - 1–60 minutes: `[3m 12s]`
   - Over 60 minutes: `[1h 23m]`

5. **Given** the daemon processes a full story pipeline **When** I observe the terminal output **Then** the output resembles the aspirational example in the Dev Notes section (proper nesting, consistent glyph usage, elapsed times on phases).

6. **Given** the README.md **When** I inspect the documentation **Then** there is a section describing the terminal output format **And** it explains the `ui_mode` configuration option with examples **And** it mentions TTY auto-detection behavior.

7. **Given** all existing tests **When** they run **Then** they all pass without modification **And** no test output is polluted by `ConsoleRenderer` output (all tests use `NullRenderer`).

## Tasks / Subtasks

- [ ] Task 1: Implement `format_duration()` helper in `ConsoleRenderer` (AC: #4)
  - [ ] 1.1 Add a private method `fn format_duration(d: Duration) -> String` to `ConsoleRenderer` (or as a module-level function in `console.rs`)
  - [ ] 1.2 Logic: if `d.as_secs() < 60` → `"{secs}s"`, if `d.as_secs() < 3600` → `"{mins}m {secs}s"`, else → `"{hours}h {mins}m"`
  - [ ] 1.3 Update `phase_complete()` to use `format_duration()` instead of the current `duration.as_secs()` raw seconds display
  - [ ] 1.4 Add unit tests for `format_duration()` covering: 0s, 47s, 60s, 90s, 3599s, 3600s, 5000s edge cases

- [ ] Task 2: Fix LLM event glyphs — `→` for request, `←` for response (AC: #1)
  - [ ] 2.1 In `llm_request()`: change glyph from `►` (dim) to `→` (dim) — the `→` arrow is the specified visual vocabulary for "LLM request sent"
  - [ ] 2.2 In `llm_response()`: change glyph from `●` (green dim) to `←` (dim) — the `←` arrow is the specified visual vocabulary for "LLM response received"
  - [ ] 2.3 In `tool_call()`: keep `►` (dim) as-is — `►` is appropriate for tool calls (outgoing call) and is distinct from LLM arrows
  - [ ] 2.4 Verify all other glyphs match the visual vocabulary: `●` (green) completed, `◉` (cyan) in-progress, `└` (dim) sub-detail, `✗` (red) error, `⚠` (yellow) warning — these are already correct from Story 10.1

- [ ] Task 3: Add `"plain"` mode support to `ConsoleRenderer` (AC: #2)
  - [ ] 3.1 Add a `plain_mode: bool` field to `ConsoleRenderer` struct
  - [ ] 3.2 Update `ConsoleRenderer::new()` to accept a `plain: bool` parameter: `ConsoleRenderer::new(plain: bool)`
  - [ ] 3.3 When `plain_mode` is true: disable spinner ticking in `create_spinner()` — use `ProgressDrawTarget::hidden()` or skip `enable_steady_tick()` and use a static message prefix like `... ` instead of animated `◉`
  - [ ] 3.4 When `plain_mode` is true: replace Unicode glyphs with ASCII equivalents in all output: `●` → `[ok]`, `◉` → `...`, `✗` → `[ERR]`, `⚠` → `[WARN]`, `→` → `->`, `←` → `<-`, `└` → `  `, `►` → `>>`. **Also replace the `→` in `story_complete()`** (L122: `format!(" → {url}")`) with `" -> {url}"` in plain mode — this inline Unicode arrow is easy to miss but breaks ASCII consistency.
  - [ ] 3.5 Update `UiHandle::console()` to `UiHandle::console(plain: bool)` — pass the `plain` flag through
  - [ ] 3.6 Update `run_start()` in `cli/mod.rs`: pass `config.ui_mode == "plain"` to `UiHandle::console()`. Note: the `console::set_colors_enabled(false)` call already exists from Story 10.2 — keep it
  - [ ] 3.7 Add unit tests: construct `ConsoleRenderer::new(true)` and exercise all methods without panic

- [ ] Task 4: Refine nesting and indentation consistency (AC: #1, #5)
  - [ ] 4.1 Audit all `ConsoleRenderer` methods for consistent indentation levels:
    - **Level 0** (no indent): system events — `daemon_start`, `stories_found`, `crash_recovery_*`, `shutdown_requested`, `poll_cycle`, `batch_start`, `batch_complete`
    - **Level 0** (no indent): story-level — `story_start`, `story_complete`, `story_error`, `story_escalated`
    - **Level 1** (2 spaces): phase events — `phase_start`, `phase_complete`, `phase_error`
    - **Level 2** (4 spaces): session events — `chat_turn`, `activation_start`, `activation_complete`, `completion_detected`, `llm_request`, `llm_response`, `llm_error`, `llm_retry`
    - **Level 3** (6 spaces): tool events — `tool_call`, `tool_result`
  - [ ] 4.2 Fix any inconsistencies found. Current code review shows: `story_start` uses no indent (correct), `phase_start`/`phase_complete` use 2-space indent (correct), `chat_turn` uses 4-space indent with `└` (correct), `tool_call`/`tool_result` use 6-space indent (correct), `llm_request`/`llm_response` use 4-space indent (correct), `poll_cycle` uses no indent with `└` (correct). `activation_start`/`activation_complete` use 4-space indent (correct). `batch_start` uses no indent with `◉` (correct). Most are already consistent — verify and fix any drift.
  - [ ] 4.3 Add story key context to phase spinners where possible: the `phase_start` spinner message should include the current story context if available. **Note:** This is a nice-to-have — the current `phase_start` takes only `phase_name: &str`, so the story context is not available inside the renderer. Skip if it would require trait changes.

- [ ] Task 5: Update README.md with Terminal Output documentation (AC: #6)
  - [ ] 5.1 Add a new `## Terminal Output` section after the `## Configuration` section (before `## Sprint Status Format`)
  - [ ] 5.2 Document the `ui_mode` config option: `"fancy"` (default, animated spinners + colors), `"plain"` (ASCII, no colors, no spinners), `"silent"` (no output)
  - [ ] 5.3 Document TTY auto-detection: non-TTY → `NullRenderer` regardless of `ui_mode`, stdout tracing layer preserved
  - [ ] 5.4 Show the `ui_mode` field in the existing `bmad-bot.yaml` example block (it's currently missing) — add `ui_mode: fancy` after `log_file: bmad-bot.log`
  - [ ] 5.5 Add an aspirational terminal output example showing the visual vocabulary in action (use the example from the epics)
  - [ ] 5.6 Document the visual vocabulary table: glyph → meaning → color

- [ ] Task 6: Update Project Structure in README.md (AC: #6)
  - [ ] 6.1 The `Project Structure` section in README.md is outdated — it lists `tools/git.rs`, `tools/fs.rs`, `tools/terminal.rs` but the actual codebase has `tools/edit_file.rs`, `tools/read_file.rs`, `tools/grep.rs`, `tools/find_path.rs`, `tools/list_directory.rs`, `tools/git.rs`, `tools/terminal.rs` (Epic 8 changes). Add the `ui/` module to the project structure tree with: `ui/mod.rs`, `ui/renderer.rs`, `ui/console.rs`, `ui/null.rs`. Also add `session/agent.rs` and `mcp/` module if missing. Fix the tools listing to reflect the actual codebase.

- [ ] Task 7: Run full test suite and linting (AC: #7)
  - [ ] 7.1 Run `cargo test` — ALL existing tests must pass with zero failures
  - [ ] 7.2 Run `cargo clippy` — zero new warnings
  - [ ] 7.3 Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Architecture Compliance

- **UI events are fire-and-forget:** All `UiRenderer` methods take `&self` and return `()`. No error propagation from UI to business logic. `ConsoleRenderer` handles errors internally via `tracing::debug!`.
- **`UiRenderer` trait MUST NOT change:** This story is **polish only** — no new methods, no signature changes on the trait. The trait is the stable contract. All changes are internal to `ConsoleRenderer` and `UiHandle`.
- **`NullRenderer` MUST NOT change:** It's already a complete no-op implementation. No modifications needed.
- **`tracing` calls remain unchanged:** All existing `tracing::info!`, `tracing::warn!`, etc. stay exactly as they are. UI events are a separate concern.
- **Tests use `NullRenderer`:** All tests continue using `UiHandle::null()`. No test behavior changes. The `ConsoleRenderer::new(bool)` signature change in Task 3 only affects the `UiHandle::console()` constructor.

### Aspirational Terminal Output Example

The final polished output should resemble:

```
● BMAD Bot started — polling every 30s
● Found 2 eligible stories

◉ Story 4-2 — Agent Session Setup & Chat Loop
  ◉ Dev Session
    → dev turn 1
    ← dev turn 1 — 4096 bytes
      ► read_file src/session/runner.rs
        └ 3567 lines (outline mode)
      ► edit_file src/session/runner.rs (edit)
      ► git commit "feat(session): add context limit recovery"
      ► terminal cargo test session::tests
        └ 42 tests passed
  ● Dev Session [47s]
  ● Push Branch [2s]
  ● Create PR [1s]
    └ https://github.com/jbanety/bmad-bot/pull/42
  ◉ Code Review
    → code-review turn 1
    ← code-review turn 1 — 2048 bytes
      ► edit_file src/session/runner.rs (edit)
      ► git commit "fix(session): handle edge case in recovery"
  ● Code Review [23s]
  ● Notification [0s]
● Story 4-2 complete → https://github.com/jbanety/bmad-bot/pull/42

◉ Story 4-3 — Pre-Development Preparation
  ◉ Dev Session
    → dev turn 1
```

**Note:** Tool events (`►`) are at 6-space indent (Level 3), session/LLM events (`→`/`←`) at 4-space indent (Level 2), phase events at 2-space indent (Level 1). The `story_complete` format matches the current code output (`● Story {key} complete → {url}`), not the epics' aspirational format which used a different structure.

### Current `ConsoleRenderer` Glyph Audit

Current implementation (from Story 10.1 + 10.4) vs. target visual vocabulary:

| Method | Current Glyph | Target Glyph | Status |
|--------|---------------|--------------|--------|
| `story_start` | `◉` cyan | `◉` cyan | ✅ Correct |
| `story_complete` | `●` green | `●` green | ✅ Correct |
| `story_error` | `✗` red | `✗` red | ✅ Correct |
| `story_escalated` | `⚠` yellow | `⚠` yellow | ✅ Correct |
| `batch_start` | `◉` cyan | `◉` cyan | ✅ Correct |
| `batch_complete` | `●` green | `●` green | ✅ Correct |
| `phase_start` | `◉` cyan (spinner) | `◉` cyan (spinner) | ✅ Correct |
| `phase_complete` | `●` green | `●` green + humanized duration | ⚠️ Fix duration format |
| `phase_error` | `✗` red | `✗` red | ✅ Correct |
| `chat_turn` | `└` dim | `└` dim | ✅ Correct |
| `activation_start` | `◉` cyan | `◉` cyan | ✅ Correct |
| `activation_complete` | `●` green | `●` green | ✅ Correct |
| `completion_detected` | `●` green | `●` green | ✅ Correct |
| `tool_call` | `►` dim bold | `►` dim bold | ✅ Correct (distinct from LLM) |
| `tool_result` | `●` green dim | `●` green dim | ✅ Correct |
| `llm_request` | `►` dim | `→` dim | ❌ **Must change** |
| `llm_response` | `●` green dim | `←` dim | ❌ **Must change** |
| `llm_error` | `✗` red | `✗` red | ✅ Correct |
| `llm_retry` | `⚠` yellow | `⚠` yellow | ✅ Correct |
| `daemon_start` | `●` green | `●` green | ✅ Correct |
| `poll_cycle` | `└` dim | `└` dim | ✅ Correct |
| `stories_found` | `●` green | `●` green | ✅ Correct |
| `crash_recovery_start` | `⚠` yellow | `⚠` yellow | ✅ Correct |
| `crash_recovery_complete` | `●` green | `●` green | ✅ Correct |
| `shutdown_requested` | `⚠` yellow | `⚠` yellow | ✅ Correct |

**Summary:** Only 2 glyphs need changing (`llm_request` and `llm_response`) + 1 formatting improvement (`phase_complete` duration).

### `plain` Mode — ASCII Fallback Table

| Fancy Glyph | Plain Equivalent | Notes |
|-------------|------------------|-------|
| `●` (green) | `[ok]` | Completed action |
| `◉` (cyan) | `...` | In-progress (static, no spinner) |
| `✗` (red) | `[ERR]` | Error |
| `⚠` (yellow) | `[WARN]` | Warning / escalation |
| `→` (dim) | `->` | LLM request |
| `←` (dim) | `<-` | LLM response |
| `└` (dim) | `  ` | Sub-detail (extra indent) |
| `►` (dim) | `>>` | Tool call |
| `→` in `story_complete` | `->` | PR URL separator (`" → {url}"` → `" -> {url}"`) |

When `plain_mode` is true, `console::set_colors_enabled(false)` is already called in `run_start()` (Story 10.2). The `ConsoleRenderer` should additionally use ASCII glyphs and disable spinner animation (`enable_steady_tick` skipped or `ProgressDrawTarget::hidden()`).

**Note:** `bmad-bot.yaml.example` already contains the `ui_mode` field (added during Story 10.2, L16-20) — no update needed there.

### `ConsoleRenderer::new()` Signature Change Impact

Changing `ConsoleRenderer::new()` to `ConsoleRenderer::new(plain: bool)` affects:
- `UiHandle::console()` → `UiHandle::console(plain: bool)` — 1 production call site in `run_start()`
- **5 test call sites in `console.rs`** that call `ConsoleRenderer::new()` directly:
  1. `test_console_renderer_new_does_not_panic`
  2. `test_console_renderer_all_methods_do_not_panic`
  3. `test_phase_start_duplicate_clears_previous_spinner`
  4. `test_phase_complete_without_start_does_not_panic`
  5. `test_phase_error_without_start_does_not_panic`
- **2 test call sites in `mod.rs`**:
  1. `test_console_renderer_implements_ui_renderer_trait_object` — calls `console::ConsoleRenderer::new()` directly
  2. `test_ui_handle_console_creation_does_not_panic` — calls `UiHandle::console()` (indirect)
- **Total: 8 call sites** (1 production + 7 tests). Update all test sites to pass `false` (fancy mode is the default for tests).

### `ui_mode` in README Configuration Block

The `bmad-bot.yaml` example in README.md (L236-283) currently does NOT include `ui_mode`. Add it between `log_file` and `git_provider`:

```yaml
log_file: bmad-bot.log
ui_mode: fancy                # "fancy" (default), "plain", or "silent"
```

### Project Structure in README Is Outdated

The `Project Structure` section (README.md L439-501) lists `tools/git.rs`, `tools/fs.rs`, `tools/terminal.rs`. The actual codebase after Epic 8:
- `tools/mod.rs`
- `tools/edit_file.rs` (Story 8.2)
- `tools/read_file.rs` (Story 8.1)
- `tools/grep.rs` (Story 8.3)
- `tools/find_path.rs` (Story 8.3)
- `tools/list_directory.rs` (Story 8.4)
- `tools/git.rs`
- `tools/terminal.rs`

Also missing: `ui/` module (Epic 10), `mcp/` module (Epic 9), `session/agent.rs` (Story 4.2+), `llm/agent_factory.rs` (Story 4.5).

### Technical Requirements

- **Rust edition 2024** — all code follows edition 2024 conventions (rustc 1.86+)
- **`#![deny(clippy::all)]`** — zero clippy warnings
- **Error handling:** No `unwrap()` or `expect()` in production code — only in tests
- **Doc comments:** `///` mandatory on any new public or `pub(crate)` functions
- **No `println!` / `eprintln!`** in daemon runtime code — use `UiHandle` for user-facing output, `tracing` for debug logging

### Library & Framework Requirements

- **`indicatif`** 0.18.4 (already in `Cargo.toml`) — `MultiProgress`, `ProgressBar`, `ProgressStyle`, `ProgressDrawTarget`
- **`console`** 0.16.2 (already in `Cargo.toml`) — `style()`, `set_colors_enabled()`, `Term::stdout().is_term()`
- **No new dependencies required** — this story is pure polish of existing code

### File Structure Requirements

- **`src/ui/console.rs`** — primary modification target: `format_duration()`, glyph fixes, `plain_mode` support, `ConsoleRenderer::new(bool)` signature
- **`src/ui/mod.rs`** — update `UiHandle::console()` to `UiHandle::console(plain: bool)`
- **`src/cli/mod.rs`** — update `UiHandle::console()` call site in `run_start()` to pass plain flag
- **`README.md`** — add Terminal Output section, update config example, update project structure
- **No new Rust files** — this story only modifies existing files

### Testing Requirements

- All tests use `NullRenderer` via `UiHandle::null()` — zero test pollution
- Tests calling `ConsoleRenderer::new()` or `UiHandle::console()` must be updated to pass `false` for the plain parameter
- Add unit tests for `format_duration()` helper
- Add unit test constructing `ConsoleRenderer::new(true)` (plain mode) and exercising all methods without panic
- Run `cargo test` — ALL tests must pass
- Run `cargo clippy` — zero new warnings
- Run `cargo fmt --check` — clean

### Previous Story Intelligence

**Story 10.4 (Review Integration — immediate predecessor, `review`):**
- Changed `None` → `Some(&self.ui)` for all `stream_chat()`/`activate_agent()` calls in review
- Added `ui.tool_result("pr_comment", ...)` events in pipeline for PR comment success/error
- Refactored `add_comment` sites in `pipeline.rs` from combined `if let` to `match` pattern
- Made `truncate_summary` `pub(crate)` in `session/runner.rs` for cross-module reuse
- All 1164 tests pass, zero new clippy warnings
- **Files modified:** `src/review/mod.rs`, `src/session/runner.rs`, `src/pipeline.rs`

**Story 10.3 (Session Integration, `review`):**
- Added `ui: Option<&UiHandle>` parameter to `streaming_chat()` and `activate_agent()`
- Tool call interception via `MultiTurnStreamItem` variant inspection
- Per-tool detail formatting: `edit_file` → `"{path} ({mode})"`, `git` → `"{sub_action} {key_arg}"`, etc.
- Chat turn summaries via `truncate_summary()` helper (now `pub(crate)`)
- **Files modified:** `src/session/runner.rs`, `src/session/agent.rs`, `src/llm/agent_factory.rs`, `src/review/mod.rs`

**Story 10.2 (Pipeline Integration, `review`):**
- Wired `ui: UiHandle` into `StoryPipeline`, `SessionRunner`, `ReviewRunner`
- TTY detection + `ui_mode` config → `ConsoleRenderer` or `NullRenderer` selection in `run_start()`
- `console::set_colors_enabled(false)` for plain mode already in `run_start()`
- Removed stdout tracing layer when UI active
- **Files modified:** `src/cli/mod.rs`, `src/pipeline.rs`, `src/session/runner.rs`, `src/review/mod.rs`, `src/config/mod.rs`

**Story 10.1 (Foundation, `review`):**
- Created `UiRenderer` trait (27 methods, all `&self` → `()`), `ConsoleRenderer`, `NullRenderer`, `UiHandle`
- `ConsoleRenderer` uses `MultiProgress` + `console::style()`, spinners for phases, `Mutex<HashMap>` for spinner tracking
- `ConsoleRenderer::new()` takes no parameters — **this story changes it to `new(plain: bool)`**
- Duplicate `phase_start` protection: clears previous spinner before inserting new one
- **Files created:** `src/ui/mod.rs`, `src/ui/renderer.rs`, `src/ui/console.rs`, `src/ui/null.rs`

### Git Intelligence

Last 5 implementation commits:
1. `202b846` — `feat(ui): add review integration UI events in code review (Story 10.4)` — modified `review/mod.rs`, `pipeline.rs`, `session/runner.rs`
2. `628fdad` — `feat(session): add UI event emissions for tool calls, chat turns, and LLM lifecycle (Story 10.3)` — modified `session/runner.rs`, `session/agent.rs`, `llm/agent_factory.rs`, `review/mod.rs`
3. `2235fe3` — `feat(ui): wire UiHandle into pipeline, emit lifecycle events (Story 10.2)` — modified `cli/mod.rs`, `pipeline.rs`, `session/runner.rs`, `review/mod.rs`, `config/mod.rs`
4. `bfa0645` — `feat(ui): story 10.1 — UiRenderer trait, ConsoleRenderer, NullRenderer, UiHandle` — created all `ui/` files
5. `0ef51b7` — `test(watcher): add autoscalp3000 multi-epic comma deps scenario`

### Estimated Scope

This is a **3-point story** (as planned in the epics). The changes are:
- **`src/ui/console.rs`**: ~40-60 lines — `format_duration()` helper + tests, glyph changes (2 methods), `plain_mode` field + conditional glyph rendering, `new(bool)` signature
- **`src/ui/mod.rs`**: ~5 lines — update `UiHandle::console(bool)`, update test call sites
- **`src/cli/mod.rs`**: ~2 lines — pass plain flag to `UiHandle::console()`
- **`README.md`**: ~50-80 lines — new Terminal Output section, config example update, project structure update
- **Total**: ~100-150 lines across 4 files

### References

- [Source: epics.md#L2586-2680 — Epic 10 / Story 10.5] — full acceptance criteria and dev notes
- [Source: epics.md#L2281-2300 — Epic 10 overview] — dependency order, reference documents, design rationale
- [Source: epics.md#L2680-2711 — Epic 10 Summary] — architecture decisions, key patterns
- [Source: project-context.md#L192-205 — Terminal UI Rules] — `UiRenderer` backend-agnostic, `UiHandle` propagation, tool call UI at call sites
- [Source: project-context.md#L121-169 — Code Quality & Style Rules] — modular structure, doc comments, no dead code
- [Source: project-context.md#L112-121 — Testing Rules] — inline tests, descriptive names, no real LLM calls
- [Source: ui/console.rs — ConsoleRenderer] — current implementation with all glyph definitions
- [Source: ui/renderer.rs — UiRenderer trait] — stable trait contract (DO NOT modify)
- [Source: ui/mod.rs — UiHandle] — wrapper struct, `console()` and `null()` constructors
- [Source: ui/null.rs — NullRenderer] — no-op implementation (DO NOT modify)
- [Source: cli/mod.rs#L1262-1285 — run_start()] — TTY detection, ui_mode branching, UiHandle creation
- [Source: cli/mod.rs#L165-226 — init_tracing()] — conditional stdout layer removal when UI active
- [Source: config/mod.rs#L105-130 — ui_mode field + default] — "fancy" default, validation for fancy/plain/silent
- [Source: README.md#L234-285 — Configuration section] — bmad-bot.yaml example (missing ui_mode)
- [Source: README.md#L439-501 — Project Structure] — outdated, needs update for ui/, tools/, mcp/
- [Source: 10-4 story — Completion Notes] — all 1164 tests passing, patterns established
- [Source: 10-3 story — Task 12] — `truncate_summary()` helper pattern
- [Source: 10-1 story — ConsoleRenderer tests] — existing test patterns for all methods

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List