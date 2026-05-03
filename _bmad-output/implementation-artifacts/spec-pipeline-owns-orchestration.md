---
title: 'Pipeline owns all orchestration — runtimes become dumb session executors'
type: 'refactor'
created: '2026-05-02'
status: 'in-progress'
baseline_commit: '539e0a0'
context:
  - '_bmad-output/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Business logic (consultation orchestration, auto-response to interactive prompts, outcome interpretation, escalation detection) is duplicated across runtime implementations (`sdk_claude.rs`, `sdk_codex.rs`, `session/runner.rs`, `sdk_consultation.rs`). Each runtime makes its own decisions about what to do next. This causes:
- Bugs (e.g., trigger pattern matching re-fires adversarial 3x because story file always contains `status: ready-for-dev`)
- Divergent behavior between SDK and API paths
- Consultation logic that can't mix runtimes (e.g., create-story on claude-code, adversarial on anthropic API)
- Duplicated BMAD-specific knowledge in provider-specific code

**Approach:** Strict separation — the pipeline is the brain, runtimes are dumb executors.
- **Runtime contract:** `execute(command) → RawSessionResult`. That's it. No interpretation, no auto-response, no consultation awareness. A runtime launches a session (fresh or resumed), returns raw output when the session reaches a natural stop point.
- **Pipeline owns:** session sequencing, auto-response loop, consultation orchestration (linear, deterministic), outcome interpretation, escalation detection, branch management.
- **Consultations become linear pipeline steps:** run_session → consult(adversarial) → resume(findings) → consult(critic) → resume(findings) → done. No trigger patterns. The pipeline knows the sequence statically.
- **Auto-response moves to pipeline:** when a session returns with an interactive prompt, the pipeline decides the response and calls the runtime again. The runtime doesn't even know it's an auto-resume vs a normal run.

## Boundaries & Constraints

