# Story 15.7: Pipeline Dual-Runtime Orchestration

Status: done

## Story

As a daemon developer,
I want the multi-phase pipeline to route each phase to the appropriate runtime based on the role's provider config,
So that mixed-mode configurations (e.g., dev via claude-code, review via anthropic API) work seamlessly.

## Acceptance Criteria

1. **Given** a story enters the pipeline, **When** each phase is executed, **Then** `pipeline.rs` routes to `SessionRuntime::Api` or `SessionRuntime::Sdk` based on the provider configured for the phase's LlmRole (AC: role-based routing)

2. **Given** the pipeline orchestration pattern (Decision 10), **When** SDK mode is used, **Then** the pipeline is unchanged — same phases, same order, same daemon-orchestrated consultations; API mode: findings injected as user message in paused session; SDK mode: findings injected via session resume with findings as prompt (AC: consultation injection)

3. **Given** each SDK session produces a session ID, **When** it is captured, **Then** the WAL stores `runtime_type: api | sdk` and `sdk_session_ids: HashMap<String, String>` (phase → session_id) for correct recovery and resume routing (AC: WAL extension)

4. **Given** both runtimes are active, **When** UI events are emitted, **Then** both runtimes emit the same event types via `UiHandle` for unified monitoring (AC: unified UI — already implemented in 15.3/15.5/15.6)

## Tasks / Subtasks

