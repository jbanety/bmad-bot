# Story 10.4: Review Integration — UI Events in Code Review

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer monitoring the daemon in tmux,
I want to see the code review cycle as it happens,
So that I know when the review starts, what fixes are applied, and whether it succeeds.

## Acceptance Criteria

1. **Given** `ReviewRunner` struct in `src/review/mod.rs` **When** I inspect the struct definition **Then** it already contains a `ui: UiHandle` field (wired by Story 10.2) **And** this story changes all `None` ui parameters to `Some(&self.ui)` and adds review-specific lifecycle UI event emissions throughout `drive_review_session()` and `run_inner()`.

2. **Given** a code review session starts via `drive_review_session()` **When** the review agent is activated **Then** `ui.activation_start()` is emitted before activation **When** activation completes **Then** `ui.activation_complete()` is emitted **When** activation fails **Then** `ui.phase_error("Agent Activation", error)` is emitted.

3. **Given** the review chat loop in `drive_review_session()` **When** a review chat turn completes **Then** `ui.chat_turn(turn, summary)` is emitted with `"[review] "` prefix prepended to the truncated summary **When** the review agent applies fixes via tool calls **Then** `ui.tool_call(tool_name, detail)` events are emitted (same interception as Story 10.3 — enabled by passing `Some(&self.ui)` to `stream_chat()`).

4. **Given** the review agent commits fixes **When** `ui.tool_call("git", "commit \"fix: ...\"")` events are emitted through the existing tool call interception from Story 10.3.

5. **Given** the review detects CR workflow completion (`ResponseAction::Completed`) **When** the post-review phase message is sent **Then** `ui.phase_start("Post-Review Report")` is emitted before the post-review `stream_chat()` call **And** `ui.phase_complete("Post-Review Report", duration)` is emitted after the report is captured **Or** `ui.phase_error("Post-Review Report", error)` if the call fails.

6. **Given** LLM requests/responses in the review session **When** `log_llm_request()` is called **Then** `self.ui.llm_request("code-review", turn)` is also emitted **When** `log_llm_response()` is called **Then** `self.ui.llm_response("code-review", turn, response.len())` is also emitted **When** `log_llm_error()` is called **Then** `self.ui.llm_error("code-review", turn, &error)` is also emitted.

7. **Given** a transient review chat error triggers a retry **When** the retry counter increments **Then** `self.ui.llm_retry("code-review", turn, retries, 0.0)` is emitted (delay_secs=0 since review retries are immediate, no exponential backoff).

8. **Given** the full session retry in `run()` is triggered (malformed tool call) **When** the session is retried from scratch **Then** `self.ui.llm_retry("code-review", 0, attempt + 1, 0.0)` is emitted before the retry to indicate a full session restart.

9. **Given** the review report is posted as a PR comment in `pipeline.rs` **When** `self.git_provider.add_comment()` succeeds **Then** `ui.tool_result("pr_comment", "Review posted")` is emitted in the pipeline **When** the comment fails **Then** `ui.tool_result("pr_comment", "Failed: {error}")` is emitted.

10. **Given** the review outcome **When** review completes successfully **Then** `ui.phase_complete("Code Review", duration)` is emitted by the pipeline (already wired by Story 10.2) **When** review fails **Then** `ui.phase_error("Code Review", error)` is emitted by the pipeline **When** review is skipped **Then** `ui.phase_complete("Code Review", Duration::ZERO)` is emitted with a skip note by the pipeline.

11. **Given** all existing tests **When** they run **Then** they pass without modification (using `NullRenderer` via `UiHandle::null()`).

## Tasks / Subtasks

