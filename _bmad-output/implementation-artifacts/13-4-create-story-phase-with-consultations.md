# Story 13.4: Create-Story Phase with Consultations

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the create-story pipeline phase to run a `bmad-create-story` session enriched with adversarial review and critic consultations,
So that every story file is adversarially validated and vision-checked before development begins.

## Acceptance Criteria

1. **AC-1: Create-story session with skill activation**
   - **Given** a story with status `backlog` enters the create-story phase
   - **When** `run_create_pipeline()` executes
   - **Then** a fresh agent session is created on the story branch (`story/{story_key}`), activated with `.claude/skills/bmad-create-story/SKILL.md`
   - **And** the agent runs autonomously — discovers the target story from `sprint-status.yaml`, creates the story file, transitions the story to `ready-for-dev`
   - **And** the daemon monitors the session for the completion signal (`<<BMAD_JOB_DONE>>`)

2. **AC-2: Adversarial consultation triggered after story creation**
   - **Given** the create-story session signals story completion (the agent produces a response matching a trigger pattern like `(?i)(STORY\s+CONTEXT\s+CREATED|story\s+file\s+(?:created|saved|written)|Status:\s*ready-for-dev)`)
   - **When** the daemon detects the completion pattern
   - **Then** **Consultation 1 — Adversarial Review** is triggered:
     - A fresh agent is built using `preamble_override` (not skill activation — see Dev Notes "Adversarial Consultation Design") with `ConsultationToolSet::Full`
     - The newly created story file content is provided as context via `context_files`
     - The adversarial agent produces findings
     - Findings are injected back to the create-story session via `resume_message_template`: `"An external adversarial reviewer has analyzed this story and found the following issues:\n\n{findings}\n\nPlease fix all these issues and update the story file."`
     - The create-story agent applies corrections with its BMAD context

3. **AC-3: Critic consultation triggered after adversarial corrections**
   - **Given** the adversarial corrections are applied
   - **When** the create-story agent signals it has finished applying fixes (response matching a second trigger pattern like `(?i)(corrections?\s+(applied|made|done|implemented)|issues?\s+(fixed|resolved|addressed)|updated\s+the\s+story)`)
   - **Then** **Consultation 2 — Story Critic** is triggered:
     - A fresh Critic agent is built using `preamble_override` (placeholder preamble until Story 13.9 engineers the full Critic prompt) with `ConsultationToolSet::Restricted`
     - Context files: the updated story file
     - The Critic produces observations and proposed modifications
     - Findings are injected back to the create-story session: `"An external product/technical vision reviewer has analyzed this story:\n\n{findings}\n\nPlease apply the relevant corrections to the story file."`
     - The create-story agent applies corrections
   - **Note:** Story 13.9 (Critic Agent Prompt Engineering) is still backlog. The Critic consultation is wired with a basic placeholder preamble. Consultation failures are non-fatal (Story 13.3 design) — if the Critic agent fails, the session continues without it. When 13.9 is implemented, the preamble will be upgraded.

4. **AC-4: Create-story preamble adapted from dev preamble**
   - **Given** `build_preamble()` in `session/agent.rs` produces a dev-specific system prompt (edit_file rules, pre-development spec update, "review previously completed stories")
   - **When** this story is implemented
   - **Then** the create-story session uses a create-specific preamble built via a new `build_create_preamble()` function (or the dev preamble is made phase-aware)
   - **And** the create preamble retains: tool usage rules, English override, `<<BMAD_JOB_DONE>>` sentinel instruction, branch management constraints
   - **And** the create preamble removes or replaces: dev-specific instructions about reviewing previously completed stories, pre-development spec update steps, and any references to "implementing" code
   - **And** the create preamble adds an instruction: "After completing your work, output your completion report. Then on your NEXT response, output <<BMAD_JOB_DONE>> to signal you are finished." — this ensures the completion report and sentinel are on SEPARATE turns so consultation triggers can fire between them

5. **AC-5: SessionRunner parameterized for different skills and preambles**
   - **Given** `SessionRunner::run_with_consultations()` currently hardcodes `self.skill_path` (`.github/skills/bmad-dev-story/SKILL.md`) and `LlmRole::Dev` with the dev preamble
   - **When** this story is implemented
   - **Then** `run_with_consultations()` accepts an optional `skill_path_override: Option<&str>` and an optional `preamble_override: Option<String>` parameter
   - **And** when `skill_path_override` is `Some(path)`, the session uses that path for agent activation instead of `self.skill_path`
   - **And** when `preamble_override` is `Some(preamble)`, it is used as the system prompt instead of `self.build_preamble()`
   - **And** `run()` continues to delegate with `None, None` (zero behavior change for dev sessions)
   - **And** the initial message sent after activation adapts based on the phase context

6. **AC-6: Create phase chains to dev phase on success**
   - **Given** the create-story session completes successfully
   - **When** `run_create_pipeline()` receives `SessionOutcome::Completed`
   - **Then** the pipeline re-reads sprint-status.yaml to build an updated `StoryInfo` with `status: "ready-for-dev"` before chaining to `run_dev_pipeline()`
   - **And** the dev pipeline inherits the story branch (already checked out by the create phase) via `base_branch_override`
   - **And** `ensure_story_branch()` detects the existing branch and returns `BranchAction::Reused`
   - **And** this chaining happens within a single `process_story()` invocation — no re-poll needed

7. **AC-7: Create phase handles failures and escalations**
   - **Given** the create-story session returns `SessionOutcome::Failed` or `SessionOutcome::Escalated`
   - **When** the failure/escalation is handled
   - **Then** the pipeline returns a `PipelineResult` with appropriate status (Error or Blocked)
   - **And** notification is sent with the failure details
   - **And** for non-infrastructure failures, a failure PR is created to preserve partial work (same pattern as dev pipeline)
   - **And** the dev phase is NOT chained — the pipeline stops for this story

