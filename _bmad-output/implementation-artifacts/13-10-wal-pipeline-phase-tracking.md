# Story 13.10: WAL Pipeline Phase Tracking

Status: done

## Story

As a daemon operator,
I want the WAL (Write-Ahead Log) to track which pipeline phase a story is in,
So that crash recovery resumes at the correct phase instead of restarting from scratch.

## Acceptance Criteria

1. **Given** the `SessionState` struct in `src/session/state.rs` **When** this story is implemented **Then** a new field `pipeline_phase: String` is added with `#[serde(default)]` for backward compatibility with pre-13.10 WAL files **And** valid phase values are defined as public constants in `src/session/state.rs`: `PHASE_CREATE`, `PHASE_CREATE_ADVERSARIAL_CONSULT`, `PHASE_CREATE_CRITIC_CONSULT`, `PHASE_DEV`, `PHASE_REVIEW`, `PHASE_REVIEW_CRITIC_CONSULT` **And** the field is updated at each phase transition BEFORE the phase starts (write-ahead guarantee).

2. **Given** the daemon starts and finds an existing WAL file **When** crash recovery is attempted via `recover_and_process()` **Then** `pipeline_phase` is extracted from the recovered `SessionState` (cloned before `RecoveryInfo` is consumed) and used to determine the correct recovery route:
   - `PHASE_CREATE` / `PHASE_CREATE_ADVERSARIAL_CONSULT` / `PHASE_CREATE_CRITIC_CONSULT` → delete stale WAL, set `StoryInfo.status` to `"backlog"`, call `run_create_pipeline()` from scratch
   - `PHASE_DEV` or empty string (legacy WAL) → existing `resume_session()` + `process_recovered_session()` flow (unchanged), `StoryInfo.status` set to `"in-progress"`
   - `PHASE_REVIEW` / `PHASE_REVIEW_CRITIC_CONSULT` → delete stale WAL, set `StoryInfo.status` to `"review"`, call `run_review_pipeline()` from scratch
   - Any unrecognized value → log `tracing::warn!` with the value, delete the corrupt WAL, fall back to dev-phase recovery (best-effort)
   **And** `tracing::info!` logs: `"Recovering story {key} from pipeline phase: {phase}"`

3. **Given** a pipeline phase completes successfully **When** the next phase starts **Then** the WAL `pipeline_phase` is updated with the new phase value before the phase begins **And** the WAL is saved atomically (existing `save()` pattern) **And** when the entire story pipeline completes (push + PR + notify), the WAL is deleted as before.

## Tasks / Subtasks