**Always:**
- One `SessionRuntime::execute()` method that works identically for SDK and API — both return `RawSessionResult`
- BMAD-specific auto-response rules (`is_checkpoint_prompt`, `is_confirmation_prompt`, patch handling, etc.) live in a pipeline module, not in provider code
- Consultation sequence is hardcoded per pipeline phase (create = adversarial → critic, review = review-critic), not pattern-matched
- Pipeline can mix providers per step: create-story on claude-code, adversarial on anthropic API, critic on openai
- Branch management (`ensure_branch`) moves to pipeline — it's orchestration, not execution
- WAL crash-recovery infrastructure stays in runtimes (it's session-level persistence, not orchestration) but pipeline phase tracking is pipeline-owned

**Ask First:**
- Whether to keep `ConsultationRunner` or make consultations just another `execute(Start)` call

**Never:**
- Provider-specific business logic in `sdk_claude.rs` or `sdk_codex.rs` — only session config building and output line parsing
- Runtime returning `SessionOutcome` — that's pipeline's interpretation layer
- Trigger regex patterns — the pipeline knows the fixed sequence
- Runtime deciding what to do with a result (retry, escalate, next step)

</frozen-after-approval>

## Code Map

### New files
- `src/pipeline/mod.rs` — submodule root, re-exports
- `src/pipeline/auto_response.rs` — `auto_response_for_prompt`, `is_checkpoint_prompt`, `is_confirmation_prompt`, `is_numeric_choice_prompt` (moved from `sdk_claude.rs`)
- `src/pipeline/outcome.rs` — `interpret_result`, `detect_escalation`, `read_decisions_json_sidecar` (moved from `sdk_claude.rs`)
- `src/pipeline/consultation.rs` — linear consultation orchestration (replaces `sdk_consultation.rs` and `check_consultation_triggers`)

### Modified files
- `src/runtime/mod.rs` — new `RawSessionResult` struct, `RuntimeCommand` enum, `SessionRuntime::execute()` replaces `run_session()`; remove `SessionContext.consultations`; move `ensure_branch` to pipeline
- `src/runtime/sdk_claude.rs` — strip to: config building (`build_claude_code_config`, `build_claude_code_resume_config`), line parser (`parse_claude_code_line`), MCP temp file handling. Remove: auto-confirm loop, `auto_response_for_prompt`, `map_sdk_result_to_outcome`, `detect_escalation`, consultation block
- `src/runtime/sdk_codex.rs` — strip to: config building, line parser, MCP config management. Remove: auto-confirm loop, consultation block
- `src/runtime/sdk.rs` — `execute_session` stays (low-level subprocess runner), `run_session` and `resume_sdk_session` replaced by unified `execute(RuntimeCommand)`
- `src/session/runner.rs` — remove `check_consultation_triggers`, remove consultation states from chat loop, `run_with_consultations` becomes `run` (no consultation param). Returns `RawSessionResult`. Keep internal session handle alive for Resume support.
- `src/session/consultation.rs` — remove `ConsultationState` and trigger infrastructure. `ConsultationRunner` removed (consultations are now just `execute(Start)` calls).
- `src/pipeline.rs` — move `ensure_branch` here, rewrite `run_create_pipeline`, `run_dev_pipeline`, `run_review_pipeline` to use new linear flow

### Deleted files
- `src/runtime/sdk_consultation.rs` — entirely replaced by `src/pipeline/consultation.rs`

## Design Decisions

### D1: How Resume works across runtimes

The pipeline sees `session_id: Option<String>` as an opaque handle. Each runtime interprets it differently:

- **SDK (claude-code, codex):** `session_id` is the CLI session ID string. Resume spawns a new subprocess with `--resume <id>`. Stateless between calls — the CLI manages its own history persistence.
- **API (rig):** `session_id` is a key into the runtime's internal `SessionHandle` map. The rig runtime keeps the agent instance + full message history alive in memory between `execute()` calls. Resume sends the new message into the existing chat and returns when the agent hits the sentinel again or completes.

This means the rig runtime's `execute(Resume)` doesn't rebuild the agent — it reuses the live instance. The pipeline doesn't need to know this.

```rust
// Internal to ApiRuntime only
struct SessionHandle {
    agent: BuiltAgent,
    history: Vec<Message>,
    story_key: String,
}
```

SessionHandles are cleaned up after the pipeline signals it's done with a session (via a `RuntimeCommand::Close { session_id }` command or simply after the pipeline's orchestration for that story ends — TBD whether explicit close is needed or Drop-on-pipeline-end suffices).

### D2: Sentinel detection stays in runtimes (it's "when done", not "what to do")

Each runtime knows how to detect that its session reached a natural stop:
- **SDK:** exit_code from the subprocess (0 = done or waiting-for-input, non-0 = error)
- **API (rig):** detects `<<BMAD_JOB_DONE>>` or `<<ESCALATION>>` sentinel in assistant response, stops the chat loop

This is runtime-specific "how to know the session stopped" — not business logic. The runtime doesn't decide what to DO about it. It just returns `RawSessionResult` with the facts.

### D3: Distinguishing "done" from "waiting for input" (SDK)

`RawSessionResult` does NOT need a `waiting_for_input` flag. The pipeline's `auto_response_for_prompt(completion_text)` handles this: if it returns `Some(response)`, the pipeline resumes; if `None`, the session is truly done. The same `exit_code: Some(0)` is used for both — the pipeline disambiguates via the completion text content.

### D4: MCP supervisor lifecycle stays in runtime (session infrastructure)

MCP config creation is provider-specific session setup, not orchestration:
- **Claude-code:** writes a temp file, passes `--mcp-config` to the subprocess
- **Codex:** writes `.codex/config.toml` and restores on exit
- **API (rig):** no MCP needed (supervisor is a rig tool)

The pipeline controls WHEN to set up supervisor via `RuntimeCommand::Start { needs_supervisor: bool }`. The runtime handles HOW. This keeps provider-specific mechanics out of the pipeline.

