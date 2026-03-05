# Story 10.2: Pipeline Integration — UI Events in Story Lifecycle

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer monitoring the daemon in tmux,
I want to see the full lifecycle of each story as it progresses through the pipeline,
So that I know at a glance which story is being processed, which phase is active, and whether things are succeeding or failing.

## Acceptance Criteria

1. **Given** `StoryPipeline` struct in `src/pipeline.rs` **When** I inspect the struct definition **Then** it contains a `ui: UiHandle` field **And** `StoryPipeline::new()` accepts a `UiHandle` parameter.

2. **Given** a story is processed via `process_story()` **When** the pipeline progresses through each phase **Then** the following UI events are emitted in order:
   - `ui.story_start(story_key, story_title)` — at the start of `process_story()`
   - `ui.phase_start("Dev Session")` — before `session_runner.run()`
   - `ui.phase_complete("Dev Session", duration)` OR `ui.phase_error("Dev Session", error)` — after session returns
   - `ui.phase_start("Push Branch")` — before `push_branch()`
   - `ui.phase_complete("Push Branch", duration)` — after push
   - `ui.phase_start("Create PR")` — before `git_provider.create_pr()`
   - `ui.phase_complete("Create PR", duration)` — after PR created
   - `ui.phase_start("Code Review")` — before `review_runner.run()` (if enabled)
   - `ui.phase_complete("Code Review", duration)` — after review
   - `ui.phase_start("Notification")` — before `notify_story_result()`
   - `ui.phase_complete("Notification", duration)` — after notification sent
   - `ui.story_complete(story_key, pr_url)` — on success OR `ui.story_error(story_key, error)` — on failure OR `ui.story_escalated(story_key, reason)` — on escalation

3. **Given** multiple stories are processed via `process_eligible_stories()` **When** the batch starts and ends **Then** `ui.batch_start(count)` is emitted at the start with the number of eligible stories **And** `ui.batch_complete(summary)` is emitted at the end with a human-readable run summary.

4. **Given** a crash recovery is triggered via `recover_and_process()` **When** a WAL file is detected at startup **Then** `ui.crash_recovery_start()` is emitted before recovery begins **And** `ui.crash_recovery_complete(story_key)` is emitted after recovery finishes.

5. **Given** `cli/mod.rs` `run_start()` function **When** the daemon starts **Then** a `UiHandle` is created:
   - `ConsoleRenderer` if stdout is a TTY and `ui_mode` is `"fancy"` (default) or `"plain"`
   - `NullRenderer` if stdout is not a TTY, or `ui_mode` is `"silent"`
   **And** the `UiHandle` is passed to `StoryPipeline::new()` **And** `ui.daemon_start(config_summary)` is emitted after tracing is initialized.

6. **Given** `init_tracing()` in `cli/mod.rs` **When** `ConsoleRenderer` is active (TTY + fancy/plain mode) **Then** the stdout `tracing` layer is **removed** — debug logs go to the JSON file layer only **When** `NullRenderer` is active (non-TTY or silent mode) **Then** the stdout `tracing` layer is **preserved** for backward compatibility.

7. **Given** the polling loop in `run_polling_loop()` **When** a poll cycle finds no eligible stories **Then** `ui.poll_cycle(cycle_num)` is emitted (quiet — no spinner, just a timestamp or nothing) **When** a poll cycle finds eligible stories **Then** `ui.stories_found(count)` is emitted before processing.

8. **Given** all existing tests **When** they run **Then** they pass without modification (using `NullRenderer`) **And** `StoryPipeline::new()` in tests receives a `UiHandle::null()` or equivalent.

## Tasks / Subtasks

