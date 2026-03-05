# Story 10.3: Session Integration — Tool Calls & Chat Turns Visible

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer monitoring the daemon in tmux,
I want to see each agent tool call, chat turn, and LLM interaction in real-time,
So that I understand what the agent is doing without reading debug logs.

## Acceptance Criteria

1. **Given** `SessionRunner` struct in `src/session/runner.rs` **When** I inspect the struct definition **Then** it already contains a `ui: UiHandle` field (wired by Story 10.2) **And** this story adds UI event emissions throughout the session lifecycle.

2. **Given** the agent activation sequence in `drive_activation_and_recover()` / `run_session()` **When** activation begins **Then** `ui.activation_start()` is emitted **When** activation completes successfully **Then** `ui.activation_complete()` is emitted **When** activation fails **Then** `ui.phase_error("Agent Activation", error)` is emitted.

3. **Given** the chat loop in `run_session()` **When** a chat turn completes (response received from LLM) **Then** `ui.chat_turn(turn_number, truncated_summary)` is emitted **And** the summary is the first 80 characters of the response, truncated with `…` if longer **When** `ResponseAction::Completed` is detected **Then** `ui.completion_detected(story_key)` is emitted.

4. **Given** the post-completion sequence in `run_session()` **When** the final commit phase starts **Then** `ui.phase_start("Final Commit")` is emitted **When** the impact analysis phase starts **Then** `ui.phase_start("Impact Analysis")` is emitted **When** the PR summary phase starts **Then** `ui.phase_start("PR Summary")` is emitted **And** each phase emits `phase_complete` or `phase_error` on completion.

5. **Given** an LLM request is sent via `stream_chat()` **When** `log_llm_request()` is called in `run_session()` **Then** `ui.llm_request(label, turn)` is also emitted (starts a thinking spinner) **When** `log_llm_response()` is called **Then** `ui.llm_response(label, turn, response_len)` is also emitted (resolves spinner) **When** `log_llm_error()` is called **Then** `ui.llm_error(label, turn, error)` is also emitted.

6. **Given** a transient LLM error triggers a retry **When** the retry loop in `run_session()` backs off **Then** `ui.llm_retry(label, turn, retry_count, delay_secs)` is emitted **And** the terminal shows the retry count and backoff duration.

7. **Given** a Copilot token refresh is triggered **When** `is_token_expired_error()` returns true and agent is rebuilt **Then** `ui.llm_retry(label, turn, refresh_count, 0)` is emitted with `delay_secs=0` to indicate token refresh (not backoff).

8. **Given** the agent calls tools during the chat loop **When** a tool invocation is detected from the rig streaming pipeline **Then** `ui.tool_call(tool_name, detail)` is emitted for each tool call **And** the detail includes the key argument per tool (see Dev Notes for per-tool detail format).

9. **Given** a tool call completes **When** the tool result is available **Then** `ui.tool_result(tool_name, brief_result)` is emitted **And** `brief_result` is a short summary (e.g., file size, line count, match count, exit code).

10. **Given** the existing `tracing::info!` calls in tool implementations (`src/tools/*.rs`) **When** tool events need to be emitted to the UI **Then** the `tracing::info!` calls remain unchanged (they continue to log to the file) **And** UI events are emitted at the session runner level, NOT inside tool implementations.

11. **Given** all existing tests **When** they run **Then** they pass without modification (using `NullRenderer` via `UiHandle::null()`).

## Tasks / Subtasks

