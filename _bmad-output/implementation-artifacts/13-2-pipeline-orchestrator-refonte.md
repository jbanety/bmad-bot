# Story 13.2: Pipeline Orchestrator Refonte

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the pipeline to route stories through the correct processing phase based on their current status (backlog → create, ready-for-dev → dev, review → code-review),
So that each story flows through the full lifecycle autonomously and can resume from any phase after a crash.

## Acceptance Criteria

1. **AC-1: Status-based phase routing in `process_story()`**
   - **Given** `src/pipeline.rs` currently has `process_story()` which unconditionally runs: dev session → push → PR → review → notify
   - **When** this story is implemented
   - **Then** `process_story()` implements a phase router that dispatches based on the story's current status:
     - `backlog` → placeholder that returns `PipelineResult` with `StoryStatus::Error` and message: "Create-story phase not yet implemented (Story 13.4)"
     - `ready-for-dev` → runs the existing dev session flow (session → push → PR → review → notify) — UNCHANGED behavior
     - `review` → placeholder that returns `PipelineResult` with `StoryStatus::Error` and message: "Code-review phase not yet implemented (Story 13.6)"
   - **And** the routing is a simple `match story.status.as_str()` at the top of `process_story()`, delegating to private methods
   - **And** unexpected statuses return `PipelineResult` with `StoryStatus::Error` and message: "Unexpected status '{status}' in process_story — should not reach pipeline"

2. **AC-2: Multi-phase sequential execution for `backlog` stories**
   - **Given** a story enters `process_story()` with status `backlog`
   - **When** the create phase placeholder completes successfully (future Story 13.4)
   - **Then** `process_story()` is structured to allow phases to run sequentially: create → dev → review → push → PR → notify
   - **And** between each phase, the pipeline verifies the outcome — if any phase fails or escalates, the pipeline stops and handles the error
   - **And** each phase is a separate method call — no session state carries between phases
   - **Note** For this story, the sequential multi-phase flow is **structural only** — the `backlog` branch returns an error placeholder. Stories 13.4/13.5/13.6 will fill in the actual phase implementations.

3. **AC-3: Remove backward-compatible pipeline guard**
   - **Given** Story 13.1 added `guard_processable_stories()` in `src/pipeline.rs` (line 1644) that filters stories to `ready-for-dev` only
   - **When** this story is implemented
   - **Then** the `guard_processable_stories()` function is REMOVED entirely
   - **And** both call sites in `process_eligible_stories()` are updated:
     - Line 921: `let mut current_stories = guard_processable_stories(stories);` → `let mut current_stories = stories;`
     - Line 1026: `current_stories = guard_processable_stories(fresh_stories);` → `current_stories = fresh_stories;`
   - **And** all three status types (`backlog`, `ready-for-dev`, `review`) now flow through to `process_story()`

4. **AC-4: Refactor `process_story()` into extracted methods**
   - **Given** `process_story()` is currently ~560 lines (lines 317-876) with all logic inlined
   - **When** this story is implemented
   - **Then** the existing dev session flow is extracted into a private method `run_dev_pipeline()` that handles the full existing sequence: dev session → push → PR → review → push review commits → post review comment → mark done → notify
   - **And** `process_story()` becomes a thin router (~30 lines) that matches on status and delegates to the appropriate method
   - **And** the extracted method preserves all existing behavior, error handling, and UI events EXACTLY — this is a pure refactor, not a behavior change
   - **And** future placeholder methods are added: `run_create_pipeline()` (returns error placeholder) and `run_review_pipeline()` (returns error placeholder)

5. **AC-5: Pipeline resume from correct phase after crash**
   - **Given** a story was interrupted mid-pipeline (e.g., crash during dev phase)
   - **When** the daemon restarts and the next poll picks up the story
   - **Then** the story's status in `sprint-status.yaml` determines which phase the pipeline enters
   - **And** `ready-for-dev` stories go directly to the dev phase (existing behavior — story 13.1 backward-compat guard was the only barrier)
   - **And** `review` stories go to the review phase placeholder (future Story 13.6)
   - **And** `backlog` stories go to the create phase placeholder (future Story 13.4)
   - **Note** WAL-based intra-phase recovery (Story 13.10) is a separate concern — this story only handles phase-level routing

