# Story 13.5: Dev-Story Phase

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the dev-story pipeline phase to run a `bmad-dev-story` session and separate the dev phase from the review phase,
So that the validated story is implemented autonomously and the pipeline follows the multi-phase architecture (create → dev → review).

## Acceptance Criteria

1. **AC-1: Dev-story session with correct skill activation**
   - **Given** a story with status `ready-for-dev` enters the dev-story phase
   - **When** `run_dev_pipeline()` executes
   - **Then** a fresh agent session is created, activated with `.claude/skills/bmad-dev-story/SKILL.md`
   - **And** the session follows the existing session runner flow: branch creation/checkout, streaming chat loop, tool calls, completion detection
   - **And** the `ask_supervisor` tool is registered and available (3-tier cascade: rules → architect → escalation)
   - **And** the `spawn_agent` tool is registered and available
   - **Note:** The current `skill_path` in `SessionRunner::new()` is `.github/skills/bmad-dev-story/SKILL.md` (wrong path — the actual BMAD skills are installed at `.claude/skills/`). This must be fixed.

2. **AC-2: Dev phase stops after PR creation — does not run code review**
   - **Given** the dev-story session completes successfully (`SessionOutcome::Completed`)
   - **When** `run_dev_pipeline()` processes the completion
   - **Then** the dev pipeline handles: push branch → create PR → mark story `review` in sprint-status.yaml → chain to `run_review_pipeline()`
   - **And** the dev pipeline does NOT run code review (Phase 4-8 of the old implementation move to `run_review_pipeline()`)
   - **And** after marking `review`, the pipeline re-reads story info via `reload_story_info()` (same pattern as create→dev in Story 13.4) and chains to `run_review_pipeline()`
   - **And** `run_review_pipeline()` receives the updated `StoryInfo`, `story_title`, `branch`, and `pr_info` needed to continue
   - **And** the Phase 8 notification is NOT emitted by `run_dev_pipeline()` — `run_review_pipeline()` owns the final notification

3. **AC-3: Review pipeline upgraded from placeholder**
   - **Given** `run_review_pipeline()` is currently a placeholder returning an error
   - **When** this story is implemented
   - **Then** `run_review_pipeline()` handles the existing code review flow: run review (respecting `code_review_enabled` config) → push review commits → post review comment → mark `done` → unblock dependents → commit sprint-status → push → notify
   - **And** the review behavior is extracted from the old `run_dev_pipeline()` Phases 4-8, preserving every line of logic
   - **And** the review comment is posted WITHOUT `strip_agent_artifacts()` (matching the `run_dev_pipeline()` behavior at line 858, not the `process_recovered_session()` behavior)
   - **And** `run_review_pipeline()` also handles stories entering directly at `review` status (from watcher)
   - **And** when entering from watcher (no `pr_info` available), the review pipeline pushes the branch, creates a minimal PR (no session-outcome data available — use a generic description), then continues with the review flow

4. **AC-4: Dev-story session outcome preserved**
   - **Given** the dev-story session completes successfully
   - **When** the agent signals `<<BMAD_JOB_DONE>>`
   - **Then** the session outcome includes: branch name, decisions log, PR context, test results
   - **And** any post-implementation impact analysis runs as before (Story 4.6 behavior preserved — this is inside `SessionRunner::run_session()`, not touched)

5. **AC-5: Failure and escalation handling unchanged**
   - **Given** the dev-story session escalates or fails
   - **When** the session outcome is `Escalated` or `Failed`
   - **Then** the pipeline handles it identically to the current behavior: partial PR for failures, `needs-clarification` status for escalations, notification sent
   - **And** no chaining to review phase occurs on failure/escalation

6. **AC-6: Push-failure path marks story as `review` before returning**
   - **Given** the dev session completes successfully but `push_branch()` fails (Phase 2)
   - **When** `run_dev_pipeline()` hits the push-failure early return (lines 730-745)
   - **Then** the pipeline marks the story as `review` in sprint-status.yaml before returning
   - **And** this ensures the next watcher poll routes the story to `run_review_pipeline()` (which will retry push + PR) instead of re-running the entire dev session