### D5: Branch management moves to pipeline

`ensure_branch` is orchestration: the pipeline decides when to branch, what base to use, and handles failures. It moves from `runtime/mod.rs` to `pipeline.rs` (called before the first `execute(Start)` for a story).

### D6: WAL ownership — split responsibility

- **Runtime WAL** (stays): session-level crash recovery data (session_id for SDK resume, chat history for rig replay). The runtime manages its own WAL file lifecycle.
- **Pipeline phase** (moves): which orchestration step we're in (create → adversarial → critic → dev → review). The pipeline tracks this in its own state (can be added to the existing WAL via a pipeline_phase field that the pipeline writes, or in a separate pipeline state file).

On crash recovery: the pipeline reads the WAL to find what session was in progress, then resumes from the last known orchestration step. Old pre-refactor WAL files are incompatible — the daemon clears stale WAL on startup (existing behavior via `check_and_recover_wal`).

### D7: ConsultationRunner removed — consultations are just sessions

Post-refactor, a consultation is just another `execute(Start)` call with a different role, preamble, and prompt. There's no need for a separate `ConsultationRunner` class. The pipeline builds the prompt (with context files), picks the role, and calls `execute()`. The rig or SDK runtime handles it like any other session.

### D8: NoReply behavior in rig runner (no consultation states)

Without consultation states, the rig runner's `NoReply` response action always sends "Continue." to the agent. This is correct: `NoReply` means the agent produced output that isn't a sentinel or explicit action — it should keep going. The chat loop only exits on sentinel detection (`<<BMAD_JOB_DONE>>`, `<<ESCALATION>>`), failure, or max turns.

### D9: RuntimeCommand carries enough info for both runtimes

```rust
pub enum RuntimeCommand {
    Start {
        role: LlmRole,
        phase: String,              // "create", "dev", "review", "consultation"
        story_key: String,
        prompt: String,             // initial message or skill activation prompt
        skill_path: Option<String>, // BMAD skill to activate (rig: load skill, SDK: /skill command)
        preamble: Option<String>,   // system prompt override (rig: agent preamble, SDK: prepended to prompt)
        needs_supervisor: bool,     // whether to set up MCP supervisor (runtime handles HOW)
    },
    Resume {
        session_id: String,         // opaque handle (SDK: CLI session ID, rig: internal handle key)
        prompt: String,             // message to send on resume
        role: LlmRole,             // needed for SDK to resolve provider config
        story_key: String,         // for logging/WAL
    },
}
```

For consultations: `Start` with `role: LlmRole::Review` (or `Critic`), `skill_path: None`, `preamble: Some(adversarial_preamble)`, `needs_supervisor: false`.

## Tasks & Acceptance

### Phase A — Extract shared logic (no behavior change)

- [ ] Create `src/pipeline/mod.rs` as submodule with `pub mod auto_response; pub mod outcome; pub mod consultation;`
- [ ] `src/pipeline/auto_response.rs` — Move `auto_response_for_prompt`, `is_checkpoint_prompt`, `is_numeric_choice_prompt`, `is_confirmation_prompt` from `sdk_claude.rs`. Keep originals as thin `pub(crate)` wrappers calling the new location. Move tests.
- [ ] `src/pipeline/outcome.rs` — Move `map_sdk_result_to_outcome`, `detect_escalation`, `read_decisions_json_sidecar` from `sdk_claude.rs`. Keep originals as thin wrappers. Move tests.
- [ ] Verify: `cargo test` passes, `cargo clippy` no new errors (pre-existing clippy errors are acceptable).

**AC:** All logic physically in new modules. Old call sites delegate. Zero behavior change.

### Phase B — Introduce RawSessionResult and execute()