6. **AC-6: Tests**
   - **Given** the pipeline module has existing tests
   - **When** this story is implemented
   - **Then** the following tests are updated or added:
     - Remove `test_guard_processable_stories_filters_non_ready_for_dev` and `test_guard_processable_stories_returns_empty_when_all_non_ready` (guard function removed)
     - Add `test_route_story_status_returns_correct_phase` — verifies the routing function maps `backlog` → `Create`, `ready-for-dev` → `Dev`, `review` → `Review`, anything else → `Unknown`
     - Add `test_process_story_routes_backlog_returns_placeholder` — verifies `backlog` stories return error with "Story 13.4" message
     - Add `test_process_story_routes_review_returns_placeholder` — verifies `review` stories return error with "Story 13.6" message
     - Add `test_process_story_routes_unexpected_status` — verifies unexpected status returns error
   - **And** the `ready-for-dev` routing path is NOT directly unit-tested (would require mocking `SessionRunner` / LLM providers). It is covered by existing pipeline tests which exercise the full `ready-for-dev` flow and remain unchanged.
   - **And** all existing pipeline tests continue to pass (they exercise `ready-for-dev` flow which is unchanged)
   - **And** the existing `test_process_story_installs_cleanup_guard_source_check` test (line 3632) continues to pass — it inspects `process_story()` source for the `StorySubAgentCleanup` guard which remains in `process_story()`
   - **And** `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` passes
   - **And** `cargo test` passes with no new failures beyond the pre-existing `test_build_context_limit_recovery_message_contains_all_sections`

## Tasks / Subtasks

**CRITICAL: Task ordering matters.** Tasks 1-3 (extract + router + placeholders) MUST be completed BEFORE Task 4 (remove guard). If the guard is removed first, `backlog` and `review` stories would flow into the old single-path `process_story()` and launch dev sessions for stories with no spec file.