- [ ] Task 1: Enable tool call visibility in `drive_review_session()` by passing `Some(&self.ui)` to `stream_chat()` and `activate_agent()` (AC: #1, #3, #4)
  - [ ] 1.1 In `drive_review_session()`, change the `activate_agent()` call (currently passing `None` for `ui` per Story 10.3 Task 11.2) to pass `Some(&self.ui)` — this enables tool call interception during activation
  - [ ] 1.2 Change the initial CR `stream_chat()` call (the `"IMPORTANT: ALL communication MUST be in English..."` message) from `None` to `Some(&self.ui)` for the `ui` parameter
  - [ ] 1.3 Change the main chat loop `stream_chat()` call from `None` to `Some(&self.ui)` for the `ui` parameter
  - [ ] 1.4 Verify that `use crate::ui::UiHandle;` import already exists in `src/review/mod.rs` (added by Story 10.2 when wiring `ui` field into the struct) — if not, add it

- [ ] Task 2: Emit activation lifecycle UI events in `drive_review_session()` (AC: #2)
  - [ ] 2.1 Before the `agent.activate_agent()` call: emit `self.ui.activation_start()`
  - [ ] 2.2 After successful activation (after the `.map_err` block): emit `self.ui.activation_complete()`
  - [ ] 2.3 In the `.map_err` closure of `activate_agent()`: emit `self.ui.phase_error("Agent Activation", &format!("Agent activation failed: {e}"))` before constructing the `ReviewError`

- [ ] Task 3: Emit LLM request/response/error UI events in `drive_review_session()` (AC: #6)
  - [ ] 3.1 After `log_llm_request("code-review", 1, &initial_message, ...)`: add `self.ui.llm_request("code-review", 1)`
  - [ ] 3.2 After `log_llm_response("code-review", 1, &response)`: add `self.ui.llm_response("code-review", 1, response.len())`
  - [ ] 3.3 In the `.map_err` of the initial `stream_chat()`: after `log_llm_error(...)`, add `self.ui.llm_error("code-review", 1, &e.to_string())`
  - [ ] 3.4 In the main chat loop, after `log_llm_request("code-review", turn, &reply, ...)`: add `self.ui.llm_request("code-review", turn)`
  - [ ] 3.5 In the main chat loop `Ok` arm of `stream_chat()`: after `log_llm_response(...)`, add `self.ui.llm_response("code-review", turn, r.len())`
  - [ ] 3.6 In the main chat loop `Err` arm of `stream_chat()`: after `log_llm_error(...)`, add `self.ui.llm_error("code-review", turn, &e.to_string())`

- [ ] Task 4: Emit chat turn UI events with `[review]` prefix in `drive_review_session()` (AC: #3)
  - [ ] 4.1 After the initial CR response is received (after `log_llm_response` for turn 1): emit `self.ui.chat_turn(1, &format!("[review] {}", truncate_summary(&response, 80)))` — reuse the `truncate_summary()` helper from `session/runner.rs` (import it or create a shared utility)
  - [ ] 4.2 In the main chat loop, after the `tracing::debug!(action = "review_chat_turn", ...)` at the end of each iteration: emit `self.ui.chat_turn(turn, &format!("[review] {}", truncate_summary(&current_response, 80)))`
  - [ ] 4.3 **Decision on `truncate_summary` reuse:** The function was created as a private helper in `src/session/runner.rs` (Story 10.3 Task 12). To reuse it from `review/mod.rs`, either: **(a)** move it to a shared location (e.g., make it `pub(crate)` in `session/runner.rs` or create a small `src/util.rs` module), or **(b)** create a minimal inline version in `review/mod.rs`. Option (a) is preferred for DRY — make it `pub(crate)` in `session/runner.rs` and import from review

- [ ] Task 5: Emit post-review phase UI events (AC: #5)
  - [ ] 5.1 When `post_review_phase` is true and the post-review `stream_chat()` call is about to happen: emit `self.ui.phase_start("Post-Review Report")` and capture `Instant::now()` — **Note:** this requires restructuring the post-review flow slightly. Currently, the code enters the `if post_review_phase` block on the **next loop iteration** after setting `post_review_phase = true`. The `stream_chat()` call happens at the bottom of the loop (before the `if post_review_phase` check on the next iteration). The post-review report is parsed at the top of the loop when `post_review_phase` is true. Emit `phase_start` when setting `post_review_phase = true` (right after the `ResponseAction::Completed` match arm), and emit `phase_complete` after the report is successfully parsed in the `if post_review_phase` block
  - [ ] 5.2 After the review report is successfully parsed and `ReviewOutcome::Completed` is about to be returned: emit `self.ui.phase_complete("Post-Review Report", elapsed)`
  - [ ] 5.3 If the post-review `stream_chat()` call fails (Err arm in the main loop while `post_review_phase` is true): emit `self.ui.phase_error("Post-Review Report", &error)` before returning `ReviewOutcome::Failed`

- [ ] Task 6: Emit retry UI events (AC: #7, #8)
  - [ ] 6.1 In the main chat loop `Err` arm, after incrementing `retries` and the `tracing::warn!(action = "review_chat_error", ...)`: emit `self.ui.llm_retry("code-review", turn, retries, 0.0)` — review retries are immediate (no backoff delay)
  - [ ] 6.2 In `run()` method, inside the `Err(e)` arm of the `run_inner()` retry loop, after the `tracing::warn!(action = "review_retry", ...)`: emit `self.ui.llm_retry("code-review", 0, attempt + 1, 0.0)` to signal a full session restart. **Note:** `run()` does not have direct access to `self.ui` since it calls `self.run_inner()`. Since `ReviewRunner` has the `ui` field (wired by Story 10.2), `self.ui` is available in `run()`

- [ ] Task 7: Emit PR comment result UI events in `pipeline.rs` (AC: #9)
  - [ ] 7.1 **REQUIRES REFACTOR** in `process_story()` Phase 6 (~L347-358): The current code uses a combined `if let Some(ref report) = review_report && let Err(e) = self.git_provider.add_comment(...)` pattern — there is no explicit success branch. Refactor to a `match` to emit both success and error events:
    ```rust
    if let Some(ref report) = review_report {
        match self.git_provider.add_comment(&pr_info.id, report).await {
            Ok(()) => {
                self.ui.tool_result("pr_comment", "Review posted");
            }
            Err(e) => {
                tracing::error!(action = "pr_comment_failed", pr_id = %pr_info.id, error = %e, "Failed to post review comment");
                self.ui.tool_result("pr_comment", &format!("Failed: {e}"));
            }
        }
    }
    ```
  - [ ] 7.2 **CONFIRMED:** `process_recovered_session()` (~L997-1008) also posts review comments via `add_comment`. Apply the same `match` refactor there. **Important difference:** `process_recovered_session()` uses `strip_agent_artifacts(report)` while `process_story()` does NOT — preserve this difference when refactoring:
    ```rust
    if let Some(ref report) = review_report {
        match self.git_provider.add_comment(&pr_info.id, &strip_agent_artifacts(report)).await {
            Ok(()) => {
                self.ui.tool_result("pr_comment", "Review posted");
            }
            Err(e) => {
                tracing::error!(action = "recovery_pr_comment_failed", pr_id = %pr_info.id, error = %e, "Failed to post review comment after recovery");
                self.ui.tool_result("pr_comment", &format!("Failed: {e}"));
            }
        }
    }
    ```
  - [ ] 7.3 Verify no other `add_comment` calls exist in the codebase (grep for `add_comment` in `pipeline.rs`) — the two above should be the complete set

- [ ] Task 8: Run full test suite and linting (AC: #11)
  - [ ] 8.1 Run `cargo test` — ALL existing tests must pass with zero failures
  - [ ] 8.2 Run `cargo clippy` — zero warnings
  - [ ] 8.3 Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Architecture Compliance

- **UI events are fire-and-forget:** All `UiRenderer` methods take `&self` and return `()`. No error propagation from UI to business logic. `ConsoleRenderer` handles errors internally via `tracing::debug!`.
- **Tool call UI events emitted via rig stream interception (Story 10.3):** By passing `Some(&self.ui)` to `stream_chat()`, tool calls during the review session become automatically visible in the terminal. No changes needed to tool implementations — the interception happens in `streaming_chat()` in `session/agent.rs`.
- **Activation events at caller level:** `activate_agent()` does NOT emit `activation_start/complete/phase_error` (it's a shared utility). These lifecycle events are emitted by the caller (`drive_review_session()` Task 2), consistent with the pattern established in Story 10.3.
- **`tracing` calls remain unchanged:** All existing `tracing::info!`, `tracing::warn!`, `tracing::error!`, and `tracing::debug!` calls in `review/mod.rs` stay exactly as they are. UI events are a separate, additive concern. Pattern: `log_llm_request(...)` → `self.ui.llm_request(...)` — always pair them.
- **Review events visually distinguishable:** The `[review]` prefix in `chat_turn` summaries distinguishes review turns from dev session turns in the terminal output. The `ConsoleRenderer` may also apply different colors/styles for review context in Story 10.5 (polish).
- **`NullRenderer` in tests:** All tests continue using `UiHandle::null()`. No test behavior changes.

### `truncate_summary` Reuse Strategy

Story 10.3 Task 12 creates `truncate_summary(text: &str, max_len: usize) -> String` as a private function in `src/session/runner.rs`. This story needs the same function in `src/review/mod.rs`.

**Preferred approach:** Change the visibility of `truncate_summary` in `session/runner.rs` from `fn` to `pub(crate) fn`. Then import it in `review/mod.rs`:

```rust
use crate::session::runner::truncate_summary;
```

If `truncate_summary` was placed in a different location by Story 10.3 (e.g., inside a private `impl` block), then extract it to module level and make it `pub(crate)`.

**Fallback approach:** If changing visibility in `session/runner.rs` is undesirable (e.g., it's deeply nested), create a minimal inline copy in `review/mod.rs`:

```rust
fn truncate_summary(text: &str, max_len: usize) -> String {
    match text.char_indices().nth(max_len) {
        Some((idx, _)) => format!("{}…", &text[..idx]),
        None => text.to_string(),
    }
}
```

The developer should check the actual location and visibility of `truncate_summary` after Story 10.3 is implemented and choose accordingly.

### Current Review Flow — Code Path Analysis

The `drive_review_session()` method (L501-711 in `src/review/mod.rs`) follows this flow:

1. **Activation:** `agent.activate_agent(project_root, "dev.md", "code-review", shutdown)` — sends agent file as XML context
2. **Initial CR message:** `agent.stream_chat(initial_message, activation_history, shutdown)` — sends "CR" command with English override
3. **Chat loop:** `loop { analyze → match action { Completed → set post_review_phase, Escalated → return Failed, Continue → reply, NoReply → "Continue." } → stream_chat(reply, history, shutdown) }`
4. **Post-review phase:** On next iteration with `post_review_phase = true`, the response from the previous `stream_chat()` is parsed as the review report via `parse_review_report()` → return `ReviewOutcome::Completed`

**Key observation for Task 5 (post-review phase events):**

The post-review flow is split across two loop iterations:
- **Iteration N:** `ResponseAction::Completed` detected → `post_review_phase = true` → reply = `build_post_review_message()` → `stream_chat()` sends the post-review message
- **Iteration N+1:** `if post_review_phase` block at top of loop → parse `current_response` as report → return

So:
- `phase_start("Post-Review Report")` should be emitted in the `ResponseAction::Completed` arm, right after setting `post_review_phase = true` (before the `stream_chat()` at the bottom of the loop sends the post-review message)
- Capture `Instant::now()` at the same point (store in a variable like `post_review_start: Option<Instant>`)
- `phase_complete("Post-Review Report", elapsed)` should be emitted just before returning `ReviewOutcome::Completed` in the post-review parsing block

### `run()` Retry Loop — UI Access

The `run()` method (L339-377) has access to `self.ui` since `ReviewRunner` has a `ui: UiHandle` field (added by Story 10.2). The retry `tracing::warn!` at L356-363 is the interception point for Task 6.2. After the warning, emit:

```rust
self.ui.llm_retry("code-review", 0, (attempt + 1) as u32, 0.0);
```

**Type note:** Check the actual `UiRenderer::llm_retry()` signature from Story 10.1. The `retry_count` parameter type may be `u32` or `usize`. The epics spec shows `llm_retry(&self, label: &str, turn: u32, retry_count: u32, delay_secs: f64)` but Story 10.1's implementation may differ. Adapt casts accordingly.

### Pipeline PR Comment Events — Scope & Refactoring

The PR comment posting happens in `pipeline.rs`, not in `review/mod.rs`. **Both code paths require a structural refactor:**

- **`process_story()` Phase 6 (~L347-358):** Uses `if let Some(ref report) = review_report && let Err(e) = self.git_provider.add_comment(&pr_info.id, report).await { ... }` — this combined `if let` pattern has NO explicit success branch, making it impossible to emit `tool_result` on success. **Must refactor to `if let Some { match ... }`** to emit both success and error events.
- **`process_recovered_session()` (~L997-1008):** Same combined `if let` pattern but with a critical difference: it wraps the report in `strip_agent_artifacts(report)` before posting. **Preserve this `strip_agent_artifacts` call** when refactoring — do NOT remove it. The `process_story()` path does NOT strip artifacts.

The `StoryPipeline` struct already has `ui: UiHandle` (wired by Story 10.2). So `self.ui.tool_result(...)` is directly available.

**Important:** The pipeline-level `phase_start("Code Review")` / `phase_complete("Code Review", duration)` / `phase_error("Code Review", error)` events are already emitted by Story 10.2 around `self.review_runner.run(story)`. This story does NOT need to add those — they're already wired. This story focuses on the **internal** review events (activation, chat turns, tool calls, LLM events) and the PR comment result event.

### Project Structure Notes

- **Modified files:**
  - `src/review/mod.rs` — add UI event emissions throughout `drive_review_session()` and `run()` retry loop. Change `None` → `Some(&self.ui)` for all `stream_chat()` and `activate_agent()` calls. Add `truncate_summary` import or inline copy. Add `use std::time::Instant;` import
  - `src/session/runner.rs` — change `truncate_summary` from `fn` to `pub(crate) fn` (if using the reuse approach)
  - `src/pipeline.rs` — refactor `add_comment()` call sites in BOTH `process_story()` (~L347-358) AND `process_recovered_session()` (~L997-1008) from combined `if let` to `if let Some { match ... }` pattern, add `ui.tool_result("pr_comment", ...)` events for both success and error paths. **Preserve `strip_agent_artifacts()` in `process_recovered_session()` only.**
- **No new Rust files** — this story only modifies existing files
- **No new dependencies** — all crates already present from Stories 10.1-10.3

### Technical Requirements

- **Rust edition 2024** — all code must follow edition 2024 conventions (rustc 1.93+)
- **`#![deny(clippy::all)]`** — zero clippy warnings
- **`#![warn(dead_code)]`** — current crate-root setting; do NOT change this attribute
- **Error handling:** No `unwrap()` or `expect()` in production code — only in tests
- **Doc comments:** `///` mandatory on any new public functions
- **No `println!` / `eprintln!`** in daemon runtime code — use `UiHandle` for user-facing output, `tracing` for debug logging

### Library & Framework Requirements

- **`rig-core`** (latest stable) — tool call interception already implemented by Story 10.3 in `streaming_chat()`. Review benefits automatically by passing `Some(&self.ui)` to `stream_chat()`
- **`indicatif`** 0.18.x (added by Story 10.1) — no direct usage in review code; `ConsoleRenderer` handles rendering
- **`console`** 0.16.x (added by Story 10.1) — no direct usage in review code
- **`std::time::Instant`** — for phase duration tracking in post-review phase

### File Structure Requirements

- Follow existing code patterns: `use` imports at top, then structs, then `impl` blocks, then `#[cfg(test)] mod tests` at bottom
- Keep UI event emissions close to the corresponding `tracing` calls — they serve parallel purposes (tracing → debug file, ui → terminal)
- Pattern: `log_llm_request(...)` → `self.ui.llm_request(...)` — always pair them

### Testing Requirements

- All tests use `NullRenderer` via `UiHandle::null()` — zero test pollution
- No new test files needed — existing tests in `review/mod.rs` already cover the review flow
- `ReviewRunner::new()` in tests already receives `UiHandle::null()` (wired by Story 10.2)
- **Critical:** Run `cargo test` at the end — ALL existing tests must pass

### Previous Story Intelligence

**Story 10.3 (Session Integration — immediate predecessor, `ready-for-dev`):**
- Adds `ui: Option<&UiHandle>` parameter to `streaming_chat()` in `session/agent.rs`
- Adds `ui: Option<&UiHandle>` parameter to `activate_agent()` in `session/agent.rs`
- Adds `ui: Option<&UiHandle>` parameter to `BuiltAgent::stream_chat()` and `BuiltAgent::activate_agent()` in `llm/agent_factory.rs`
- **Review explicitly passes `None` for `ui`** in Task 11.2: "In `src/review/mod.rs`: pass `None` for `ui` in `activate_agent()` and `streaming_chat()` calls — review UI events are deferred to Story 10.4"
- Creates `truncate_summary()` as a private helper in `session/runner.rs` (Task 12)
- Tool call interception is in `streaming_chat()` — works automatically when `ui` is `Some`
- Emits `ui.tool_call(tool_name, detail)` and `ui.tool_result(tool_name, brief_result)` from the rig stream
- Per-tool detail format defined: `edit_file` → `"{path} ({mode})"`, `git` → `"{sub_action} {key_arg}"`, etc.

**Story 10.2 (Pipeline Integration — predecessor of 10.3, `ready-for-dev`):**
- Wires `ui: UiHandle` field into `ReviewRunner` struct
- `ReviewRunner::new()` gains a `ui: UiHandle` parameter
- Pipeline emits `phase_start("Code Review")` / `phase_complete("Code Review", duration)` / `phase_error("Code Review", error)` around `self.review_runner.run(story)` — these are ALREADY WIRED
- `StoryPipeline` has `ui: UiHandle` — available for PR comment events
- Constructor signature after 10.2: `ReviewRunner::new(config, secrets, agent_factory, shutdown, mcp_manager, ui)`

**Story 10.1 (Foundation — `ready-for-dev`):**
- `UiRenderer` trait with all method signatures including:
  - `activation_start(&self)`, `activation_complete(&self)`
  - `chat_turn(&self, turn: usize, summary: &str)`
  - `tool_call(&self, tool_name: &str, detail: &str)`, `tool_result(&self, tool_name: &str, detail: &str)`
  - `llm_request(&self, label: &str, turn: usize)`, `llm_response(&self, label: &str, turn: usize, response_len: usize)`
  - `llm_error(&self, label: &str, turn: usize, error: &str)`, `llm_retry(&self, label: &str, turn: u32, retry_count: u32, delay_secs: f64)`
  - `phase_start(&self, phase_name: &str)`, `phase_complete(&self, phase_name: &str, duration: Duration)`, `phase_error(&self, phase_name: &str, error: &str)`
- All methods take `&self`, return `()` — fire-and-forget
- `UiHandle` wraps `Arc<dyn UiRenderer>` — `Clone + Send + Sync`

**Key code patterns from `drive_review_session()` (L501-711 in current `review/mod.rs`):**
- Activation: `agent.activate_agent(project_root, agent_path, label, shutdown)` → returns `(rig_history, chat_history)`
- Initial CR: `agent.stream_chat(initial_message, activation_history, shutdown)` → `(response, full_history)`
- Chat loop: `loop { if post_review_phase { parse report, return } → analyze → match action → stream_chat(reply, history, shutdown) → turn += 1 }`
- Retry handling: `retries` counter, `MAX_RETRIES = 3`, immediate retry (no backoff)
- Post-review: `build_post_review_message()` sent as reply when `Completed` detected, parsed on next iteration

### Git Intelligence

Last 10 commits (most recent first):
1. `95f7a85` — `docs(story): add validated story 10.3 — session integration tool calls & chat turns visible`
2. `92a83e0` — `docs(story): add validated story 10.2 — pipeline integration UI events in story lifecycle`
3. `c933004` — `docs(story): add validated story 10.1 — ui/ module foundation, trait & console renderer`
4. `eadda06` — `chore(sprint): regenerate sprint-status with Epic 10, updated epic statuses`
5. `fb68dd8` — `docs(project-context): add Terminal UI rules for ui/ module (Epic 10)`
6. `e95a955` — `docs(epics): add Epic 10 — Terminal UI & Developer Experience (5 stories)`
7. `8280802` — `docs(architecture): add ui/ module for Terminal UI (Epic 10 / FR43)`
8. `75ef883` — `docs(prd): add FR43 — Terminal UI & Developer Experience`
9. `45dcb40` — `docs(planning): add sprint change proposal — Epic 10 Terminal UI`
10. `11f284a` — `fix(session): rebuild agent on Copilot token expiry mid-session`

All Epic 10 commits are planning/documentation only. No implementation code exists yet. Stories 10.1, 10.2, and 10.3 must be implemented first before this story can start.

### Copilot Token Refresh — Not Applicable

The current `drive_review_session()` does **NOT** have Copilot token expired detection or inline agent rebuild logic (unlike `run_session()` which has `is_token_expired_error()` handling with per-phase rebuilds). If a Copilot token expires mid-review, the error propagates up as a transient error and the full session retry in `run()` handles it. Therefore, there are **no Copilot token refresh UI events** to emit from review code — the full session retry event (Task 6.2) covers this case.

### Estimated Scope

This is a **3-point story** (as planned in the epics). It reuses all patterns established by Story 10.3. The changes are:
- **`review/mod.rs`**: ~30-40 lines of UI event emissions added (no structural changes)
- **`session/runner.rs`**: 1 line change (`fn` → `pub(crate) fn` on `truncate_summary`)
- **`pipeline.rs`**: ~15-20 lines — refactor 2 `add_comment` call sites from combined `if let` to `match`, add `tool_result` events
- **Total new/modified lines**: ~50-60 lines across 3 files

### References

- [Source: architecture.md#L716-781 — Tracing Pattern — Terminal UI Layer] — defines `UiHandle` usage, event emission patterns
- [Source: architecture.md#L954-980 — Enforcement Guidelines] — mandates `NullRenderer` in tests, `UiHandle` propagation
- [Source: project-context.md#L192-205 — Terminal UI Rules] — tool call UI at call sites not inside tools, `UiHandle` propagation chain
- [Source: project-context.md#L39-112 — Framework-Specific Rules] — rig agent + tool calling, streaming architecture
- [Source: epics.md#L2536-2586 — Epic 10 / Story 10.4] — full acceptance criteria and dev notes
- [Source: epics.md#L2447-2536 — Epic 10 / Story 10.3] — predecessor story, tool call interception approach
- [Source: epics.md#L2372-2447 — Epic 10 / Story 10.2] — pipeline wiring, `UiHandle` in `ReviewRunner`
- [Source: epics.md#L2303-2372 — Epic 10 / Story 10.1] — trait method signatures for all UI events
- [Source: review/mod.rs#L263-285 — ReviewOutcome enum] — three possible review results
- [Source: review/mod.rs#L297-310 — ReviewRunner struct] — current fields (ui field added by Story 10.2)
- [Source: review/mod.rs#L312-378 — ReviewRunner::new() and run()] — constructor and retry loop
- [Source: review/mod.rs#L409-489 — run_inner()] — agent build and session setup
- [Source: review/mod.rs#L501-711 — drive_review_session()] — chat loop, post-review phase, all interception points
- [Source: pipeline.rs#L300-360 — process_story() review phases] — review execution, PR comment posting
- [Source: session/agent.rs#L276-348 — streaming_chat()] — rig streaming loop with tool call interception (after Story 10.3)
- [Source: session/agent.rs#L367-426 — activate_agent()] — activation flow with ui parameter (after Story 10.3)
- [Source: llm/agent_factory.rs#L91-169 — BuiltAgent methods] — stream_chat() and activate_agent() wrappers with ui parameter (after Story 10.3)
- [Source: 10-3 story — Task 11.2] — review explicitly passes None for ui, deferred to this story
- [Source: 10-3 story — Task 12] — truncate_summary() helper creation

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List