- [ ] `src/runtime/mod.rs` — Define `RawSessionResult` (reuse `SdkSessionResult` fields: `exit_code`, `session_id`, `completion_text`, `stderr`, `stream_error`, `shutdown_requested`, `timed_out`, `rate_limit_resets_at`). Can be a type alias or thin wrapper around `SdkSessionResult` since fields are identical.
- [ ] `src/runtime/mod.rs` — Define `RuntimeCommand` enum (see D9 above).
- [ ] `src/runtime/mod.rs` — Add `SessionRuntime::execute(&self, command: RuntimeCommand) -> RawSessionResult` alongside existing `run_session` (parallel path).
- [ ] `src/runtime/sdk_claude.rs` — Extract `execute_claude_start(runtime, command) -> RawSessionResult` and `execute_claude_resume(runtime, command) -> RawSessionResult` that ONLY spawn subprocess and return raw result. No auto-confirm, no consultation, no outcome mapping. MCP temp file created/cleaned internally based on `needs_supervisor`.
- [ ] `src/runtime/sdk_codex.rs` — Same: `execute_codex_start` and `execute_codex_resume`.
- [ ] `src/session/runner.rs` — Add `SessionHandle` struct (agent + history + story_key). Add `SessionRunner::execute(&self, command: RuntimeCommand) -> RawSessionResult`:
  - `Start`: build agent, run chat loop to sentinel, store handle in internal map, return `RawSessionResult` (completion_text = last assistant message, exit_code 0 on BMAD_JOB_DONE, 1 on ESCALATION, session_id = generated handle key).
  - `Resume`: look up handle by session_id, send prompt, continue chat loop to next sentinel, return new result.
- [ ] `src/runtime/sdk.rs` — Wire `SessionRuntime::execute()` to dispatch `Start`/`Resume` to correct provider's execute function.
- [ ] Move `ensure_branch` from `SessionRuntime` to `pipeline.rs` as a standalone `ensure_story_branch()` call.
- [ ] Verify: new `execute()` path works for a simple start+return (unit test with mocked subprocess).

**AC:** `SessionRuntime::execute()` exists and works for both Start and Resume on both runtime types. Old `run_session()` still works in parallel. Both paths coexist. `ensure_branch` called from pipeline.

### Phase C — Pipeline takes control

- [ ] `src/pipeline/consultation.rs` — Implement `run_consultation_sequence(&self, runtime, story, phase, session_id, consultations) -> (RawSessionResult, String)` as a linear loop:
  1. For each consultation config in order:
     - Call `execute(Start { role, preamble, prompt_with_context, ... })` for the consultation
     - Auto-response-loop the consultation result (consultations can also have interactive prompts)
     - Extract findings from completion_text
     - Call `execute(Resume { session_id, findings_formatted })` on the main session
     - Auto-response-loop the resume result
  2. Return final result + last session_id
- [ ] `src/pipeline.rs` — Implement `auto_response_loop(&self, raw, role, story_key) -> RawSessionResult`:
  - While `auto_response_for_prompt(raw.completion_text)` returns `Some(response)` and attempts < 15:
    - Call `execute(Resume { session_id, prompt: response })`
    - Update raw
  - Return final raw
- [ ] `src/pipeline.rs` — Implement `interpret_result(&self, raw, story) -> SessionOutcome`:
  - Read decisions sidecar
  - Check rate limit
  - Check escalation
  - Map exit_code to Completed/Failed
- [ ] `src/pipeline.rs` — Move `ensure_branch` call before session execution (already done in Phase B, wire it into pipeline flow)
- [ ] `src/pipeline.rs` — Rewrite `run_create_pipeline`:
  1. `ensure_branch()`
  2. `execute(Start { phase: "create", skill: create_story, needs_supervisor: true })`
  3. `auto_response_loop()`
  4. `interpret_result()` → if not Completed, return early
  5. `run_consultation_sequence([adversarial, critic])`
  6. `interpret_result()` → final outcome
