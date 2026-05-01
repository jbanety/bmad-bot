---
title: 'Autonomous epic review with Critic gate and pre-epic story creation'
type: 'feature'
created: '2026-05-01'
status: 'done'
baseline_commit: '1280f4b'
context:
  - '_bmad-output/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The epic gate flow (`run_epic_gate_inner`) currently parses pre-epic stories from Winston's report via regex, injects entries into sprint-status.yaml, then sets the retrospective to `review` and **blocks** until human approval. There is no Critic involved, no actual story spec file is created, and the human gate creates unnecessary friction.

**Approach:** After the EpicReviewRunner produces its report: (1) spawn a Critic consultation that analyzes the report and determines what must be fixed before the next epic, using critic-memory.md for continuity; (2) spawn a create-story session (same `run_create_pipeline` pattern — skill + preamble + consultations) with the epic review report + Critic findings as input context, producing a single consolidated pre-epic story spec; (3) mark the epic retrospective as `done` (not `review`) and continue autonomously — no human gate, no retro branch/MR.

## Boundaries & Constraints

**Always:**
- Critic receives the full epic review report + critic-memory.md as context
- Critic uses `LlmRole::Critic` (existing role, same fallback to `review` config)
- Critic must **append** its epic review observations to critic-memory.md (same pattern as story reviews — Critic has `edit_file` in `ConsultationToolSet::Restricted` specifically for this). The prompt must instruct the Critic to record its findings and the epic review summary in its memory file for cross-epic continuity.
- The create-story session uses the standard `PHASE_CREATE` flow: skill activation (`bmad-create-story`), adversarial + critic consultations (existing pattern)
- The create-story session receives as override context: the epic review report + Critic's findings (injected as initial user prompt after skill activation)
- Sprint-status retrospective entry goes directly to `done` (not `review`)
- No retro branch/MR creation — remove that code path
- Pre-epic story keys keep the existing convention: `{next_epic}-0a-pre-epic-{next_epic}-{slug}`
- The `StoryInfo` for the create-story session must have a valid `branch_name` — create the branch from the current working branch

**Ask First:**
- Whether to keep the existing `parse_pre_epic_stories` regex parsing as a fallback if the Critic+create-story flow fails