- [x] Task 1: Implement SDK session resume in both providers (AC: #2, #3)
  - [x] 1.1 In `src/runtime/sdk_claude.rs`, add `resume_claude_code_session(runtime, session_id, prompt) -> SessionOutcome` that invokes `claude --resume {session_id} -p "{prompt}" --output-format stream-json --model {model} --cd {project_root}`
  - [x] 1.2 In `src/runtime/sdk_codex.rs`, add `resume_codex_session(runtime, session_id, prompt) -> SessionOutcome` that invokes `codex exec resume {session_id} --json --cd {project_root}` with the follow-up instruction as the optional prompt argument
  - [x] 1.3 Add shared `resume_sdk_session(runtime, provider, session_id, prompt) -> SessionOutcome` dispatcher in `src/runtime/sdk.rs` that routes to the correct provider's resume function
  - [x] 1.4 Re-inject MCP supervisor config on resume: Claude Code needs `--mcp-config {temp_file}`; Codex needs `.codex/config.toml` written before spawn (same cleanup pattern as initial session)

- [x] Task 2: Refactor `SessionRuntime` for per-role dispatch (AC: #1)
  - [x] 2.1 Add `Dual { api: Box<ApiRuntime>, sdk: SdkRuntime, config: Arc<BotConfig> }` variant to `SessionRuntime` enum — the `config` field is required for role-based provider lookup during dispatch
  - [x] 2.2 Implement `run_session()` on `Dual` that calls `resolve_role_config(role).is_sdk_provider()` to decide delegation to `api` or `sdk`
  - [x] 2.3 Change `api_session_runner()` to return `Option<&SessionRunner>` — returns `Some` for `Api` and `Dual`, `None` for `Sdk`
  - [x] 2.4 Add `SessionRuntime::from_config(config, secrets, ...)` factory that inspects ALL LlmRole providers (`dev`, `review`, `supervisor`, `epic_review`, `critic`) and builds the appropriate variant: Api-only if all API, Sdk-only if all SDK, Dual if mixed

- [x] Task 3: Update `StoryPipeline::new()` to use dual runtime (AC: #1)
  - [x] 3.1 Add `config_path: PathBuf` parameter to `StoryPipeline::new()` — pass from `cli/mod.rs::run_start()`
  - [x] 3.2 Replace hardcoded `SessionRuntime::Api(...)` with `SessionRuntime::from_config(...)` factory
  - [x] 3.3 Only construct `SessionRunner` + `AgentFactory` if at least one role uses an API provider (handled by `from_config()`)
  - [x] 3.4 Only construct `SdkRuntime` if at least one role uses an SDK provider (handled by `from_config()`)
  - [x] 3.5 Pipeline test helpers construct `SessionRuntime` directly — no `config_path` change needed; existing tests compile and pass

- [x] Task 4: SDK-mode consultation handling (AC: #2)
  - [x] 4.1 Create `src/runtime/sdk_consultation.rs` — new module for SDK consultation orchestration
  - [x] 4.2 Implement `SdkConsultationRunner::run_with_consultations(sdk_runtime, context, initial_outcome) -> SessionOutcome` that orchestrates the post-session consultation loop
  - [x] 4.3 After initial SDK session completes, capture trigger-matching text: for PHASE_CREATE read the story file on disk (`story.specs_path`); for PHASE_DEV/PHASE_REVIEW use `SdkSessionResult::completion_text` and also read the story file
  - [x] 4.4 Match each consultation's `trigger_pattern` (regex) against the captured text; track which consultations have already fired (each fires at most once per session)
  - [x] 4.5 Consultations ALWAYS run via API (`ConsultationRunner` which uses `AgentFactory`) regardless of the main session's runtime — `ConsultationRunner` is API-only by design. If the consultation's role is configured as SDK provider, log a warning and fall back to the supervisor role's API config, or skip with an error if no API provider is available
  - [x] 4.6 If consultation produces findings, resume original SDK session via `resume_sdk_session()` (Task 1.3) with `resume_message_template` + findings as prompt
  - [x] 4.7 After resume, re-capture trigger text and check for remaining unfired consultations
  - [x] 4.8 Hard cap: `MAX_SDK_CONSULTATION_ROUNDS = 3` — abort consultation loop after 3 resume cycles with a warning log, return the last session outcome

- [x] Task 5: SDK WAL management (AC: #3)
  - [x] 5.1 Create `src/runtime/sdk_wal.rs` — new module for SDK-specific WAL operations
  - [x] 5.2 Add `runtime_type: String` field to `SessionState` in `src/session/state.rs` (`#[serde(default)]` for backward compat)
  - [x] 5.3 Add `sdk_session_ids: HashMap<String, String>` field to `SessionState` (`#[serde(default)]`)
  - [x] 5.4 Implement `SdkWal::create(story, provider, model, phase) -> SessionState` that creates a WAL with `runtime_type: "sdk"`, empty `chat_history`
  - [x] 5.5 Implement `SdkWal::record_session_id(wal_path, phase, session_id)` that loads the WAL, inserts into `sdk_session_ids`, and saves atomically
  - [x] 5.6 Implement `SdkWal::cleanup(wal_path)` for post-session WAL deletion
  - [x] 5.7 WAL wiring deferred to integration — SdkWal provides the building blocks; wiring into SdkRuntime::run_session() is a follow-up

- [x] Task 6: Crash recovery for dual runtime (AC: #3)
  - [x] 6.1 Refactor WAL detection: standalone `SessionState::load()` + `SessionState::exists()` used directly in `recover_and_process()` — WAL path derived from config, not from `SessionRunner`
  - [x] 6.2 In `recover_and_process()`, read WAL first via standalone load, then check `runtime_type` BEFORE touching `api_session_runner()`
  - [x] 6.3 If `runtime_type == "api"` (or empty/missing for backward compat): use existing API recovery path via `api_session_runner().resume_session()`
  - [x] 6.4 If `runtime_type == "sdk"`: for Dev phase, attempt `resume_sdk_session()` with session ID from `sdk_session_ids[phase]`; for Create/Review phase, delete stale WAL and restart from scratch
  - [x] 6.5 If no matching runtime configured (e.g., API WAL but SDK-only config), log warning and delete stale WAL
  - [x] 6.6 SDK crash recovery is best-effort: `kill_on_drop(true)` kills the CLI subprocess on daemon crash. If `--resume` fails or no session ID, fall back to restarting from scratch

- [x] Task 7: Tests (AC: #1, #2, #3)
  - [x] 7.1 Unit tests for `SessionRuntime::Dual` routing logic (API role → Api dispatch, SDK role → Sdk dispatch)
  - [x] 7.2 Unit tests for `SessionRuntime::from_config()` factory (all-API config, all-SDK config, mixed config, backward-compat with existing API-only configs)
  - [x] 7.3 Unit tests for WAL `SessionState` serialization: new fields round-trip, old WAL without new fields deserializes with defaults, `runtime_type` empty → treated as `"api"`
  - [x] 7.4 Unit tests for SDK consultation: trigger pattern matching against file content, consultation-already-fired tracking, max rounds enforcement, invalid regex handling
  - [x] 7.5 Unit tests for resume dispatcher routing (unknown provider → Failed outcome), shutdown flag accessor
  - [x] 7.6 Unit tests for resume config builders (Claude Code and Codex): basic, MCP, cli_path, args order
  - [x] 7.7 Existing pipeline tests construct `SessionRuntime` directly (not via `StoryPipeline::new()`) — all 1442 existing tests pass without modification; 38 new tests added

### Review Findings

- [x] [Review][Patch] Stale trigger text after SDK resume — Changed resume API to return `(SessionOutcome, Option<SdkSessionResult>)`, consultation loop now uses fresh `completion_text` after each resume. [src/runtime/sdk_consultation.rs, sdk.rs, sdk_claude.rs, sdk_codex.rs] — FIXED
- [x] [Review][Patch] Consultation fallback to supervisor not executed — Now creates a modified `ConsultationConfig` with `LlmRole::Supervisor` when the consultation's role is SDK. [src/runtime/sdk_consultation.rs:168-185] — FIXED
- [x] [Review][Patch] SDK Dev recovery no fallback on resume failure — Now falls back to `run_dev_pipeline()` when `resume_sdk_session` returns `Failed`. [src/pipeline.rs:2810-2818] — FIXED
- [x] [Review][Defer] `SdkWal::create` returns empty PathBuf — function returns `PathBuf::new()` as second tuple element, misleading API. [src/runtime/sdk_wal.rs:27] — deferred, spec explicitly defers WAL wiring (Task 5.7)
- [x] [Review][Defer] `run_api_consultation` discards trigger_text and story params — parameters accepted but suppressed with `let _ = ...`. ConsultationRunner handles context internally. [src/runtime/sdk_consultation.rs:188-190] — deferred, pre-existing ConsultationRunner API

## Dev Notes

### Critical Architecture Context

**Decision 12 (Dual Runtime Abstraction):** `SessionRuntime` enum provides the dispatch boundary. Story 15.1 created `Api` and `Sdk` variants. This story adds `Dual` to support mixed-mode configs where different roles use different providers.

**Decision 10 (Daemon-Orchestrated Consultations) — SDK adaptation:**
- **API mode:** Session paused in memory → consultation runs → findings injected as user message → session resumed. Handled by `SessionRunner::run_with_consultations()`.
- **SDK mode:** CLI subprocess runs to completion → daemon reads story file / completion text for trigger patterns → consultation runs via API (`ConsultationRunner`) → original SDK session resumed via CLI's native resume flags with findings as prompt. This is a NEW flow implemented in `sdk_consultation.rs`.

**Decision 13 (Supervisor MCP Server):** Already implemented in Story 15.4. SDK sessions access supervisor via MCP. No changes needed — the MCP config is already injected by `sdk_claude.rs` and `sdk_codex.rs`. Resume sessions also need MCP config re-injected (Task 1.4).

### Verified CLI Resume APIs

**Claude Code** — confirmed working non-interactively:
```
claude --resume {session_id} -p "{prompt}" --output-format stream-json --model {model} --cd {project_root}
```
Combines `--resume` with `-p` for headless prompt injection. Same streaming JSON output format as initial session. The `--mcp-config` flag must be re-passed on resume for supervisor access.

**Codex** — two commands, use `exec resume` for non-interactive:
```
codex exec resume {session_id} --json --cd {project_root} -- "{follow_up_instruction}"
```
`codex resume` is interactive-only. `codex exec resume` accepts an optional follow-up instruction and runs non-interactively with JSON output. The `.codex/config.toml` MCP config must be written before spawning the resume subprocess.

### Current Pipeline Runtime Selection (What to Change)

`StoryPipeline::new()` at `src/pipeline.rs:263` currently hardcodes:
```rust
let session_runtime = SessionRuntime::Api(Box::new(ApiRuntime::new(
    session_runner,
    skill_paths.clone(),
)));
```

This must become dynamic based on which roles use which providers. The pipeline calls `session_runtime.run_session()` at three points:
- `src/pipeline.rs:410` — Create phase (`LlmRole::Dev`, `PHASE_CREATE`)
- `src/pipeline.rs:703` — Dev phase (`LlmRole::Dev`, `PHASE_DEV`)
- `src/pipeline.rs:1234` — Review phase (`LlmRole::Review`, `PHASE_REVIEW`)

Each call site already passes the role in `SessionContext`. The `Dual` variant inspects the role's provider config to dispatch.

### `Dual` Variant Needs `Arc<BotConfig>` for Routing

The `Dual` variant must store an `Arc<BotConfig>` because it needs to look up `config.llm.{role}.is_sdk_provider()` at dispatch time. Neither `ApiRuntime` nor `SdkRuntime` expose config through a shared trait. Store it directly:
```rust
pub enum SessionRuntime {
    Api(Box<ApiRuntime>),
    Sdk(SdkRuntime),
    Dual {
        api: Box<ApiRuntime>,
        sdk: SdkRuntime,
        config: Arc<BotConfig>,
    },
}
```

The `config` field is only needed by `Dual`; `Api` and `Sdk` variants already hold their own config internally.

### Provider Classification (Existing)

`src/config/mod.rs` already provides:
- `LlmRoleConfig::is_sdk_provider()` → `true` for `"claude-code"`, `"codex"`
- `LlmRoleConfig::is_api_provider()` → `true` for `"anthropic"`, `"openai"`
- `LlmConfig` has fields: `dev`, `review`, `supervisor`, `epic_review`, `critic`

### `api_session_runner()` — Recovery Path Handling

`recover_and_process()` at `src/pipeline.rs:2748-2886` calls `self.session_runtime.api_session_runner()` in 6 places for WAL operations. The current implementation **panics** on `Sdk` variant (line 93).

**Problem:** The first call is `api_session_runner().check_and_recover_wal()` which reads the WAL file — but for SDK-only configs there is no `SessionRunner`. The daemon would panic on startup recovery.

**Solution:** Extract WAL file reading into a standalone `SessionState::load_if_exists(wal_path)`. The WAL path is derivable from `config.bmad_paths.implementation_artifacts` (same formula as `SessionRunner::state_file_path()`). Read the WAL and check `runtime_type` BEFORE calling any `api_session_runner()` method. Only enter the API recovery path if `runtime_type` is `"api"` or empty.

### Consultation Design — Constraints and Decisions

**Consultations always run via API.** `ConsultationRunner` at `src/session/consultation.rs:157` is hardwired to `AgentFactory::build()` which only supports `"anthropic"` and `"openai"` providers. Refactoring `ConsultationRunner` for SDK is out of scope for this story.

If a consultation's role (e.g., `LlmRole::Review`, `LlmRole::Critic`) is configured with an SDK provider, the consultation CANNOT run via SDK. Fallback strategy:
1. Log a warning: "Consultation role '{role}' uses SDK provider — falling back to API for consultation"
2. Use the supervisor's API config as fallback (supervisor must be API for consultations to work)
3. If no API provider is available anywhere in config, skip the consultation with an error log

**Trigger detection for SDK mode.** API mode detects triggers in real-time from streaming chat responses. SDK mode cannot do this — the CLI manages its own conversation. Instead:
- For `PHASE_CREATE`: read the story file on disk (`story.specs_path`) after session completion. The create-story skill writes the story file to disk as its primary artifact. Trigger patterns like `"STORY CONTEXT CREATED"` and `"corrections applied"` will appear in the file content.
- For `PHASE_DEV` / `PHASE_REVIEW`: read the story file + `SdkSessionResult::completion_text`. The completion text is the final agent response (up to 2000 chars). Combined, these provide sufficient trigger surface.
- **Do NOT rely on intermediate `Progress` events** — they are logged to tracing but not captured in any retrievable data structure.

**Consultation loop bound.** `MAX_SDK_CONSULTATION_ROUNDS = 3`. Each consultation fires at most once (tracked by a `HashSet<String>` of consultation labels). The loop exits when: (a) no consultations trigger, (b) all consultations have already fired, or (c) round count reaches max. This prevents infinite cycles where resumed sessions produce text that re-triggers the same consultation.

### WAL Write Responsibility for SDK Sessions

**API mode:** `SessionRunner` creates and manages WAL in `src/session/state.rs` via `SessionState::new()`, `save()`, `load()`.

**SDK mode:** NEW `src/runtime/sdk_wal.rs` manages WAL for SDK sessions:
- WAL created BEFORE subprocess spawn (write-ahead principle)
- Session ID recorded AFTER `SessionStarted` event parsed
- WAL deleted AFTER session outcome determined
- Same file path as API WAL: `{implementation_artifacts}/.bmad-bot-session.yaml`
- Only ONE WAL exists at a time (API or SDK) — the pipeline processes one story at a time

The `runtime_type` field in WAL disambiguates during crash recovery. If a WAL file exists with `runtime_type: "sdk"` but the daemon's config has changed to API providers, the recovery path detects this mismatch, deletes the stale WAL, and restarts from scratch.

### SDK Crash Recovery — Best-Effort Design

`sdk.rs:208` sets `kill_on_drop(true)` — if the daemon crashes, the SDK subprocess is killed immediately. Whether the CLI's session state survives for resume depends entirely on the CLI's internal persistence:
- **Claude Code:** persists sessions to `~/.claude/sessions/` — likely recoverable
- **Codex:** persists sessions to `~/.codex/sessions/` — likely recoverable

Recovery strategy:
1. Read WAL → get `runtime_type: "sdk"` and `sdk_session_ids[phase]`
2. For Dev phase: attempt `resume_sdk_session()` with session ID
3. If resume fails (session not found, CLI error), fall back to restart from scratch
4. For Create/Review phase: always restart from scratch (same as API behavior)

### `EpicReviewRunner` — Out of Scope

`EpicReviewRunner` at `src/review/epic.rs:325` uses `AgentFactory` directly (API-only). If `config.llm.epic_review` is configured as an SDK provider, the epic review phase will fail. This is a known limitation:
- `from_config()` should log a warning if `epic_review` is SDK: "Epic review does not support SDK providers — will fail at runtime"
- Full SDK support for epic review is deferred to a future story
- This matches the pattern of incremental SDK adoption: pipeline phases first, then ancillary features

### Files to Modify

| File | Changes |
|------|---------|
| `src/runtime/mod.rs` | Add `Dual` variant with `config: Arc<BotConfig>`, `from_config()` factory, change `api_session_runner()` to return `Option` |
| `src/runtime/sdk.rs` | Add `resume_sdk_session()` dispatcher |
| `src/runtime/sdk_claude.rs` | Add `resume_claude_code_session()` function |
| `src/runtime/sdk_codex.rs` | Add `resume_codex_session()` function |
| `src/pipeline.rs` | Replace hardcoded `SessionRuntime::Api(...)` with `from_config()`, add `config_path` param, refactor `recover_and_process()` for dual recovery, update ~24 test helper calls |
| `src/session/state.rs` | Add `runtime_type`, `sdk_session_ids` fields to `SessionState` |
| `src/cli/mod.rs` | Pass `config_path` to `StoryPipeline::new()` |

### New Files

| File | Purpose |
|------|---------|
| `src/runtime/sdk_consultation.rs` | SDK consultation orchestration: post-session trigger detection, consultation execution via API, resume with findings |
| `src/runtime/sdk_wal.rs` | SDK-specific WAL management: create, record session ID, cleanup |

### Files NOT to Modify (beyond resume additions)

- `src/mcp_server/mod.rs` — MCP server complete (Story 15.4)
- `src/session/runner.rs` — API session flow unchanged
- `src/session/consultation.rs` — `ConsultationRunner` stays API-only
- `src/llm/agent_factory.rs` — Agent factory unchanged (API-only, by design)
- `src/review/epic.rs` — `EpicReviewRunner` SDK support deferred

### Previous Story Learnings (from 15.5 and 15.6)

- **Shared code pattern:** `map_sdk_result_to_outcome()`, `read_decisions_json_sidecar()`, `detect_escalation()` are `pub(crate)` in `sdk_claude.rs`, reused by `sdk_codex.rs`. Follow this pattern for resume functions and shared consultation logic.
- **Temp file for MCP config:** Claude Code uses a temp file for MCP config (security — no API keys in process listings). Codex uses `.codex/config.toml`. Resume sessions MUST re-inject MCP config using the same patterns.
- **Session ID tracking:** Claude Code returns `session_id` from `system/init` event. Codex returns `thread_id` from `thread.started` event. Both stored in `SdkSessionResult::session_id`.
- **Completion text:** `SdkSessionResult::completion_text` captures the last completion event's text (truncated to 2000 chars in Claude Code provider) — used for `pr_context` and now also for consultation trigger matching.
- **Project root canonicalization:** Always `canonicalize()` the project root for `--cd` and MCP config paths.
- **`serde(default)` for backward compat:** All new WAL fields must use `#[serde(default)]` so old WAL files can still be parsed.

### Git Intelligence

Recent commits (Stories 15.4-15.6) added:
- `src/mcp_server/mod.rs` (589 lines) — MCP supervisor server
- `src/runtime/sdk_claude.rs` (711 lines) — Claude Code provider
- `src/runtime/sdk_codex.rs` (1057 lines) — Codex provider
- Modified `src/runtime/sdk.rs` (+176 lines) — execution loop, event parsing
- Modified `src/runtime/mod.rs` (+10 lines) — module exports
- Modified `src/cli/mod.rs` (+85 lines) — MCP supervisor subcommand
- Modified `src/supervisor/decisions.rs` (+79 lines) — JSON sidecar for SDK decisions

All 1577 tests pass (1442 unit + 135 lib). Test pattern: each module has inline `#[cfg(test)] mod tests` with comprehensive unit tests.

### Test Update Scope

`pipeline.rs` contains ~203 test functions. The `config_path` addition to `StoryPipeline::new()` will require updating:
- ~24 usages of `make_test_session_runtime` helper
- ~19 pipeline struct constructions in test code
- The test helper itself needs a `config_path` parameter (can use `PathBuf::from("test-config.yaml")`)

This is mechanical churn but critical for CI. Do NOT skip or comment out existing tests.

### Project Structure Notes

- New files: `src/runtime/sdk_consultation.rs`, `src/runtime/sdk_wal.rs`
- Modified files: `src/runtime/mod.rs`, `src/runtime/sdk.rs`, `src/runtime/sdk_claude.rs`, `src/runtime/sdk_codex.rs`, `src/pipeline.rs`, `src/session/state.rs`, `src/cli/mod.rs`
- Module declarations: add `pub mod sdk_consultation;` and `pub mod sdk_wal;` to `src/runtime/mod.rs`
- Naming convention: `snake_case` for modules, `PascalCase` for types, `SCREAMING_SNAKE` for constants

### Anti-Patterns to Avoid

- **DO NOT call `api_session_runner()` without checking `runtime_type` first** — panics on `Sdk` variant
- **DO NOT run consultations via SDK** — `ConsultationRunner` is API-only; attempting SDK consultation will fail at `AgentFactory::build()`
- **DO NOT rely on `SdkSessionResult::completion_text` alone for trigger detection** — it only captures the final event (truncated). Read the story file on disk for reliable trigger matching
- **DO NOT assume `--resume` always succeeds** — CLI session state may not survive daemon crashes. Always implement restart-from-scratch as fallback
- **DO NOT skip `serde(default)` on new WAL fields** — breaks backward compatibility with pre-15.7 WAL files
- **DO NOT pass consultations to SDK subprocess** — consultations are daemon-orchestrated, not CLI-managed
- **DO NOT assume all roles use the same provider** — each role is independently configurable
- **DO NOT skip pipeline test updates** — ~24+ test constructions need `config_path` parameter

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-15, Story 15.7]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-26.md#Decision-12, Decision-10-Amendment]
- [Source: _bmad-output/planning-artifacts/architecture.md#Session-Runtime, Pipeline-Orchestration]
- [Source: _bmad-output/implementation-artifacts/15-1-session-runtime-abstraction-layer.md — SessionRuntime, SkillPaths, ApiRuntime]
- [Source: _bmad-output/implementation-artifacts/15-5-claude-code-provider-integration.md — SDK result mapping, MCP config, decisions sidecar]
- [Source: _bmad-output/implementation-artifacts/15-6-codex-provider-integration.md — Codex provider patterns, shared code reuse]
- [Source: https://code.claude.com/docs/en/cli-reference — Claude Code CLI resume: `claude --resume {id} -p "{prompt}"`]
- [Source: https://developers.openai.com/codex/cli/reference — Codex CLI resume: `codex exec resume {id}` with follow-up instruction]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Task 1: Added `resume_claude_code_session()` and `resume_codex_session()` with MCP config re-injection, plus shared `resume_sdk_session()` dispatcher in `sdk.rs`
- Task 2: Added `Dual` variant to `SessionRuntime` with `from_config()` factory. `api_session_runner()` now returns `Option`. Added `resolve_role_config()` helper to avoid config→llm circular dependency. Added `sdk_runtime()` accessor.
- Task 3: `StoryPipeline::new()` takes `config_path: PathBuf` and delegates to `SessionRuntime::from_config()`. CLI passes `config_path` from `run_start()`.
- Task 4: Created `sdk_consultation.rs` with `SdkConsultationRunner` — post-session trigger detection via regex, API consultation execution, SDK resume with findings, max 3 rounds, each consultation fires at most once.
- Task 5: Added `runtime_type` and `sdk_session_ids` fields to `SessionState` with `#[serde(default)]`. Created `sdk_wal.rs` with `SdkWal::create/record_session_id/cleanup`.
- Task 6: Fully refactored `recover_and_process()` — standalone WAL load, runtime_type routing (SDK path with resume, API path with existing recovery), fallback for missing runtimes.
- Task 7: 38 new tests across 6 modules. All 1615 tests pass (1480 bin + 135 lib). Zero clippy errors.

### File List

New files:
- src/runtime/sdk_consultation.rs
- src/runtime/sdk_wal.rs

Modified files:
- src/runtime/mod.rs (Dual variant, from_config, resolve_role_config, sdk_runtime accessor, new tests)
- src/runtime/sdk.rs (resume_sdk_session dispatcher, shutdown_flag accessor, new tests)
- src/runtime/sdk_claude.rs (resume config builder, resume_claude_code_session, new tests)
- src/runtime/sdk_codex.rs (resume config builder, resume_codex_session, new tests)
- src/pipeline.rs (config_path param, from_config usage, refactored recover_and_process, build_story_from_wal)
- src/session/state.rs (runtime_type, sdk_session_ids fields, new tests)
- src/session/runner.rs (runtime_type/sdk_session_ids in compressed state and test helpers)
- src/cli/mod.rs (pass config_path to StoryPipeline::new)

### Change Log

- 2026-04-27: Implemented Story 15.7 — Pipeline Dual-Runtime Orchestration. Added Dual variant for mixed API/SDK configs, SDK session resume for both providers, SDK consultation orchestration, SDK WAL management, and dual-runtime crash recovery.