- [ ] `src/pipeline.rs` — Rewrite `run_dev_pipeline` session phase with same pattern (no consultations, just execute → auto_response → interpret)
- [ ] `src/pipeline.rs` — Rewrite `run_review_pipeline`:
  1. `execute(Start { phase: "review", skill: code_review, needs_supervisor: false })`
  2. `auto_response_loop()`
  3. `interpret_result()` → if not Completed, return early
  4. `run_consultation_sequence([review-critic])` (only if review has decision-needed findings — pipeline checks completion_text)
  5. `interpret_result()` → final outcome
- [ ] Remove `consultations` field from `SessionContext`
- [ ] Remove old `SessionRuntime::run_session()` — all callers now use `execute()`
- [ ] Remove auto-confirm loop from `sdk_claude.rs` (lines 480-519) and `sdk_codex.rs` (lines 606-644)
- [ ] Remove consultation block from `sdk_claude.rs` (lines 526-553) and `sdk_codex.rs` (lines 655-682)
- [ ] Remove `map_sdk_result_to_outcome` wrapper from `sdk_claude.rs`
- [ ] `src/session/runner.rs` — Remove `run_with_consultations`, `check_consultation_triggers`, consultation states from chat loop. Public API is now `execute(RuntimeCommand)` only.
- [ ] Remove `ConsultationState` from `src/session/consultation.rs`
- [ ] Remove `ConsultationRunner` from `src/session/consultation.rs` (consultations are just `execute(Start)` calls now)
- [ ] Verify: full `cargo test`, run daemon manually on a test story to confirm linear flow works

**AC:** Pipeline orchestrates everything. Runtimes only execute. Auto-response and consultations work identically regardless of provider. No trigger-pattern matching anywhere. Consultations can use a different provider than the main session.

### Phase D — Cleanup

- [ ] Delete `src/runtime/sdk_consultation.rs` entirely
- [ ] Delete thin wrapper functions left in `sdk_claude.rs` from Phase A
- [ ] Remove `pub mod sdk_consultation` from `src/runtime/mod.rs`
- [ ] Remove `ApiRuntime` and `SdkRuntime::run_session()` — replaced by unified `execute()` dispatch
- [ ] Remove `SessionContext` struct (replaced by `RuntimeCommand`)
- [ ] Clean up `src/session/consultation.rs` — keep only `ConsultationConfig` (definition of what a consultation is: role, preamble, prompt_template, context_files, resume_message_template)
- [ ] Update `src/runtime/mod.rs` tests
- [ ] Remove reverse-iteration fix in `sdk_consultation.rs` (file deleted anyway)
- [ ] `cargo test`, `cargo clippy`, `cargo fmt`

**AC:** No dead code. Single code path. Runtime files contain only config building, line parsing, and subprocess/chat-loop execution.

### Review Findings

_Code review 2026-05-03 — 3 layers (blind adversarial, edge-case hunter, acceptance auditor)_

