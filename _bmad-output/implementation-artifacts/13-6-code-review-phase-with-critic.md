# Story 13.6: Code-Review Phase with Critic Consultation

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the code-review pipeline phase to use `SessionRunner::run_with_consultations()` and invoke the critic for `decision-needed` findings,
So that the review phase follows the multi-phase architecture (create → dev → review) with daemon-orchestrated consultations, and ambiguous findings are resolved by the vision guardian instead of blocking on human input.

## Acceptance Criteria

1. **AC-1: Review phase uses `SessionRunner::run_with_consultations()` with code review skill**
   - **Given** a story with status `review` enters the code-review phase
   - **When** `run_review_pipeline()` invokes the review session
   - **Then** `self.session_runner.run_with_consultations()` is called with:
     - `skill_path_override = Some(".claude/skills/bmad-code-review/SKILL.md")`
     - `preamble_override = Some(build_review_preamble())`
     - Role `LlmRole::Review` (not `LlmRole::Dev`)
     - Critic consultation config (from `build_review_consultations()`)
   - **And** `self.review_runner.run(story)` is no longer called from `run_review_pipeline()`
   - **And** the code review session runs autonomously, with the skill discovering the review target from the initial message

2. **AC-2: `run_with_consultations()` parameterized by `LlmRole`**
   - **Given** `run_with_consultations()` currently hardcodes `LlmRole::Dev` for provider/model resolution and agent build
   - **When** this story is implemented
   - **Then** `run_with_consultations()` accepts a new `role: LlmRole` parameter
   - **And** provider/model are resolved from `self.config` based on the role (not hardcoded `self.config.llm.dev`)
   - **And** `build_agent_for_role()` receives the passed role instead of hardcoded `LlmRole::Dev`
   - **And** all existing call sites are updated to pass the correct role
   - **And** `run_session()` and `context_limit_recovery()` propagate the role for agent rebuilds

3. **AC-3: Initial message for code review sessions**
   - **Given** `run_session()` branches on `skill_path.contains("bmad-create-story")` for the initial message
   - **When** the skill path contains `"bmad-code-review"`
   - **Then** the initial message includes: English override, branch reminder, story file path, diff source (branch diff against target branch), and autonomous mode directives
   - **And** the skill receives enough context to skip Tiers 1-5 of step-01-gather-context.md and construct the diff directly

4. **AC-4: Critic consultation triggered for `decision-needed` findings**
   - **Given** the code-review session produces output containing `decision-needed` findings
   - **When** the daemon detects the trigger pattern in the session output
   - **Then** the consultation mechanism pauses the review session and builds a fresh Critic agent
   - **And** the Critic agent uses `LlmRole::Review` (placeholder — Story 13.9 introduces `LlmRole::Critic`) and `ConsultationToolSet::Restricted`
   - **And** the Critic's context includes the story file content and the decision-needed findings
   - **And** decisions are sent back to the code-review session via the resume message template
   - **And** the code-review agent applies the decisions

5. **AC-5: No consultation when no `decision-needed` findings**
   - **Given** the code-review session has no `decision-needed` findings
   - **When** the review completes
   - **Then** no Critic consultation is triggered — the review proceeds directly to completion

6. **AC-6: Review report extracted from story file for PR comment**
   - **Given** the code-review session completes (`SessionOutcome::Completed`)
   - **When** the review pipeline processes the completion
   - **Then** the pipeline reads the story file and extracts the `### Review Findings` section
   - **And** the findings are formatted as a markdown PR comment
   - **And** if no `### Review Findings` section exists (clean review), `None` is returned — no comment posted
   - **And** the existing PR comment posting flow (Phase 6) is preserved

7. **AC-7: Phase completion transitions story to `done`**
   - **Given** the code-review session completes successfully
   - **Then** the story status transitions to `done` via the existing Phase 7 flow in `run_review_pipeline()`
   - **And** review fixes are committed by the agent during the session (separate from dev commits)
   - **And** Phases 5-8 of `run_review_pipeline()` remain unchanged

8. **AC-8: Session failure and escalation handling**
   - **Given** the review session returns `SessionOutcome::Failed` or `SessionOutcome::Escalated`
   - **When** the review pipeline handles the result
   - **Then** `review_report` is set to `None` (no PR comment), and the pipeline continues to Phase 5-8 (mark done, notify)
   - **And** review failure is non-blocking — the story still transitions to `done` (matching current behavior where `ReviewOutcome::Failed` is non-blocking)

9. **AC-9: `review_runner` field removed from `StoryPipeline`**
   - **Given** `self.review_runner.run()` is no longer called
   - **When** this story is implemented
   - **Then** the `review_runner` field is removed from `StoryPipeline` struct (line 146)
   - **And** `ReviewRunner::new()` construction is removed from `StoryPipeline::new()` (lines 260-269)
   - **And** the `review_runner` assignment in the `Self { .. }` block (line 284) is removed
   - **And** the `use crate::review::ReviewRunner` import (line 28) is removed
   - **And** in tests, `helper_pipeline()` or equivalent test setup is updated to remove `review_runner` construction (line 4349)

10. **AC-10: Tests**
    - **Given** the code-review pipeline phase refactoring
    - **When** this story is implemented
    - **Then** `test_build_review_consultations` validates the critic consultation config
    - **And** `test_review_initial_message_format` validates the code review initial message
    - **And** `test_extract_review_report_from_story` validates report extraction from story files
    - **And** existing routing tests pass unchanged
    - **And** `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` passes
    - **And** `cargo test` passes with no new failures beyond pre-existing `test_build_context_limit_recovery_message_contains_all_sections`