7. **AC-7: Fix all `.github/skills/` path references to `.claude/skills/`**
   - **Given** the codebase has several references to `.github/skills/` paths
   - **When** this story is implemented
   - **Then** `SessionRunner::new()` uses `.claude/skills/bmad-dev-story/SKILL.md` (at `src/session/runner.rs:384`)
   - **And** `ReviewRunner` uses `.claude/skills/bmad-code-review/SKILL.md` (at `src/review/mod.rs:575`)
   - **And** doc comments in `session/agent.rs` (lines 12, 823, 835) are updated to reference `.claude/skills/`
   - **And** doc comments in `llm/agent_factory.rs` (line 116) are updated to reference `.claude/skills/`
   - **And** test assertions in `pipeline.rs` (line 4403) are updated to reference `.claude/skills/`

8. **AC-8: `process_recovered_session()` refactored to use `run_review_pipeline()`**
   - **Given** `process_recovered_session()` at `src/pipeline.rs:2290` contains a duplicate copy of the review→PR→mark-done flow
   - **When** this story is implemented
   - **Then** `process_recovered_session()`'s `Completed` arm is refactored to: push → PR → mark `review` → chain to `run_review_pipeline()` (same pattern as `run_dev_pipeline()`)
   - **And** the duplicated review/done logic is removed from `process_recovered_session()`
   - **And** crash recovery stories follow the same review pipeline as normal stories

9. **AC-9: Tests**
   - **Given** the dev-story pipeline phase refactoring
   - **When** this story is implemented
   - **Then** existing routing tests (`test_route_story_status_*`) pass unchanged
   - **And** existing `test_build_create_story_consultations` and related tests pass unchanged
   - **And** `test_process_story_routes_review_returns_placeholder` is updated or removed (review is no longer a placeholder)
   - **And** `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` passes
   - **And** `cargo test` passes with no new failures beyond the pre-existing `test_build_context_limit_recovery_message_contains_all_sections`

## Tasks / Subtasks