- [ ] Task 1: Add `ui_mode` config field to `BotConfig` (AC: #5, #6)
  - [ ] 1.1 Add `pub ui_mode: String` field to `BotConfig` in `src/config/mod.rs` with `#[serde(default = "default_ui_mode")]` — default `"fancy"`
  - [ ] 1.2 Add `default_ui_mode()` function returning `"fancy".to_string()`
  - [ ] 1.3 Add validation in `BotConfig::validate()`: reject values not in `["fancy", "plain", "silent"]`
  - [ ] 1.4 Add `ui_mode` to the `VALID_YAML` constant in tests and existing test helpers (`make_test_config`, `_test_minimal`)
  - [ ] 1.5 Add unit tests: `test_config_default_ui_mode_is_fancy`, `test_config_ui_mode_accepts_valid_values`, `test_config_ui_mode_rejects_invalid_value`
  - [ ] 1.6 Add `ui_mode` entry to `bmad-bot.yaml.example` with comment — place it after `log_file`, before `git_provider`:
    ```
    # Terminal UI mode for foreground daemon output.
    # "fancy" — animated spinners + colors (default), "plain" — no colors/spinners, "silent" — no terminal output
    # Non-TTY environments (pipes, CI) automatically use silent mode regardless of this setting.
    # ui_mode: fancy
    ```

- [ ] Task 2: Remove `#[allow(dead_code)]` from `mod ui;` in `src/main.rs` (AC: #8)
  - [ ] 2.1 Change `#[allow(dead_code)] mod ui;` to `mod ui;` — the module is now consumed by pipeline

- [ ] Task 3: Add `UiHandle::null()` convenience constructor (if not already in Story 10.1) (AC: #8)
  - [ ] 3.1 Verify `UiHandle::null()` exists in `src/ui/mod.rs` — if not, add it (wraps `NullRenderer`)

- [ ] Task 4: Wire `UiHandle` into `StoryPipeline` (AC: #1, #8)
  - [ ] 4.1 Add `ui: UiHandle` field to `StoryPipeline` struct in `src/pipeline.rs`
  - [ ] 4.2 Add `ui: UiHandle` parameter to `StoryPipeline::new()` — after `mcp_manager`
  - [ ] 4.3 Pass `ui.clone()` to `SessionRunner::new()` (store but don't emit events yet — Stories 10.3/10.4)
  - [ ] 4.4 Pass `ui.clone()` to `ReviewRunner::new()` (store but don't emit events yet — Stories 10.3/10.4)
  - [ ] 4.5 Store `ui` in `Self { ..., ui }`

- [ ] Task 5: Wire `UiHandle` into `SessionRunner` (AC: #1)
  - [ ] 5.1 Add `ui: UiHandle` field to `SessionRunner` struct in `src/session/runner.rs`
  - [ ] 5.2 Add `ui: UiHandle` parameter to `SessionRunner::new()` — after `mcp_manager`
  - [ ] 5.3 Store `ui` in `Self { ..., ui }` — no event emissions in this story (deferred to Story 10.3)
  - [ ] 5.4 Update all test helpers (`make_test_runner`, `make_test_runner_with_dir`, `make_runner_test_config`) to pass `UiHandle::null()`

- [ ] Task 6: Wire `UiHandle` into `ReviewRunner` (AC: #1)
  - [ ] 6.1 Add `ui: UiHandle` field to `ReviewRunner` struct in `src/review/mod.rs`
  - [ ] 6.2 Add `ui: UiHandle` parameter to `ReviewRunner::new()` — after `mcp_manager`
  - [ ] 6.3 Store `ui` in `Self { ..., ui }` — no event emissions in this story (deferred to Story 10.4)
  - [ ] 6.4 Update all test helpers in `review/mod.rs` tests to pass `UiHandle::null()`

- [ ] Task 7: Create `UiHandle` in `run_start()` and pass to pipeline (AC: #5, #6)
  - [ ] 7.1 In `run_start()`, after `init_tracing()`, create `UiHandle` based on TTY detection + `ui_mode` config:
    - Use `console::Term::stdout().is_term()` for TTY detection
    - If non-TTY or `ui_mode == "silent"` → `UiHandle::null()`
    - If TTY and `ui_mode == "fancy"` or `"plain"` → `UiHandle::console()`
  - [ ] 7.2 Pass `ui.clone()` to `StoryPipeline::new()`
  - [ ] 7.3 Emit `ui.daemon_start(config_summary)` after tracing init — config_summary is a human-readable string with key config values (polling interval, git provider, llm providers)

- [ ] Task 8: Modify `init_tracing()` to conditionally remove stdout layer (AC: #6)
  - [ ] 8.1 Add `ui_active: bool` parameter to `init_tracing()` — `true` when `ConsoleRenderer` is active
  - [ ] 8.2 When `ui_active` is `true`: omit the stdout layer — only the file JSON layer is registered
  - [ ] 8.3 When `ui_active` is `false`: preserve the stdout layer (existing behavior — backward compatible)
  - [ ] 8.4 Update all call sites: `run_start()` passes computed `ui_active`, test helpers pass `false`
  - [ ] 8.5 Update existing `init_tracing` tests to pass the new parameter

- [ ] Task 9: Emit pipeline-level UI events in `process_story()` (AC: #2)
  - [ ] 9.1 At start of `process_story()`: emit `ui.story_start(&story.story_key, &story_title)`
  - [ ] 9.2 Before `session_runner.run()`: emit `ui.phase_start("Dev Session")`, capture `Instant::now()`
  - [ ] 9.3 After `session_runner.run()` returns: emit `ui.phase_complete("Dev Session", elapsed)` on success, `ui.phase_error("Dev Session", &error)` on failure/escalation
  - [ ] 9.4 Before `push_branch()`: emit `ui.phase_start("Push Branch")`, capture `Instant::now()`
  - [ ] 9.5 After `push_branch()` returns: emit `ui.phase_complete("Push Branch", elapsed)` or `ui.phase_error("Push Branch", &error)`
  - [ ] 9.6 Before `git_provider.create_pr()`: emit `ui.phase_start("Create PR")`, capture `Instant::now()`
  - [ ] 9.7 After `git_provider.create_pr()` returns: emit `ui.phase_complete("Create PR", elapsed)` or `ui.phase_error("Create PR", &error)`
  - [ ] 9.8 Before `review_runner.run()` (if code_review_enabled): emit `ui.phase_start("Code Review")`, capture `Instant::now()`
  - [ ] 9.9 After `review_runner.run()` returns: emit `ui.phase_complete("Code Review", elapsed)` or `ui.phase_error("Code Review", &error)` — emit `ui.phase_complete` with `Duration::ZERO` if review is skipped
  - [ ] 9.10 Before `notify_story_result()`: emit `ui.phase_start("Notification")`, capture `Instant::now()`
  - [ ] 9.11 After `notify_story_result()` returns: emit `ui.phase_complete("Notification", elapsed)`
  - [ ] 9.12 At end of `process_story()`: emit `ui.story_complete(story_key, pr_url)` on success, `ui.story_error(story_key, error)` on failure, `ui.story_escalated(story_key, reason)` on escalation

- [ ] Task 9b: Emit UI events in `process_recovered_session()` (AC: #2, #4)
  - [ ] 9b.1 This is a **separate method** from `process_story()` at `pipeline.rs` L881-1197 — it handles the post-session phases for crash-recovered sessions. It has its own complete flow and MUST also emit UI events.
  - [ ] 9b.2 At start of method: emit `ui.story_start(&story.story_key, &story_title)`
  - [ ] 9b.3 **Completed arm:** emit phase events for Code Review (if enabled), Push Branch, Create PR, then `ui.story_complete(story_key, pr_url)` — or `ui.story_error` on PR/push failure early returns
  - [ ] 9b.4 **Escalated arm:** emit phase events for Push Branch, Create PR, then `ui.story_escalated(story_key, reason)`
  - [ ] 9b.5 **Failed arm (infra):** emit `ui.story_error(story_key, error)` immediately (no PR created)
  - [ ] 9b.6 **Failed arm (non-infra):** emit phase events for Push Branch, Create PR, then `ui.story_error(story_key, error)`
  - [ ] 9b.7 **Key difference from `process_story()`:** No Dev Session phase (session already ran during recovery). No sprint-status update phase. No notification phase (notification is emitted by the caller `recover_and_process()`). Phase order in Completed arm is: Code Review → Push Branch → Create PR.

- [ ] Task 10: Emit batch-level UI events in `process_eligible_stories()` (AC: #3)
  - [ ] 10.1 At start of `process_eligible_stories()`: emit `ui.batch_start(stories.len())`
  - [ ] 10.2 At end of `process_eligible_stories()`: emit `ui.batch_complete(&summary_string)` — use the `RunSummary` `Display` or format a human-readable string

- [ ] Task 11: Emit crash recovery UI events in `recover_and_process()` (AC: #4)
  - [ ] 11.1 Before `session_runner.check_and_recover_wal()`: emit `ui.crash_recovery_start()`
  - [ ] 11.2 After recovery completes: emit `ui.crash_recovery_complete(&result.story_key)`

- [ ] Task 12: Emit polling loop UI events in `run_polling_loop()` (AC: #7)
  - [ ] 12.1 Pass `ui: &UiHandle` parameter to `run_polling_loop()` — add to the function signature
  - [ ] 12.2 Maintain a cycle counter, increment each tick — use the type matching the `poll_cycle` trait signature defined by Story 10.1 in `renderer.rs` (likely `usize`)
  - [ ] 12.3 On each poll cycle (no stories): emit `ui.poll_cycle(cycle_num)`
  - [ ] 12.4 When stories found: emit `ui.stories_found(stories.len())`
  - [ ] 12.5 Update `run_start()` to pass `&ui` to `run_polling_loop()`

- [ ] Task 13: Emit shutdown event (AC: #5)
  - [ ] 13.1 Before the shutdown MCP cleanup in `run_start()`: emit `ui.shutdown_requested()`

- [ ] Task 14: Update all existing tests (AC: #8)
  - [ ] 14.1 Update `StoryPipeline` test assertions that check struct fields or constructors
  - [ ] 14.2 Verify `cargo test` passes with zero failures
  - [ ] 14.3 Verify `cargo clippy` passes with zero warnings
  - [ ] 14.4 Verify `cargo fmt --check` passes

## Dev Notes

### Architecture Compliance

- **`UiHandle` propagation pattern:** Follows the exact same propagation pattern as `ShutdownFlag` and `Arc<McpManager>` — created in `cli/run_start()`, passed to `StoryPipeline::new()`, which passes clones to `SessionRunner::new()` and `ReviewRunner::new()`. This is a well-established pattern in the codebase.
- **`UiRenderer` trait methods all take `&self` and return `()`** — fire-and-forget. No error propagation from UI to business logic. The `ConsoleRenderer` handles its own errors internally.
- **`init_tracing()` modification:** The stdout layer is conditionally omitted (not "removed at runtime"). The function signature changes to accept a `ui_active: bool` flag. When `true`, only the JSON file layer is registered. This is clean and avoids any runtime layer manipulation.
- **Duration tracking:** Use `std::time::Instant::now()` before each phase, then `start.elapsed()` after. Pass the `Duration` to `ui.phase_complete()`. This is simple and correct for wall-clock timing.
- **All `tracing::info!` calls remain unchanged** — they continue to log to the JSON file layer. UI events are a separate, additive concern. No existing tracing calls should be removed or modified.
- **`NullRenderer` in tests** — all test code uses `UiHandle::null()`. This ensures zero test pollution from terminal output and zero behavior change in existing tests.

### Project Structure Notes

- **Modified files:**
  - `src/config/mod.rs` — add `ui_mode` field + validation
  - `src/main.rs` — remove `#[allow(dead_code)]` from `mod ui;`
  - `src/pipeline.rs` — add `ui: UiHandle` field and all pipeline-level event emissions (both `process_story()` AND `process_recovered_session()`)
  - `src/session/runner.rs` — add `ui: UiHandle` field (store only, no emissions)
  - `src/review/mod.rs` — add `ui: UiHandle` field (store only, no emissions)
  - `src/cli/mod.rs` — create `UiHandle`, modify `init_tracing()` signature, wire to pipeline, add polling/system events
  - `bmad-bot.yaml.example` — add `ui_mode` entry with documentation comment
- **No new Rust files** — this story only modifies existing files (plus the YAML example)
- Alignment with architecture doc's project structure: the `ui/` module is created by Story 10.1 — this story wires it into the existing pipeline and CLI code

### Technical Requirements

- **Rust edition 2024** — all code must follow edition 2024 conventions (rustc 1.93+)
- **`#![deny(clippy::all)]`** — zero clippy warnings
- **`#![warn(dead_code)]`** — current crate-root setting; do NOT change this attribute
- **Error handling:** No `unwrap()` or `expect()` in production code — only in tests
- **Doc comments:** `///` mandatory on all new public items
- **No `println!` / `eprintln!`** in daemon runtime code — use `UiHandle` for user-facing output, `tracing` for debug logging

### Library & Framework Requirements

- **`console` crate** (added by Story 10.1, version 0.16.x) — used for TTY detection:
  - `console::Term::stdout().is_term()` — returns `true` if stdout is a terminal (not a pipe or redirect)
  - This is the canonical way to detect TTY in Rust with the `console` crate
- **`indicatif` crate** (added by Story 10.1, version 0.18.x) — no direct usage in this story; `ConsoleRenderer` handles spinners internally
- **`std::time::Instant`** — for phase duration tracking (already in `std`, no additional dependency)

### File Structure Requirements

- Follow existing code patterns: `use` imports at top, then structs, then `impl` blocks, then `#[cfg(test)] mod tests` at bottom
- Keep `UiHandle` parameters last in constructor signatures (after `mcp_manager`) for consistency
- **Required `use` import per modified file:**
  - `src/pipeline.rs` — add `use crate::ui::UiHandle;`
  - `src/session/runner.rs` — add `use crate::ui::UiHandle;`
  - `src/review/mod.rs` — add `use crate::ui::UiHandle;`
  - `src/cli/mod.rs` — add `use crate::ui::UiHandle;`

### Testing Requirements

- All tests use `NullRenderer` via `UiHandle::null()`
- Test naming: `test_{module}_{behavior}_{scenario}` in snake_case
- Arrange → Act → Assert pattern
- New config tests for `ui_mode` validation:
  - `test_config_default_ui_mode_is_fancy`
  - `test_config_ui_mode_accepts_valid_values` (fancy, plain, silent)
  - `test_config_ui_mode_rejects_invalid_value`
- Existing `init_tracing` tests must be updated to pass the new `ui_active` parameter (use `false` for backward compat)
- **Critical:** Run `cargo test` at the end — ALL existing tests must pass

### Previous Story Intelligence

**Story 10.1 (Foundation — NOT yet implemented):**
- Creates `src/ui/mod.rs`, `src/ui/renderer.rs`, `src/ui/console.rs`, `src/ui/null.rs`
- Defines `UiRenderer` trait with all method signatures (pipeline, phase, session, tool, LLM, system events)
- Defines `UiHandle` struct wrapping `Arc<dyn UiRenderer>` — `Clone + Send + Sync`
- Defines `UiHandle::null()` → wraps `NullRenderer`
- Defines `UiHandle::console()` → wraps `ConsoleRenderer::new()`
- `ConsoleRenderer` uses `indicatif::MultiProgress` + `console::style()`
- All methods take `&self`, return `()` — fire-and-forget
- `mod ui;` added to `main.rs` with `#[allow(dead_code)]` (this story removes that attribute)
- **CRITICAL DEPENDENCY:** Story 10.1 MUST be completed before this story can start. The `ui/` module must exist with all trait methods defined.

**Constructor signatures that will be modified (current → new):**

`StoryPipeline::new()`:
```
// Current:
pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>, shutdown: ShutdownFlag, mcp_manager: Arc<McpManager>) -> Result<Self, PipelineError>
// New:
pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>, shutdown: ShutdownFlag, mcp_manager: Arc<McpManager>, ui: UiHandle) -> Result<Self, PipelineError>
```

`SessionRunner::new()`:
```
// Current:
pub fn new(config: Arc<BotConfig>, agent_factory: Arc<AgentFactory>, shutdown: ShutdownFlag, mcp_manager: Arc<McpManager>) -> Self
// New:
pub fn new(config: Arc<BotConfig>, agent_factory: Arc<AgentFactory>, shutdown: ShutdownFlag, mcp_manager: Arc<McpManager>, ui: UiHandle) -> Self
```

`ReviewRunner::new()`:
```
// Current:
pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>, agent_factory: Arc<AgentFactory>, shutdown: ShutdownFlag, mcp_manager: Arc<McpManager>) -> Self
// New:
pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>, agent_factory: Arc<AgentFactory>, shutdown: ShutdownFlag, mcp_manager: Arc<McpManager>, ui: UiHandle) -> Self
```

`init_tracing()`:
```
// Current:
pub fn init_tracing(config: &BotConfig) -> Result<(), CliError>
// New:
pub fn init_tracing(config: &BotConfig, ui_active: bool) -> Result<(), CliError>
```

`run_polling_loop()`:
```
// Current:
async fn run_polling_loop(config, watcher, pipeline, daemon_state, state_path, shutdown) -> Result<(), CliError>
// New:
async fn run_polling_loop(config, watcher, pipeline, daemon_state, state_path, shutdown, ui: &UiHandle) -> Result<(), CliError>
```

### Implementation Guidance: `process_story()` Event Emission Pattern

The `process_story()` method has three match arms for `SessionOutcome`: `Completed`, `Escalated`, and `Failed`. Each arm should emit UI events at the appropriate points. Here is the general pattern:

```
pub async fn process_story(&self, story: &StoryInfo) -> PipelineResult {
    let story_title = story_title_from_label(&story.label);
    self.ui.story_start(&story.story_key, &story_title);

    // Phase 1 — Dev Session
    self.ui.phase_start("Dev Session");
    let session_start = std::time::Instant::now();
    let session_outcome = self.session_runner.run(story).await;
    let session_elapsed = session_start.elapsed();

    match session_outcome {
        SessionOutcome::Completed { .. } => {
            self.ui.phase_complete("Dev Session", session_elapsed);
            // ... push branch with phase_start/phase_complete ...
            // ... create PR with phase_start/phase_complete ...
            // ... code review with phase_start/phase_complete ...
            // ... notification with phase_start/phase_complete ...
            self.ui.story_complete(&story_key, Some(&pr_info.url));
            result
        }
        SessionOutcome::Escalated { .. } => {
            self.ui.phase_error("Dev Session", &format!("Escalated: {}", report.reason));
            // ... push branch, PR, notification with phase events ...
            self.ui.story_escalated(&story_key, &format!("{}", report.reason));
            result
        }
        SessionOutcome::Failed { .. } => {
            self.ui.phase_error("Dev Session", &error);
            // ... push branch, PR if non-infra, notification with phase events ...
            self.ui.story_error(&story_key, &error);
            result
        }
    }
}
```

**Key detail for the `Completed` arm:** The method has many intermediate returns (push failure, PR creation failure, sprint-status commit failure). Each early return path must emit `ui.story_error()` or `ui.story_complete()` appropriately before returning.

**Key detail for the `Failed` arm:** Infrastructure errors (`is_infra_error`) skip PR creation and return early. Non-infra errors attempt a failure PR. Both paths need appropriate UI events.

### Implementation Guidance: `process_recovered_session()` Event Emission Pattern

`process_recovered_session()` (pipeline.rs L881-1197) is a **separate method** from `process_story()`. It handles the post-session pipeline phases after a crash-recovered session completes. It does NOT include a Dev Session phase (that already ran during recovery) and does NOT update sprint-status or send notifications (the caller `recover_and_process()` handles notification).

The method has the same three `SessionOutcome` match arms but with a different phase ordering:

**Completed arm:** Code Review (optional) → Push Branch → Create PR → PR Comment (optional)
**Escalated arm:** Push Branch → Create PR (escalation)
**Failed arm (infra):** No phases — immediate return with error
**Failed arm (non-infra):** Push Branch → Create PR (failure)

Apply the same `ui.phase_start()` / `ui.phase_complete()` / `ui.phase_error()` pattern from Task 9. Emit `ui.story_start()` at the top and the appropriate `ui.story_complete()` / `ui.story_error()` / `ui.story_escalated()` before each return.

**Key difference:** In the Completed arm, Code Review happens BEFORE Push Branch (opposite order from `process_story()` where review is after PR creation). Follow the actual code flow, not `process_story()` ordering.

### Implementation Guidance: `init_tracing()` Conditional Stdout Layer

The current implementation creates both a file layer and a stdout layer. The modification adds a conditional:

```
pub fn init_tracing(config: &BotConfig, ui_active: bool) -> Result<(), CliError> {
    // ... env_filter and file_layer setup unchanged ...

    if ui_active {
        // UI is rendering to terminal — only log to file
        tracing_subscriber::registry()
            .with(env_filter)
            .with(file_layer)
            .try_init()
            .map_err(|e| CliError::TracingInit { reason: e.to_string() })?;
    } else {
        // No UI — preserve stdout layer for backward compatibility
        let stdout_layer = match config.log_format.as_str() {
            "json" => fmt::layer().json().with_target(true).with_thread_ids(false).boxed(),
            _ => fmt::layer().with_target(true).with_thread_ids(false).boxed(),
        };
        tracing_subscriber::registry()
            .with(env_filter)
            .with(file_layer)
            .with(stdout_layer)
            .try_init()
            .map_err(|e| CliError::TracingInit { reason: e.to_string() })?;
    }

    Ok(())
}
```

### Implementation Guidance: `run_start()` UiHandle Creation

The sequence in `run_start()` must be carefully ordered because `init_tracing()` needs to know whether UI is active, but `UiHandle` creation depends on config being loaded:

```
pub async fn run_start(config_path: &Path) -> Result<(), CliError> {
    let config = BotConfig::load(config_path)?;
    config.validate()?;

    // Determine if ConsoleRenderer should be active
    let is_tty = console::Term::stdout().is_term();
    let ui_active = is_tty && config.ui_mode != "silent";

    // Init tracing FIRST — conditionally omit stdout layer if UI is active
    init_tracing(&config, ui_active)?;

    // Create UiHandle AFTER tracing (so ConsoleRenderer doesn't fight with tracing stdout)
    let ui = if ui_active {
        UiHandle::console()
    } else {
        UiHandle::null()
    };

    // ... rest of run_start unchanged until pipeline creation ...

    // Emit daemon_start after all initialization
    ui.daemon_start(&format!(
        "polling={}s, git={}, llm_dev={}/{}, review={}",
        config.polling_interval_secs,
        config.git_provider.provider,
        config.llm.dev.provider,
        config.llm.dev.model,
        if config.code_review_enabled { "on" } else { "off" },
    ));

    // Pass ui to StoryPipeline::new()
    let pipeline = crate::pipeline::StoryPipeline::new(
        Arc::clone(&config),
        Arc::clone(&secrets),
        std::sync::Arc::clone(&shutdown),
        Arc::clone(&mcp_manager),
        ui.clone(),
    )?;

    // ... crash recovery (with UI events) ...
    // ... pass &ui to run_polling_loop() ...

    ui.shutdown_requested();
    // ... MCP shutdown, daemon state cleanup ...
}
```

### Implementation Guidance: `"plain"` Mode

For `"plain"` mode, `ConsoleRenderer` is still used (not `NullRenderer`), but it should disable colors and animations. This is handled by `ConsoleRenderer` internally via `console::set_colors_enabled(false)`. Story 10.2 needs to pass the `ui_mode` to the `ConsoleRenderer` so it can configure itself. If `ConsoleRenderer::new()` from Story 10.1 takes no parameters, consider one of these approaches:
1. **Add `ConsoleRenderer::new_with_mode(mode: &str)`** — preferred if the `ConsoleRenderer` needs mode info
2. **Call `console::set_colors_enabled(false)` in `run_start()` before creating `UiHandle`** — simpler, keeps ConsoleRenderer unchanged
3. **Defer plain mode styling to Story 10.5** (Polish) — acceptable since 10.5 is explicitly about visual polish and config-driven rendering

Option 2 or 3 is recommended for this story to keep scope focused.

### Implementation Guidance: `unstick_orphan_stories` — No UI Events Needed

The `run_start()` function contains an `unstick_orphan_stories` block between crash recovery and the polling loop (around L1370-1390). This is a **silent internal cleanup** that resets orphan `in-progress` stories to `ready-for-dev`. It does NOT need any UI events — it is a background administrative operation. The existing `tracing::info!` is sufficient. Do NOT add `ui.phase_start/complete` calls here.

### Implementation Guidance: `generate_config_yaml()` — Automatic Serialization

The `generate_config_yaml()` function (cli/mod.rs L740-755) uses `serde_yml::to_string(config)` to serialize the entire `BotConfig`. Since `ui_mode` has `#[serde(default)]`, it will automatically appear in the generated YAML with value `"fancy"` when `bmad-bot init` creates a new config. No manual string building needed — serde handles it. The field will appear without a documentation comment though, which is acceptable since `bmad-bot.yaml.example` provides the reference docs.

### Git Intelligence

Last 7 commits (most recent first):
1. `c933004` — `docs(story): add validated story 10.1 — ui/ module foundation, trait & console renderer`
2. `eadda06` — `chore(sprint): regenerate sprint-status with Epic 10, updated epic statuses`
3. `fb68dd8` — `docs(project-context): add Terminal UI rules for ui/ module (Epic 10)`
4. `e95a955` — `docs(epics): add Epic 10 — Terminal UI & Developer Experience (5 stories)`
5. `8280802` — `docs(architecture): add ui/ module for Terminal UI (Epic 10 / FR43)`
6. `75ef883` — `docs(prd): add FR43 — Terminal UI & Developer Experience`
7. `45dcb40` — `docs(planning): add sprint change proposal — Epic 10 Terminal UI`

All commits are planning/documentation only. No implementation code for Epic 10 has been written yet. Story 10.1 must be implemented first.

**Propagation pattern reference (from `run_start()`):**
- `ShutdownFlag` → `Arc::new(AtomicBool::new(false))` → cloned to signal task + `StoryPipeline`
- `McpManager` → `Arc::new(McpManager::init(...))` → cloned to `StoryPipeline`
- `UiHandle` (new) → `UiHandle::console()` or `UiHandle::null()` → cloned to `StoryPipeline` → cloned to `SessionRunner` + `ReviewRunner`

### References

- [Source: architecture.md#L980-1055 — Project Structure & Boundaries] — `src/ui/` module layout, pipeline.rs, cli/mod.rs
- [Source: architecture.md#L716-781 — Tracing Pattern] — Terminal UI Layer documentation, `UiHandle` usage patterns
- [Source: architecture.md#L781-820 — Cooperative Shutdown Pattern] — `ShutdownFlag` propagation chain (same pattern for `UiHandle`)
- [Source: architecture.md#L954-980 — Enforcement Guidelines] — mandates `UiHandle` propagation and `NullRenderer` in tests
- [Source: project-context.md#L192-205 — Terminal UI Rules] — comprehensive rules for the `ui/` module
- [Source: project-context.md#L28-39 — Language-Specific Rules] — no `println!`, `tracing` for debug, `UiHandle` for user output
- [Source: epics.md#L2372-2447 — Epic 10 / Story 10.2] — full acceptance criteria and dev notes
- [Source: epics.md#L2303-2372 — Epic 10 / Story 10.1] — foundation story with trait definitions (dependency)
- [Source: pipeline.rs#L123-193 — StoryPipeline struct + new()] — current constructor signature
- [Source: pipeline.rs#L199-657 — process_story()] — full method implementation with all phases
- [Source: pipeline.rs#L667-733 — process_eligible_stories()] — batch processing loop
- [Source: pipeline.rs#L854-876 — recover_and_process()] — crash recovery entry point
- [Source: pipeline.rs#L881-1197 — process_recovered_session()] — complete post-recovery pipeline (CRITICAL: separate from process_story)
- [Source: session/runner.rs#L290-330 — SessionRunner struct + new()] — current constructor signature
- [Source: review/mod.rs#L297-329 — ReviewRunner struct + new()] — current constructor signature
- [Source: cli/mod.rs#L165-216 — init_tracing()] — current tracing setup with stdout + file layers
- [Source: cli/mod.rs#L740-755 — generate_config_yaml()] — serde-based config serialization (auto-includes ui_mode)
- [Source: cli/mod.rs#L1251-1396 — run_start()] — daemon startup, pipeline creation, signal handler
- [Source: cli/mod.rs#L1408-1495 — run_polling_loop()] — polling loop with watcher
- [Source: config/mod.rs#L75-112 — BotConfig struct] — current config fields, where ui_mode will be added
- [Source: bmad-bot.yaml.example] — template config file, needs ui_mode entry

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (via Copilot)

### Debug Log References

None — clean implementation, no debug sessions required.

### Completion Notes List

- All 14 tasks implemented across 7 modified files + 1 YAML example
- Task 1: Added `ui_mode` field to `BotConfig` with `#[serde(default = "default_ui_mode")]`, `default_ui_mode()` returning `"fancy"`, validation rejecting values not in `["fancy", "plain", "silent"]`, 3 unit tests, and `bmad-bot.yaml.example` entry
- Task 2: Removed `#[allow(dead_code)]` from `mod ui;` in `main.rs`
- Task 3: Verified `UiHandle::null()` already exists from Story 10.1
- Task 4: Wired `UiHandle` into `StoryPipeline` — field, constructor param (last after `mcp_manager`), cloned to `SessionRunner` and `ReviewRunner`
- Task 5: Wired `UiHandle` into `SessionRunner` — field with `#[allow(dead_code)]`, constructor param, stored (no emissions, deferred to 10.3). Updated 7 test call sites.
- Task 6: Wired `UiHandle` into `ReviewRunner` — field with `#[allow(dead_code)]`, constructor param, stored (no emissions, deferred to 10.4). Updated 1 test call site.
- Task 7: Created `UiHandle` in `run_start()` based on TTY detection (`console::Term::stdout().is_term()`) + `ui_mode` config. Plain mode disables colors via `console::set_colors_enabled(false)`. Emits `ui.daemon_start()` with config summary.
- Task 8: Modified `init_tracing()` to accept `ui_active: bool` — when `true`, omits stdout layer (file-only). Updated 2 test call sites to pass `false`.
- Task 9: Emitted all pipeline UI events in `process_story()` — `story_start`, `phase_start/complete/error` for Dev Session, Push Branch, Create PR, Code Review, Notification phases, and `story_complete/error/escalated` on all exit paths. Duration tracked with `Instant::now()`.
- Task 9b: Emitted UI events in `process_recovered_session()` — same pattern, adapted to recovery flow (no Dev Session phase, Code Review before Push in Completed arm)
- Task 10: Emitted `batch_start(count)` and `batch_complete(summary)` in `process_eligible_stories()`
- Task 11: Emitted `crash_recovery_start()` and `crash_recovery_complete(story_key)` in `recover_and_process()`
- Task 12: Emitted `poll_cycle(cycle_num)` and `stories_found(count)` in `run_polling_loop()` with `u32` cycle counter
- Task 13: Emitted `shutdown_requested()` in `run_start()` before MCP shutdown
- Task 14: All existing tests pass (1082 passed, 0 failed). Clippy clean for new code (3 pre-existing errors in unrelated files). `cargo fmt --check` passes.
- Added `Debug` impl for `UiHandle` (required by `ReviewRunner`'s `#[derive(Debug)]`)
- Added `ui_mode` field to all 5 test `BotConfig` struct literals across codebase (config/mod.rs, cli/mod.rs, session/runner.rs, review/mod.rs via _test_minimal, llm/agent_factory.rs, watcher/mod.rs)

### File List

- `src/config/mod.rs` — added `ui_mode` field, `default_ui_mode()`, validation, `_test_minimal` update, `VALID_YAML` update, 3 new tests
- `src/main.rs` — removed `#[allow(dead_code)]` from `mod ui;`
- `src/ui/mod.rs` — added `Debug` impl for `UiHandle`
- `src/pipeline.rs` — added `ui: UiHandle` field, constructor param, all pipeline/batch/recovery UI event emissions in `process_story()`, `process_recovered_session()`, `process_eligible_stories()`, `recover_and_process()`
- `src/session/runner.rs` — added `ui: UiHandle` field + constructor param (store only), updated 7 test `SessionRunner::new()` calls, added `ui_mode` to test config
- `src/review/mod.rs` — added `ui: UiHandle` field + constructor param (store only), updated 1 test `ReviewRunner::new()` call
- `src/cli/mod.rs` — modified `init_tracing()` to accept `ui_active: bool`, created `UiHandle` in `run_start()`, emitted `daemon_start`/`shutdown_requested`, added `ui` param to `run_polling_loop()` with `poll_cycle`/`stories_found` events, added `ui_mode` to test configs and `collect_config_interactively`, updated `init_tracing` test calls
- `src/llm/agent_factory.rs` — added `ui_mode` to test config
- `src/watcher/mod.rs` — added `ui_mode` to test config
- `bmad-bot.yaml.example` — added `ui_mode` entry with documentation comment