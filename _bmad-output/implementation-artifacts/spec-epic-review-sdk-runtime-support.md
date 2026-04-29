---
title: 'SDK runtime support for EpicReviewRunner and ArchitectSession'
type: 'feature'
created: '2026-04-29'
status: 'done'
baseline_commit: '73bc035'
context:
  - '_bmad-output/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `EpicReviewRunner` and `ArchitectSession` (supervisor LLM fallback) only work via API (`AgentFactory.build()`). When their respective LLM roles (`epic_review`, `supervisor`) are configured with SDK providers (claude-code, codex), they fail at runtime. The warning at `runtime/mod.rs:218` confirms `EpicReviewRunner` is API-only. `ArchitectSession` has the same gap.

**Approach:** Make both modules dispatch to SDK subprocess when their role config uses an SDK provider, following the existing dual-runtime pattern. `EpicReviewRunner` gets a direct SDK session (no supervisor MCP — read-only). `ArchitectSession` gets an `SdkAnswerProvider` implementing the existing `AnswerProvider` trait, spawning a single-turn SDK subprocess per question (no supervisor MCP — prevents recursion).

## Boundaries & Constraints

**Always:**
- Existing API path must remain unchanged — SDK is additive
- Epic review SDK session must NOT receive supervisor MCP config (read-only, no ask_supervisor)
- Architect SDK session must NOT receive supervisor MCP config (recursion prevention)
- Both must respect shutdown flag and timeout

**Ask First:**
- If the SDK prompt for epic review exceeds reasonable size (the current prompt is ~4K chars with dynamic sections)

**Never:**
- Don't change `SessionRuntime` enum — these are standalone runners, not pipeline-phase sessions
- Don't add SDK support to `SpawnAgentTool` (API-only by design)

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Epic review with SDK provider | `epic_review.provider = "claude-code"` | SDK subprocess runs, report extracted from completion_text | Failed outcome with error message |
| Epic review with API provider | `epic_review.provider = "anthropic"` | Existing AgentFactory path (unchanged) | Existing error handling |
| Epic review SDK empty completion | SDK returns exit 0, empty completion_text | `EpicReviewOutcome::Failed` with descriptive reason | N/A |
| Supervisor fallback with SDK | `supervisor.provider = "claude-code"` | `SdkAnswerProvider.ask()` spawns subprocess, returns answer | `ArchitectSessionError` on failure |
| Supervisor fallback with API | `supervisor.provider = "anthropic"` | Existing ArchitectSession path (unchanged) | Existing error handling |
| SDK subprocess spawn failure | CLI binary not found | Error propagated as Failed/ArchitectSessionError | N/A |

</frozen-after-approval>

## Code Map

- `src/review/epic.rs` -- EpicReviewRunner: add SDK dispatch in `run_inner()`
- `src/supervisor/architect.rs` -- AnswerProvider trait, ArchitectSession: add `SdkAnswerProvider`
- `src/supervisor/mod.rs` -- `with_architect_from_config()`: choose provider based on config
- `src/mcp_server/mod.rs` -- MCP server construction: choose provider based on config
- `src/runtime/mod.rs` -- Remove epic review SDK warning (line 218-219)
- `src/runtime/sdk_claude.rs` -- Reuse `build_claude_code_config()`, `map_sdk_result_to_outcome()` patterns
- `src/runtime/sdk_codex.rs` -- Reuse codex session config patterns
- `src/pipeline.rs` -- Pass `SdkRuntime` or config/secrets to `EpicReviewRunner`
- `src/config/mod.rs` -- `LlmRoleConfig::is_sdk_provider()` already exists

## Tasks & Acceptance

**Execution:**
- [x] `src/supervisor/architect.rs` -- Add `SdkAnswerProvider` struct implementing `AnswerProvider`. It takes `Arc<BotConfig>`, `Arc<BotSecrets>`, `PathBuf` (config_path), `ShutdownFlag`, `UiHandle`. The `ask()` method builds a combined prompt (architect persona + English override + question + context), spawns a SDK subprocess via `SdkRuntime::execute_session()`, and returns the completion text. No supervisor MCP injected.
- [x] `src/supervisor/architect.rs` -- Add `build_answer_provider()` factory function that checks `resolve_role_config(supervisor)` — if SDK provider, return `SdkAnswerProvider`; if API, return existing `ArchitectSession`.
- [x] `src/supervisor/mod.rs` -- Update `with_architect_from_config()` to use `build_answer_provider()` instead of always creating `ArchitectSession`.
- [x] `src/mcp_server/mod.rs` -- Update MCP server construction (in `cli/mod.rs` or wherever `serve_stdio` caller builds the server) to use `build_answer_provider()`.
- [x] `src/review/epic.rs` -- Add SDK dependencies to `EpicReviewRunner`: `Arc<BotSecrets>`, `PathBuf` (config_path), `ShutdownFlag`, `UiHandle` are already fields. Add method `run_sdk_epic_review()` that builds the epic review prompt, spawns SDK subprocess (no MCP), extracts report from completion_text.
- [x] `src/review/epic.rs` -- In `run_inner()`, check `resolve_role_config(EpicReview).is_sdk_provider()` — if true, call `run_sdk_epic_review()`; otherwise existing API path.
- [x] `src/pipeline.rs` -- Pass additional dependencies to `EpicReviewRunner::new()` if needed (config_path, secrets for SDK).
- [x] `src/runtime/mod.rs` -- Remove the warning at lines 218-219 about epic review not supporting SDK.
- [x] `src/review/epic.rs` + `src/supervisor/architect.rs` -- Add unit tests for SDK dispatch logic (mock subprocess or test config detection).