- [x] [Review][Patch] Consultation error handling: check exit_code before extracting findings — abort consultation sequence on exit_code != 0 (fail-fast). Resolved: option 1. **FIXED**
- [x] [Review][Patch] run_review_pipeline: add completion_text pattern check before launching review-critic consultation. Resolved: option 1. **FIXED**
- [ ] [Review][Patch] Remove legacy run_session path — delete SessionRuntime::run_session(), run_claude_code_session, run_codex_session and associated dead code. Accelerate Phase C/D. Resolved: option 1. **SKIPPED — requires significant refactoring beyond batch-apply**
- [x] [Review][Patch] UTF-8 byte-index slicing panic — `text[text.len().saturating_sub(N)..]` can panic on multi-byte chars [auto_response.rs:35,74,93,113] **FIXED**
- [x] [Review][Patch] execute_claude_resume: unconditional MCP config with empty story_key [sdk_claude.rs:426-427] **FIXED**
- [x] [Review][Patch] execute_codex_resume: missing MCP config setup via new execute() path [sdk_codex.rs:539-574] **FIXED**
- [x] [Review][Patch] Preamble applied twice in run_single_consultation — prepended to prompt AND in RuntimeCommand::Start.preamble [pipeline/consultation.rs:22-26] **FIXED**
- [x] [Review][Patch] Missing termination checks in auto_response_loop and run_consultation_sequence — no timed_out/shutdown_requested/is_shutdown() guard [pipeline/mod.rs] **FIXED**
- [x] [Review][Patch] map_sdk_result_to_outcome: timed_out + exit_code=0 treated as Completed [pipeline/outcome.rs] **FIXED**
- [x] [Review][Patch] RuntimeCommand fields ignored in execute_claude_start — _story_key misleading underscore fixed, phase used in tracing [sdk_claude.rs] **FIXED**
- [x] [Review][Patch] outcome_to_pipeline_result catch-all `_ =>` loses SessionOutcome data, returns story_key: "unknown" [pipeline/mod.rs] **FIXED**
- [x] [Review][Patch] is_checkpoint_prompt lowercase precondition undocumented — public fn now lowercases internally [auto_response.rs] **FIXED**
- [x] [Review][Defer] ConsultationConfig.trigger_pattern field still present — deferred, Phase D cleanup
- [x] [Review][Defer] No tests for auto_response_loop — deferred, requires runtime mocking infrastructure
- [x] [Review][Defer] API/rig execute() not implemented — deferred, Phase B task secondary to SDK path
- [x] [Review][Defer] Legacy code not fully removed (SessionContext, run_session, thin wrappers) — deferred, Phase C/D tasks
- [x] [Review][Defer] ConsultationRunner still used by run_epic_critic_review — deferred, Phase D migration
- [x] [Review][Defer] Test duplication sdk_claude.rs ↔ pipeline modules — deferred, Phase D cleanup
- [x] [Review][Defer] session/runner.rs and session/consultation.rs not cleaned up — deferred, Phase C tasks

## Design Notes

### Why linear consultation instead of trigger patterns
Trigger patterns were designed for the rig runner where consultations happen mid-chat-loop. But: (a) the SDK runtime is subprocess-based so consultations are always post-session anyway, (b) the rig runtime can also do post-session consultations (run to sentinel, return, then consult), (c) trigger patterns on file content cause false positives (story always has `status: ready-for-dev`), (d) the sequence is always deterministic per phase. Linear is simpler, correct, and allows mixing providers.

### Auto-response as pipeline logic
The auto-response loop handles BMAD-specific interactive prompts (checkpoints, confirmations, patch choices). This is workflow knowledge, not runtime knowledge. A different workflow would have different auto-response rules. By putting it in the pipeline, the runtime becomes truly provider-agnostic.

### RawSessionResult vs SessionOutcome
`RawSessionResult` is what the runtime produces — facts about what happened (exit code, text, errors). `SessionOutcome` is what the pipeline interprets — meaning (completed, failed, escalated). This separation means outcome interpretation rules live in one place (`pipeline/outcome.rs`) regardless of runtime.

### Rig SessionHandle lifecycle
The rig runtime holds live agent instances in a `HashMap<String, SessionHandle>`. Handles are created on `Start`, reused on `Resume`, and dropped when the pipeline's story orchestration ends. No explicit `Close` command needed — the pipeline holds a borrow on the runtime for the duration of a story, and handles are keyed by story_key so there's at most one per story.

### WAL compatibility on upgrade
Pre-refactor WAL files store consultation state and pipeline phase in formats incompatible with the new design. The daemon already clears stale WAL on startup via `check_and_recover_wal` — on version mismatch (missing expected fields), it treats the WAL as corrupt and starts fresh. No migration needed.

## Spec Change Log

- 2026-05-02: Initial draft based on live bug analysis (adversarial consultation re-triggering 3x)
- 2026-05-02: Fixed all adversarial findings — added D1-D9 design decisions addressing: Resume for rig (SessionHandle), sentinel ownership, done-vs-prompt disambiguation, MCP lifecycle, branch management, WAL split, ConsultationRunner removal, NoReply behavior, RuntimeCommand completeness