- [x] Task 1: Add UI event emissions to `streaming_chat()` in `src/session/agent.rs` for tool call visibility (AC: #8, #9, #10)
  - [x] 1.1 Add an optional `ui: Option<&UiHandle>` parameter to `streaming_chat()` — passing `None` preserves backward compatibility for supervisor/review callers
  - [x] 1.2 In the `match chunk` loop, add a new arm for `MultiTurnStreamItem::ToolCallStart` (or equivalent rig stream item variant that carries the tool name and arguments) — emit `ui.tool_call(tool_name, detail)` with the appropriate detail extraction
  - [x] 1.3 If rig does not expose a `ToolCallStart` variant, use approach 3 from the epics: inspect `MultiTurnStreamItem` variants for tool call deltas. The `_ => continue` catch-all currently swallows these — add explicit matching for tool-related variants
  - [x] 1.4 For tool result visibility: if rig provides a `ToolResult` stream item variant, match it and emit `ui.tool_result(tool_name, brief_result)`. If not, emit `ui.tool_result` from the `ChatHistoryHook::on_completion_call` by inspecting the history for new tool result messages since the last snapshot
  - [x] 1.5 **FALLBACK approach if rig stream items don't carry tool info:** Extend `ChatHistoryHook` to detect tool calls/results by comparing successive history snapshots in `on_completion_call`. Each new tool call/result message in the history diff triggers a `ui.tool_call` or `ui.tool_result` emission. This requires adding `ui: Option<UiHandle>` to `ChatHistoryHook`
  - [x] 1.6 Add `use crate::ui::UiHandle;` import to `src/session/agent.rs`
  - [x] 1.7 Verify `cargo check` passes

- [x] Task 2: Add `ui` parameter to `activate_agent()` in `src/session/agent.rs` for tool call forwarding (AC: #8)
  - [x] 2.1 Add an optional `ui: Option<&UiHandle>` parameter to `activate_agent()`
  - [x] 2.2 Pass `ui` through to the inner `streaming_chat()` call so tool calls during activation are also visible
  - [x] 2.3 Do NOT emit `activation_start()` / `activation_complete()` / `phase_error()` here — `activate_agent()` is a shared utility used by dev session, review, and supervisor. Activation lifecycle events are emitted at the **caller level** (Tasks 4, 9, 10) where the context is known

- [x] Task 3: Update `BuiltAgent` wrapper methods in `src/llm/agent_factory.rs` (AC: #2, #8)
  - [x] 3.1 Add optional `ui: Option<&UiHandle>` parameter to `BuiltAgent::stream_chat()` — forward to `streaming_chat()`
  - [x] 3.2 Add optional `ui: Option<&UiHandle>` parameter to `BuiltAgent::activate_agent()` — forward to `activate_agent()`
  - [x] 3.3 Add `use crate::ui::UiHandle;` import
  - [x] 3.4 Update all match arms in both methods to pass `ui` through

- [x] Task 4: Emit activation UI events in `run_session()` normal path (AC: #2, #5)
  - [x] 4.1 Before the activation retry loop: emit `self.ui.activation_start()`
  - [x] 4.2 After successful activation (after the loop breaks): emit `self.ui.activation_complete()`
  - [x] 4.3 On activation failure (permanent): emit `self.ui.phase_error("Agent Activation", &error)` before returning `SessionOutcome::Failed`
  - [x] 4.4 Pass `Some(&self.ui)` to all `agent.activate_agent()` and `agent.stream_chat()` calls in the normal initialization path

- [x] Task 5: Emit LLM request/response/error UI events in `run_session()` chat loop (AC: #3, #4, #5)
  - [x] 5.1 After every `log_llm_request()` call: add `self.ui.llm_request(label, turn)` — this starts a thinking spinner in `ConsoleRenderer`
  - [x] 5.2 After every `log_llm_response()` call: add `self.ui.llm_response(label, turn, response.len())` — this resolves the spinner
  - [x] 5.3 After every `log_llm_error()` call: add `self.ui.llm_error(label, turn, &error_string)` — this shows error in UI
  - [x] 5.4 Pass `Some(&self.ui)` to all `agent.stream_chat()` calls in the chat loop so tool calls are visible

- [x] Task 6: Emit chat turn and completion UI events in `run_session()` (AC: #3)
  - [x] 6.1 After the `tracing::debug!(action = "chat_turn", ...)` at the bottom of each loop iteration (inside the main `loop {}`): emit `self.ui.chat_turn(turn, &truncated_summary)` where `truncated_summary` = first 80 chars of `current_response` + `"…"` if longer. **Note:** The initial DS response (turn 0) is emitted BEFORE the loop starts, so `chat_turn` is naturally only emitted for turn 1+ inside the loop — this is correct and avoids noise from the activation response
  - [x] 6.2 In the `ResponseAction::Completed` match arm, before the final commit phase: emit `self.ui.completion_detected(&story.story_key)`

- [x] Task 7: Emit post-completion phase UI events in `run_session()` (AC: #4)
  - [x] 7.1 Before the final commit `stream_chat()` call: emit `self.ui.phase_start("Final Commit")`, capture `Instant::now()`
  - [x] 7.2 After final commit succeeds: emit `self.ui.phase_complete("Final Commit", elapsed)`
  - [x] 7.3 After final commit fails (non-fatal): emit `self.ui.phase_error("Final Commit", &error)` — session continues
  - [x] 7.4 Before the impact analysis `stream_chat()` call: emit `self.ui.phase_start("Impact Analysis")`, capture `Instant::now()`
  - [x] 7.5 After impact analysis succeeds: emit `self.ui.phase_complete("Impact Analysis", elapsed)`
  - [x] 7.6 After impact analysis fails (non-fatal): emit `self.ui.phase_error("Impact Analysis", &error)` — session continues
  - [x] 7.7 Before the PR summary `stream_chat()` call: emit `self.ui.phase_start("PR Summary")`, capture `Instant::now()`
  - [x] 7.8 After PR summary succeeds: emit `self.ui.phase_complete("PR Summary", elapsed)`
  - [x] 7.9 After PR summary fails (non-fatal): emit `self.ui.phase_error("PR Summary", &error)`

- [x] Task 8: Emit retry and token refresh UI events (AC: #6, #7)
  - [x] 8.0 **FIRST:** Verify the actual `UiRenderer::llm_retry()` signature in `src/ui/renderer.rs` (implemented by Story 10.1). The type casts below (`as u32`, `as f64`) are indicative — adapt to match the trait's actual parameter types. If the trait uses `usize` for turn/retry_count (like other methods), use those types directly without casting
  - [x] 8.1 In the activation retry loop: after `tracing::warn!(action = "activation_transient_retry", ...)`, emit `self.ui.llm_retry("dev-session", 0, activation_retries as u32, delay as f64)`
  - [x] 8.2 In the DS send retry loop: after the transient retry `tracing::warn!`, emit `self.ui.llm_retry("dev-session", 0, ds_retries as u32, delay as f64)`
  - [x] 8.3 In the main chat loop retry path: after `tracing::warn!(action = "chat_error", ...)`, emit `self.ui.llm_retry("dev-session", turn as u32, retries as u32, backoff_secs as f64)`
  - [x] 8.4 On every Copilot token expired rebuild (activation, DS, chat loop, final commit, impact analysis, PR summary): emit `self.ui.llm_retry(label, turn as u32, token_refreshes as u32, 0.0)` — `delay_secs=0` signals token refresh, not backoff

- [x] Task 9: Emit UI events in `drive_activation_and_recover()` for context limit recovery (AC: #2, #5)
  - [x] 9.1 At the start of `drive_activation_and_recover()`: emit `self.ui.activation_start()`
  - [x] 9.2 After the activation `activate_agent()` succeeds: emit `self.ui.activation_complete()`
  - [x] 9.3 On activation failure: emit `self.ui.phase_error("Agent Activation", &error)`
  - [x] 9.4 Add `self.ui.llm_request` / `self.ui.llm_response` / `self.ui.llm_error` around the CH and "Load project context" `stream_chat()` calls (steps 4b and 4c)
  - [x] 9.5 Pass `Some(&self.ui)` to all `agent.stream_chat()` and `agent.activate_agent()` calls

- [x] Task 10: Emit UI events in recovery path of `run_session()` (AC: #2, #5)
  - [x] 10.1 In `Some(mut state)` recovery branch — empty history sub-case: emit `self.ui.activation_start()` before activation, `self.ui.activation_complete()` after, and LLM request/response around `stream_chat`
  - [x] 10.2 In sub-case B (last message is user — re-send): emit `self.ui.llm_request` before and `self.ui.llm_response` / `self.ui.llm_error` after the re-send
  - [x] 10.3 Pass `Some(&self.ui)` to all `agent.stream_chat()` and `agent.activate_agent()` calls in recovery paths

- [x] Task 11: Update all callers of `streaming_chat()` and `activate_agent()` (AC: #11)
  - [x] 11.1 In `src/supervisor/architect.rs`: pass `None` for `ui` in `streaming_chat()` and `activate_agent()` calls — supervisor does not emit session-level UI events
  - [x] 11.2 In `src/review/mod.rs`: pass `None` for `ui` in `activate_agent()` and `streaming_chat()` calls — review UI events are deferred to Story 10.4
  - [x] 11.3 Verify all other callers (grep for `streaming_chat` and `activate_agent` usage) pass the correct `ui` value

- [x] Task 12: Add helper function for truncating response summaries (AC: #3)
  - [x] 12.1 Add a `fn truncate_summary(text: &str, max_len: usize) -> String` utility in `src/session/runner.rs` (private) that returns first `max_len` chars + `"…"` if truncated, or the full string if within limit. **Use `text.char_indices().nth(max_len)` to find the correct Unicode boundary — NEVER slice by byte index** (e.g., `&text[..80]` panics on multi-byte chars)
  - [x] 12.2 Add unit tests: `test_truncate_summary_short_text`, `test_truncate_summary_exact_limit`, `test_truncate_summary_long_text`, `test_truncate_summary_empty`, `test_truncate_summary_unicode_boundary`

- [x] Task 13: Run full test suite and linting (AC: #11)
  - [x] 13.1 Run `cargo test` — ALL existing tests must pass with zero failures
  - [x] 13.2 Run `cargo clippy` — zero warnings from our changes (3 pre-existing clippy errors in `session/cleanup.rs` and `watcher/deps.rs` unrelated to this story)
  - [x] 13.3 Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Architecture Compliance

- **UI events are fire-and-forget:** All `UiRenderer` methods take `&self` and return `()`. No error propagation from UI to business logic. The `ConsoleRenderer` handles its own errors internally via `tracing::debug!`.
- **Tool call UI events emitted at session runner level, NOT inside tools:** The `UiHandle` is NOT injected into tool structs (`EditFileTool`, `ReadFileTool`, etc.). Tool implementations remain unchanged — their existing `tracing::info!(action = ...)` calls continue to log to the JSON file. UI events are emitted either from `streaming_chat()` by inspecting rig stream items, or from `ChatHistoryHook` by diffing successive history snapshots.
- **`tracing::info!` calls remain unchanged:** All existing tracing calls in `session/runner.rs`, `session/agent.rs`, `tools/*.rs`, and `llm/logging.rs` stay exactly as they are. UI events are a separate, additive concern.
- **`NullRenderer` in tests:** All tests continue using `UiHandle::null()`. No test behavior changes. No test pollution from `ConsoleRenderer` output.
- **`UiHandle` parameter convention:** Use `Option<&UiHandle>` for functions shared across contexts where UI may not be desired (e.g., supervisor, review). Use `&self.ui` directly in `SessionRunner` methods where `UiHandle` is always available.
- **Activation events at caller level only:** `activate_agent()` is a shared utility — it does NOT emit `activation_start/complete/phase_error`. These lifecycle events are emitted by the **callers** (`run_session()` Task 4, `drive_activation_and_recover()` Task 9, recovery paths Task 10) who know the semantic context. `activate_agent()` only forwards `ui` to `streaming_chat()` for tool call visibility.
- **`&UiHandle` lifetime in async generics:** `streaming_chat()` and `activate_agent()` are async generic functions. The `Option<&UiHandle>` borrow must outlive the async future. Since callers hold `self.ui` (owned `UiHandle` on `SessionRunner`), the borrow is valid for the entire `run_session()` scope. If the compiler rejects the borrow (unlikely), the fallback is `Option<UiHandle>` (owned clone — cheap, Arc-based).

### Tool Call Detail Format

When emitting `ui.tool_call(tool_name, detail)`, use the following per-tool detail format:

- **`edit_file`** → `"{path} ({mode})"` — e.g., `"src/session/runner.rs (edit)"`, `"src/ui/mod.rs (create)"`
- **`read_file`** → `"{path}"` or `"{path} L{start}-{end}"` — e.g., `"src/main.rs L100-200"`
- **`grep`** → `"/{pattern}/"` — e.g., `"/fn run_session/"`
- **`find_path`** → `"{glob}"` — e.g., `"src/**/*.rs"`
- **`list_directory`** → `"{path}"` — e.g., `"src/session/"`
- **`git`** → `"{sub_action} {key_arg}"` — e.g., `"commit \"feat(session): add recovery\""`, `"checkout story/1-2"`
- **`terminal`** → first 80 chars of the command — e.g., `"cargo test session::tests"`
- **`ask_supervisor`** → first 80 chars of the question
- **`think`** → `"(reasoning)"` — no detail needed, just note thinking is happening

**Extraction approach:** Tool call details are extracted from the tool's JSON arguments (`Self::Args` struct). The extraction happens in `streaming_chat()` if rig stream items carry the tool name + args, or in the `ChatHistoryHook` by parsing the tool call messages in the history diff.

### Implementation Approach for Tool Call Visibility

**🚨 FIRST STEP — Before writing any code:**

Inspect the actual `MultiTurnStreamItem` enum definition from the rig crate source. Use `grep` or `read_file` on `~/.cargo/registry/src/` (or check the rig docs/GitHub) to find:
- The exact variant names in `rig::agent::MultiTurnStreamItem`
- Whether tool call variants carry the tool name and JSON arguments
- Whether tool result variants exist and what payload they carry

This discovery determines whether the primary approach or the fallback is used. Do NOT assume variant names — verify them.

**Primary approach — Inspect rig `MultiTurnStreamItem` variants in `streaming_chat()`:**

The current `streaming_chat()` function has a `_ => continue` catch-all that swallows tool call variants. Rig's `MultiTurnStreamItem` enum includes variants for tool calls that are currently ignored. The implementation should:

1. **Enumerate the actual rig `MultiTurnStreamItem` variants** by checking the rig source or docs. Look for variants like `ToolCallStart`, `ToolCall`, `ToolResult`, or similar.
2. **Match tool call variants explicitly** instead of using `_ => continue`. Extract the tool name and arguments from the variant payload.
3. **Emit `ui.tool_call()`** when a tool call is detected, with the detail extracted from the arguments.
4. **Emit `ui.tool_result()`** when a tool result variant appears.

If rig does not provide enough information in stream items:

**Fallback approach — Extend `ChatHistoryHook`:**

1. Add `ui: Option<UiHandle>` to `ChatHistoryHook` (cloneable since `UiHandle` is `Clone`).
2. In `on_completion_call()`, compare the new `history` with the previously captured snapshot.
3. New messages with role `"tool_call"` or tool-call content trigger `ui.tool_call()`.
4. New messages with role `"tool_result"` or tool-result content trigger `ui.tool_result()`.

**The developer should inspect rig's actual `MultiTurnStreamItem` enum definition to determine which approach is feasible.** Use `grep` or `read_file` on rig's source to find the exact variant names and payloads. The key rig types to inspect:
- `rig::agent::MultiTurnStreamItem` — the streaming chunk enum
- `rig::message::ToolCall` / `rig::message::ToolResult` — tool call/result message types
- The `Message` enum variants in `rig::completion` — may contain tool call/result variants in the history

### Critical: Understand rig's Streaming Architecture

The `streaming_chat()` function in `src/session/agent.rs` (L276-348) currently handles:
- `MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(Text { text }))` → accumulates text
- `MultiTurnStreamItem::FinalResponse(_)` → breaks the loop
- `Err(e)` → returns error
- `_ => continue` → **swallows everything else including tool calls**

The `_ => continue` arm is the interception point. Rig handles tool dispatch internally within the multi-turn stream — the agent calls tools, gets results, and continues streaming. The stream items that flow through include tool call events that we currently ignore.

**IMPORTANT:** The `ChatHistoryHook::on_completion_call()` is invoked by rig **before each LLM call** (not after tool calls). It captures `(history, prompt)` where `history` includes all prior messages including tool calls and tool results. This means successive invocations of `on_completion_call` will have progressively longer histories that include tool call/result messages — making the diff-based approach viable.

### Project Structure Notes

- **Modified files:**
  - `src/session/agent.rs` — add `ui` parameter to `streaming_chat()` and `activate_agent()`, tool call matching in stream loop, optional `ChatHistoryHook` extension
  - `src/session/runner.rs` — add `ui.*` event emissions throughout `run_session()`, `drive_activation_and_recover()`, recovery paths. Add `truncate_summary()` helper
  - `src/llm/agent_factory.rs` — update `BuiltAgent::stream_chat()` and `BuiltAgent::activate_agent()` signatures to accept `ui` parameter
  - `src/supervisor/architect.rs` — pass `None` for `ui` in `streaming_chat()` / `activate_agent()` calls
  - `src/review/mod.rs` — pass `None` for `ui` in `activate_agent()` / `streaming_chat()` calls (review events deferred to Story 10.4)
- **No new Rust files** — this story only modifies existing files

### Code Path Coverage Notes

- **`resume_session()` (L373-606):** This is the public crash recovery entry point. It builds the agent and delegates to `run_session()` (via `self.run()` → `self.run_session()`). All UI events from Tasks 4-10 in `run_session()` automatically cover this path — no separate events needed in `resume_session()` itself.
- **`context_limit_recovery()` (L930-1012):** Calls `drive_activation_and_recover()` which calls `run_session()`. UI events are inherited transitively — Task 9 covers `drive_activation_and_recover()`, and the inner `run_session()` call picks up Tasks 4-8. No separate events needed in `context_limit_recovery()` itself.
- **`run()` (L614-750):** Public entry point for normal story execution. Builds agent, calls `run_session()`. UI events from Tasks 4-10 cover this path. The only addition needed is passing `Some(&self.ui)` to `build_agent_for_role` → `agent.activate_agent()` calls.

### Technical Requirements

- **Rust edition 2024** — all code must follow edition 2024 conventions (rustc 1.93+)
- **`#![deny(clippy::all)]`** — zero clippy warnings
- **`#![warn(dead_code)]`** — current crate-root setting; do NOT change this attribute
- **Error handling:** No `unwrap()` or `expect()` in production code — only in tests
- **Doc comments:** `///` mandatory on all new public functions and parameters
- **No `println!` / `eprintln!`** in daemon runtime code — use `UiHandle` for user-facing output, `tracing` for debug logging

### Library & Framework Requirements

- **`rig-core`** (latest stable) — the AI agent framework. Key types for tool call interception:
  - `rig::agent::MultiTurnStreamItem` — streaming chunk enum with variants for text, tool calls, tool results, and final response
  - `rig::message::ToolCall` — tool call payload (name, arguments)
  - `rig::message::ToolResult` — tool result payload
  - `rig::completion::Message` — conversation message (may include tool call/result variants)
  - `rig::agent::StreamingPromptHook` — hook trait for `ChatHistoryHook` (already used)
- **`indicatif`** 0.18.x (added by Story 10.1) — `ConsoleRenderer` uses this for spinners; no direct usage in session code
- **`console`** 0.16.x (added by Story 10.1) — no direct usage in session code
- **`std::time::Instant`** — for phase duration tracking in post-completion phases

### File Structure Requirements

- Follow existing code patterns: `use` imports at top, then structs, then `impl` blocks, then `#[cfg(test)] mod tests` at bottom
- Keep UI event emissions close to the corresponding `tracing` calls — they serve parallel purposes (tracing → debug file, ui → terminal)
- Pattern: `log_llm_request(...)` → `self.ui.llm_request(...)` — always pair them

### Testing Requirements

- All tests use `NullRenderer` via `UiHandle::null()` — zero test pollution
- Test naming: `test_{module}_{behavior}_{scenario}` in snake_case
- Arrange → Act → Assert pattern
- New tests for `truncate_summary()`:
  - `test_truncate_summary_short_text` — returns unchanged if under limit
  - `test_truncate_summary_exact_limit` — returns unchanged at exact limit
  - `test_truncate_summary_long_text` — truncates with `…` appended
  - `test_truncate_summary_empty` — returns empty string
  - `test_truncate_summary_unicode_boundary` — handles multi-byte chars at boundary without panic
- Existing tests for `streaming_chat()`, `activate_agent()`, `ChatHistoryHook` — may need `None` passed for new `ui` parameter
- Existing `SessionRunner` tests — already use `UiHandle::null()` (wired by Story 10.2)
- **Critical:** Run `cargo test` at the end — ALL existing tests must pass

### Previous Story Intelligence

**Story 10.2 (Pipeline Integration — immediate predecessor, `ready-for-dev`):**
- Wires `UiHandle` into `StoryPipeline`, `SessionRunner`, `ReviewRunner` via constructor parameters
- `SessionRunner` struct gains `ui: UiHandle` field — but does NOT emit any events (that's this story)
- `ReviewRunner` struct gains `ui: UiHandle` field — does NOT emit events (Story 10.4)
- Pipeline emits `story_start`, `phase_start/complete/error`, `batch_start/complete`, `crash_recovery_*`, `daemon_start`, `poll_cycle`, `stories_found`, `shutdown_requested`
- `init_tracing()` gains `ui_active: bool` parameter — conditionally removes stdout layer
- `UiHandle::null()` added to all test helpers
- Constructor signature changes:
  - `SessionRunner::new(config, agent_factory, shutdown, mcp_manager, ui)`
  - `ReviewRunner::new(config, secrets, agent_factory, shutdown, mcp_manager, ui)`

**Story 10.1 (Foundation — `ready-for-dev`):**
- Creates `src/ui/mod.rs`, `renderer.rs`, `console.rs`, `null.rs`
- `UiRenderer` trait with all method signatures including:
  - `chat_turn(&self, turn: usize, summary: &str)`
  - `activation_start(&self)`, `activation_complete(&self)`
  - `completion_detected(&self, story_key: &str)`
  - `tool_call(&self, tool_name: &str, detail: &str)`
  - `tool_result(&self, tool_name: &str, detail: &str)`
  - `llm_request(&self, label: &str, turn: usize)`
  - `llm_response(&self, label: &str, turn: usize, response_len: usize)`
  - `llm_error(&self, label: &str, turn: usize, error: &str)`
  - `llm_retry(&self, label: &str, turn: u32, retry_count: u32, delay_secs: f64)`
  - `phase_start(&self, phase_name: &str)`, `phase_complete(&self, phase_name: &str, duration: Duration)`, `phase_error(&self, phase_name: &str, error: &str)`
- All methods take `&self`, return `()` — fire-and-forget
- `UiHandle` wraps `Arc<dyn UiRenderer>` — `Clone + Send + Sync`

**Key code patterns from `run_session()` (L1188-2152 in `session/runner.rs`):**
- Chat loop structure: `loop { analyze → match action { Completed → ..., Escalated → ..., Continue/NoReply → stream_chat() } }`
- Post-completion has 3 sequential phases: Final Commit (step 7), Impact Analysis (step 8), PR Summary (step 9)
- Each phase has token-expired rebuild retry logic (one retry attempt)
- Error handling: non-fatal failures log a warning and continue to next phase
- `retries` counter for transient errors, `token_refreshes` counter for Copilot token rebuilds

**Key code patterns from `activate_agent()` (L367-426 in `session/agent.rs`):**
- Sends agent file as XML context → gets activation response → returns `(rig_history, chat_history)`
- Uses `streaming_chat()` internally for the activation LLM call
- Logs via `log_llm_request/response/error` — same pattern to follow for UI events

**Key code patterns from `streaming_chat()` (L276-348 in `session/agent.rs`):**
- Creates `ChatHistoryHook`, builds stream via `agent.stream_chat().with_hook().multi_turn()`
- Loops over `stream.next().await` chunks
- `_ => continue` swallows tool call variants — this is the interception point
- Returns `(accumulated_text, full_history)`

**Key code patterns from `drive_review_session()` (L501-711 in `review/mod.rs`):**
- Same pattern as `run_session()` but for code review
- Uses `activate_agent()` then chat loop with `stream_chat()`
- Will be updated in Story 10.4 to emit UI events — for now, pass `None` for `ui`

### Git Intelligence

Last 10 commits (most recent first):
1. `92a83e0` — `docs(story): add validated story 10.2 — pipeline integration UI events in story lifecycle`
2. `c933004` — `docs(story): add validated story 10.1 — ui/ module foundation, trait & console renderer`
3. `eadda06` — `chore(sprint): regenerate sprint-status with Epic 10, updated epic statuses`
4. `fb68dd8` — `docs(project-context): add Terminal UI rules for ui/ module (Epic 10)`
5. `e95a955` — `docs(epics): add Epic 10 — Terminal UI & Developer Experience (5 stories)`
6. `8280802` — `docs(architecture): add ui/ module for Terminal UI (Epic 10 / FR43)`
7. `75ef883` — `docs(prd): add FR43 — Terminal UI & Developer Experience`
8. `45dcb40` — `docs(planning): add sprint change proposal — Epic 10 Terminal UI`
9. `11f284a` — `fix(session): rebuild agent on Copilot token expiry mid-session`
10. `5d297f6` — `fix(gitlab): encode all slashes in nested group project paths`

All Epic 10 commits are planning/documentation only. No implementation code exists yet. Stories 10.1 and 10.2 must be implemented first before this story can start.

### References

- [Source: architecture.md#L716-781 — Tracing Pattern — Terminal UI Layer] — defines `UiHandle` usage, tool call visibility, event emission patterns
- [Source: architecture.md#L659-716 — Rig Tool Implementation Pattern] — tool struct design, `Tool::call()` signature, tracing conventions
- [Source: architecture.md#L954-980 — Enforcement Guidelines] — mandates `NullRenderer` in tests, `UiHandle` propagation
- [Source: project-context.md#L192-205 — Terminal UI Rules] — tool call UI at call sites not inside tools, `UiHandle` propagation chain
- [Source: project-context.md#L39-112 — Framework-Specific Rules] — rig agent + tool calling, streaming architecture
- [Source: epics.md#L2447-2536 — Epic 10 / Story 10.3] — full acceptance criteria, implementation notes, 3 suggested approaches
- [Source: epics.md#L2303-2372 — Epic 10 / Story 10.1] — trait method signatures for all UI events
- [Source: epics.md#L2372-2447 — Epic 10 / Story 10.2] — pipeline wiring, `UiHandle` in `SessionRunner`
- [Source: session/agent.rs#L276-348 — streaming_chat()] — current streaming loop with `_ => continue` catch-all
- [Source: session/agent.rs#L367-426 — activate_agent()] — activation flow with `streaming_chat()` call
- [Source: session/agent.rs#L136-179 — ChatHistoryHook] — hook that captures history snapshots on each LLM call
- [Source: session/runner.rs#L290-330 — SessionRunner struct + new()] — struct with `ui: UiHandle` field (after Story 10.2)
- [Source: session/runner.rs#L1188-2152 — run_session()] — chat loop, post-completion phases, retry logic
- [Source: session/runner.rs#L1022-1173 — drive_activation_and_recover()] — context limit recovery activation
- [Source: llm/agent_factory.rs#L91-169 — BuiltAgent methods] — `stream_chat()` and `activate_agent()` wrappers
- [Source: review/mod.rs#L501-711 — drive_review_session()] — review chat loop using same agent patterns
- [Source: supervisor/architect.rs] — supervisor uses `streaming_chat()` / `activate_agent()` — pass `None` for `ui`
- [Source: llm/logging.rs — log_llm_request/response/error] — existing logging functions to pair with UI events

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

### Completion Notes List

- ✅ Task 1: Added tool call/result visibility to `streaming_chat()`. Rig's `MultiTurnStreamItem` exposes `StreamedAssistantContent::ToolCall { tool_call, internal_call_id }` and `StreamUserItem(StreamedUserContent::ToolResult { tool_result, internal_call_id })`. Primary approach used — no ChatHistoryHook fallback needed. Added `extract_tool_call_detail()` (per-tool format from Dev Notes), `extract_tool_result_brief()` (heuristic: error prefix, line count, or truncated text), and `truncate_str()` helper. HashMap tracks `internal_call_id → tool_name` for correlating results back to their tool. All 15 call sites in runner.rs updated to pass `Some(&self.ui)`. Supervisor and review callers pass `None`. 1082 tests pass, 0 failures.
- ✅ Task 2: `activate_agent()` already received `ui: Option<&UiHandle>` in Task 1 — forwarded to inner `streaming_chat()`. No activation lifecycle events emitted here (shared utility).
- ✅ Task 3: `BuiltAgent::stream_chat()` and `BuiltAgent::activate_agent()` in `agent_factory.rs` both received `ui` param and forward through all match arms (Anthropic, OpenAiResponses, OpenAiCompletions).
- ✅ Task 4: Emitted `activation_start()` before activation retry loop, `activation_complete()` after successful activation, `phase_error("Agent Activation", &e)` on permanent failure in normal path.
- ✅ Task 5: Added `self.ui.llm_request()` after every `log_llm_request()`, `self.ui.llm_response()` after every `log_llm_response()`, `self.ui.llm_error()` after every `log_llm_error()` across DS send, main chat loop, final commit, impact analysis, PR summary, and all recovery paths.
- ✅ Task 6: Added `self.ui.chat_turn(turn, &summary)` with `truncate_summary()` at end of main loop iteration. Added `self.ui.completion_detected(&story.story_key)` in `ResponseAction::Completed` arm.
- ✅ Task 7: Added `phase_start`/`phase_complete`/`phase_error` for "Final Commit", "Impact Analysis", and "PR Summary" phases with `Instant::now()` duration tracking. Both primary and token-refresh-retry paths emit correct phase events.
- ✅ Task 8: Added `self.ui.llm_retry()` for: activation transient retries (delay > 0), DS send transient retries (delay > 0), chat loop backoff retries (delay > 0), and all Copilot token expired rebuilds (delay_secs=0.0 to signal token refresh). Covers activation, DS, chat loop, final commit, impact analysis, and PR summary token refresh paths.
- ✅ Task 9: Added `activation_start()`/`activation_complete()`/`phase_error()` in `drive_activation_and_recover()`. Added `llm_request`/`llm_response`/`llm_error` around CH and "Load project context" `stream_chat()` calls. All calls pass `Some(&self.ui)`.
- ✅ Task 10: Added activation lifecycle events and LLM request/response/error events in all recovery paths: empty history sub-case (activation + DS), sub-case B (re-send last user message). All calls pass `Some(&self.ui)`.
- ✅ Task 11: All callers updated — supervisor (3 `stream_chat` + 1 `activate_agent` → `None`), review (2 `stream_chat` + 1 `activate_agent` → `None`), runner (12 `stream_chat` + 3 `activate_agent` → `Some(&self.ui)`). Verified via grep — no remaining callers missing the `ui` parameter.
- ✅ Task 12: Added `truncate_summary()` utility in `runner.rs`. Uses `char_indices().nth(max_len)` for safe Unicode boundary detection. 5 unit tests pass: short text, exact limit, long text, empty, unicode boundary.
- ✅ Task 13: `cargo test` → 1087 passed (1082 existing + 5 new), 0 failed. `cargo clippy` → 0 new warnings (3 pre-existing in cleanup.rs/deps.rs). `cargo fmt --check` → clean.

### File List

- `src/session/agent.rs` — added `ui: Option<&UiHandle>` to `streaming_chat()` and `activate_agent()`, added `StreamedUserContent` import, explicit ToolCall/ToolResult match arms in streaming loop, `extract_tool_call_detail()` (per-tool detail format), `extract_tool_result_brief()` (heuristic summary), `truncate_str()` helper, HashMap for internal_call_id→tool_name correlation
- `src/llm/agent_factory.rs` — added `ui: Option<&crate::ui::UiHandle>` to `BuiltAgent::stream_chat()` and `BuiltAgent::activate_agent()`, forwarded through all 3 provider match arms
- `src/session/runner.rs` — removed `#[allow(dead_code)]` on `ui` field, added `truncate_summary()` utility function, added 5 unit tests for `truncate_summary`, emitted UI events throughout `run_session()` (activation lifecycle, LLM request/response/error, chat turn, completion detected, phase start/complete/error for Final Commit/Impact Analysis/PR Summary, retry/token refresh), emitted UI events in `drive_activation_and_recover()` (activation lifecycle, LLM request/response/error for CH and context load), emitted UI events in all recovery paths (empty history, last-user re-send), passed `Some(&self.ui)` to all 15 `stream_chat()`/`activate_agent()` call sites
- `src/supervisor/architect.rs` — passed `None` for `ui` in 3 `stream_chat()` + 1 `activate_agent()` calls
- `src/review/mod.rs` — passed `None` for `ui` in 2 `stream_chat()` + 1 `activate_agent()` calls
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — updated 10-3 status to in-progress