**Acceptance Criteria:**
- Given `epic_review.provider = "claude-code"`, when an epic completes, then EpicReviewRunner spawns a claude-code subprocess and extracts the report from completion_text
- Given `supervisor.provider = "claude-code"`, when the supervisor LLM fallback triggers, then `SdkAnswerProvider` spawns a subprocess and returns the answer
- Given `epic_review.provider = "anthropic"`, when an epic completes, then existing API path is used (no regression)
- Given `supervisor.provider = "anthropic"`, when the supervisor fallback triggers, then existing ArchitectSession is used (no regression)
- Given SDK subprocess fails, when either module handles the error, then appropriate error type is returned (no panic)

## Design Notes

**SdkAnswerProvider prompt strategy:** Combine the architect persona, English override, and developer question into a single prompt. Unlike API mode which uses multi-turn activation (activate → CH → load context → question), SDK mode sends everything in one shot — the SDK agent (Claude Code/Codex) already has native tool access and doesn't need BMAD activation.

**EpicReviewRunner SDK prompt:** The existing `build_epic_review_prompt()` output is self-contained and suitable as-is for SDK. The report delimiters (`<<EPIC_REVIEW_REPORT_START/END>>`) work with SDK completion_text extraction. Include the preamble (Winston persona + constraints) as a `--append-system-prompt` or prepend to the prompt.

**SdkRuntime reuse:** Both modules need to spawn SDK subprocesses. Rather than taking `SdkRuntime` as a dependency, create a lightweight `sdk_oneshot()` helper (or reuse `SdkRuntime::execute_session()` by constructing a temporary `SdkRuntime`) that takes config, secrets, shutdown, and session config. This avoids coupling these standalone modules to the full runtime lifecycle.

## Verification

**Commands:**
- `cargo build` -- expected: clean compilation, no errors
- `cargo test` -- expected: all existing + new tests pass
- `cargo clippy` -- expected: no warnings

## Suggested Review Order

**SDK AnswerProvider (supervisor fallback)**

- Core abstraction: `SdkAnswerProvider` struct + `AnswerProvider` impl + `build_answer_provider` factory
  [`architect.rs:440`](../../src/supervisor/architect.rs#L440)

- Updated `with_architect_from_config()` dispatches via factory instead of hardcoding `ArchitectSession`
  [`mod.rs:201`](../../src/supervisor/mod.rs#L201)

- MCP server (`cli/mod.rs`) now uses factory — previously skipped SDK providers entirely
  [`mod.rs:1410`](../../src/cli/mod.rs#L1410)

**SDK Epic Review**

- `run_sdk_epic_review()` — spawns SDK subprocess with combined preamble+prompt, no MCP
  [`epic.rs:523`](../../src/review/epic.rs#L523)

- `run_inner()` dispatch — checks `is_sdk_provider()` before choosing path
  [`epic.rs:481`](../../src/review/epic.rs#L481)

- `config_path` added to `EpicReviewRunner` struct + constructor
  [`epic.rs:337`](../../src/review/epic.rs#L337)

**Pipeline & runtime wiring**

- Pipeline clones `config_path` before RuntimeDeps consumes it, passes to EpicReviewRunner
  [`pipeline.rs:253`](../../src/pipeline.rs#L253)

- SDK warning removed from `SessionRuntime::from_config`
  [`mod.rs:218`](../../src/runtime/mod.rs#L218)

- `AgentFactory::config_arc()` / `secrets_arc()` accessors added
  [`agent_factory.rs:362`](../../src/llm/agent_factory.rs#L362)

**Callers updated for new `shutdown`/`ui` params**

- `create_tools_with_supervisor()` signature extended
  [`agent.rs:95`](../../src/session/agent.rs#L95)

- `SessionRunner::build_agent_for_role()` passes shutdown+ui
  [`runner.rs:906`](../../src/session/runner.rs#L906)

- `ReviewRunner::build_review_agent()` passes shutdown+ui
  [`mod.rs:475`](../../src/review/mod.rs#L475)

**Tests**

- `SdkAnswerProvider` + `build_answer_provider` tests (prompt building, trait impl, factory dispatch)
  [`architect.rs:828`](../../src/supervisor/architect.rs#L828)

- `EpicReviewRunner` SDK dispatch tests (config detection, constructor)
  [`epic.rs:1963`](../../src/review/epic.rs#L1963)