## Tasks / Subtasks

- [x] Task 1: Parameterize `run_with_consultations()` with `LlmRole` (AC: #2)
  - [x] 1.1 Add `role: LlmRole` parameter to `run_with_consultations()` at `src/session/runner.rs:722`. New signature:
    ```rust
    pub async fn run_with_consultations(
        &self,
        story: &StoryInfo,
        base_branch_override: Option<&str>,
        consultations: Vec<ConsultationConfig>,
        skill_path_override: Option<&str>,
        preamble_override: Option<String>,
        role: LlmRole,
    ) -> SessionOutcome
    ```
  - [x] 1.2 Replace hardcoded `self.config.llm.dev.provider` / `self.config.llm.dev.model` (lines 825-826) with role-based resolution:
    ```rust
    let role_config = match role {
        LlmRole::Dev => &self.config.llm.dev,
        LlmRole::Review => &self.config.llm.review,
        LlmRole::Supervisor => &self.config.llm.supervisor,
        LlmRole::EpicReview => {
            if self.config.llm.epic_review.provider.is_empty() {
                &self.config.llm.review
            } else {
                &self.config.llm.epic_review
            }
        }
    };
    let provider = &role_config.provider;
    let model = &role_config.model;
    ```
  - [x] 1.3 Pass `role` instead of hardcoded `LlmRole::Dev` to `build_agent_for_role()` at line 855
  - [x] 1.4 Add `role: LlmRole` parameter to `run_session()` at line 1418 and `context_limit_recovery()` at line 1105. Propagate from `run_with_consultations()` → `run_session()` → `context_limit_recovery()`. Update the hardcoded `LlmRole::Dev` at line 1148 (`context_limit_recovery()` → `build_agent_for_role()`) to use the passed `role`.
  - [x] 1.5 Update `run()` at line 705-712 to pass `LlmRole::Dev`:
    ```rust
    self.run_with_consultations(story, base_branch_override, vec![], None, None, LlmRole::Dev).await
    ```
  - [x] 1.6 Update `run_create_pipeline()` call site at `src/pipeline.rs:403-411` to pass `LlmRole::Dev`:
    ```rust
    self.session_runner.run_with_consultations(
        story, base_branch_override, consultations,
        Some(".claude/skills/bmad-create-story/SKILL.md"),
        Some(create_preamble),
        LlmRole::Dev,
    ).await
    ```

- [x] Task 2: Add code review initial message in `run_session()` (AC: #3)
  - [x] 2.1 Add third branch to initial message at `src/session/runner.rs:1538-1554`:
    ```rust
    let initial_message = if skill_path.contains("bmad-create-story") {
        // ... existing
    } else if skill_path.contains("bmad-code-review") {
        format!(
            "IMPORTANT: ALL communication MUST be in English regardless of config file settings.\n\
             BRANCH REMINDER: You are already on branch `{}`. Do NOT create, checkout, or switch branches — the daemon manages branch lifecycle. Just commit your work on the current branch.\n\
             AUTONOMOUS CODE REVIEW: Review the changes on this branch.\n\
             Diff source: branch diff against `{}`\n\
             Story file: {}\n\
             AUTONOMOUS MODE RULES:\n\
             - Do NOT wait for human input at any HALT or checkpoint — proceed automatically.\n\
             - For checkpoints: confirm and proceed without waiting.\n\
             - For patch findings: auto-apply all fixes (batch-apply).\n\
             - For findings tagged [Review][Decision]: present them clearly with your analysis, then HALT. An external reviewer will provide decisions.\n\
             - For defer findings: leave as action items.\n\
             - After all findings are resolved, commit all review fixes and signal completion.",
            story.branch_name,
            self.config.git_provider.target_branch,
            story.specs_path.display()
        )
    } else {
        // ... existing dev-story
    };
    ```
  - [x] 2.2 The initial message provides explicit diff source and spec file so the code review skill's step-01-gather-context.md skips the Tier 1-5 cascade and goes straight to constructing the diff

- [x] Task 3: Create `build_review_preamble()` in `src/session/agent.rs` (AC: #1)
  - [x] 3.1 Add `pub(crate) fn build_review_preamble() -> String` following the pattern of `build_create_preamble()` at line 320
  - [x] 3.2 The preamble includes:
    - Tool usage rules (edit_file for fixes, read_file, grep, git for diff/commit)
    - English language override: `OVERRIDE: communication_language = English`
    - Autonomous review mode: no menus, no HALTs for user input except decision-needed
    - Branch management: do NOT create/checkout branches, only commit and push on current branch
    - Decision-needed handling: present clearly and halt — daemon will inject external decisions
    - Completion signal: end with `<<BMAD_JOB_DONE>>` when done

- [x] Task 4: Create `build_review_consultations()` in `src/pipeline.rs` (AC: #4, #5)
  - [x] 4.1 Add method following the pattern of `build_create_story_consultations()` at line 1388:
    ```rust
    fn build_review_consultations(&self, story: &StoryInfo) -> Vec<ConsultationConfig> {
        let story_file_path = PathBuf::from(&self.config.bmad_paths.project_root)
            .join(&story.specs_path)
            .to_string_lossy()
            .to_string();

        vec![
            ConsultationConfig {
                label: "review-critic".to_string(),
                skill_path: None,
                preamble_override: Some(build_review_critic_preamble()),
                role: LlmRole::Review, // placeholder — Story 13.9 uses LlmRole::Critic
                tool_set: ConsultationToolSet::Restricted,
                context_files: vec![story_file_path],
                trigger_pattern: r"- \[ \] \[Review\]\[Decision\]".to_string(),
                prompt_template: "The following code review findings need decisions. For each decision-needed finding, decide: patch (the fix is clear and unambiguous), defer (real issue but not actionable now), or dismiss (noise/false positive). Provide clear rationale for each decision.\n\n{context}".to_string(),
                resume_message_template: "An external vision reviewer has resolved the following flagged findings:\n\n{findings}\n\nPlease apply these decisions accordingly: apply patches for 'patch' decisions, leave 'defer' items as deferred, and remove 'dismiss' items. Then continue with the remaining workflow steps.".to_string(),
            },
        ]
    }
    ```
  - [x] 4.2 Add `fn build_review_critic_preamble() -> String` in `src/pipeline.rs` (following the pattern of `build_placeholder_critic_preamble()` at line 2895). Include instructions for the critic to read the findings, analyze against code quality and project context, and decide each finding. The preamble must explicitly instruct the critic NOT to use `edit_file` — the critic is a judge, not an editor.

- [x] Task 5: Refactor `run_review_pipeline()` Phase 4 to use `SessionRunner` (AC: #1, #6, #7, #8)
  - [x] 5.1 Replace the `self.review_runner.run(story)` block at `src/pipeline.rs:1207-1245` with:
    ```rust
    let review_report = if self.config.code_review_enabled {
        self.ui.phase_start("Code Review");
        let review_start = std::time::Instant::now();

        let consultations = self.build_review_consultations(story);
        let review_preamble = crate::session::agent::build_review_preamble();

        let session_outcome = self.session_runner.run_with_consultations(
            story,
            Some(&branch),
            consultations,
            Some(".claude/skills/bmad-code-review/SKILL.md"),
            Some(review_preamble),
            LlmRole::Review,
        ).await;

        match session_outcome {
            SessionOutcome::Completed { .. } => {
                self.ui.phase_complete("Code Review", review_start.elapsed());
                let report = extract_review_report_from_story(story);
                if report.is_none() {
                    tracing::info!(
                        action = "review_clean",
                        story_key = %story_key,
                        "No '### Review Findings' section in story file — clean review or skill did not write findings"
                    );
                }
                report
            }
            SessionOutcome::Escalated { report, .. } => {
                self.ui.phase_error("Code Review", &format!("Escalated: {}", report.reason));
                tracing::warn!(
                    action = "review_escalated",
                    story_key = %story_key,
                    reason = %report.reason,
                    "Code review session escalated — continuing without review report"
                );
                None
            }
            SessionOutcome::Failed { error, .. } => {
                self.ui.phase_error("Code Review", &error);
                tracing::warn!(
                    action = "review_failed",
                    story_key = %story_key,
                    error = %error,
                    "Code review session failed — continuing without review report"
                );
                None
            }
        }
    } else {
        self.ui.phase_complete("Code Review", std::time::Duration::ZERO);
        None
    };
    ```
  - [x] 5.2 Phases 5-8 (push review commits, post PR comment, mark done + unblock, notify) remain UNCHANGED. The `review_report` variable feeds into Phase 6 exactly as before.
  - [x] 5.3 Import `SessionOutcome` in pipeline.rs (add `use crate::session::SessionOutcome;` if not already imported)

- [x] Task 6: Implement `extract_review_report_from_story()` (AC: #6)
  - [x] 6.1 Add `fn extract_review_report_from_story(story: &StoryInfo) -> Option<String>` in `src/pipeline.rs`
  - [x] 6.2 Read the story file at `story.specs_path`:
    ```rust
    fn extract_review_report_from_story(story: &StoryInfo) -> Option<String> {
        let content = std::fs::read_to_string(&story.specs_path).ok()?;
        let start_marker = "### Review Findings";
        let start_idx = content.find(start_marker)?;
        let section = &content[start_idx..];
        // Terminate only at next ## heading (same level or higher). Do NOT stop at
        // ### sub-headings — the skill may organize findings under sub-sections.
        let end_idx = section[start_marker.len()..]
            .find("\n## ")
            .map(|i| i + start_marker.len())
            .unwrap_or(section.len());
        let findings = section[..end_idx].trim();
        if findings.is_empty() || findings == start_marker {
            return None;
        }
        Some(format!("## Code Review\n\n{findings}"))
    }
    ```
  - [x] 6.3 This replaces the XML-based `parse_review_report()` + `build_review_comment()` chain from `ReviewRunner`. The code review skill writes structured findings to the story file, so XML parsing is unnecessary.

- [x] Task 7: Remove `review_runner` from `StoryPipeline` (AC: #9)
  - [x] 7.1 Remove `review_runner: ReviewRunner` field from `StoryPipeline` struct at `src/pipeline.rs:146`
  - [x] 7.2 Remove `ReviewRunner::new()` construction at lines 260-269
  - [x] 7.3 Remove `review_runner` from the `Self { .. }` block at line 284
  - [x] 7.4 Remove `use crate::review::ReviewRunner` import at line 28
  - [x] 7.5 In tests, remove `review_runner` from `helper_pipeline()` or equivalent test setup at line 4349. **Check:** search for ALL `ReviewRunner::new(` in pipeline.rs tests and remove each one along with the corresponding struct field.
  - [x] 7.6 If `review/mod.rs` imports are no longer needed in `pipeline.rs` (check for `ReviewOutcome`, `parse_review_report`, etc.), clean up those imports too. Keep the `review` module itself — it still has `EpicReviewRunner` which is used (line 1709).

- [x] Task 8: Update tests (AC: #10)
  - [x] 8.1 Add `test_build_review_consultations` — validate: label is `"review-critic"`, role is `LlmRole::Review`, tool set is `ConsultationToolSet::Restricted`, trigger regex compiles and matches `"- [ ] [Review][Decision] Some Finding"`, does NOT match natural language like `"For findings tagged [Review][Decision]"` or `"decision-needed"` or checked items like `"- [x] [Review][Decision]"`, resume template contains `{findings}` placeholder and does NOT contain the word `"decision-needed"`
  - [x] 8.2 Add `test_review_initial_message_format` — validate the code review initial message contains: English override, branch reminder, autonomous mode directives, story file path reference, diff source instruction, target branch reference
  - [x] 8.3 Add `test_extract_review_report_from_story` — test: (a) story with `### Review Findings` section containing findings → returns `Some`, (b) story without the section → returns `None`, (c) story with empty `### Review Findings` (heading only) → returns `None`, (d) story with `### Review Findings` followed by `### Sub-Section` → includes both sub-sections in output, (e) story with `### Review Findings` followed by `## Next Section` → terminates at `## ` boundary
  - [x] 8.4 Update tests that previously tested `review_runner.run()` behavior via `run_review_pipeline()` — the review pipeline now uses `session_runner.run_with_consultations()`. Check `test_process_story_routes_review_runs_pipeline` (from Story 13.5, line ~4394+) and update assertions.
  - [x] 8.5 Verify `test_build_create_story_consultations` still passes (unchanged)
  - [x] 8.6 Run `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` — zero new warnings
  - [x] 8.7 Run `cargo test` — all pass, no new failures beyond pre-existing `test_build_context_limit_recovery_message_contains_all_sections`

## Dev Notes

### Architecture Compliance

- **Decision 10 (Daemon-Orchestrated Consultations):** The code-review phase uses the same consultation mechanism as create-story (Story 13.4). The daemon pauses the review session when `decision-needed` is detected, runs a Critic agent, and feeds decisions back. This is the second use of the consultation pattern — no new infrastructure needed. The `ConsultationConfig`, `ConsultationState`, and `ConsultationRunner` from `src/session/consultation.rs` are reused as-is.
- **Decision 11 (Story Critic):** The Critic is a placeholder in this story — uses `LlmRole::Review` and a simple preamble. Story 13.9 will introduce `LlmRole::Critic` and proper Critic agent construction with extended thinking. Story 13.8 introduces `critic-memory.md`. Both are in `backlog`. This story establishes the consultation wiring; 13.9 upgrades the agent quality.
- **Decision 5 (Skill-Based Activation):** The code review session uses `.claude/skills/bmad-code-review/SKILL.md` — already used by `ReviewRunner` (line 575 of review/mod.rs). The switch from `ReviewRunner` to `SessionRunner` changes the chat loop mechanics but not the skill activation.
- **ReviewRunner → SessionRunner gap analysis:** `ReviewRunner.drive_review_session()` managed: (a) skill menu navigation — now handled by `ResponseAnalyzer` auto-replies, (b) XML `<review-report>` parsing — replaced by `extract_review_report_from_story()` which reads the `### Review Findings` section from the story file, (c) review-specific chat loop with hardcoded response patterns — now the generic `SessionRunner` chat loop with `ResponseAnalyzer` handles HALTs and checkpoints. The `ResponseAnalyzer` was validated in Stories 12.1-12.2 for skill-based sessions and handles the code review skill's interaction patterns (HALTs → auto-continue, menus → auto-select).
- **Multi-Phase Pipeline Vision:** After this story, all three pipeline phases (create, dev, review) use `SessionRunner::run_with_consultations()`, completing the unified multi-phase architecture from the sprint-change-proposal-2026-04-15.

### Critical Implementation Details

**This story replaces `ReviewRunner::run()` usage with `SessionRunner::run_with_consultations()`.** The `ReviewRunner` struct and its methods (`run()`, `run_inner()`, `drive_review_session()`, `build_review_agent()`) are no longer called from the pipeline. The `review_runner` field is removed from `StoryPipeline`. The `review/mod.rs` module itself is NOT deleted — it still exports `EpicReviewRunner` (used at line 1709), `EpicReviewOutcome`, and utility functions.

**`run_with_consultations()` must be parameterized by `LlmRole`.** At `src/session/runner.rs:825-826`, it hardcodes `self.config.llm.dev.provider/model`. For review, it must use `self.config.llm.review`. Add `role: LlmRole` as a parameter and resolve config with a match:
```rust
let role_config = match role {
    LlmRole::Dev => &self.config.llm.dev,
    LlmRole::Review => &self.config.llm.review,
    LlmRole::Supervisor => &self.config.llm.supervisor,
    LlmRole::EpicReview => {
        if self.config.llm.epic_review.provider.is_empty() {
            &self.config.llm.review
        } else {
            &self.config.llm.epic_review
        }
    }
};
```
The `EpicReview` fallback logic mirrors `AgentFactory::config_for_role()` at `src/llm/agent_factory.rs:210-220`.

**`run_session()` and `context_limit_recovery()` both need a `role: LlmRole` parameter.** While `run_with_consultations()` resolves `provider`/`model` from the role and passes them to `run_session()`, both `run_session()` and `context_limit_recovery()` also call `build_agent_for_role()` — which requires the actual `LlmRole`. At line ~1148, `context_limit_recovery()` currently hardcodes `self.build_agent_for_role(LlmRole::Dev, ...)`, and `build_agent_for_role()` at line 855 also hardcodes `LlmRole::Dev`. Both must be updated to accept and propagate the role parameter: `run_with_consultations()` → `run_session()` → `context_limit_recovery()` → `build_agent_for_role(role, ...)`.

**Initial message for code review.** The code review skill's step-01-gather-context.md goes through 5 tiers to find the review target. The initial message must provide enough context to skip this:
- Explicit diff source: `"branch diff against <target_branch>"`
- Story file path: sets `{spec_file}` and `{review_mode} = "full"`
- Autonomous mode: no HALTs, auto-apply patches, present decision-needed clearly

The `ResponseAnalyzer` handles auto-responding to the skill's remaining HALTs with `"Continue."` or auto-generated replies. For decision-needed HALTs, the consultation trigger regex intercepts first.

**Review report extraction.** The code review skill writes findings to the story file's `### Review Findings` section (step-04-present.md, instruction 2). After session completion, read this section. This replaces the XML `<review-report>` parsing from `ReviewRunner`. The extraction is a simple string search — no regex needed.

**Session failure is non-blocking.** The current `run_review_pipeline()` at lines 1207-1245 treats `ReviewOutcome::Failed` as non-blocking: sets `review_report = None` and continues to Phase 5-8 (mark done, notify). Preserve this: `SessionOutcome::Failed` → no review report, story still transitions to `done`. Same for `SessionOutcome::Escalated`.

**`ReviewOutcome::Skipped` has no `SessionOutcome` equivalent — by design.** The current code handles `ReviewOutcome::Skipped { reason }` (e.g., no diff found, PR not ready). With `SessionRunner`, skip conditions are handled differently: (1) `code_review_enabled = false` is checked in the outer `if` before the session starts, (2) if the session runs but finds nothing to review, it completes normally with no findings → `extract_review_report_from_story()` returns `None` (clean review, logged as `review_clean`). There is no need for a `Skipped` outcome — the session either completes (with or without findings) or fails.

**Branch management in `SessionRunner`.** `run_with_consultations()` calls `ensure_story_branch()` which creates or reuses the story branch. For the review phase, the branch already exists (created during dev). `ensure_story_branch()` returns `BranchAction::Reused` and checks it out. Pass `Some(&branch)` as `base_branch_override` — the base branch value doesn't matter when the branch already exists, but it avoids the `determine_base_branch()` dependency resolution overhead.

**WAL creation for review sessions.** `SessionRunner` creates a WAL file during the review session. This is new — `ReviewRunner` didn't use a WAL. On crash during review, the WAL is found on restart. `process_recovered_session()` handles it: push → PR → mark review → chain to `run_review_pipeline()`. Since the PR already exists, the GitProvider should handle the duplicate creation gracefully. Story 13.10 (WAL Pipeline Phase Tracking) will add `pipeline_phase` to the WAL for precise recovery routing.

**Consultation trigger pattern.** The code review skill outputs findings in the structured format `- [ ] [Review][Decision] <Title> — <Detail>` (step-04-present.md). The trigger regex `r"- \[ \] \[Review\]\[Decision\]"` matches this exact checklist format. This narrow pattern is intentional — a loose regex like `(?i)decision.needed` would false-positive on instruction text (the initial message and resume template both discuss these findings in natural language) and could cause infinite consultation loops when the agent echoes trigger words in its response to the resume message. After decisions are applied, findings are checked off as `- [x] [Review][Decision]`, which no longer matches the unchecked pattern.

**`ConsultationToolSet::Restricted` for the critic.** Includes: `read_file`, `edit_file`, `grep`, `find_path`, `list_directory` + `ThinkTool`. No `git`, `terminal`, `ask_supervisor`, `spawn_agent`. **Note:** `edit_file` is included in the `Restricted` set but the placeholder critic must NOT modify code — its job is to judge findings (patch/defer/dismiss), not apply fixes. The `build_review_critic_preamble()` must explicitly instruct the critic not to edit files. Story 13.9 may introduce a `ReadOnly` tool set variant if the preamble-based restriction proves insufficient.

**`build_agent_for_role()` hardcodes `LlmRole::Dev` for SpawnAgentTool.** At `src/session/runner.rs:938`, `create_spawn_agent_tool()` is called with `LlmRole::Dev`. This is a known deferred issue from Story 12.4 code review. For the review session, sub-agents spawned via `spawn_agent` will use the dev provider/model, not the review provider/model. This is acceptable — the deferred item tracks it.

### Previous Story Intelligence (Story 13.5)

- **Baseline test count:** 1183 passing, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)
- **Pre-existing clippy allowances:** `-A clippy::needless_splitn -A clippy::unnecessary_map_or`
- **`run_review_pipeline()`** at `src/pipeline.rs:1114-1381` — ~267 lines. Phase 4 (code review invocation, lines 1207-1245) is what this story replaces. Phases 5-8 remain unchanged.
- **`ReviewRunner::run(story)`** called at line 1211 — returns `ReviewOutcome::Completed { report }`, `Failed`, or `Skipped`
- **`review_runner`** field at line 146, constructed at lines 260-269, used only at line 1211
- **`session_runner`** field at line 145, used at line 697 (`run()` for dev) and by `run_create_pipeline()` (line 403, `run_with_consultations()`)
- **`run_with_consultations()` signature** at `src/session/runner.rs:722`: `(story, base_branch_override, consultations, skill_path_override, preamble_override) -> SessionOutcome` — 5 params, adding `role` makes 6
- **`run()` delegates to `run_with_consultations()`** at line 710: `self.run_with_consultations(story, base_branch_override, vec![], None, None)`
- **`build_agent_for_role()` hardcodes `LlmRole::Dev`** at line 855 — must be updated to use the role param
- **`context_limit_recovery()` calls `build_agent_for_role()`** — check if it also hardcodes `LlmRole::Dev` and update accordingly
- **`process_story()` router** at line 343-384 — routes `StoryPhase::Review` to `self.run_review_pipeline(story, &story_title, None, None)` — unchanged by this story
- **`build_create_story_consultations()`** at line 1388-1423 — reference pattern for building consultation configs
- **`build_placeholder_critic_preamble()`** function exists in `src/pipeline.rs:2895` — reuse or create similar for review critic
- **`SessionOutcome`** defined at `src/session/mod.rs:102`: `Completed { story_key, branch, decisions, pr_context, pr_how_to_test, pr_additional_info }`, `Escalated { report, decisions }`, `Failed { story_key, error, decisions }`
- **Imports in pipeline.rs:** `SessionOutcome` is likely already imported — check via `use crate::session::SessionOutcome` or similar
- **`review/mod.rs` exports:** `ReviewRunner`, `ReviewOutcome`, `EpicReviewRunner`, `EpicReviewOutcome`, `extract_epic_recap`, `generate_failure_report`, `parse_review_report`, `build_review_comment`. After removing `ReviewRunner` usage, keep the module for `EpicReview*` exports.
- **Test helper `helper_pipeline()`** at `src/pipeline.rs:~4230+` constructs `StoryPipeline` for tests — includes `ReviewRunner::new()` which must be removed

### Known Limitations

**Crash recovery uses `LlmRole::Dev` for all sessions.** `check_and_recover_wal()` at `src/session/runner.rs:589` hardcodes `LlmRole::Dev` when rebuilding an agent from WAL. If the daemon crashes during a review session, recovery builds a Dev agent (wrong provider/model). The WAL does not store the LLM role — Story 13.10 (WAL Pipeline Phase Tracking) will add `pipeline_phase` which enables correct role resolution. **Operational risk:** if dev and review use different providers or API keys, WAL recovery of a review session could fail with auth errors or consume dev-tier tokens. **Mitigation:** even if WAL recovery fails, the story remains in `review` status, so the next watcher poll routes it to `run_review_pipeline()` which starts a fresh session with the correct role. WAL recovery failure is logged but non-fatal.

**Crash recovery preamble doesn't distinguish review sessions.** `check_and_recover_wal()` at `src/session/runner.rs:564-585` resolves preamble based on `is_create_session` (skill_path contains "bmad-create-story"). A review session (skill_path contains "bmad-code-review") falls through to the standard dev preamble. Same Story 13.10 concern — the WAL stores `skill_path`, so a code-review-aware preamble branch could be added alongside the phase tracking. Out of scope for this story.

**`create_spawn_agent_tool()` always uses `LlmRole::Dev`.** At `src/session/runner.rs:940`, sub-agents spawned via `spawn_agent` tool use the dev provider/model regardless of the parent session's role. Pre-existing deferred item from Story 12.4 code review. Acceptable for now — sub-agents during review are rare and non-critical.

**`run_with_consultations()` has 6 non-self parameters.** Adding `role` brings the count to 6 (story, base_branch_override, consultations, skill_path_override, preamble_override, role). If future stories add more parameters, consider refactoring into a `SessionConfig` struct. Tracked as minor design debt.

### Git Intelligence — Recent Commits

```
b68fc0d feat(epic-13): separate dev phase from review phase in pipeline (Story 13.5)
5f4a497 feat(epic-13): implement create-story phase with consultations (Story 13.4)
63932ed feat(epic-13): add daemon-orchestrated consultation mechanism (Story 13.3)
147f57d feat(epic-13): refactor pipeline into status-based phase router (Story 13.2)
fb38013 feat(epic-13): extend watcher to detect backlog and review stories (Story 13.1)
```

Files most recently modified:
- `src/pipeline.rs` — 514 additions in 13.4, significant refactor in 13.5
- `src/session/runner.rs` — 141 additions in 13.4 (parameterized run_with_consultations, skill/preamble overrides)
- `src/session/agent.rs` — `build_create_preamble()` added in 13.4
- `src/session/consultation.rs` — entire module added in 13.3

### Project Structure Notes

- `src/pipeline.rs` modified — `run_review_pipeline()` Phase 4 replaced, `build_review_consultations()` added, `extract_review_report_from_story()` added, `review_runner` field removed
- `src/session/runner.rs` modified — `run_with_consultations()` signature gains `role: LlmRole`, `run()` updated, `run_session()` and `context_limit_recovery()` gain `role` parameter, initial message gains third branch for `bmad-code-review`
- `src/session/agent.rs` modified — `build_review_preamble()` added
- No new files created
- `review/mod.rs` NOT modified — `ReviewRunner` struct remains but is no longer constructed in pipeline

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Story 13.6 AC (Code-Review Phase with Critic)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Decision 10 (Daemon-Orchestrated Consultations)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Decision 11 (Story Critic)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Decision 5 (Skill-Based Activation)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md — Pipeline model, Epic 13]
- [Source: _bmad-output/project-context.md — Daemon Lifecycle, Agent Construction, Tool Calling]
- [Source: src/pipeline.rs:1114-1381 — run_review_pipeline() (Phase 4 to replace)]
- [Source: src/pipeline.rs:1388-1423 — build_create_story_consultations() (reference pattern)]
- [Source: src/pipeline.rs:390-412 — run_create_pipeline() (reference for SessionRunner usage)]
- [Source: src/pipeline.rs:146 — review_runner field (to remove)]
- [Source: src/pipeline.rs:260-269 — ReviewRunner::new() construction (to remove)]
- [Source: src/pipeline.rs:1211 — self.review_runner.run(story) (to replace)]
- [Source: src/session/runner.rs:722-893 — run_with_consultations() (to parameterize with role)]
- [Source: src/session/runner.rs:825-826 — hardcoded LlmRole::Dev config (to fix)]
- [Source: src/session/runner.rs:855 — hardcoded LlmRole::Dev in build_agent_for_role (to fix)]
- [Source: src/session/runner.rs:705-712 — run() delegates to run_with_consultations() (update)]
- [Source: src/session/runner.rs:1105 — context_limit_recovery() (propagate role)]
- [Source: src/session/runner.rs:1418-1554 — run_session() initial message branching (add review)]
- [Source: src/session/agent.rs:320 — build_create_preamble() (reference pattern)]
- [Source: src/session/consultation.rs:24-49 — ConsultationConfig, ConsultationToolSet]
- [Source: src/review/mod.rs:311-409 — ReviewRunner struct and run() method (no longer used)]
- [Source: src/review/mod.rs:510-859 — run_inner(), drive_review_session() (no longer used)]
- [Source: src/review/mod.rs:115-176 — parse_review_report() (replaced by extract_review_report_from_story)]
- [Source: src/llm/agent_factory.rs:37-46 — LlmRole enum]
- [Source: src/llm/agent_factory.rs:210-220 — config_for_role() (reference for role resolution)]
- [Source: .claude/skills/bmad-code-review/SKILL.md — Code review skill entry point]
- [Source: .claude/skills/bmad-code-review/steps/step-01-gather-context.md — Tier 1-5 cascade]
- [Source: .claude/skills/bmad-code-review/steps/step-03-triage.md — decision_needed classification]
- [Source: .claude/skills/bmad-code-review/steps/step-04-present.md — Review Findings output format]
- [Source: _bmad-output/implementation-artifacts/13-5-dev-story-phase.md — Previous story intelligence]

### Existing Code to Reuse

- `ConsultationConfig` struct — `src/session/consultation.rs:39` (consultation wiring)
- `ConsultationToolSet::Restricted` — `src/session/consultation.rs:28` (critic tool set)
- `build_placeholder_critic_preamble()` — `src/pipeline.rs:2895` (reference for review critic preamble)
- `build_create_preamble()` — `src/session/agent.rs:320` (pattern for build_review_preamble)
- `build_create_story_consultations()` — `src/pipeline.rs:1388` (pattern for build_review_consultations)
- `update_story_status()` — `src/session/cleanup.rs` (sprint-status updates, used by Phase 7)
- `unblock_dependents()` — `src/session/cleanup.rs` (used by Phase 7)
- `commit_sprint_status()` — `src/pipeline.rs` (used by Phase 7)
- `notify_story_result()` — `src/pipeline.rs` (used by Phase 8)
- `AgentFactory::config_for_role()` — `src/llm/agent_factory.rs:210` (reference for role-based config)

### Anti-Patterns to Avoid

- **DO NOT** modify Phases 5-8 of `run_review_pipeline()` — only Phase 4 (code review invocation) changes. The push, comment, mark-done, and notify flow is preserved verbatim.
- **DO NOT** delete `review/mod.rs` — it still exports `EpicReviewRunner`, `EpicReviewOutcome`, `extract_epic_recap`, `generate_failure_report` used by the epic review at pipeline.rs line 1709.
- **DO NOT** add `LlmRole::Critic` — that is Story 13.9. Use `LlmRole::Review` as placeholder for the critic consultation.
- **DO NOT** add `critic-memory.md` handling — that is Story 13.8. The placeholder critic does not use persistent memory.
- **DO NOT** add WAL phase tracking — that is Story 13.10. The review session creates a standard WAL.
- **DO NOT** add new UI events for consultation phases — that is Story 13.11.
- **DO NOT** modify `run_create_pipeline()` beyond updating the `run_with_consultations()` call site for the new `role` parameter.
- **DO NOT** modify `process_recovered_session()` — it chains to `run_review_pipeline()` which will use the updated code.
- **DO NOT** block story completion on review failure — review failure is non-blocking (current behavior). Both `SessionOutcome::Failed` and `SessionOutcome::Escalated` map to `review_report = None`, and the pipeline continues to mark done + notify.
- **DO NOT** use `parse_review_report()` or `build_review_comment()` — these are XML-based and specific to `ReviewRunner`. Use `extract_review_report_from_story()` which reads the story file directly.
- **DO NOT** use a loose trigger regex — the trigger must match only the structured checklist format (`- [ ] [Review][Decision]`), not natural language mentions. A loose regex causes infinite consultation loops when the resume message or agent discussion contains trigger words.
- **DO NOT** allow the review critic to edit files — the preamble must explicitly prohibit `edit_file` usage. The critic decides (patch/defer/dismiss), the review agent applies.
- **DO NOT** forget to update `context_limit_recovery()` — it also calls `build_agent_for_role()` and may hardcode `LlmRole::Dev`.
- **DO** use `tracing::info!` and `tracing::warn!` for operational logging (not `println!`).
- **DO** follow the consultation pattern from `build_create_story_consultations()` — same structure, different trigger and preamble.
- **DO** verify the trigger regex matches the code review skill's actual output format before finalizing.

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

### Completion Notes List

- Task 1: Parameterized `run_with_consultations()`, `run_session()`, `context_limit_recovery()`, and `drive_activation_and_recover()` with `LlmRole`. Role-based config resolution replaces hardcoded `self.config.llm.dev`. All call sites updated; WAL recovery call sites keep `LlmRole::Dev` (known limitation per story notes).
- Task 2: Added third branch to initial message in `run_session()` for `bmad-code-review` skill — includes English override, branch reminder, autonomous mode directives, diff source, and story file path.
- Task 3: Created `build_review_preamble()` in `src/session/agent.rs` — review-specific preamble with autonomous mode, decision-needed handling, sentinel separation, and tool usage rules.
- Task 4: Created `build_review_consultations()` in `src/pipeline.rs` returning one `ConsultationConfig` for `review-critic` with narrow trigger regex `r"- \[ \] \[Review\]\[Decision\]"`. Created `build_review_critic_preamble()` explicitly prohibiting `edit_file` usage.
- Task 5: Replaced `self.review_runner.run(story)` in `run_review_pipeline()` Phase 4 with `self.session_runner.run_with_consultations()` using `LlmRole::Review`. Handles `Completed`/`Escalated`/`Failed` outcomes. Non-blocking on failure (preserves existing behavior).
- Task 6: Implemented `extract_review_report_from_story()` — reads `### Review Findings` section from story file, terminates at `## ` boundary (not `### `). Returns `None` for missing/empty sections.
- Task 7: Removed `review_runner` field from `StoryPipeline` struct, constructor, and test helper. Removed `use crate::review::ReviewRunner` and `ReviewOutcome` imports from pipeline.rs. Added `#[allow(dead_code)]` to now-unused items in `review/mod.rs` (module kept for `EpicReviewRunner`).
- Task 8: Added 4 new tests: `test_build_review_consultations` (validates config, trigger regex positive/negative matching), `test_review_initial_message_format` (validates all required directives), `test_extract_review_report_from_story` (5 sub-cases: findings present, missing, empty, sub-sections, boundary termination), `test_review_preamble_contains_key_directives`. All pre-existing tests pass unchanged. 1187 passing, 1 pre-existing failure.

### Change Log

- 2026-04-23: Story 13.6 implementation — replaced ReviewRunner with SessionRunner for code review phase, parameterized LlmRole across session runner chain, added critic consultation config and review preamble

### Review Findings

- [x] [Review][Patch] `extract_review_report_from_story` reads relative `specs_path` without joining with `project_root` — inconsistent with `build_review_consultations` which joins [src/pipeline.rs:2925] — FIXED: added `project_root: &Path` parameter
- [x] [Review][Patch] Doc comment collision — `extract_review_report_from_story` inserted between `is_infra_error`'s doc comment and its function body, `rustdoc` associates wrong documentation [src/pipeline.rs:2911-2924] — FIXED: moved function before `is_infra_error` doc comment, restored proper separation
- [x] [Review][Defer] Dead code in `review/mod.rs` suppressed with `#[allow(dead_code)]` instead of removal — deferred, by design (module kept for `EpicReviewRunner`)
- [x] [Review][Defer] Crash recovery always uses `LlmRole::Dev` regardless of session type — deferred, tracked Story 13.10
- [x] [Review][Defer] `SpawnAgentTool` hardcodes `LlmRole::Dev` instead of forwarding role — deferred, tracked Story 12.4
- [x] [Review][Defer] Logging/UI calls hardcode `"dev"` label for all roles — deferred, tracked Story 13.11
- [x] [Review][Defer] Stringly-typed skill path dispatch (`contains`) — deferred, pre-existing pattern
- [x] [Review][Defer] Critic preamble prohibits `edit_file` but tool set still includes it — deferred, tracked Story 13.9

### File List

- src/session/runner.rs (modified) — `run_with_consultations()`, `run_session()`, `context_limit_recovery()`, `drive_activation_and_recover()` gain `role: LlmRole` param; initial message gains code-review branch; all call sites updated
- src/session/agent.rs (modified) — added `build_review_preamble()`
- src/pipeline.rs (modified) — Phase 4 refactored to use SessionRunner; `build_review_consultations()`, `build_review_critic_preamble()`, `extract_review_report_from_story()` added; `review_runner` field removed; imports cleaned; 4 new tests added
- src/review/mod.rs (modified) — `#[allow(dead_code)]` added to items no longer called from pipeline (module kept for EpicReviewRunner)