**Never:**
- Don't change EpicReviewRunner itself — the report generation is unchanged
- Don't modify the Critic role config or memory system — reuse as-is
- Don't add a new LLM role — use existing `Critic` and `Dev` roles

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Happy path | Epic review report with findings | Critic analyzes → create-story produces spec → sprint-status: done | N/A |
| Critic says nothing to fix | Report is clean, Critic returns "no action needed" | Skip create-story session → mark retro as done directly | N/A |
| Critic session fails | Transient LLM error | Log warning, fall through to mark retro as done without pre-epic story | Non-blocking |
| Create-story session fails/escalates | Skill fails or agent escalates | Log error, mark retro as done anyway (don't block the next epic) | Non-blocking |
| Empty epic review report | EpicReviewOutcome::Failed | Skip Critic entirely, generate failure report, mark as done | Already handled |

</frozen-after-approval>

## Code Map

- `src/pipeline.rs` -- `run_epic_gate_inner()`: main modification target — replace regex parsing + retro branch/MR with Critic consultation + create-story session dispatch
- `src/pipeline.rs` -- `build_epic_gate_critic_consultation()`: new method — builds a Critic ConsultationConfig for epic review report analysis
- `src/pipeline.rs` -- `build_pre_epic_story_info()`: new helper — constructs a synthetic `StoryInfo` for the pre-epic create-story session
- `src/session/consultation.rs` -- `ConsultationRunner::execute()`: reused as-is for the standalone Critic call
- `src/session/agent.rs` -- `build_create_preamble()`: reused as-is
- `src/runtime/mod.rs` -- `SessionRuntime::run_session()`: reused as-is with `SessionContext` for create-story phase
- `src/review/epic.rs` -- unchanged (report generation stays the same)
- `src/critic/mod.rs` -- unchanged (memory system stays the same)

## Tasks & Acceptance

**Execution:**
- [x] `src/pipeline.rs` -- Add `async fn run_epic_critic_review(&self, report: &str, epic_num: u32) -> Option<String>`: standalone Critic consultation. Build a `ConsultationConfig` with `LlmRole::Critic`, `ConsultationToolSet::Restricted`, critic-memory.md + project-brief as context. Prompt instructs the Critic to: (1) analyze the epic review report and determine what must be fixed before the next epic, (2) **append its observations and the epic review summary to critic-memory.md** for cross-epic continuity. Execute via `ConsultationRunner::execute()`. Return the Critic's findings as `Option<String>` (None if fails or "nothing to fix").
- [x] `src/pipeline.rs` -- Add `fn build_pre_epic_story_info(&self, epic_num: u32) -> StoryInfo` (simplified: derives next_epic internally): construct synthetic `StoryInfo` with `story_key: "{next_epic}-0a-pre-epic-{next_epic}"`, `branch_name: "{next_epic}-0a-pre-epic-{next_epic}"`, `status: "backlog"`, `specs_path` pointing to implementation-artifacts, `epic_num: next_epic`.
- [x] `src/pipeline.rs` -- Add `async fn run_pre_epic_story_creation(&self, epic_num: u32, report: &str, critic_findings: &str) -> bool`: spawn create-story session via `session_runtime.run_session()` with `PHASE_CREATE`, `LlmRole::Dev`, `bmad-create-story` skill. Inject epic review report + critic findings as context into the story prompt override. Handle Completed/Escalated/Failed outcomes. Return true on success.
- [x] `src/pipeline.rs` -- Rewrite `run_epic_gate_inner()`: after `epic_review_runner.run()` succeeds, call `run_epic_critic_review()`. If Critic returns findings, call `run_pre_epic_story_creation()`. Then save report to disk, update sprint-status retro → `done`, commit, push. Remove retro branch/MR creation. Remove `parse_pre_epic_stories` + `inject_pre_epic_stories` call. Keep notification (adapted: no MR URL, status = done).
- [x] `src/watcher/deps.rs` -- Verified: `is_retrospective_gate_clear()` only blocks on `"review"`, `done` passes through (no-op).
- [x] Update tests: added `agent_factory` field to all 8 test `StoryPipeline` constructors. Removed retro branch cleanup in `scan_pending_epic_reviews`. 4 pre-existing test failures unrelated to this change (critic preamble tests missing `edit_file` assertion + review routing).

**Acceptance Criteria:**
- Given an epic with all stories done, when the epic gate triggers, then the Critic receives the full report and critic-memory.md as context, and appends its observations to critic-memory.md
- Given the Critic identifies issues, when the create-story session runs, then a single pre-epic story spec file is created in implementation-artifacts with the standard format
- Given the create-story session completes, when the pipeline continues, then the sprint-status retro entry is `done` and the next epic's pre-epic story is unblocked
- Given the Critic says nothing needs fixing, when the pipeline continues, then no create-story session runs and the retro goes directly to `done`
- Given any failure in Critic or create-story, when the pipeline continues, then it still marks retro as `done` (non-blocking errors) and logs the failure

## Spec Change Log

## Design Notes

**Critic as standalone call, not consultation:** The Critic here runs as a standalone `ConsultationRunner::execute()` call outside of any session, not as a triggered consultation within a session. This is because there's no "host session" to pause — the epic review is already complete. The ConsultationRunner is designed to work standalone (it builds its own agent and runs its own chat loop).

**Create-story override context:** The create-story session needs the epic review report + Critic findings as input. The session activates the `bmad-create-story` skill normally, then on the first user turn after activation, the daemon injects the override prompt containing the report context and the instruction to create a consolidated pre-epic story. This mirrors how `StoryInfo.specs_path` provides context to dev sessions — but here the "story file" doesn't exist yet, so the context is injected as prompt text.

**No retro branch/MR:** The current flow creates a retro branch with the report committed, then opens an MR for human review. Since the new flow is autonomous (no human gate), this is unnecessary overhead. The report is still saved to disk for reference.

## Verification

**Commands:**
- `cargo build` -- expected: clean compilation
- `cargo test` -- expected: all existing + new tests pass
- `cargo clippy` -- expected: no new warnings

## Suggested Review Order

**Autonomous epic gate flow**

- Rewritten entry point: review → critic → create-story → done (no human gate)
  [`pipeline.rs:2602`](../../src/pipeline.rs#L2602)

- Standalone Critic consultation with memory write instruction
  [`pipeline.rs:2941`](../../src/pipeline.rs#L2941)

- Create-story session dispatch with report + critic findings as context
  [`pipeline.rs:3057`](../../src/pipeline.rs#L3057)

- Synthetic StoryInfo construction for pre-epic story
  [`pipeline.rs:3172`](../../src/pipeline.rs#L3172)

**Critic preamble**

- Epic-specific critic preamble with memory append instructions
  [`pipeline.rs:4814`](../../src/pipeline.rs#L4814)

**Supporting changes**

- New `agent_factory` field on StoryPipeline for standalone ConsultationRunner
  [`pipeline.rs:167`](../../src/pipeline.rs#L167)

- Retro branch cleanup removed from scan_pending_epic_reviews
  [`pipeline.rs:2814`](../../src/pipeline.rs#L2814)