- [x] Task 1: Extract existing `process_story()` body into `run_dev_pipeline()` (AC: #4)
  - [x] 1.1 Create a new private method on `StoryPipeline`:
    ```rust
    /// Run the full dev pipeline: session → push → PR → review → mark done → notify.
    ///
    /// This is the existing `process_story()` behavior, extracted verbatim.
    async fn run_dev_pipeline(
        &self,
        story: &StoryInfo,
        story_title: &str,
        base_branch_override: Option<&str>,
    ) -> PipelineResult
    ```
  - [x] 1.2 Move the ENTIRE body of the current `process_story()` — from the `session_runner.run()` call through all SessionOutcome match arms and their complete flows (push, PR, review, mark done, notify) — into `run_dev_pipeline()`. Preserve every line of logic, error handling, logging, and UI events EXACTLY. The `StorySubAgentCleanup` guard, `story_title_from_label()` call, `ui.story_start()` call, and the initial `tracing::info!` log remain in `process_story()`.
  - [x] 1.3 `run_dev_pipeline()` should contain the entire `match session_outcome { ... }` block, including all three arms (Completed, Escalated, Failed) with their full error handling, PR creation, review, mark-done, and notification logic.

- [x] Task 2: Refactor `process_story()` into a status router (AC: #1, #2)
  - [x] 2.1 Replace the current inlined body of `process_story()` with a status-based router:
    ```rust
    pub async fn process_story(
        &self,
        story: &StoryInfo,
        base_branch_override: Option<&str>,
    ) -> PipelineResult {
        let _sub_agent_cleanup = StorySubAgentCleanup {
            sessions: &self.sub_agent_sessions,
            in_flight: &self.sub_agent_in_flight,
        };

        let story_title = story_title_from_label(&story.label);
        self.ui.story_start(&story.story_key, &story_title);

        tracing::info!(
            action = "pipeline_start",
            story_key = %story.story_key,
            story_id = %story.story_id,
            status = %story.status,
            "Starting pipeline for story"
        );

        match story.status.as_str() {
            "backlog" => self.run_create_pipeline(story, &story_title, base_branch_override).await,
            "ready-for-dev" => self.run_dev_pipeline(story, &story_title, base_branch_override).await,
            "review" => self.run_review_pipeline(story, &story_title).await,
            other => {
                tracing::error!(
                    action = "unexpected_status",
                    story_key = %story.story_key,
                    status = %other,
                    "Unexpected status in process_story — should not reach pipeline"
                );
                let result = PipelineResult {
                    story_key: story.story_key.clone(),
                    status: StoryStatus::Error,
                    pr_url: None,
                    error_detail: Some(format!(
                        "Unexpected status '{other}' in process_story — should not reach pipeline"
                    )),
                    fatal: false,
                };
                self.notify_story_result(&result).await;
                result
            }
        }
    }
    ```
  - [x] 2.2 Add the `status = %story.status` field to the existing `tracing::info!` log at pipeline start (it currently logs `story_key` and `story_id` but not `status` — now relevant since routing depends on it).

- [x] Task 3: Add placeholder pipeline methods (AC: #1, #2)
  - [x] 3.1 Add `run_create_pipeline()` placeholder:
    ```rust
    /// Placeholder for the create-story pipeline phase.
    ///
    /// Story 13.4 will implement: create-story session → adversarial consultation →
    /// critic consultation → commit. On success, continues to dev phase.
    async fn run_create_pipeline(
        &self,
        story: &StoryInfo,
        _story_title: &str,
        _base_branch_override: Option<&str>,
    ) -> PipelineResult {
        tracing::warn!(
            action = "create_phase_not_implemented",
            story_key = %story.story_key,
            "Create-story phase not yet implemented (Story 13.4) — skipping story"
        );
        self.ui.story_error(
            &story.story_key,
            "Create-story phase not yet implemented (Story 13.4)",
        );
        let result = PipelineResult {
            story_key: story.story_key.clone(),
            status: StoryStatus::Error,
            pr_url: None,
            error_detail: Some(
                "Create-story phase not yet implemented (Story 13.4)".to_string(),
            ),
            fatal: false,
        };
        self.notify_story_result(&result).await;
        result
    }
    ```
  - [x] 4.2 Add `run_review_pipeline()` placeholder:
    ```rust
    /// Placeholder for the code-review pipeline phase.
    ///
    /// Story 13.6 will implement: code-review session → optional critic consultation →
    /// push → PR → notify. Handles `review` status stories (resumed after crash or
    /// entering directly from watcher).
    async fn run_review_pipeline(
        &self,
        story: &StoryInfo,
        _story_title: &str,
    ) -> PipelineResult {
        tracing::warn!(
            action = "review_phase_not_implemented",
            story_key = %story.story_key,
            "Code-review phase not yet implemented (Story 13.6) — skipping story"
        );
        self.ui.story_error(
            &story.story_key,
            "Code-review phase not yet implemented (Story 13.6)",
        );
        let result = PipelineResult {
            story_key: story.story_key.clone(),
            status: StoryStatus::Error,
            pr_url: None,
            error_detail: Some(
                "Code-review phase not yet implemented (Story 13.6)".to_string(),
            ),
            fatal: false,
        };
        self.notify_story_result(&result).await;
        result
    }
    ```
  - [x] 3.3 Both placeholders are non-fatal (`fatal: false`) — the pipeline continues to the next story. They log `tracing::warn!` (not error) since this is an expected temporary state.
  - [x] 3.4 Both placeholders MUST call `self.notify_story_result(&result).await` before returning — every exit path from `process_story()` must send a notification to the operator. Without this, placeholder stories would be silently swallowed with no Telegram notification.

- [x] Task 4: Remove `guard_processable_stories()` and its call sites (AC: #3)
  - [x] 4.1 Delete the `guard_processable_stories()` function (lines 1642-1669 in `src/pipeline.rs`)
  - [x] 4.2 In `process_eligible_stories()` line 921, change:
    ```rust
    let mut current_stories = guard_processable_stories(stories);
    ```
    to:
    ```rust
    let mut current_stories = stories;
    ```
  - [x] 4.3 In `process_eligible_stories()` line 1026, change:
    ```rust
    current_stories = guard_processable_stories(fresh_stories);
    ```
    to:
    ```rust
    current_stories = fresh_stories;
    ```
  - [x] 4.4 Remove the two guard-related tests:
    - `test_guard_processable_stories_filters_non_ready_for_dev`
    - `test_guard_processable_stories_returns_empty_when_all_non_ready`
  - [x] 4.5 **Note:** After removing the guard, `batch_start(current_stories.len())` (line 922) will report the unfiltered count including `backlog`/`review` stories. This is correct — the batch processes all stories, and placeholder errors will be reflected in the summary ("3 processed, 1 completed, 0 blocked, 2 errored"). Do NOT add filtering logic to adjust the count.

- [x] Task 5: Update and add tests (AC: #6)
  - [x] 5.1 Remove the two guard tests that are no longer relevant:
    - `test_guard_processable_stories_filters_non_ready_for_dev`
    - `test_guard_processable_stories_returns_empty_when_all_non_ready`
  - [x] 5.2 Add routing tests. These test `process_story()` routing logic by verifying the returned `PipelineResult` based on story status. Since `process_story()` is an `async fn` on `StoryPipeline` which requires real `SessionRunner`/`ReviewRunner`, and the project does not mock LLM providers in unit tests (only in integration tests), the routing tests should focus on the placeholder paths that DON'T invoke real sessions:
    - `test_process_story_routes_backlog_returns_placeholder` — create a `StoryInfo` with status `"backlog"`, call `process_story()`, verify result has `StoryStatus::Error` and `error_detail` contains "Story 13.4"
    - `test_process_story_routes_review_returns_placeholder` — same for `"review"`, verify "Story 13.6"
    - `test_process_story_routes_unexpected_status` — status `"done"`, verify error with "Unexpected status"
  - [x] 5.3 If constructing `StoryPipeline` for tests is too complex (requires `GitProvider`, `Notifier`, `SessionRunner`, `ReviewRunner`), use one of these strategies:
    - **Strategy A (preferred):** Extract routing logic into a standalone function `route_story_status(status: &str) -> StoryPhase` where `StoryPhase` is a simple enum (`Create`, `Dev`, `Review`, `Unknown`), and test that function directly. Then `process_story()` uses this function for routing. This separates routing logic from pipeline infrastructure. NOTE: `StoryPhase` is a pure routing discriminant — it has no state transition logic, no methods, no associated data beyond the variant name. This is NOT the "PipelinePhase enum with state transition logic" forbidden in the anti-patterns.
    - **Strategy B:** Create a minimal `StoryPipeline` with mock/noop components for test infrastructure. This is more heavyweight but tests the actual method.
  - [x] 5.4 Verify all existing pipeline tests pass — the `ready-for-dev` flow is unchanged, only extracted to `run_dev_pipeline()`.
  - [x] 5.5 `cargo build` — zero new warnings
  - [x] 5.6 `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` — clean (no new errors from this story)
  - [x] 5.7 `cargo test` — 1163 passed, 1 pre-existing failure (unchanged)

- [x] Task 6: Verify end-to-end flow (AC: #5)
  - [x] 6.1 Verify that `process_eligible_stories()` now receives all three status types from the watcher (no guard filtering)
  - [x] 6.2 Verify that `backlog` stories hit the create placeholder, send notification, and continue to next story (non-fatal)
  - [x] 6.3 Verify that `ready-for-dev` stories follow the EXACT same flow as before this refactor
  - [x] 6.4 Verify that `review` stories hit the review placeholder, send notification, and continue to next story (non-fatal)
  - [x] 6.5 Verify that `test_process_story_installs_cleanup_guard_source_check` (line 3632) still passes — it does `include_str!` source inspection of `process_story()` and `recover_and_process()` for the `StorySubAgentCleanup` guard

## Dev Notes

### Architecture Compliance

- **Decision 2 (Daemon Reads, Agent Writes):** Unchanged. The pipeline remains a read-only consumer of `sprint-status.yaml` for routing. The agent writes status transitions during its session.
- **Decision 10 (Daemon-Orchestrated Consultations):** This story creates the structural foundation for consultations by establishing the multi-phase router. Story 13.3 implements the actual consultation mechanism. Story 13.4/13.6 wire consultations into the create/review phases.
- **Decision 3 (WAL):** WAL changes are deferred to Story 13.10. This story does not modify `SessionState` or WAL persistence.
- **Error handling pattern:** Per-module `thiserror` enums. `PipelineError` variants unchanged. Placeholder methods return `PipelineResult` (not `PipelineError`) because they are non-fatal pipeline results, not errors.

### Critical Implementation Details

**This is primarily a REFACTOR, not new functionality.** The key change is:
1. Remove the guard that filters to `ready-for-dev` only
2. Extract the existing `process_story()` body into `run_dev_pipeline()`
3. Add a status-based router in `process_story()`
4. Add placeholder methods for create and review phases

**The `ready-for-dev` path MUST remain byte-for-byte identical in behavior.** The only difference is that it's now in `run_dev_pipeline()` instead of inlined in `process_story()`. Test this by verifying existing tests pass unchanged.

**Placeholder methods are intentionally simple.** They return `PipelineResult` with `StoryStatus::Error` and a clear message. They are NOT empty stubs — they log via `tracing::warn!`, emit UI events via `self.ui.story_error()`, send a notification to the operator via `self.notify_story_result()`, and return a proper result so the pipeline continues to the next story. **Every exit path from `process_story()` MUST call `notify_story_result()`** — this is an existing contract (all current Completed/Escalated/Failed arms do it). The placeholders and the unexpected-status fallback must respect this contract.

**`run_create_pipeline()` signature includes `base_branch_override`** because Story 13.4 will need it when the create phase succeeds and chains into the dev phase. The placeholder ignores it.

**`run_review_pipeline()` does NOT take `base_branch_override`** because review-phase stories already have their branch created during the dev phase. The branch name is in `StoryInfo.branch_name`.

**Why not a state machine enum?** The AC says "state machine" but the implementation is a simple `match` on the story status string. An explicit `PipelinePhase` enum would add complexity without benefit — the story status string IS the phase identifier, and the watcher already provides it. A `route_story_status()` helper function (Task 5.3 Strategy A) provides type safety for routing without adding a state machine abstraction.

### `process_recovered_session()` — Parallel Post-Session Logic (NOT Modified)

`process_recovered_session()` (lines 1851-2130+) is a separate ~280-line method that handles post-recovery session outcomes (Completed/Escalated/Failed) with its own push → PR → review → notify logic. It overlaps significantly with what is being extracted into `run_dev_pipeline()`. **This story does NOT refactor `process_recovered_session()`.** Rationale: `process_recovered_session()` handles WAL-recovered sessions which have different entry conditions (no branch creation, different base branch logic, recovery-specific logging). Consolidating both into a shared method would require abstracting the differences, which is out of scope for a routing refactor. The duplication is acknowledged and can be addressed as a follow-up optimization when Story 13.10 (WAL pipeline phase tracking) changes how recovery interacts with the multi-phase pipeline.

### Sequential Multi-Phase Flow (Future Structure)

When Stories 13.4, 13.5, and 13.6 are implemented, the `backlog` branch in `process_story()` will become:

```rust
"backlog" => {
    // Phase 1: Create story (13.4)
    let create_result = self.run_create_pipeline(story, &story_title, base_branch_override).await;
    if create_result.status != StoryStatus::Completed {
        return create_result;
    }
    // Phase 2: Dev story (13.5) — story is now ready-for-dev
    let dev_result = self.run_dev_pipeline(story, &story_title, base_branch_override).await;
    if dev_result.status != StoryStatus::Completed {
        return dev_result;
    }
    // Phase 3: Code review (13.6) — story is now in review
    self.run_review_pipeline(story, &story_title).await
}
```

This story establishes the router structure to enable this. The actual sequential chaining happens when the phase implementations land.

**Stale `StoryInfo.status` concern:** When chaining phases for a `backlog` story (create → dev → review), the `story` object still carries `status: "backlog"` throughout — it's a snapshot from the watcher. `run_dev_pipeline()` and `run_review_pipeline()` MUST NOT inspect `story.status` for routing decisions; they are called explicitly by the router and should assume they are in the correct phase. Stories 13.4/13.5/13.6 must be aware of this: they receive a `StoryInfo` whose `status` field may not reflect the current pipeline phase.

### `processed_keys` Interaction with Placeholder Errors

When a `backlog` or `review` story hits a placeholder and returns `StoryStatus::Error`, it is added to `processed_keys` in `process_eligible_stories()`. After re-poll, if the story is still in the same status (nothing changed it), it appears in the fresh eligible list but is SKIPPED by `processed_keys`. This is correct and intended — the story should not be retried in the same run (it would hit the same placeholder). It will be retried in the next polling cycle (5 minutes). Do NOT "fix" this by allowing retries within the same run.

### Files to Modify

| File | Change Type | Scope |
|---|---|---|
| `src/pipeline.rs` | **Modify** | Remove `guard_processable_stories()`; extract `run_dev_pipeline()`; add router in `process_story()`; add `run_create_pipeline()` and `run_review_pipeline()` placeholders; update 2 call sites in `process_eligible_stories()`; remove 2 tests, add 3-4 routing tests |

**NOT modified:**
- `process_recovered_session()` (lines 1851-2130+ in `src/pipeline.rs`) — Parallel post-session pipeline for WAL-recovered sessions. Duplicates some post-session logic but has different entry conditions. Consciously deferred to Story 13.10 when WAL gets pipeline phase tracking.
- `src/session/` — No session changes; session runner called identically from `run_dev_pipeline()`
- `src/review/` — No review changes; review runner called identically from `run_dev_pipeline()`
- `src/watcher/` — No watcher changes; all three statuses already flow through from Story 13.1
- `src/session/state.rs` — No WAL changes; deferred to Story 13.10
- `src/llm/agent_factory.rs` — No new LlmRole; `Critic` role is Story 13.9
- `src/config/` — No config changes
- `Cargo.toml` — No new dependencies

### Existing Code to Reuse

- `StoryPipeline.session_runner.run()` — Called identically from `run_dev_pipeline()` as it was from `process_story()`.
- `StoryPipeline.review_runner.run()` — Called identically from `run_dev_pipeline()`.
- `push_branch()`, `notify_story_result()` — Called identically from `run_dev_pipeline()`.
- `build_pr_title()`, `build_pr_description()` — Called identically.
- `StorySubAgentCleanup` — Remains in `process_story()` (RAII guard, wraps the entire processing).
- `story_title_from_label()` — Called in `process_story()`, passed to sub-methods.
- `re_poll_eligible()` — Unchanged, still called in `process_eligible_stories()`.
- `StoryInfo.status` — Already populated by the watcher with `backlog`, `ready-for-dev`, or `review`.

### Anti-Patterns to Avoid

- **DO NOT** change the behavior of the `ready-for-dev` pipeline path. This is a pure refactor — extract, don't modify. The existing Completed/Escalated/Failed handling, PR creation, review, mark-done, notification logic MUST be identical.
- **DO NOT** add WAL pipeline_phase tracking — that is Story 13.10.
- **DO NOT** add consultation mechanism -- that is Story 13.3.
- **DO NOT** implement the create-story session — that is Story 13.4.
- **DO NOT** implement the review-phase session — that is Story 13.6.
- **DO NOT** add a new `LlmRole::Critic` or `LlmRole::Create` — those are Story 13.9.
- **DO NOT** modify `SessionRunner` or `ReviewRunner` — they are consumed identically from `run_dev_pipeline()`.
- **DO NOT** add an explicit `PipelinePhase` enum with state transition logic, methods, or associated data — a simple `match` on the status string is sufficient. Exception: a pure routing discriminant enum (`StoryPhase { Create, Dev, Review, Unknown }`) with no methods is acceptable if used for testability (see Task 5.3 Strategy A). The watcher provides the status; the pipeline routes on it.
- **DO NOT** make placeholder methods return `StoryStatus::Completed` — they must return `Error` so that the story is NOT falsely marked as done. The pipeline continues to the next story (non-fatal).
- **DO** prefix unused parameters in placeholder methods with `_` (e.g., `_story_title`, `_base_branch_override`) — standard Rust convention for intentionally unused parameters that will be used by future implementations.
- **DO NOT** apply the `StorySubAgentCleanup` RAII guard inside `run_dev_pipeline()` — it MUST stay in `process_story()` so it covers ALL phase methods (including future create/review). Moving it would break cleanup for future phases.

### Previous Story Intelligence (Story 13.1)

- **Baseline test count:** 1162 passing, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)
- **Pre-existing clippy allowances:** `-A clippy::needless_splitn -A clippy::unnecessary_map_or`
- **Guard function location:** `src/pipeline.rs` lines 1642-1669 — this entire function is deleted
- **Guard call sites:** Line 921 (initial stories) and line 1026 (re-polled stories)
- **Guard tests to remove:** `test_guard_processable_stories_filters_non_ready_for_dev` and `test_guard_processable_stories_returns_empty_when_all_non_ready`
- **Key learning from 13.1:** The backward-compat guard was placed at two consumption points (initial list + re-poll). Similarly, the guard removal must update BOTH points.
- **Story 13.1 introduced `status_priority()` in deps.rs** — review > ready-for-dev > backlog. After removing the guard, all three priorities flow through. The pipeline should process them in this order naturally (the watcher/deps already sort them).
- **Brittle source-check test:** `test_process_story_installs_cleanup_guard_source_check` (line 3632) does `include_str!("pipeline.rs")` and searches for string patterns in `process_story()` and `recover_and_process()`. After refactoring, `process_story()` will be shorter (~30 lines) but the `StorySubAgentCleanup` guard stays at the top, so the 2000-char window should still capture it. Verify explicitly after refactoring.
- **`batch_start()` count change:** After removing the guard, `batch_start(current_stories.len())` reports the unfiltered count (all 3 status types). Placeholder errors will appear in the batch summary. This is expected behavior — the batch processes all stories the watcher provides.

### Git Intelligence — Recent Commits

```
fb38013 feat(epic-13): extend watcher to detect backlog and review stories (Story 13.1)
ab07b29 test(epic-12): add skill-based session and spawn-agent integration tests (Story 12.5)
cd7cce9 docs(epic-13): advance epic-13 to in-progress, create story 13-1 spec
a47a720 feat(epic-12): wire SpawnAgentTool universally in dev + review sessions (Story 12.4)
9b2dbdf feat(epic-12): add SpawnAgentTool with review hardening (Story 12.3)
```

**Expected commit message:** `feat(epic-13): refactor pipeline into status-based phase router (Story 13.2)`

### Project Structure Notes

- Changes are confined to `src/pipeline.rs` — single file refactor
- No new modules, no new files, no new dependencies
- The refactored code follows existing patterns in pipeline.rs (error handling, UI events, tracing)

### References

- [Source: _bmad-output/planning-artifacts/epics.md:3165–3189 — Story 13.2 AC (Pipeline Orchestrator Refonte)]
- [Source: _bmad-output/planning-artifacts/epics.md:3480–3503 — Epic 13 Summary and execution strategy]
- [Source: _bmad-output/planning-artifacts/architecture.md:191–218 — Decision 1 (Supervisor Interception / Chat Loop + Amendment)]
- [Source: _bmad-output/planning-artifacts/architecture.md:221–238 — Decision 2 (Daemon Reads, Agent Writes)]
- [Source: _bmad-output/planning-artifacts/architecture.md:240–298 — Decision 3 (WAL + Amendment for pipeline_phase)]
- [Source: _bmad-output/planning-artifacts/architecture.md:664–693 — Decision 10 (Daemon-Orchestrated Consultations)]
- [Source: _bmad-output/planning-artifacts/architecture.md:695–716 — Decision 11 (Story Critic)]
- [Source: _bmad-output/planning-artifacts/architecture.md:1096–1169 — Project Structure]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md:263 — Story 13.2 description]
- [Source: _bmad-output/project-context.md:62–68 — Daemon Lifecycle (sequential execution)]
- [Source: _bmad-output/project-context.md:109–117 — Testing Rules]
- [Source: src/pipeline.rs:50–103 — PipelineError enum]
- [Source: src/pipeline.rs:110–125 — PipelineResult struct]
- [Source: src/pipeline.rs:135–156 — StoryPipeline struct]
- [Source: src/pipeline.rs:317–876 — Current process_story() implementation]
- [Source: src/pipeline.rs:898–1058 — process_eligible_stories() implementation]
- [Source: src/pipeline.rs:921 — guard call site 1 (initial stories)]
- [Source: src/pipeline.rs:1026 — guard call site 2 (re-polled stories)]
- [Source: src/pipeline.rs:1642–1669 — guard_processable_stories() function (TO DELETE)]
- [Source: src/pipeline.rs:1671–1694 — re_poll_eligible() (unchanged)]
- [Source: src/pipeline.rs:1785 — notify_story_result() method]
- [Source: src/pipeline.rs:1817–1846 — recover_and_process() (NOT modified — parallel post-session pipeline)]
- [Source: src/pipeline.rs:1851–2130 — process_recovered_session() (NOT modified — duplicates post-session logic)]
- [Source: src/pipeline.rs:3632–3658 — test_process_story_installs_cleanup_guard_source_check (brittle source-check test — must still pass)]
- [Source: src/pipeline.rs:3663–3682 — guard tests to remove]
- [Source: src/session/mod.rs:100–133 — SessionOutcome enum]
- [Source: src/session/runner.rs:334–337 — SessionRunner.skill_path field]
- [Source: src/session/state.rs:82–111 — SessionState struct (NOT modified in this story)]
- [Source: src/watcher/deps.rs — status_priority() sorts review > ready-for-dev > backlog]
- [Source: _bmad-output/implementation-artifacts/13-1-watcher-backlog-stories-extension.md — Previous story intelligence]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

### Completion Notes List

- Extracted existing `process_story()` body (~540 lines) into `run_dev_pipeline()` — pure move, zero behavior change
- Transformed `process_story()` into a thin router (~30 lines) using `route_story_status()` + `StoryPhase` enum (Strategy A from Task 5.3)
- Added `run_create_pipeline()` placeholder returning `StoryStatus::Error` with "Story 13.4" message, with notification
- Added `run_review_pipeline()` placeholder returning `StoryStatus::Error` with "Story 13.6" message, with notification
- Added `StoryPhase` enum (pure routing discriminant) and `route_story_status()` standalone function
- Removed `guard_processable_stories()` function and both call sites (line 921 + 1026)
- Removed 2 guard tests, added 3 routing tests (`test_route_story_status_returns_correct_phase`, `test_route_story_status_backlog_maps_to_create`, `test_route_story_status_unexpected_maps_to_unknown`)
- Added `status` field to `tracing::info!` at pipeline start
- All 1163 tests pass (1 pre-existing failure unchanged)
- `StorySubAgentCleanup` guard remains in `process_story()` — source-check test passes
- All placeholders call `notify_story_result()` and emit `ui.story_error()` — no silent swallowing

### Review Findings

- [ ] [Review][Decision] **Placeholder phases cause notification spam every poll cycle** — After removing `guard_processable_stories()`, `backlog` and `review` stories hit placeholder errors and return `StoryStatus::Error` with `fatal: false`. The story status in `sprint-status.yaml` is never updated, so on the next poll cycle (5 min), the watcher rediscovers the same story as eligible and re-processes it — producing the same error, UI event, and Telegram notification. This repeats indefinitely until Stories 13.4/13.6 are implemented. The old guard silently filtered these out. (Sources: blind+edge)
- [x] [Review][Decision→Patch] **Missing 3 required `process_story`-level integration tests from AC-6** — Fixed: added `test_process_story_routes_backlog_returns_placeholder`, `test_process_story_routes_review_returns_placeholder`, and `test_process_story_routes_unexpected_status` with full `StoryPipeline` construction using mock/noop components. All 3 pass. (Source: auditor, AC-6)
- [x] [Review][Patch] **Missing `ui.story_error()` in `Unknown` arm of `process_story()` router** — Fixed: added `self.ui.story_error()` call in the `Unknown` arm before `notify_story_result()`, consistent with placeholder methods. [src/pipeline.rs:~353] (Source: blind)
- [x] [Review][Defer] **`review` stories prioritized first by `status_priority()`, guaranteed to fail with placeholder** [src/watcher/deps.rs] — deferred, by-design ordering for target state; transient annoyance resolves when Story 13.6 lands
- [x] [Review][Defer] **Duplicated status string literals between `route_story_status` and `is_eligible`** [src/pipeline.rs + src/watcher/mod.rs] — deferred, pre-existing pattern from Story 13.1

### Change Log

- 2026-04-22: Story 13.2 implemented — pipeline refactored into status-based phase router with create/review placeholders

### File List

- `src/pipeline.rs` — Modified: extracted `run_dev_pipeline()`, added router in `process_story()`, added `run_create_pipeline()` and `run_review_pipeline()` placeholders, added `StoryPhase` enum and `route_story_status()` function, removed `guard_processable_stories()` and its call sites, removed 2 guard tests, added 3 routing tests
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — Modified: story 13-2 status updated to in-progress
- `_bmad-output/implementation-artifacts/13-2-pipeline-orchestrator-refonte.md` — Modified: task checkboxes, dev agent record, file list, change log, status