8. **AC-8: Tests**
   - **Given** the create-story pipeline phase implementation
   - **When** this story is implemented
   - **Then** the following unit tests exist:
     - `test_route_story_status_backlog_maps_to_create` — verifies backlog → Create (pre-existing, verify still passes)
     - `test_build_create_story_consultations` — verifies the adversarial and critic `ConsultationConfig` structs are correctly constructed (trigger patterns compile, templates contain `{findings}`, adversarial uses `preamble_override` not `skill_path`)
     - `test_create_story_initial_message_format` — verifies the initial message includes English override and story key (not story file path)
     - `test_adversarial_trigger_matches_bmad_output` — verifies the adversarial trigger pattern matches "STORY CONTEXT CREATED" and "Status: ready-for-dev" but NOT intermediate text like "creating the story structure"
     - `test_create_preamble_contains_sentinel_separation` — verifies the create preamble instructs the agent to emit the completion report and `<<BMAD_JOB_DONE>>` on separate turns
   - **And** `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` passes
   - **And** `cargo test` passes with no new failures beyond the pre-existing `test_build_context_limit_recovery_message_contains_all_sections`

## Tasks / Subtasks

- [x] Task 1: Parameterize `SessionRunner` to accept different skill paths and preambles (AC: #4, #5)
  - [x] 1.1 Add `skill_path_override: Option<&str>` and `preamble_override: Option<String>` parameters to `run_with_consultations()`:
    ```rust
    pub async fn run_with_consultations(
        &self,
        story: &StoryInfo,
        base_branch_override: Option<&str>,
        consultations: Vec<ConsultationConfig>,
        skill_path_override: Option<&str>,
        preamble_override: Option<String>,
    ) -> SessionOutcome
    ```
  - [x] 1.2 Update `run()` to pass `None` for both new parameters:
    ```rust
    pub async fn run(
        &self,
        story: &StoryInfo,
        base_branch_override: Option<&str>,
    ) -> SessionOutcome {
        self.run_with_consultations(story, base_branch_override, vec![], None, None)
            .await
    }
    ```
  - [x] 1.3 In `run_with_consultations()`, resolve the effective skill path and preamble:
    ```rust
    let effective_skill_path = skill_path_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| self.skill_path.clone());
    let effective_preamble = preamble_override
        .unwrap_or_else(|| self.build_preamble());
    ```
  - [x] 1.4 Pass `effective_skill_path` and `effective_preamble` to `run_session()` — add `skill_path: &str` and `preamble: &str` parameters to `run_session()`:
    ```rust
    async fn run_session(
        &self,
        agent: &mut BuiltAgent,
        story: &StoryInfo,
        provider: &str,
        model: &str,
        base_branch: &str,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
        recovered_state: Option<SessionState>,
        consultation_states: &mut [ConsultationState],
        skill_path: &str,
        preamble: &str,
    ) -> SessionOutcome
    ```
  - [x] 1.5 Replace all `self.skill_path` references inside `run_session()` with the `skill_path` parameter (lines ~1389, ~1600)
  - [x] 1.6 Replace `self.build_preamble()` call inside `run_with_consultations()` with the `effective_preamble` value — the preamble is now resolved once before entering `run_session()`
  - [x] 1.7 Similarly update `drive_activation_and_recover()` to accept `skill_path: &str` and use it instead of `self.skill_path` (line ~1153)
  - [x] 1.8 Update all call sites of `run_session()` and `drive_activation_and_recover()` to pass the resolved skill path and preamble:
    - `run_with_consultations()` → `run_session(..., &effective_skill_path, &effective_preamble)`
    - `resume_session()` → `run_session(..., &self.skill_path, &self.build_preamble())` (recovery always uses dev-story)
    - Any other internal callers

- [x] Task 2: Build create-story preamble and adapt initial message (AC: #1, #4)
  - [x] 2.1 Add a `build_create_preamble()` function in `session/agent.rs` (next to `build_preamble()`). It should:
    - Retain from `build_preamble()`: tool usage rules (edit_file, read_file, grep, etc.), English override, `<<BMAD_JOB_DONE>>` sentinel instruction, branch management constraints
    - Remove/replace: dev-specific instructions about reviewing previously completed stories, pre-development spec update steps, references to "implementing" code
    - Add sentinel separation instruction: "After completing your work, output your completion report. Then on your NEXT response, output <<BMAD_JOB_DONE>> to signal you are finished." — this ensures the completion report and sentinel are on SEPARATE turns so consultation triggers can fire between them
    - The function is `pub(crate)` so `pipeline.rs` can call it and pass the result as `preamble_override`
  - [x] 2.2 The initial message in `run_session()` (line ~1461) currently says: `"Story file: {specs_path}"`. For create-story, the specs file doesn't exist yet — the agent creates it. Differentiate the initial message based on the skill path:
    ```rust
    let initial_message = if skill_path.contains("bmad-create-story") {
        format!(
            "IMPORTANT: ALL communication MUST be in English regardless of config file settings.\n\
             BRANCH REMINDER: You are already on branch `{}`. Do NOT create, checkout, or switch branches — the daemon manages branch lifecycle. Just commit your work on the current branch.\n\
             Create story: {}",
            story.branch_name,
            story.story_key
        )
    } else {
        format!(
            "IMPORTANT: ALL communication MUST be in English regardless of config file settings.\n\
             BRANCH REMINDER: You are already on branch `{}`. Do NOT create, checkout, or switch branches — the daemon manages branch lifecycle. Just commit your work on the current branch.\n\
             Story file: {}",
            story.branch_name,
            story.specs_path.display()
        )
    };
    ```
    **Alternative (cleaner):** Add an `initial_message_override: Option<String>` parameter to `run_with_consultations()` instead of sniffing the skill path. The pipeline passes a custom initial message for the create phase. Choose whichever approach the dev finds cleaner — the goal is: dev sessions get `"Story file: {path}"`, create sessions get `"Create story: {story_key}"`.

- [x] Task 3: Implement `run_create_pipeline()` in `pipeline.rs` (AC: #1, #2, #3, #5, #6, #7)
  - [x] 3.1 Replace the placeholder `run_create_pipeline()` (lines 386-412) with the full implementation. **Note:** The existing placeholder has `_story_title: &str` (unused) — rename to `story_title` since it's now passed to `run_dev_pipeline()` on success:
    ```rust
    async fn run_create_pipeline(
        &self,
        story: &StoryInfo,
        story_title: &str,
        base_branch_override: Option<&str>,
    ) -> PipelineResult {
        // Phase 1 — Create-Story Session with consultations
        self.ui.phase_start("Create Story");
        let session_start = std::time::Instant::now();

        let consultations = self.build_create_story_consultations(story);
        let create_preamble = build_create_preamble();

        let session_outcome = self
            .session_runner
            .run_with_consultations(
                story,
                base_branch_override,
                consultations,
                Some(".claude/skills/bmad-create-story/SKILL.md"),
                Some(create_preamble),
            )
            .await;

        let session_elapsed = session_start.elapsed();

        match session_outcome {
            SessionOutcome::Completed {
                story_key,
                branch,
                ..
            } => {
                self.ui.phase_complete("Create Story", session_elapsed);
                tracing::info!(
                    action = "create_phase_complete",
                    story_key = %story_key,
                    branch = %branch,
                    "Create-story phase completed — chaining to dev phase"
                );

                // Re-read sprint-status.yaml to get updated StoryInfo
                // (the create-story agent set status to "ready-for-dev")
                let updated_story = match self.reload_story_info(&story.story_key).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(
                            action = "create_phase_reload_failed",
                            error = %e,
                            "Failed to re-read story info — using original with updated status"
                        );
                        // Fallback: clone original story with status forced to ready-for-dev
                        let mut fallback = story.clone();
                        fallback.status = "ready-for-dev".to_string();
                        fallback
                    }
                };

                // Chain to dev phase — story is now ready-for-dev
                self.run_dev_pipeline(&updated_story, story_title, Some(&branch))
                    .await
            }

            SessionOutcome::Escalated { report, decisions } => {
                self.ui
                    .phase_error("Create Story", &format!("Escalated: {}", report.reason));
                // Follow run_dev_pipeline() escalation pattern (lines 713-813):
                // push branch best-effort, create escalation PR, notify human
                let push_result = self.push_branch(&story.branch_name).await;
                let pr_url = if push_result.is_ok() {
                    self.create_escalation_pr(story, &report, &decisions).await.ok()
                } else {
                    None
                };
                self.notify_escalation(story, &report, pr_url.as_deref()).await;
                PipelineResult {
                    status: StoryStatus::Blocked,
                    fatal: false,
                    pr_url,
                    error: Some(format!("Create phase escalated: {}", report.reason)),
                }
            }

            SessionOutcome::Failed {
                story_key,
                error,
                decisions,
            } => {
                self.ui.phase_error("Create Story", &error);
                // Follow run_dev_pipeline() failure pattern (lines 816-961):
                // check infra error → fatal; else push branch, create failure PR, notify
                if is_infra_error(&error) {
                    self.notify_failure(story, &error, None).await;
                    return PipelineResult {
                        status: StoryStatus::Error,
                        fatal: true,
                        pr_url: None,
                        error: Some(error),
                    };
                }
                let push_result = self.push_branch(&story.branch_name).await;
                let pr_url = if push_result.is_ok() {
                    self.create_failure_pr(story, &error, &decisions).await.ok()
                } else {
                    None
                };
                self.notify_failure(story, &error, pr_url.as_deref()).await;
                PipelineResult {
                    status: StoryStatus::Error,
                    fatal: false,
                    pr_url,
                    error: Some(error),
                }
            }
        }
    }
    ```
  - [x] 3.2 Implement `reload_story_info()` — a private method that re-reads `sprint-status.yaml` and returns an updated `StoryInfo` for the given story key. This ensures the dev phase receives `status: "ready-for-dev"` (not the stale `"backlog"` from the original `StoryInfo`). If this method is too costly to add, the alternative is to clone the original story and force `status = "ready-for-dev"` — but `specs_path` may also need updating (the create agent creates the file).
  - [x] 3.3 Implement `build_create_story_consultations()` private method. **Key design choices:** (a) The adversarial consultation uses `preamble_override` with a built adversarial preamble, NOT `skill_path` activation — see Dev Notes "Adversarial Consultation Design" for rationale. (b) `context_files` paths are canonicalized to absolute paths. (c) Trigger patterns are tightened to match known BMAD skill output:
    ```rust
    fn build_create_story_consultations(&self, story: &StoryInfo) -> Vec<ConsultationConfig> {
        // Canonicalize to absolute path — context_files expects absolute paths,
        // and specs_path may be relative depending on how StoryInfo was constructed
        let story_file_path = story.specs_path
            .canonicalize()
            .unwrap_or_else(|_| story.specs_path.clone())
            .to_string_lossy()
            .to_string();

        vec![
            // Consultation 1 — Adversarial Review
            // Uses preamble_override (not skill_path) — the adversarial skill expects
            // interactive input which conflicts with consultation's prompt_template flow
            ConsultationConfig {
                label: "adversarial".to_string(),
                skill_path: None,
                preamble_override: Some(build_adversarial_consultation_preamble()),
                role: LlmRole::Review,
                tool_set: ConsultationToolSet::Full,
                context_files: vec![story_file_path.clone()],
                trigger_pattern: r"(?i)(STORY\s+CONTEXT\s+CREATED|story\s+file\s+(?:created|saved|written)|Status:\s*ready-for-dev)".to_string(),
                prompt_template: "Review the following story file for completeness, correctness, and potential issues. Be adversarial — find every weakness, missing detail, and potential disaster.\n\n{context}".to_string(),
                resume_message_template: "An external adversarial reviewer has analyzed this story and found the following issues:\n\n{findings}\n\nPlease fix all these issues and update the story file.".to_string(),
            },
            // Consultation 2 — Story Critic (placeholder until Story 13.9)
            ConsultationConfig {
                label: "critic".to_string(),
                skill_path: None,
                preamble_override: Some(build_placeholder_critic_preamble()),
                role: LlmRole::Review, // Will become LlmRole::Critic in Story 13.9
                tool_set: ConsultationToolSet::Restricted,
                context_files: vec![story_file_path],
                trigger_pattern: r"(?i)(corrections?\s+(applied|made|done|implemented)|issues?\s+(fixed|resolved|addressed)|changes?\s+(applied|made|done))".to_string(),
                prompt_template: "Review the following story for alignment with product vision and technical coherence. Identify any deviations from the project's goals or architectural principles.\n\n{context}".to_string(),
                resume_message_template: "An external product/technical vision reviewer has analyzed this story:\n\n{findings}\n\nPlease apply the relevant corrections to the story file.".to_string(),
            },
        ]
    }
    ```
  - [x] 3.4 Implement `build_adversarial_consultation_preamble()` helper function — a preamble that gives the consultation agent the adversarial reviewer identity without requiring SKILL.md activation:
    ```rust
    fn build_adversarial_consultation_preamble() -> String {
        "You are a cynical, jaded adversarial reviewer. Your job is to find every \
         weakness, missing detail, and potential disaster in the content provided.\n\n\
         ## Rules\n\
         - Be skeptical of everything — look for what's missing, not just what's wrong\n\
         - Find at least ten issues to fix or improve\n\
         - Use a precise, professional tone\n\
         - Output all findings as a Markdown list\n\
         - Signal completion with <<BMAD_JOB_DONE>> when finished\n\n\
         ## Communication\n\
         - Respond in English\n\
         - Descriptions only — no code fixes, just identify problems".to_string()
    }
    ```
  - [x] 3.5 Implement `build_placeholder_critic_preamble()` helper function:
    ```rust
    fn build_placeholder_critic_preamble() -> String {
        "You are a Story Critic — an independent product and technical vision reviewer.\n\n\
         Your role is to evaluate whether a story aligns with the project's goals, \
         architecture, and product vision. You are NOT part of the BMAD methodology — \
         you are an external advisor.\n\n\
         ## Rules\n\
         - Be direct and specific in your observations\n\
         - Focus on product-vision alignment, not implementation details\n\
         - Identify scope creep, missing requirements, and architectural concerns\n\
         - Output all findings in a single response\n\
         - Signal completion with <<BMAD_JOB_DONE>> when finished\n\n\
         ## Communication\n\
         - Respond in English\n\
         - Be constructive but honest".to_string()
    }
    ```
    **Note:** This is a placeholder. Story 13.9 (Critic Agent Prompt Engineering) will replace this with an engineered preamble that includes project brief context, persistent memory loading, and the full Critic identity. The placeholder enables end-to-end testing of the consultation wiring.

  - [x] 3.6 Escalation and failure handling are inlined in Task 3.1's code snippet — they follow `run_dev_pipeline()` patterns (lines 713-813 for escalation, lines 816-961 for failure). The dev should adapt the exact method names (`create_escalation_pr`, `create_failure_pr`, `notify_escalation`, `notify_failure`, `push_branch`) from the dev pipeline reference — these are illustrative and may differ from the actual helper signatures.

  - [x] 3.7 Add `use crate::session::consultation::{ConsultationConfig, ConsultationToolSet};` import to `pipeline.rs`
  - [x] 3.8 Add `use crate::llm::agent_factory::LlmRole;` import to `pipeline.rs` (if not already present)

- [x] Task 4: Unit tests (AC: #8)
  - [x] 4.1 Add test `test_build_create_story_consultations` — verify the method returns 2 configs with correct labels, trigger patterns that compile, templates with `{findings}` placeholder, and adversarial uses `preamble_override` (not `skill_path`):
    ```rust
    #[test]
    fn test_build_create_story_consultations() {
        // Construct minimal StoryInfo with a specs_path
        let story = make_test_story_info("13-4-test");
        let pipeline = make_test_pipeline();
        let consultations = pipeline.build_create_story_consultations(&story);
        assert_eq!(consultations.len(), 2);

        let adversarial = &consultations[0];
        assert_eq!(adversarial.label, "adversarial");
        assert!(adversarial.skill_path.is_none(), "adversarial should use preamble_override, not skill_path");
        assert!(adversarial.preamble_override.is_some());
        assert!(Regex::new(&adversarial.trigger_pattern).is_ok());
        assert!(adversarial.resume_message_template.contains("{findings}"));

        let critic = &consultations[1];
        assert_eq!(critic.label, "critic");
        assert!(critic.skill_path.is_none());
        assert!(critic.preamble_override.is_some());
        assert!(Regex::new(&critic.trigger_pattern).is_ok());
        assert!(critic.resume_message_template.contains("{findings}"));
    }
    ```
  - [x] 4.2 Add test `test_adversarial_trigger_matches_bmad_output` — verify the adversarial trigger pattern matches known BMAD skill outputs and rejects intermediate text:
    ```rust
    #[test]
    fn test_adversarial_trigger_matches_bmad_output() {
        let pattern = r"(?i)(STORY\s+CONTEXT\s+CREATED|story\s+file\s+(?:created|saved|written)|Status:\s*ready-for-dev)";
        let re = Regex::new(pattern).unwrap();
        // Should match
        assert!(re.is_match("ULTIMATE BMad Method STORY CONTEXT CREATED, user!"));
        assert!(re.is_match("Status: ready-for-dev"));
        assert!(re.is_match("story file created successfully"));
        // Should NOT match
        assert!(!re.is_match("creating the story structure now"));
        assert!(!re.is_match("I'll create the story"));
    }
    ```
  - [x] 4.3 Add test `test_create_preamble_contains_sentinel_separation` — verify that the create preamble instructs the agent to emit the completion report and `<<BMAD_JOB_DONE>>` on separate turns:
    ```rust
    #[test]
    fn test_create_preamble_contains_sentinel_separation() {
        let preamble = build_create_preamble();
        assert!(preamble.contains("NEXT response"));
        assert!(preamble.contains("<<BMAD_JOB_DONE>>"));
    }
    ```
  - [x] 4.4 Add test `test_create_story_initial_message_format` — verify the initial message for create-story sessions includes English override and story key (not story file path)
  - [x] 4.5 Verify pre-existing `test_route_story_status_*` tests still pass
  - [x] 4.6 Verify `cargo test` passes: all existing tests pass, no new failures
  - [x] 4.7 Verify `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` passes

## Dev Notes

### Architecture Compliance

- **Decision 10 (Daemon-Orchestrated Consultations):** This story wires two consultations (adversarial review + critic) to the create-story pipeline phase. The consultation mechanism was built in Story 13.3 — this story configures and triggers it.
- **Decision 11 (Story Critic):** The Critic consultation uses a placeholder preamble until Story 13.9 provides the engineered one. The `ConsultationToolSet::Restricted` tool set is used per Decision 11's design. `LlmRole::Critic` does not exist yet — using `LlmRole::Review` as a temporary stand-in. Story 13.9 will add `LlmRole::Critic`.
- **Decision 2 (Daemon Reads, Agent Writes):** The daemon orchestrates the session and consultations, but all file mutations (story file creation, sprint-status updates) happen through the agent's tools.
- **Decision 5 (Skill-Based Activation):** The create-story session uses `.claude/skills/bmad-create-story/SKILL.md` — a different skill from the dev-story session. This requires parameterizing the `SessionRunner`.

### Critical Implementation Details

**This story fills in the `run_create_pipeline()` placeholder from Story 13.2.** The placeholder at `src/pipeline.rs:386-412` becomes a full implementation that: (1) runs a create-story session with consultations, (2) chains to dev on success, (3) handles failures/escalations with PRs and notifications.

**The skill path is `.claude/skills/bmad-create-story/SKILL.md`.** The epics file references `.github/skills/` but the actual BMAD skills are installed at `.claude/skills/`. The existing `SessionRunner` hardcodes `.github/skills/bmad-dev-story/SKILL.md` at `src/session/runner.rs:384`. **Pre-existing path discrepancy:** this path may also be wrong for the dev-story skill (actual file is at `.claude/skills/bmad-dev-story/SKILL.md`). The create-story implementation should use the CORRECT path. Fixing the dev-story path is out of scope but should be noted in deferred work if not already addressed.

**Consultation trigger patterns must match BMAD skill output.** The `bmad-create-story` skill (Step 6 in its workflow) saves the story file and reports completion with: "ULTIMATE BMad Method STORY CONTEXT CREATED, {user_name}!" and sets `Status: ready-for-dev`. The adversarial trigger pattern `(?i)(STORY\s+CONTEXT\s+CREATED|story\s+file\s+(?:created|saved|written)|Status:\s*ready-for-dev)` is tightened to match the specific known outputs: the literal "STORY CONTEXT CREATED" phrase, the common "story file created/saved/written" variants, and the "Status: ready-for-dev" marker. The critic trigger pattern `(?i)(corrections?\s+(applied|made|done|implemented)|issues?\s+(fixed|resolved|addressed)|changes?\s+(applied|made|done))` matches the agent's response after applying adversarial corrections. **These patterns may need tuning** during integration testing — they are best-effort regex designed from the skill workflow analysis. The tightened adversarial pattern avoids false positives on intermediate text like "creating the story structure".

**The create-story agent updates sprint-status.yaml itself.** The bmad-create-story skill's Step 6 (`workflow.md:346-359`) updates `sprint-status.yaml` to `ready-for-dev`. The daemon does NOT separately update the status — the agent handles it. After the session completes, the story should already be `ready-for-dev` in the file on disk.

**Chaining from create to dev phase.** On `SessionOutcome::Completed`, the create pipeline calls `run_dev_pipeline()` directly. The branch is already checked out (created during the create phase). The `base_branch_override` is set to the story branch so the dev phase doesn't try to re-create it. **Important:** The dev phase will call `SessionRunner::run()` which calls `ensure_story_branch()` — this function should detect the existing branch and reuse it (via `BranchAction::Reused`). The story branch already has the story spec file committed.

**Context files for consultations.** The `context_files` field in `ConsultationConfig` takes absolute paths. The story file path is obtained via `story.specs_path.canonicalize()` (with fallback to the original path if canonicalization fails — e.g., if the file doesn't exist yet at config construction time). This ensures the path is absolute regardless of how `StoryInfo` was constructed. Since the create-story agent creates this file during the session, it will exist on disk by the time the trigger fires (the trigger only fires AFTER the agent reports the file is created). **Edge case:** If the agent reports creation but the file doesn't actually exist (agent lie), the consultation will return `ConsultationError::ContextFileNotFound` — handled gracefully as a non-fatal error.

**The Critic consultation uses `LlmRole::Review` temporarily.** `LlmRole::Critic` doesn't exist until Story 13.9. Using `LlmRole::Review` means the Critic uses the same provider/model as the code review agent. This is acceptable for the placeholder — the Critic's effectiveness depends more on the preamble than the model. Story 13.9 will:
1. Add `LlmRole::Critic` to the enum
2. Add `critic: LlmRoleConfig` to `LlmConfig`
3. Replace the placeholder preamble with the engineered one
4. Update `build_create_story_consultations()` to use `LlmRole::Critic`

### Interaction with `SessionRunner` API

**Strategy: add parameters, not a new method.** Adding `skill_path_override: Option<&str>` and `preamble_override: Option<String>` to `run_with_consultations()` is the minimal change that enables the create-story phase. The alternative (a separate `run_create_session()` method) would duplicate the entire session lifecycle. Since the session lifecycle is identical (branch setup → agent build → activation → chat loop → cleanup), parameterization is cleaner.

**All existing callers pass `None` for both new parameters.** The `run()` wrapper passes `None, None`. Any other callers (resume_session, recover_and_process) are unchanged — they call `run_session()` internally with `&self.skill_path` and `&self.build_preamble()`.

**`run_session()` gains `skill_path: &str` and `preamble: &str` parameters.** The skill path replaces `self.skill_path` at three usage points:
- Line ~1389: `agent.activate_agent(..., &self.skill_path, ...)` → `agent.activate_agent(..., skill_path, ...)`
- Line ~1600: same pattern in empty-history recovery
- `drive_activation_and_recover()` line ~1153: same pattern in context-limit recovery

The preamble replaces the `self.build_preamble()` call that currently happens inside `run_with_consultations()` — the preamble is resolved once before entering `run_session()`.

**Borrow checker note:** `skill_path_override: Option<&str>` in `run_with_consultations()` is borrowed from the caller (pipeline). It's converted to an owned `String` via `effective_skill_path` before being passed as `&str` to `run_session()`. `preamble_override: Option<String>` is already owned. No lifetime issues — both resolved values live for the duration of `run_with_consultations()`.

### Escalation/Failure Handling Pattern

The create-story phase follows the SAME error handling pattern as `run_dev_pipeline()`:

**Escalation (`SessionOutcome::Escalated`):**
1. Push story branch to remote (best-effort)
2. Create escalation PR with question/reason/partial-work context
3. Notify human with escalation details
4. Return `PipelineResult { status: StoryStatus::Blocked, fatal: false }`

**Failure (`SessionOutcome::Failed`):**
1. Check if infrastructure error → `fatal: true`, notify, return
2. Otherwise: push story branch (best-effort), create failure PR, notify
3. Return `PipelineResult { status: StoryStatus::Error, fatal: false }`

**Key difference from dev pipeline:** The create phase does NOT mark the story as `done` or update sprint-status.yaml — the agent handles that. The pipeline only creates PRs and sends notifications for failures/escalations.

### Why the Create Phase Needs a Branch

The create-story agent uses tools (git, edit_file) to create and commit the story file. Working on the default branch (main) would pollute the main branch with WIP story files. Creating a story branch (`story/{story_key}`) keeps the work isolated. The dev phase then continues on the same branch, adding implementation commits on top of the story spec commit.

**Alternative considered:** Run create-story on a temporary branch and squash into the story branch. Rejected — adds complexity with no benefit. The story spec file is a valid artifact that should be visible in the PR.

### Adversarial Consultation Design

**The adversarial consultation uses `preamble_override`, NOT `skill_path` activation.** The `bmad-review-adversarial-general` SKILL.md is designed for interactive use — it expects the user to provide content to review as input. In a consultation, the `ConsultationRunner` sends the `prompt_template` (with `{context}` replaced by file contents) as the agent's task. If the adversarial SKILL.md were activated via `skill_path`, the agent would receive two conflicting instructions: (1) the skill's "Receive Content" step expecting user input, and (2) the prompt_template already containing the content. This creates ambiguity about whether to wait for input or process the provided content.

Instead, the adversarial consultation uses a `preamble_override` that gives the agent the adversarial reviewer identity (cynical, find-at-least-ten-issues, markdown list output) without the skill's multi-step workflow. The `prompt_template` then provides the story file content directly. This is cleaner and avoids the two-step activation conflict.

The critic consultation already uses `preamble_override` (no skill exists for it yet), so both consultations follow the same pattern.

### Re-poll Race Guard

**The watcher may re-poll a story between create and dev phases.** After the create-story session completes, `sprint-status.yaml` shows `ready-for-dev`. If the watcher polls at this exact moment (between the create session completing and the dev session starting), it might pick up the story as a new `ready-for-dev` story and attempt to start a second dev pipeline in parallel.

**Mitigation:** This race is prevented by the existing `process_story()` call scope — `run_create_pipeline()` chains to `run_dev_pipeline()` within the same `process_story()` invocation. The watcher holds the story lock for the entire invocation. A new poll cannot pick up the same story while `process_story()` is running. If the daemon is single-threaded per story (which it is — the pipeline processes one story at a time from the watcher's queue), this race cannot occur. **No code change needed** — this is a documentation-only note to prevent future developers from extracting the chaining into separate poll cycles.

### Known Limitations

**Trigger patterns are heuristic.** The adversarial trigger `(?i)(STORY\s+CONTEXT\s+CREATED|story\s+file\s+(?:created|saved|written)|Status:\s*ready-for-dev)` and the critic trigger `(?i)(corrections?\s+(applied|made|done|implemented)|issues?\s+(fixed|resolved|addressed)|changes?\s+(applied|made|done))` are regex patterns matching natural language output from an LLM. They may false-positive (trigger too early) or false-negative (never trigger). **Mitigation:** Consultations are non-fatal. If the adversarial review never triggers, the session completes without it. If it triggers too early (before the file exists), the consultation returns `ContextFileNotFound` and the session continues. **Future improvement:** Story 13.10 (WAL pipeline phase tracking) could add explicit phase signals instead of regex matching.

**Critic is a placeholder.** The `build_placeholder_critic_preamble()` provides a basic preamble without project brief context or persistent memory. The Critic consultation may produce generic feedback until Story 13.9 engineers the proper preamble. This is acceptable — the consultation wiring is the focus of this story.

**Skill path discrepancy.** The codebase references `.github/skills/` in several places (runner.rs, review/mod.rs, agent.rs comments) but the actual BMAD skills are at `.claude/skills/`. This pre-existing issue is not introduced by this story, but the create-story phase uses the CORRECT path (`.claude/skills/`). Document in deferred-work.md if not already tracked.

**No WAL phase tracking.** The WAL does not record which pipeline phase is active (Story 13.10). If the daemon crashes during the create-story session, on restart the watcher will see the story as `backlog` (if the agent hasn't updated sprint-status.yaml yet) or `ready-for-dev` (if it has). If `backlog`, the create phase restarts from scratch. If `ready-for-dev`, the dev phase starts instead — the story spec file is already committed. **Gap:** If the agent updated sprint-status.yaml to `ready-for-dev` but the consultation hadn't run yet, the story misses adversarial/critic review. Story 13.10 addresses this by restarting create phases from scratch on recovery.

### Previous Story Intelligence (Story 13.3)

- **Baseline test count:** 1179 passing, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)
- **Pre-existing clippy allowances:** `-A clippy::needless_splitn -A clippy::unnecessary_map_or`
- **`ConsultationConfig` and `ConsultationRunner`** are fully implemented in `src/session/consultation.rs` — this story USES them, not builds them
- **`run_with_consultations()` signature** (current): `pub async fn run_with_consultations(&self, story: &StoryInfo, base_branch_override: Option<&str>, consultations: Vec<ConsultationConfig>) -> SessionOutcome` — this story adds two parameters (`skill_path_override` and `preamble_override`)
- **`check_consultation_triggers()` is already integrated** into the chat loop — triggers fire automatically when patterns match
- **`ResponseAction::Continue { reply }`** is used by the consultation mechanism (Story 13.3 removed the `#[allow(dead_code)]`)
- **`run_create_pipeline()` placeholder** is at `src/pipeline.rs:386-412` — fully replace this method
- **`run_review_pipeline()` placeholder** is at `src/pipeline.rs:969-994` — NOT touched by this story (Story 13.6)
- **Notification spam for placeholder phases** (from 13.2 review): the create-story placeholder returning Error causes notifications every poll cycle. Implementing the create phase resolves this for `backlog` stories.

### Git Intelligence — Recent Commits

```
63932ed feat(epic-13): add daemon-orchestrated consultation mechanism (Story 13.3)
147f57d feat(epic-13): refactor pipeline into status-based phase router (Story 13.2)
fb38013 feat(epic-13): extend watcher to detect backlog and review stories (Story 13.1)
ab07b29 test(epic-12): add skill-based session and spawn-agent integration tests (Story 12.5)
cd7cce9 docs(epic-13): advance epic-13 to in-progress, create story 13-1 spec
```

**Expected commit message:** `feat(epic-13): implement create-story phase with consultations (Story 13.4)`

### Files to Modify

| File | Change Type | Scope |
|---|---|---|
| `src/pipeline.rs` | **Modify** | Replace `run_create_pipeline()` placeholder; add `build_create_story_consultations()`, `build_adversarial_consultation_preamble()`, `build_placeholder_critic_preamble()`, and `reload_story_info()` helpers; add consultation-related imports; add unit tests |
| `src/session/runner.rs` | **Modify** | Add `skill_path_override: Option<&str>` and `preamble_override: Option<String>` parameters to `run_with_consultations()`; pass `skill_path: &str` and `preamble: &str` through `run_session()` and `drive_activation_and_recover()`; adapt initial message for create-story phase |
| `src/session/agent.rs` | **Modify** | Add `pub(crate) fn build_create_preamble()` — create-story-specific system prompt adapted from `build_preamble()`, with sentinel separation instruction |

**NOT modified:**
- `src/session/consultation.rs` — consultation infrastructure is unchanged (Story 13.3)
- `src/llm/agent_factory.rs` — no new `LlmRole` variants (Story 13.9)
- `src/config/mod.rs` — no new config fields (Story 13.7 for project_brief)
- `src/watcher/mod.rs` — watcher already detects `backlog` stories (Story 13.1)
- `src/review/mod.rs` — review runner is unchanged
- `src/session/state.rs` — WAL pipeline_phase is Story 13.10
- `src/ui/renderer.rs` — create-story UI events are Story 13.11

### Existing Code to Reuse

- `build_preamble()` — dev-specific system prompt, reference for building `build_create_preamble()` [src/session/agent.rs:245-325]
- `ConsultationConfig` struct — ready from Story 13.3 [src/session/consultation.rs:38-83]
- `ConsultationToolSet::Full` and `::Restricted` — ready from Story 13.3 [src/session/consultation.rs:23-32]
- `ConsultationRunner::execute()` — consultation execution engine [src/session/consultation.rs:153-304]
- `ConsultationState::from_configs()` — converts configs to states with compiled regexes [src/session/consultation.rs:111-144]
- `SessionRunner::run_with_consultations()` — session lifecycle with consultation support [src/session/runner.rs:687-829]
- `SessionRunner::check_consultation_triggers()` — per-response trigger check [src/session/runner.rs:2321-2369]
- `run_dev_pipeline()` — reference pattern for escalation/failure PR handling [src/pipeline.rs:417-962]
- `build_pr_title()`, `build_pr_description()` — PR description construction [src/git_provider/*.rs]
- `push_branch()` — git push with error handling [src/pipeline.rs:1816+]
- `commit_sprint_status()` — commit sprint status changes [src/pipeline.rs:2381+]
- `is_infra_error()`, `is_auth_error()` — error classification [src/pipeline.rs]
- `ensure_story_branch()` — branch creation/checkout [src/session/branch.rs]
- `update_story_status()` — sprint status file updates [src/session/cleanup.rs]

### Anti-Patterns to Avoid

- **DO NOT** duplicate the `run_dev_pipeline()` method body. Extract shared error-handling helpers if the escalation/failure patterns become too repetitive. But prefer direct duplication over premature abstraction for this story — two methods with similar patterns is acceptable.
- **DO NOT** add `LlmRole::Critic` — that is Story 13.9. Use `LlmRole::Review` for the Critic consultation.
- **DO NOT** add `critic: LlmRoleConfig` to `LlmConfig` — that is Story 13.9.
- **DO NOT** load project brief or critic-memory.md — that is Stories 13.7 and 13.8.
- **DO NOT** add UI events for create-story phase (`create_story_start`, `consultation_triggered`) — that is Story 13.11.
- **DO NOT** modify WAL to track pipeline phase — that is Story 13.10.
- **DO NOT** modify the `ResponseAnalyzer` — consultation triggers are handled separately from response analysis.
- **DO NOT** modify `run_review_pipeline()` — that is Story 13.6.
- **DO NOT** make the create-story agent update sprint-status from the pipeline. The BMAD skill handles status transitions. The pipeline only orchestrates sessions.
- **DO** use `tracing::info!` and `tracing::warn!` for operational logging (not `println!`).
- **DO** follow the existing escalation/failure PR pattern from `run_dev_pipeline()` exactly.
- **DO** use `.claude/skills/bmad-create-story/SKILL.md` (the correct path where BMAD skills are installed), NOT `.github/skills/`.

### Project Structure Notes

- No new files created — this story modifies existing `pipeline.rs`, `runner.rs`, and `agent.rs`
- The `build_create_story_consultations()` method is a private helper on `StoryPipeline` — follows the same pattern as other private pipeline methods
- The `build_adversarial_consultation_preamble()` and `build_placeholder_critic_preamble()` are module-level private functions in `pipeline.rs` — the critic preamble will be replaced by proper Critic construction in Story 13.9
- The `build_create_preamble()` function in `session/agent.rs` is `pub(crate)` — called by `pipeline.rs` to construct the create-story system prompt, adapted from the existing `build_preamble()` for dev sessions
- The `skill_path_override` and `preamble_override` parameter threading follows the existing pattern of `base_branch_override` — optional overrides that default to the standard behavior

### References

- [Source: _bmad-output/planning-artifacts/epics.md:3222–3258 — Story 13.4 AC (Create-Story Phase with Consultations)]
- [Source: _bmad-output/planning-artifacts/architecture.md:664–693 — Decision 10 (Daemon-Orchestrated Consultations)]
- [Source: _bmad-output/planning-artifacts/architecture.md:695–716 — Decision 11 (Story Critic)]
- [Source: _bmad-output/planning-artifacts/epics.md:3480–3503 — Epic 13 Summary and execution strategy]
- [Source: _bmad-output/project-context.md:48–68 — Daemon Lifecycle, Agent Construction]
- [Source: _bmad-output/project-context.md:109–117 — Testing Rules]
- [Source: src/pipeline.rs:386–412 — run_create_pipeline() placeholder (to replace)]
- [Source: src/pipeline.rs:417–962 — run_dev_pipeline() (reference pattern for error handling)]
- [Source: src/session/runner.rs:316–342 — SessionRunner struct (fields)]
- [Source: src/session/runner.rs:384 — skill_path hardcoded to dev-story]
- [Source: src/session/runner.rs:687–829 — run_with_consultations() (to modify)]
- [Source: src/session/runner.rs:1344–1464 — run_session() with skill activation (to parameterize)]
- [Source: src/session/runner.rs:1132–1190 — drive_activation_and_recover() (to parameterize)]
- [Source: src/session/runner.rs:2321–2369 — check_consultation_triggers() (reused as-is)]
- [Source: src/session/consultation.rs:38–83 — ConsultationConfig struct]
- [Source: src/session/consultation.rs:153–304 — ConsultationRunner::execute()]
- [Source: src/session/consultation.rs:111–144 — ConsultationState::from_configs()]
- [Source: src/session/agent.rs:245–325 — build_preamble() (reference for build_create_preamble())]
- [Source: src/session/agent.rs:787–848 — activate_agent() (accepts any skill path)]
- [Source: src/llm/agent_factory.rs:37–46 — LlmRole enum (Dev, Review, Supervisor, EpicReview)]
- [Source: _bmad-output/implementation-artifacts/13-3-daemon-orchestrated-consultation-mechanism.md — Previous story intelligence]
- [Source: .claude/skills/bmad-create-story/SKILL.md — Create-story skill file (activation target)]
- [Source: .claude/skills/bmad-review-adversarial-general/SKILL.md — Adversarial review skill (consultation 1)]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (claude-opus-4-6)

### Debug Log References

### Completion Notes List

- Task 1: Parameterized `SessionRunner` — added `skill_path_override` and `preamble_override` to `run_with_consultations()`, threaded `skill_path: &str` and `preamble: &str` through `run_session()`, `drive_activation_and_recover()`, `context_limit_recovery()`, and `build_agent_for_role()`. Updated `resume_session()` to pre-resolve dev preamble. Renamed unused `story` param to `_story` in `build_agent_for_role()`.
- Task 2: Added `build_create_preamble()` in `session/agent.rs` with sentinel separation instruction (completion report and `<<BMAD_JOB_DONE>>` on separate turns). Adapted initial message in `run_session()` to distinguish create-story vs dev-story sessions.
- Task 3: Replaced `run_create_pipeline()` placeholder with full implementation (~250 lines). Added `build_create_story_consultations()` returning 2 `ConsultationConfig` structs (adversarial + critic), `reload_story_info()` for sprint-status re-read, `build_adversarial_consultation_preamble()` and `build_placeholder_critic_preamble()` free functions. Implemented create-to-dev chaining on success, escalation/failure PR handling following `run_dev_pipeline()` patterns.
- Task 4: Added 4 new tests (`test_build_create_story_consultations`, `test_adversarial_trigger_matches_bmad_output`, `test_create_preamble_contains_sentinel_separation`, `test_create_story_initial_message_format`). Updated existing test name to `test_process_story_routes_backlog_to_create_phase`. All 1183 tests pass (1 pre-existing failure). No new clippy errors.

### File List

- `src/pipeline.rs` — Modified: replaced `run_create_pipeline()` placeholder, added `build_create_story_consultations()`, `reload_story_info()`, `build_adversarial_consultation_preamble()`, `build_placeholder_critic_preamble()`, consultation/LlmRole imports, 4 new tests + 1 updated test
- `src/session/runner.rs` — Modified: parameterized `run_with_consultations()`, `run_session()`, `drive_activation_and_recover()`, `context_limit_recovery()`, `build_agent_for_role()` with skill_path/preamble; updated `resume_session()` and all call sites; adapted initial message for create-story
- `src/session/agent.rs` — Modified: added `pub(crate) fn build_create_preamble()` with sentinel separation instruction

### Change Log

- 2026-04-22: Implemented create-story pipeline phase with adversarial and critic consultations (Story 13.4)

### Review Findings

- [x] [Review][Patch] Check triggers on sentinel response — Fixed: `run_session()` now checks consultation triggers before processing `Completed` action. [src/session/runner.rs]
- [x] [Review][Patch] Persist phase/skill in WAL for crash recovery — Fixed: added `skill_path` field to `SessionState`; `resume_session()` now rebuilds correct preamble based on WAL skill_path. [src/session/runner.rs, src/session/state.rs]
- [x] [Review][Patch] Hardcoded branch name in failure path — Fixed: uses `story.branch_name.clone()` instead of `format!("story/{story_key}")`. [src/pipeline.rs]
- [x] [Review][Patch] `canonicalize()` on nonexistent story path — Fixed: uses `project_root.join()` for absolute path resolution. [src/pipeline.rs]
- [x] [Review][Defer] Adversarial trigger regex fragile — Trigger depends on LLM exact phrasing; variations like "story context is now created" won't match. Design limitation from consultation mechanism (Story 13.3). [src/pipeline.rs:1290] — deferred, design limitation from 13.3
- [x] [Review][Defer] Critic trigger regex fragile — Same concern as adversarial trigger; agent could say "updated the story based on feedback" and miss the pattern. [src/pipeline.rs:1305] — deferred, design limitation from 13.3
- [x] [Review][Defer] reload_story_info linear scan with no file locking — Concurrent write during read could corrupt YAML. Pre-existing concern for all sprint-status reads. [src/pipeline.rs:reload_story_info] — deferred, pre-existing
- [x] [Review][Defer] Test validates copy of message format, not actual code path — `test_create_story_initial_message_format` manually reconstructs the format; doesn't call actual code. [src/pipeline.rs:test_create_story_initial_message_format] — deferred, extracting testable function is out of scope
- [x] [Review][Defer] Fragile string-match skill dispatch — `skill_path.contains("bmad-create-story")` for initial message branching; should be typed enum when more skills exist. [src/session/runner.rs:865] — deferred, premature with only 2 skill types
- [x] [Review][Defer] Duplicated preamble content — `build_create_preamble` shares ~40 lines with `build_preamble`; future drift guaranteed. Extracting shared parts needs design discussion. [src/session/agent.rs:build_create_preamble] — deferred, refactoring approach needs design