- [x] Task 1: Add `pipeline_phase` field and phase constants to `SessionState` (AC: #1)
  - [x] 1.1 Define public string constants in `src/session/state.rs`: `pub const PHASE_CREATE: &str = "create"`, `PHASE_CREATE_ADVERSARIAL_CONSULT: &str = "create-adversarial-consult"`, `PHASE_CREATE_CRITIC_CONSULT: &str = "create-critic-consult"`, `PHASE_DEV: &str = "dev"`, `PHASE_REVIEW: &str = "review"`, `PHASE_REVIEW_CRITIC_CONSULT: &str = "review-critic-consult"`
  - [x] 1.2 Add `pipeline_phase: String` field with `#[serde(default)]` and doc comment to `SessionState`
  - [x] 1.3 Add `set_pipeline_phase(&mut self, phase: &str)` method that updates `pipeline_phase` only (do NOT update `last_activity` — phase transitions are structural metadata, not chat activity)
  - [x] 1.4 Update `SessionState::new()` to initialize `pipeline_phase` as empty string
  - [x] 1.5 Add unit tests: serialization roundtrip with pipeline_phase, deserialization of legacy WAL without the field (backward compat), `set_pipeline_phase` behavior

- [x] Task 2: Pass initial pipeline phase into session runner (AC: #1, #3)
  - [x] 2.1 Add an `initial_phase: &str` parameter to `run_with_consultations()` in `src/session/runner.rs:724`
  - [x] 2.2 Forward `initial_phase` to `run_session()` — set `state.pipeline_phase = initial_phase.to_string()` right after `SessionState::new()` at line 1470, BEFORE the first `state.save()`
  - [x] 2.3 Update `SessionRunner::run()` (the dev-only shortcut at line 712) to pass `PHASE_DEV` as initial phase
  - [x] 2.4 Update pipeline call sites:
    - `run_create_pipeline()` at line 403 → pass `PHASE_CREATE`
    - `run_dev_pipeline()` at line 697 → pass `PHASE_DEV` (via `run()`)
    - `run_review_pipeline()` at line 1220 → pass `PHASE_REVIEW`

- [x] Task 3: Track consultation sub-phases inside session runner (AC: #1, #3)
  - [x] 3.1 In `check_consultation_triggers()` (`src/session/runner.rs:2478`): when a consultation trigger matches (line 2491, after `state.triggered = true`), update `state.pipeline_phase` to the consultation's sub-phase value BEFORE executing the consultation, then call `state.save()`. This requires `state: &mut SessionState` to be passed into `check_consultation_triggers()` — add it as a parameter
  - [x] 3.2 To map consultation label → sub-phase value: add an optional `pipeline_phase: Option<String>` field to `ConsultationConfig` (`src/session/consultation.rs`). When set, `check_consultation_triggers()` uses it to update the WAL phase before executing. When `None`, the WAL phase is unchanged
  - [x] 3.3 Update consultation config construction in pipeline:
    - `build_create_story_consultations()`: adversarial consultation gets `pipeline_phase: Some(PHASE_CREATE_ADVERSARIAL_CONSULT.into())`, critic consultation gets `pipeline_phase: Some(PHASE_CREATE_CRITIC_CONSULT.into())`
    - `build_review_consultations()`: critic consultation gets `pipeline_phase: Some(PHASE_REVIEW_CRITIC_CONSULT.into())`
  - [x] 3.4 Add `tracing::debug!` log at each phase update inside `check_consultation_triggers()`

- [x] Task 4: Phase-aware recovery routing in `recover_and_process()` (AC: #2)
  - [x] 4.1 Extract `pipeline_phase` from `recovery.state.pipeline_phase.clone()` BEFORE passing `recovery` into `resume_session()` (since `RecoveryInfo` is consumed by ownership)
  - [x] 4.2 Route recovery based on `pipeline_phase` value using the phase constants. Use a helper function `recovery_phase_to_story_phase(phase: &str) -> StoryPhase` that maps granular phases to coarse routing:
    - `PHASE_CREATE` | `PHASE_CREATE_ADVERSARIAL_CONSULT` | `PHASE_CREATE_CRITIC_CONSULT` → `StoryPhase::Create`
    - `PHASE_DEV` | `""` → `StoryPhase::Dev`
    - `PHASE_REVIEW` | `PHASE_REVIEW_CRITIC_CONSULT` → `StoryPhase::Review`
    - anything else → log `tracing::warn!("Unknown pipeline_phase '{}' in WAL — falling back to dev recovery", phase)`, return `StoryPhase::Dev`
  - [x] 4.3 For `StoryPhase::Create` recovery: delete the stale WAL via `SessionState::delete()`, set `story_for_pipeline.status = "backlog"`, call `run_create_pipeline()`. The fresh session will create its own WAL. Branch state is safe — `ensure_story_branch()` in `run_with_consultations()` handles both create-or-checkout
  - [x] 4.4 For `StoryPhase::Dev` recovery: unchanged — pass `RecoveryInfo` to `resume_session()` + `process_recovered_session()` as today, keep `status: "in-progress"`
  - [x] 4.5 For `StoryPhase::Review` recovery: delete the stale WAL, set `story_for_pipeline.status = "review"`, call `run_review_pipeline()`
  - [x] 4.6 Add `tracing::info!` log with story_key and pipeline_phase at recovery decision point

- [x] Task 5: Unit tests (AC: #1, #2, #3)
  - [x] 5.1 `state.rs` — Test: WAL with `pipeline_phase: "create"` serializes/deserializes correctly
  - [x] 5.2 `state.rs` — Test: WAL YAML without `pipeline_phase` field deserializes with empty string (backward compat)
  - [x] 5.3 `state.rs` — Test: `set_pipeline_phase()` updates field without touching `last_activity`
  - [x] 5.4 `pipeline.rs` — Test: `recovery_phase_to_story_phase()` returns correct `StoryPhase` for all 6 constants + empty string + unknown value
  - [x] 5.5 `pipeline.rs` — Test: WAL with `pipeline_phase: "create"` → recovery routes to create-story phase (source-check or mock)
  - [x] 5.6 `pipeline.rs` — Test: WAL with `pipeline_phase: "dev"` → recovery routes to dev session recovery
  - [x] 5.7 `pipeline.rs` — Test: WAL with `pipeline_phase: "review"` → recovery routes to review phase
  - [x] 5.8 `pipeline.rs` — Test: WAL with `pipeline_phase: ""` (legacy) → recovery falls back to dev recovery
  - [x] 5.9 `pipeline.rs` — Test: WAL with `pipeline_phase: "create-adversarial-consult"` → routes to Create (not mid-consultation)
  - [x] 5.10 `pipeline.rs` — Test: WAL with `pipeline_phase: "garbage-value"` → falls back to dev recovery with warning
  - [x] 5.11 `runner.rs` — Test: after consultation trigger matches, WAL on disk contains the expected sub-phase value (verify write-ahead behavior)
  - [x] 5.12 `runner.rs` — Test: consultation with `pipeline_phase: None` does NOT change WAL phase

## Dev Notes

### Architecture Compliance

This story implements **Architecture Decision 3 Amendment** (2026-04-15): "Multi-phase pipeline tracking in WAL." The architecture specifies:

- New WAL field: `pipeline_phase: String` recording the active phase
- Updated at each phase transition BEFORE the phase starts (write-ahead semantics)
- WAL tracks current phase's session only — no multi-phase history needed (phases are sequential)
- Recovery routing by phase: create/consult → restart from scratch, dev → attempt WAL recovery, review/consult → restart from scratch
- Dev phase is the only phase where mid-session recovery is attempted (longest-running, most expensive)

[Source: `_bmad-output/planning-artifacts/architecture.md` — Decision 3 Amendment, Decision 10]

### Current WAL Implementation

`SessionState` in `src/session/state.rs:82-117` has fields: `story_id`, `story_key`, `branch`, `started_at`, `last_activity`, `provider`, `model`, `branch_name`, `base_branch`, `skill_path`, `chat_history`. No `pipeline_phase` field currently exists.

The backward-compat pattern is established: `branch_name`, `base_branch`, and `skill_path` all use `#[serde(default)]` for WAL files from earlier versions. Use the same pattern for `pipeline_phase`.

`SessionState::new()` at line 124 initializes all fields — add `pipeline_phase: String::new()` there.

### Ownership Model — Who Updates What

There are two distinct levels of phase tracking:

**Top-level phases (create / dev / review):** Set by the pipeline before starting a session. The pipeline knows which phase it's entering. The session runner receives this as an `initial_phase` parameter to `run_with_consultations()`, sets it on `SessionState` at creation time (line 1470), and persists it in the first `state.save()`.

**Consultation sub-phases (create-adversarial-consult, create-critic-consult, review-critic-consult):** Set INSIDE the session runner. Consultations are triggered internally by `check_consultation_triggers()` at `src/session/runner.rs:2478` via regex pattern matching on agent responses. The pipeline has no visibility into when consultations fire — it calls `run_with_consultations()` as a single async call and doesn't regain control until the session completes. Therefore, sub-phase WAL updates MUST happen inside `check_consultation_triggers()`, where the mutable `state` local variable is naturally available. No disk round-trip needed — update `state.pipeline_phase` in-place and call `state.save()` (same pattern as the chat turn save at line 1558).

To map consultations to their sub-phase values, add an optional `pipeline_phase: Option<String>` field to `ConsultationConfig`. The pipeline sets this when building consultations. `check_consultation_triggers()` reads it before executing each consultation.

### Current Recovery Flow

`recover_and_process()` in `src/pipeline.rs:2526` currently:
1. Calls `session_runner.check_and_recover_wal()` → gets `RecoveryInfo { state, story_info }`
2. Hard-codes `status: "in-progress"` on the story (line 2545) — assumes dev phase
3. Calls `session_runner.resume_session(recovery)` → attempts chat history replay
4. Routes outcome through `process_recovered_session()` → push → PR → review chain

This flow is correct for dev-phase recovery but wrong for create/review phases. The new phase-aware routing must intercept BEFORE `resume_session()` is called.

**Ownership caveat:** `RecoveryInfo` is consumed by `resume_session()` via ownership move. Extract `pipeline_phase` via `recovery.state.pipeline_phase.clone()` BEFORE passing `recovery` to `resume_session()`.

**For create/review recovery:** do NOT call `resume_session()` (no chat history replay needed). Delete the stale WAL via `SessionState::delete(&state_file_path)`, then call `run_create_pipeline()` or `run_review_pipeline()` directly — they will create their own fresh WAL. Branch state is safe: `ensure_story_branch()` inside `run_with_consultations()` handles both create-or-checkout of the story branch, so a pre-existing branch from a crashed session is just checked out.

**Story status for recovery:** The hard-coded `status: "in-progress"` at line 2545 must be set correctly per phase:
- Create recovery → `status: "backlog"` (so `run_create_pipeline()` route check sees the right status)
- Dev recovery → `status: "in-progress"` (unchanged)
- Review recovery → `status: "review"` (so `run_review_pipeline()` route check sees the right status)

### Create-Phase Recovery Idempotency

If the daemon crashes during `create-critic-consult`, the create-story agent may have already written the story file and updated sprint-status.yaml to `ready-for-dev`. Re-running `run_create_pipeline()` from scratch will activate the create-story skill again. This is safe because:
- The create-story BMAD skill reads sprint-status.yaml to discover the target story. If the story is already `ready-for-dev`, the skill picks the next `backlog` story or reports no work.
- The pipeline also re-reads sprint-status.yaml via `reload_story_info()` after the create phase, so it will see the real current status.
- Worst case: the create phase completes as a no-op and chains to dev phase normally.

If this idempotency assumption proves wrong in practice, a pre-recovery check on sprint-status.yaml for the story's actual status could short-circuit to the correct phase. But for now, trust the skill's built-in discovery logic.

### Interaction with `process_recovered_session()`

`process_recovered_session()` at `src/pipeline.rs:2561` currently handles post-recovery routing (push → PR → review chain). After this story:

- For `dev` phase recovery: behavior is UNCHANGED — `resume_session()` → `process_recovered_session()`
- For `create` phase recovery: no interaction — `run_create_pipeline()` handles its own flow
- For `review` phase recovery: no interaction — `run_review_pipeline()` handles its own flow

### `StoryPhase` Reuse for Recovery Routing

The existing `StoryPhase` enum (`src/pipeline.rs:2363`) maps story STATUS to pipeline phase. Reuse it for recovery routing via a new helper `recovery_phase_to_story_phase(phase: &str) -> StoryPhase`:
```
PHASE_CREATE | PHASE_CREATE_ADVERSARIAL_CONSULT | PHASE_CREATE_CRITIC_CONSULT → StoryPhase::Create
PHASE_DEV | "" → StoryPhase::Dev
PHASE_REVIEW | PHASE_REVIEW_CRITIC_CONSULT → StoryPhase::Review
_ → log warning, StoryPhase::Dev (fallback)
```

### Previous Story Intelligence

Story 13.9 (last completed) added `LlmRole::Critic` with `#[serde(default)]` pattern for config backward compat. Story 13.2 established the `StoryPhase` router and extracted `run_create_pipeline()` / `run_dev_pipeline()` / `run_review_pipeline()`. All three pipeline methods are fully implemented (Stories 13.4, 13.5, 13.6).

The consultation mechanism (Story 13.3) uses `ConsultationRunner` inside `SessionRunner`. Consultation triggers are detected by `check_consultation_triggers()` at `src/session/runner.rs:2478` via regex pattern matching on agent responses. The session runner holds the mutable `SessionState` (`state` variable) throughout `run_session()` — phase updates on consultation boundaries happen in-place, then the next `state.save()` persists them.

### Git Intelligence

Recent commits show consistent patterns:
- Branch naming: `story/13-10-wal-pipeline-phase-tracking`
- Commit style: `feat(epic-13): ...` with conventional commits
- Test count: 1237 tests as of Story 13.9
- All tests inline in `#[cfg(test)] mod tests { ... }` at bottom of each module

### Project Structure Notes

Files to modify:
- `src/session/state.rs` — Add phase constants, `pipeline_phase` field, `set_pipeline_phase()` method, update `new()`, add tests
- `src/session/runner.rs` — Add `initial_phase` parameter to `run_with_consultations()` and `run_session()`, pass `&mut state` into `check_consultation_triggers()`, update sub-phase before consultation execution, add tests
- `src/session/consultation.rs` — Add `pipeline_phase: Option<String>` field to `ConsultationConfig`
- `src/pipeline.rs` — Phase-aware recovery in `recover_and_process()`, pass phase constants to `run_with_consultations()` call sites, set `pipeline_phase` on consultation configs, add `recovery_phase_to_story_phase()` helper, add tests

Files that may need test config updates:
- `src/session/runner.rs` — existing tests calling `run_with_consultations()` need the new `initial_phase` parameter
- `src/review/epic.rs` — if it calls `run_with_consultations()` directly

Since `pipeline_phase` on `SessionState` uses `#[serde(default)]`, existing tests constructing `SessionState` via `SessionState::new()` need no changes (field initializes to empty string).

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Naming: `test_{module}_{behavior}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert
- All tests inline in same file at bottom
- Mock LLM responses — never call real APIs
- Zero-warning policy: `#![deny(clippy::all)]`

### References

- [Source: `_bmad-output/planning-artifacts/architecture.md` — Decision 3 Amendment (2026-04-15)]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Decision 10: Daemon-Orchestrated Consultations]
- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 13, Story 13.10]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md` — Epic 13 pipeline model]
- [Source: `src/session/state.rs:82-117` — Current SessionState struct]
- [Source: `src/session/runner.rs:724-732` — run_with_consultations() signature]
- [Source: `src/session/runner.rs:1467-1474` — SessionState creation and first save in run_session()]
- [Source: `src/session/runner.rs:2478-2526` — check_consultation_triggers() with mutable state access]
- [Source: `src/pipeline.rs:2526-2555` — Current recover_and_process() flow]
- [Source: `src/pipeline.rs:2363-2376` — StoryPhase enum and route_story_status()]
- [Source: `src/pipeline.rs:385,403` — run_create_pipeline() and its run_with_consultations() call]
- [Source: `src/pipeline.rs:688,697` — run_dev_pipeline() and its run() call]
- [Source: `src/pipeline.rs:1114,1220` — run_review_pipeline() and its run_with_consultations() call]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None

### Completion Notes List

- Task 1: Added 6 pipeline phase constants (PHASE_CREATE, PHASE_CREATE_ADVERSARIAL_CONSULT, PHASE_CREATE_CRITIC_CONSULT, PHASE_DEV, PHASE_REVIEW, PHASE_REVIEW_CRITIC_CONSULT) to state.rs. Added `pipeline_phase: String` field with `#[serde(default)]` to SessionState. Added `set_pipeline_phase()` method that updates phase without touching `last_activity`. Updated `new()` to initialize empty. 4 unit tests added.
- Task 2: Added `initial_phase: &str` parameter to `run_with_consultations()` and `run_session()`. Phase is set on SessionState BEFORE first `state.save()` (write-ahead guarantee). Updated `run()` shortcut to pass PHASE_DEV. Updated pipeline call sites: create→PHASE_CREATE, review→PHASE_REVIEW. Added `#[allow(clippy::too_many_arguments)]` for the expanded signature.
- Task 3: Added `pipeline_phase: Option<String>` field to `ConsultationConfig`. Updated `check_consultation_triggers()` to accept `&mut SessionState`, update phase and save WAL before consultation execution. Added tracing::debug log. Updated all 3 consultation configs with correct sub-phase values.
- Task 4: Rewrote `recover_and_process()` with phase-aware routing. Added `recovery_phase_to_story_phase()` helper mapping granular phases to coarse StoryPhase. Create/review recovery deletes stale WAL and restarts from scratch. Dev recovery unchanged (mid-session replay). Added `state_file_path()` accessor to SessionRunner. Proper status assignment per phase.
- Task 5: 12 new tests total — 4 in state.rs (serialization, backward compat, set_pipeline_phase, new() default), 6 in pipeline.rs (recovery routing for all constants + empty + unknown + consultation phase assignment), 2 in runner.rs (write-ahead behavior verification, None phase preservation).

### File List

- `src/session/state.rs` — Added phase constants, pipeline_phase field, set_pipeline_phase() method, 4 tests
- `src/session/runner.rs` — Added initial_phase parameter to run_with_consultations/run_session, phase-aware check_consultation_triggers, state_file_path() accessor, updated 3 manual SessionState constructions, 2 tests
- `src/session/consultation.rs` — Added pipeline_phase: Option<String> to ConsultationConfig, updated test default_config()
- `src/pipeline.rs` — Added recovery_phase_to_story_phase() helper, rewrote recover_and_process() with phase routing, added pipeline_phase to all consultation configs, imported phase constants, 6 tests
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — Status: in-progress → review
- `_bmad-output/implementation-artifacts/13-10-wal-pipeline-phase-tracking.md` — Story file updates

### Review Findings

- [x] [Review][Decision→Dismiss] WAL save failure in `check_consultation_triggers` continues execution — accepted as best-effort (crash + WAL write failure simultaneous is very improbable)
- [x] [Review][Decision→Dismiss] Review-phase recovery duplicate PRs — already handled by GitHubProvider: `create_pr` catches `DuplicatePr` and returns existing PR via `find_open_pr`
- [x] [Review][Decision→Patch] WAL delete failure in create/review/unknown recovery — retry 3× + hard error: `delete_wal_with_retry()` helper added [src/pipeline.rs]
- [x] [Review][Patch] Unrecognized `pipeline_phase` now returns `StoryPhase::Unknown` → deletes corrupt WAL before dev fallback (AC #2 fix) [src/pipeline.rs]
- [x] [Review][Patch] Phase constants now have individual `///` doc comments [src/session/state.rs:17-24]
- [x] [Review][Patch] `drop(recovery)` added before Create/Review pipeline restart [src/pipeline.rs]
- [x] [Review][Defer] `resume_session` hardcodes `LlmRole::Dev` for all recovery — pre-existing [src/session/runner.rs:590] — deferred, pre-existing (also noted in 13.6 review)
- [x] [Review][Defer] `state_file_path()` returns `PathBuf` clone instead of `&Path` reference — optimization opportunity [src/session/runner.rs:388] — deferred, minor performance
- [x] [Review][Defer] `#[allow(clippy::too_many_arguments)]` lint suppression on `run_with_consultations` [src/session/runner.rs:735] — deferred, refactor into options struct later

## Change Log

- 2026-04-25: Implemented WAL pipeline phase tracking — all 5 tasks complete, 12 new tests (1252 total passing, 1 pre-existing failure unrelated)