- [x] Task 1: Fix all `.github/skills/` path references to `.claude/skills/` (AC: #1, #7)
  - [x] 1.1 Update `SessionRunner::new()` skill_path from `".github/skills/bmad-dev-story/SKILL.md"` to `".claude/skills/bmad-dev-story/SKILL.md"` at `src/session/runner.rs:384`
  - [x] 1.2 Update `ReviewRunner` skill_path from `".github/skills/bmad-code-review/SKILL.md"` to `".claude/skills/bmad-code-review/SKILL.md"` at `src/review/mod.rs:575`
  - [x] 1.3 Update doc comments in `src/session/agent.rs` (lines 12, 823, 835) — change `.github/skills/` references to `.claude/skills/`
  - [x] 1.4 Update doc comment in `src/llm/agent_factory.rs` (line 116) — change `.github/skills/` to `.claude/skills/`
  - [x] 1.5 Update test assertion in `src/pipeline.rs` (line 4403) — change `".github/skills/bmad-dev-story/SKILL.md"` to `".claude/skills/bmad-dev-story/SKILL.md"`

- [x] Task 2: Implement `run_review_pipeline()` — replace placeholder with extracted review logic (AC: #3)
  - [x] 2.1 Update `run_review_pipeline()` signature to accept the data it needs. The method handles two entry points:
    - **Chained from dev:** receives `pr_info` (PR already created) and `branch` from `run_dev_pipeline()`
    - **Direct from watcher:** story has `review` status but no `pr_info` — needs to push branch and create a PR
    New signature:
    ```rust
    async fn run_review_pipeline(
        &self,
        story: &StoryInfo,
        story_title: &str,
        branch_override: Option<&str>,
        pr_info_override: Option<PrInfo>,
    ) -> PipelineResult
    ```
    **Note:** No `decisions_override` parameter — decisions are consumed during PR creation (Phase 3 in dev pipeline) and are NOT used by the review flow (Phases 4-8). The extracted review code never touches decisions.
  - [x] 2.2 Update the `process_story()` router call site at line 353 to pass the new parameters:
    ```rust
    StoryPhase::Review => {
        self.run_review_pipeline(story, &story_title, None, None).await
    }
    ```
  - [x] 2.3 Implement the "chained from dev" path (when `pr_info_override` is `Some`): skip push/PR creation, go straight to code review. The review code block is moved verbatim from `run_dev_pipeline()` Phases 4-8 (lines 803-978):
    - Phase 4: Code review — **MUST respect `self.config.code_review_enabled`** guard (currently at line 804). When disabled, skip review entirely.
    - Phase 5: Push review fix commits
    - Phase 6: Post review comment on PR — use `self.git_provider.add_comment(&pr_info.id, report)` WITHOUT `strip_agent_artifacts()` (matching `run_dev_pipeline()` behavior at line 858)
    - Phase 7: Mark `done` in sprint-status, unblock dependents, commit, push
    - Phase 8: Notify + `ui.story_complete()`
  - [x] 2.4 Implement the "direct from watcher" path (when `pr_info_override` is `None`): the review pipeline must first:
    - Resolve branch name from `branch_override.unwrap_or(&story.branch_name)`
    - Push the branch to remote (best-effort)
    - Create a PR with a minimal description (no session-outcome context available):
      ```rust
      let pr_title = build_pr_title(&story.story_key, story_title, false);
      let pr_body = build_pr_description(&PrDescriptionParams {
          story_key: story.story_key.clone(),
          story_title: story_title.to_string(),
          outcome_summary: "resuming from review status".to_string(),
          decisions_section: String::new(),
          failure_details: None,
          pr_summary: None,
      });
      ```
    - On push/PR failure: return `PipelineResult` with `StoryStatus::Error`, non-fatal
    - On success: continue to code review with the new `pr_info` (same as chained path)
  - [x] 2.5 Handle push/PR failure in watcher path with appropriate error result and notification — same pattern as `run_dev_pipeline()` push-failure path

- [x] Task 3: Refactor `run_dev_pipeline()` completion path to stop after PR creation (AC: #2, #4)
  - [x] 3.1 In the `SessionOutcome::Completed` arm of `run_dev_pipeline()`, after PR creation (Phase 3):
    - Mark story as `review` in sprint-status.yaml via `update_story_status()`
    - Re-read story info via `reload_story_info()` to get updated `StoryInfo` with `status: "review"` (same pattern as create→dev in Story 13.4, prevents passing stale status)
    - Chain to `run_review_pipeline()` with the updated story, pr_info, and branch:
    ```rust
    // Mark story as "review" in sprint-status.yaml
    let sprint_status_path = Path::new(&self.config.bmad_paths.implementation_artifacts)
        .join("sprint-status.yaml");
    if sprint_status_path.exists() {
        if let Err(e) = update_story_status(&sprint_status_path, &story_key, "review").await {
            tracing::warn!(
                action = "sprint_status_update_failed",
                story_key = %story_key,
                error = %e,
                "Failed to mark story as review in sprint-status.yaml"
            );
        }
    }

    // Re-read story info with updated status (same pattern as create→dev in 13.4)
    let updated_story = match self.reload_story_info(&story.story_key) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                action = "dev_phase_reload_failed",
                error = %e,
                "Failed to re-read story info — using original with updated status"
            );
            let mut fallback = story.clone();
            fallback.status = "review".to_string();
            fallback
        }
    };

    // Chain to review phase
    self.run_review_pipeline(&updated_story, story_title, Some(&branch), Some(pr_info))
        .await
    ```
  - [x] 3.2 Remove the old Phase 4-8 code from `run_dev_pipeline()` Completed arm (code review, push review commits, post review comment, mark done + unblock + commit + push, AND notification). The Phase 8 notification moves to `run_review_pipeline()` — `run_dev_pipeline()` does NOT emit a notification on success (the review pipeline handles it).
  - [x] 3.3 Update the doc comment on `run_dev_pipeline()` from "Run the full dev pipeline: session → push → PR → review → mark done → notify" to "Run the dev session phase: session → push → PR → mark review → chain to review phase"

- [x] Task 4: Fix push-failure early return to mark `review` status (AC: #6)
  - [x] 4.1 In `run_dev_pipeline()`, the push-failure early return (lines 730-745) currently returns `StoryStatus::Completed` without updating sprint-status. Add `update_story_status()` to mark the story as `review` before returning, so the next watcher poll routes to `run_review_pipeline()`:
    ```rust
    if !push_ok {
        // Mark as "review" so next poll retries via run_review_pipeline()
        let sprint_status_path = Path::new(&self.config.bmad_paths.implementation_artifacts)
            .join("sprint-status.yaml");
        if sprint_status_path.exists() {
            let _ = update_story_status(&sprint_status_path, &story_key, "review").await;
        }

        self.ui.story_error(&story_key, "Push failed — work preserved locally, will retry via review phase");
        let result = PipelineResult {
            story_key: story_key.clone(),
            status: StoryStatus::Completed,
            pr_url: None,
            error_detail: Some(format!("Push failed — work preserved on local branch: {branch}. Story marked review for retry.")),
            fatal: false,
        };
        self.notify_story_result(&result).await;
        return result;
    }
    ```

- [x] Task 5: Refactor `process_recovered_session()` to use `run_review_pipeline()` (AC: #8)
  - [x] 5.1 In `process_recovered_session()` (lines 2290-2440+), refactor the `Completed` arm to follow the same pattern as `run_dev_pipeline()`:
    - Keep: push branch, create PR (these happen before review)
    - Replace: the duplicated review/mark-done/notify logic with a call to `run_review_pipeline()`
    - After PR creation succeeds, mark `review` in sprint-status, then chain:
      ```rust
      self.run_review_pipeline(story, &story_title, Some(&branch), Some(pr_info)).await
      ```
  - [x] 5.2 The `Escalated` and `Failed` arms in `process_recovered_session()` remain unchanged

- [x] Task 6: Verify escalation and failure paths remain unchanged (AC: #5)
  - [x] 6.1 Verify that `SessionOutcome::Escalated` arm in `run_dev_pipeline()` is untouched — same push, PR, notify pattern
  - [x] 6.2 Verify that `SessionOutcome::Failed` arm in `run_dev_pipeline()` is untouched — same infra check, push, PR, notify pattern
  - [x] 6.3 Both arms do NOT chain to review — only `Completed` chains

- [x] Task 7: Update tests (AC: #9)
  - [x] 7.1 Update `test_process_story_routes_review_returns_placeholder` — the review pipeline is no longer a placeholder. Update test expectations to reflect functional review pipeline behavior (or remove if no longer applicable and replace with a test that verifies the review pipeline runs code review)
  - [x] 7.2 Verify `test_route_story_status_*` tests pass unchanged
  - [x] 7.3 Verify `test_build_create_story_consultations` and related tests pass unchanged
  - [x] 7.4 Update any tests that reference `.github/skills/` paths to use `.claude/skills/`
  - [x] 7.5 Run `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` — zero new warnings
  - [x] 7.6 Run `cargo test` — all existing tests pass, no new failures beyond pre-existing `test_build_context_limit_recovery_message_contains_all_sections`

## Dev Notes

### Architecture Compliance

- **Decision 10 (Daemon-Orchestrated Consultations):** The dev-story phase does NOT have consultations — it calls `session_runner.run()` (which delegates to `run_with_consultations()` with empty consultations). No consultation wiring needed for this phase. The consultation mechanism from Story 13.3 is only used by create-story (13.4) and code-review (13.6).
- **Multi-Phase Pipeline Vision (from sprint-change-proposal):** The pipeline flow is: `SESSION CREATE-STORY → SESSION DEV-STORY → SESSION CODE-REVIEW → Push + PR + Notify`. This story aligns `run_dev_pipeline()` with this vision by separating the dev session from the review session.
- **Decision 5 (Skill-Based Activation):** The dev-story session uses `.claude/skills/bmad-dev-story/SKILL.md` — the skill is self-starting, no menu, no commands, no post-activation message. The agent reads the story file, discovers tasks, implements them autonomously.
- **Scope note:** The epic AC for Story 13.5 describes the dev session behavior which already works. This story EXPANDS scope to include: (1) extracting review logic into `run_review_pipeline()` to enable the multi-phase architecture, (2) fixing the pre-existing skill path bug, (3) refactoring `process_recovered_session()` to eliminate code duplication. These are necessary to make the pipeline architecture consistent before Story 13.6 upgrades the review phase.

### Critical Implementation Details

**This story is primarily a REFACTOR — separating the dev phase from the review phase.** The existing `run_dev_pipeline()` currently bundles both dev and review into one method (8 phases). After this story: dev pipeline handles Phases 1-3 (dev session, push, PR) + mark `review` + chain; review pipeline handles Phases 4-8 (code review, push review, comment, mark done, notify).

**Skill path fix is a critical bugfix.** The current `skill_path` at `src/session/runner.rs:384` points to `.github/skills/bmad-dev-story/SKILL.md` but BMAD skills are installed at `.claude/skills/`. This means the dev session currently cannot find the skill file. This is a pre-existing issue documented in Story 13.4's dev notes as a known path discrepancy. Same issue exists for the review runner at `src/review/mod.rs:575`. Both `.claude/skills/bmad-dev-story/SKILL.md` and `.claude/skills/bmad-code-review/SKILL.md` exist and are confirmed present.

**Chaining from dev to review.** On `SessionOutcome::Completed`, the dev pipeline marks the story as `review` in sprint-status.yaml, re-reads story info via `reload_story_info()` (same pattern as create→dev in Story 13.4 — prevents passing stale status), then calls `run_review_pipeline()` directly. The branch is already pushed and the PR already exists. The review pipeline receives the `pr_info` to post the review comment on the existing PR.

**The new `update_story_status(..., "review")` call is a behavioral addition.** The old `run_dev_pipeline()` went straight from PR creation to code review without a sprint-status write. The new flow adds this write between PR and review. This is intentional — it ensures crash recovery routes correctly. However, this write can fail; failure is logged but non-blocking (the review still chains).

**`run_review_pipeline()` handles two entry points:**
1. **Chained from dev (this story):** `pr_info` is provided, branch is already pushed, PR already exists. Skip push/PR creation, go straight to code review.
2. **Direct from watcher (crash recovery or manual `review` status):** No `pr_info` available. Review pipeline must push the branch, create a PR with a minimal description (no session-outcome context — use generic text), then run code review.

Both paths converge at code review → push review commits → post comment → mark done → unblock → commit → push → notify.

**No `decisions_override` parameter.** Decisions from `SessionOutcome::Completed` are consumed during PR creation (Phase 3, `format_pr_decisions_section()`) which stays in `run_dev_pipeline()`. The extracted review code (Phases 4-8) never touches decisions. The "direct from watcher" path has no decisions either. Passing decisions to the review pipeline would be dead weight.

**`code_review_enabled` config flag must be preserved.** The current code at `run_dev_pipeline()` line 804 guards the review with `if self.config.code_review_enabled`. The extracted review pipeline MUST preserve this conditional. When disabled, the review pipeline skips directly to mark-done and notification.

**Review comment posting: no `strip_agent_artifacts()`.** Two inconsistent behaviors exist in the codebase: `run_dev_pipeline()` (line 858) posts the review comment WITHOUT stripping, while `process_recovered_session()` (line 2402) strips with `strip_agent_artifacts()`. The review pipeline follows `run_dev_pipeline()`'s behavior (no stripping), since the comment says "Report is already formatted by build_review_comment — no stripping needed."

**Push-failure early return must mark `review`.** The current push-failure path (lines 730-745) returns `StoryStatus::Completed` without updating sprint-status. This leaves the story in `in-progress` (from the agent's write), causing the next watcher poll to either skip it or re-run the dev session. After this fix, the story is marked `review` before returning, so the next poll routes to `run_review_pipeline()` which will retry push + PR.

**The dev-story agent manages its own sprint-status transitions.** The `bmad-dev-story` skill (Step 4 of `workflow.md`) transitions the story to `in-progress`. Step 9 transitions it to `review`. However, the daemon also updates sprint-status between phases for robustness. The daemon's `review` status write (in `run_dev_pipeline()`) may be redundant if the agent already wrote it, but it's a safety net — `update_story_status()` is idempotent.

**Post-implementation impact analysis is inside `SessionRunner::run_session()`.** The impact analysis (Story 4.6) runs as part of the session's post-completion sequence inside the session runner. It is NOT affected by this refactor — it happens before `SessionOutcome::Completed` is returned to the pipeline.

**`process_recovered_session()` must be refactored.** At `src/pipeline.rs:2290`, `process_recovered_session()` contains a full duplicate of the review→PR→mark-done flow. If left untouched, Story 13.6's review upgrade would need to update TWO locations. This story refactors `process_recovered_session()` to call `run_review_pipeline()` — same push→PR→mark-review→chain pattern used in `run_dev_pipeline()`. This eliminates the duplication and ensures crash recovery uses the same review pipeline as normal execution.

### Interaction with `run_create_pipeline()` Chaining

When create→dev chains (Story 13.4), `run_create_pipeline()` calls `run_dev_pipeline()` with `Some(&branch)` as `base_branch_override`. After this story, `run_dev_pipeline()` will chain to `run_review_pipeline()`, making the full chain: create → dev → review. The story branch is reused throughout. The PR is created once (in the dev phase) and the review comment is posted on that PR.

### Known Limitations

**Review pipeline does not use skill-based sessions yet.** This story moves the EXISTING `review_runner.run()` into `run_review_pipeline()`. Story 13.6 will replace this with a skill-based code review session (`bmad-code-review` SKILL.md) enriched with Critic consultations. This story provides backward compatibility while enabling the multi-phase architecture.

**No WAL phase tracking.** The WAL does not record which pipeline phase is active (Story 13.10). If the daemon crashes after dev completes but before review starts, on recovery the story will be at `review` status and the pipeline will route to `run_review_pipeline()` via the watcher — which is the correct behavior (review phase starts fresh). If the daemon crashes mid-review, the WAL still references the dev session, but Story 13.10 addresses this.

**Sprint-status double-write.** Both the dev-story agent (via `bmad-dev-story/workflow.md` Step 9) and the daemon (in `run_dev_pipeline()`) write `review` status to sprint-status.yaml. This is intentional redundancy — the agent writes it during the session, the daemon writes it after the session completes. The later write wins, and both write the same value.

**Watcher "direct" path PR has no session context.** When `run_review_pipeline()` is called from the watcher (no `pr_info`), it creates a PR with a generic description ("resuming from review status"). No `pr_context`, `pr_how_to_test`, or `decisions_section` is available. This is acceptable — the code is already committed on the branch, and the PR exists primarily to host the review comment and enable merge. Story 13.10 may improve this by persisting PR context in the WAL.

### Previous Story Intelligence (Story 13.4)

- **Baseline test count:** 1183 passing, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)
- **Pre-existing clippy allowances:** `-A clippy::needless_splitn -A clippy::unnecessary_map_or`
- **`run_dev_pipeline()`** is at `src/pipeline.rs:686-1231` — ~545 lines. The Completed arm (lines 698-980) handles dev+review+done. Escalated (982-1083) and Failed (1085-1229) are unchanged.
- **`run_review_pipeline()`** placeholder at `src/pipeline.rs:1238-1263` — 25 lines, returns error.
- **`run_create_pipeline()`** chains to `run_dev_pipeline()` on success at line 445.
- **`process_recovered_session()`** at `src/pipeline.rs:2290-2440+` — contains duplicate review→PR→done flow that MUST be refactored.
- **Skill path discrepancy** documented at Story 13.4 dev notes: `.github/skills/` vs `.claude/skills/`. This story fixes it.
- **Story 13.4 parameterized `SessionRunner`** — `run_with_consultations()` accepts `skill_path_override` and `preamble_override`. Dev sessions pass `None` for both (using defaults).
- **`build_preamble()` is now async** — `async fn build_preamble(&self, _story: &StoryInfo) -> Result<String, ProviderError>` at runner.rs:977
- **`review_runner`** is a field on `StoryPipeline` (line 147) — `ReviewRunner` with its own `run()` method
- **`update_story_status()`** imported from `session::cleanup` — async function that reads/updates sprint-status.yaml
- **`unblock_dependents()`** imported from `session::cleanup` — unblocks dependent stories after marking done
- **`commit_sprint_status()`** — commits sprint-status.yaml changes and pushes
- **`format_pr_decisions_section()`** — formats decisions for PR body — used in Phase 3 (stays in dev pipeline), NOT in review
- **`build_pr_title()`, `build_pr_description()`** — PR description construction helpers
- **`PrSummary`, `PrDescriptionParams`, `CreatePrParams`** — PR-related structs
- **`PrInfo`** — returned by `git_provider.create_pr()`, `#[derive(Debug, Clone)]`, contains `id`, `url`, `number` — Clone is available for ownership transfer
- **`reload_story_info()`** — re-reads sprint-status.yaml and returns updated `StoryInfo` (used in create→dev chaining, reuse for dev→review)
- **`strip_agent_artifacts()`** — strips `<<BMAD_JOB_DONE>>` and other sentinels from text. Used in `process_recovered_session()` line 2402 for review comments but NOT used in `run_dev_pipeline()` line 858. The review pipeline follows `run_dev_pipeline()`'s convention (no stripping).
- **`code_review_enabled`** — config flag at `self.config.code_review_enabled`, checked at line 804 before running code review. MUST be preserved in extracted review pipeline.
- **`process_story()` router** — line 353 calls `self.run_review_pipeline(story, &story_title).await` — call site must be updated for new signature.

### Git Intelligence — Recent Commits

```
5f4a497 feat(epic-13): implement create-story phase with consultations (Story 13.4)
63932ed feat(epic-13): add daemon-orchestrated consultation mechanism (Story 13.3)
147f57d feat(epic-13): refactor pipeline into status-based phase router (Story 13.2)
fb38013 feat(epic-13): extend watcher to detect backlog and review stories (Story 13.1)
ab07b29 test(epic-12): add skill-based session and spawn-agent integration tests (Story 12.5)
```

Files most recently modified in the pipeline area:
- `src/pipeline.rs` — 514 additions in Story 13.4 (create pipeline, consultations, preamble helpers, tests)
- `src/session/runner.rs` — 141 additions in Story 13.4 (parameterized run_with_consultations, skill/preamble overrides, initial message branching)
- `src/session/agent.rs` — 52 additions in Story 13.4 (build_create_preamble)

### Project Structure Notes

- No new files created — this story modifies existing `pipeline.rs`, `runner.rs`, `review/mod.rs`, `session/agent.rs`, `llm/agent_factory.rs`
- The review logic moves from `pipeline.rs:run_dev_pipeline()` to `pipeline.rs:run_review_pipeline()` — same file, different method
- `run_review_pipeline()` signature changes from `(&self, story, _story_title)` to `(&self, story, story_title, branch_override, pr_info_override)`
- `run_dev_pipeline()` shrinks from ~545 lines to ~300 lines (Completed arm loses review/done/notify phases)
- `process_recovered_session()` shrinks significantly — review/done logic replaced by call to `run_review_pipeline()`

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Story 13.5 AC (Dev-Story Phase)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Decision 5 (Skill-Based Activation, amendment)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Decision 3 (WAL, multi-phase amendment)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Decision 10 (Daemon-Orchestrated Consultations)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md — Pipeline model diagram]
- [Source: _bmad-output/project-context.md — Daemon Lifecycle, Agent Construction]
- [Source: src/pipeline.rs:686-1231 — run_dev_pipeline() (to refactor)]
- [Source: src/pipeline.rs:1238-1263 — run_review_pipeline() placeholder (to replace)]
- [Source: src/pipeline.rs:388-681 — run_create_pipeline() (reference for chaining pattern)]
- [Source: src/pipeline.rs:2290-2440+ — process_recovered_session() (to refactor)]
- [Source: src/pipeline.rs:730-745 — push-failure early return (to fix)]
- [Source: src/pipeline.rs:804 — code_review_enabled guard (to preserve)]
- [Source: src/pipeline.rs:858 — review comment without strip_agent_artifacts (to follow)]
- [Source: src/session/runner.rs:384 — skill_path hardcoded to .github/skills/ (to fix)]
- [Source: src/session/runner.rs:705-712 — run() delegates to run_with_consultations()]
- [Source: src/session/runner.rs:977-984 — build_preamble() (async, for dev sessions)]
- [Source: src/review/mod.rs:370 — ReviewRunner::run() (used by review pipeline)]
- [Source: src/review/mod.rs:575 — .github/skills/ path (to fix)]
- [Source: src/git_provider/mod.rs:127-134 — PrInfo #[derive(Debug, Clone)]]
- [Source: .claude/skills/bmad-dev-story/SKILL.md — Dev-story skill file (correct path)]
- [Source: .claude/skills/bmad-code-review/SKILL.md — Code-review skill file (correct path)]
- [Source: _bmad-output/implementation-artifacts/13-4-create-story-phase-with-consultations.md — Previous story intelligence]
- [Source: _bmad-output/implementation-artifacts/deferred-work.md:70 — Skill path discrepancy noted]

### Existing Code to Reuse

- `update_story_status()` — async function for sprint-status.yaml updates [src/session/cleanup.rs]
- `unblock_dependents()` — unblocks dependent stories [src/session/cleanup.rs]
- `commit_sprint_status()` — git commit for sprint-status changes [src/pipeline.rs]
- `push_branch()` — git push with error handling [src/pipeline.rs]
- `build_pr_title()`, `build_pr_description()` — PR construction helpers [src/pipeline.rs]
- `format_pr_decisions_section()` — formats decisions for PR body [src/pipeline.rs]
- `notify_story_result()` — sends notification [src/pipeline.rs]
- `reload_story_info()` — re-reads sprint-status for updated StoryInfo [src/pipeline.rs]
- `ReviewRunner::run()` — existing code review session [src/review/mod.rs]
- `is_infra_error()`, `is_auth_error()` — error classification [src/pipeline.rs]

### Anti-Patterns to Avoid

- **DO NOT** rewrite the review logic — MOVE it verbatim from `run_dev_pipeline()` to `run_review_pipeline()`. This is an extraction, not a rewrite.
- **DO NOT** add a `decisions_override` parameter to `run_review_pipeline()` — the review flow does not use decisions. They are consumed during PR creation in `run_dev_pipeline()`.
- **DO NOT** add consultations to the dev-story phase — the dev session runs without consultations (epic AC is explicit).
- **DO NOT** modify `SessionRunner` internals — the session runner already handles skill activation, tool registration, branch management correctly.
- **DO NOT** modify `ResponseAnalyzer` — completion detection is unchanged.
- **DO NOT** modify `run_create_pipeline()` — the create→dev chaining works correctly.
- **DO NOT** add UI events for pipeline phases — that is Story 13.11.
- **DO NOT** add WAL phase tracking — that is Story 13.10.
- **DO NOT** replace `ReviewRunner::run()` with a skill-based session — that is Story 13.6.
- **DO NOT** use `strip_agent_artifacts()` on the review comment — follow `run_dev_pipeline()` line 858 convention, not `process_recovered_session()` line 2402.
- **DO NOT** forget the `code_review_enabled` guard — it must be preserved in the extracted review pipeline.
- **DO NOT** pass stale `StoryInfo` when chaining — always call `reload_story_info()` first (or fallback with updated status field).
- **DO NOT** forget to update the `process_story()` router call site for the new `run_review_pipeline()` signature.
- **DO** use `tracing::info!` and `tracing::warn!` for operational logging (not `println!`).
- **DO** follow the existing chaining pattern from `run_create_pipeline()` (Story 13.4) when chaining dev→review.
- **DO** verify that `run_review_pipeline()` handles BOTH entry points (chained with pr_info AND direct from watcher without pr_info).

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

### Completion Notes List

- Fixed all `.github/skills/` path references to `.claude/skills/` across 5 files (runner.rs, review/mod.rs, agent.rs, agent_factory.rs, pipeline.rs test)
- Replaced `run_review_pipeline()` placeholder with full implementation handling two entry points: chained from dev (with pr_info) and direct from watcher (push+PR first)
- Refactored `run_dev_pipeline()` Completed arm: removed Phases 4-8, added mark review → reload_story_info → chain to run_review_pipeline()
- Fixed push-failure early return to mark story as `review` in sprint-status before returning (enables retry via review pipeline)
- Refactored `process_recovered_session()` Completed arm: removed duplicated review/done logic, replaced with push → PR → mark review → chain to run_review_pipeline()
- Removed unused `strip_agent_artifacts` import from pipeline.rs
- Fixed 2 clippy `collapsible_if` warnings in new code
- Updated `test_process_story_routes_review_returns_placeholder` → `test_process_story_routes_review_runs_pipeline` with updated assertions
- All 1183 tests pass, 1 pre-existing failure unchanged
- Zero new clippy warnings (76 pre-existing, 76 after changes)

### Change Log

- 2026-04-23: Story 13.5 implementation — Dev-story phase separation from review phase

### File List

- src/pipeline.rs (modified — run_review_pipeline implementation, run_dev_pipeline refactored, process_recovered_session refactored, PrInfo import added, strip_agent_artifacts import removed, test updated)
- src/session/runner.rs (modified — skill_path fixed to .claude/skills/)
- src/review/mod.rs (modified — skill_path fixed to .claude/skills/)
- src/session/agent.rs (modified — doc comments updated .github/skills/ → .claude/skills/)
- src/llm/agent_factory.rs (modified — doc comment updated .github/skills/ → .claude/skills/)

### Review Findings

- [x] [Review][Decision] Push-failure path returns `StoryStatus::Completed` but story is marked `review` on disk — Fixed: changed to `StoryStatus::Error` (non-fatal). [src/pipeline.rs:748]
- [x] [Review][Patch] Push-failure sprint-status write error silently swallowed with `let _ =` — Fixed: added `tracing::warn!` on failure. [src/pipeline.rs:738-745]
- [x] [Review][Defer] Watcher entry path does not detect pre-existing PRs on GitLab — GitLab provider maps HTTP 422 to `BranchNotFound` instead of `DuplicatePr`. When a `review` story re-enters via watcher and a PR already exists, GitLab fails instead of fetching the existing MR. Pre-existing GitLab provider limitation. [src/git_provider/gitlab.rs:214-230] — deferred, pre-existing
- [x] [Review][Defer] Watcher retry re-runs code review and may post duplicate PR comments — No tracking of whether code review already completed. If daemon crashes post-review but pre-done, the retry re-runs the entire review. Addressed by Story 13.10 (WAL phase tracking). — deferred, pre-existing
- [x] [Review][Defer] Recovery path (`process_recovered_session`) does not mark `review` on push failure — Unlike `run_dev_pipeline()` which now marks `review` for retry, the recovery push-failure path leaves the story in `in-progress` limbo until daemon restart. Pre-existing gap. [src/pipeline.rs:2419-2441] — deferred, pre-existing
- [x] [Review][Defer] `update_story_status` regex replaces only first match — `re.replace()` (not `replace_all()`) updates only the first occurrence of a story key. Duplicate keys in manually-edited sprint-status.yaml leave inconsistent state. Pre-existing. [src/session/cleanup.rs:288] — deferred, pre-existing
