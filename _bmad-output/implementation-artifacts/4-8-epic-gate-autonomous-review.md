# Story 4.8: Epic Gate — Autonomous Retrospective Review

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the daemon to automatically pause between epics, run an autonomous LLM-driven codebase review, and block the next epic until a human validates the report,
So that architectural drift, pattern inconsistencies, and technical debt are caught at epic boundaries instead of compounding silently across the entire project.

## Acceptance Criteria

### Epic Completion Detection

1. **Given** the pipeline has just finished processing a story via `process_story()`
   **When** the story completes successfully (status = `Completed`)
   **Then** the pipeline re-reads sprint-status.yaml from disk (fresh load, NOT the cached SprintStatusFile from the poll cycle) and checks whether ALL `{epic_num}-*` story entries for the completed story's epic are `done`
   **And** retrospective entries (`epic-{X}-retrospective`) and epic entries (`epic-{X}`) are excluded from the story count

2. **Given** the last story of epic X has completed
   **When** the pipeline detects epic completion
   **Then** it launches the autonomous epic review process before continuing to the next story

3. **Given** the last story of epic X has completed but epic X has no `epic-X-retrospective` entry in sprint-status.yaml
   **When** the pipeline checks for the gate
   **Then** it skips the epic gate entirely and proceeds to the next eligible story (backward compatibility — projects without retrospective entries are unaffected)

### Autonomous Review Session

4. **Given** epic completion is detected and `epic-X-retrospective` entry exists
   **When** the review session launches
   **Then** a new `BuiltAgent` is constructed using `AgentFactory::build(LlmRole::EpicReview, &preamble, tools)` with the review tool set (read_file, grep, find_path, list_directory, terminal, git, think — NO edit_file, NO ask_supervisor)
   **And** the preamble is built from the Architect agent persona (`_bmad/bmm/agents/architect.md`) loaded as identity context, combined with the project-context.md as project rules context

5. **Given** the review agent is built
   **When** the review prompt is sent
   **Then** the prompt instructs the agent to use tools to load the epic definition, story files, and architecture doc (tool-based loading, NOT inlined — avoids context window issues for large epics)
   **And** the prompt provides the four-section report structure and instructs the agent to output the final report between `<<EPIC_REVIEW_REPORT_START>>` and `<<EPIC_REVIEW_REPORT_END>>` delimiters
   **And** the agent uses tools autonomously to explore the codebase, run `cargo check`, `cargo test`, `cargo clippy`, and analyze implementation patterns

6. **Given** the review session completes
   **When** the agent produces the report
   **Then** the report is extracted from the agent's output by finding content between the `<<EPIC_REVIEW_REPORT_START>>` and `<<EPIC_REVIEW_REPORT_END>>` delimiters
   **And** the report is saved to `{implementation_artifacts}/epic-{X}-retrospective-report.md`
   **And** if no delimiters are found, the full concatenated agent output is used as the report (graceful fallback)

7. **Given** the review session fails (LLM error, timeout, context window exhaustion)
   **When** the error is caught
   **Then** the sprint-status is still updated to `review` (the gate still activates — a failed review is MORE reason to pause, not less)
   **And** a minimal failure report is generated with the error details and saved in place of the review report
   **And** the failure is logged via `tracing::error!` and included in the notification
   **And** pipeline processing continues to the sprint-status update step (AC #10)

### Report Structure

8. **Given** the review agent is analyzing the epic
   **When** it produces the report
   **Then** the report contains these sections in order:
   - **Epic Recap**: Epic objectives, stories delivered, scope summary, what was planned vs what was actually built
   - **Functional Testing Guide**: Step-by-step scenarios the human can follow to verify each capability delivered in this epic — concrete CLI commands, expected outputs, edge cases to try — based on actual implementation (not just ACs)
   - **Technical Analysis**: Pattern consistency across stories, architecture adherence vs architecture.md, technical debt inventory (TODOs, shortcuts, known issues), cross-cutting concerns (test coverage, error handling uniformity, logging consistency), codebase health metrics (cargo check/test/clippy results)
   - **Recommendations**: Actionable items for the next epic, risks to watch, debt to address, patterns to reinforce or abandon

### Sprint-Status Update & Commit (Before Branch)

9. **Given** the review report has been generated (or the review session failed)
   **When** the pipeline updates sprint-status.yaml
   **Then** it updates `epic-X-retrospective` from `optional` to `review` in sprint-status.yaml ON THE CURRENT BRANCH (the last story's branch or target_branch — wherever HEAD is)
   **And** commits the sprint-status change with message `chore(sprint-status): epic {X} gate activated — awaiting human review`
   **And** pushes this commit so the watcher sees the gate on next poll
   **And** this happens BEFORE the retrospective branch is created

### Branch, Commit & MR

10. **Given** sprint-status has been updated and pushed
    **When** the pipeline creates the retrospective artifacts
    **Then** it creates a branch named `epic-{X}-retrospective` from the `last_completed_branch` (the sequential chaining branch — same base as the next story would use)
    **And** commits the report file with message `docs(retrospective): epic {X} autonomous review report`
    **And** pushes the branch to remote
    **And** creates a Merge Request / Pull Request via `GitProvider` with the report's Epic Recap section as MR description (NOT the full report — keep the MR description readable, link to the full file)
    **And** the MR title follows format: `Epic {X} Retrospective — Review Gate`

### Gate Enforcement

11. **Given** `epic-X-retrospective` is set to `review`
    **When** the watcher computes eligible stories
    **Then** ALL stories from epic X+1 and beyond are excluded from the eligible list
    **And** stories from epic X (if any remain) are still eligible (the gate blocks the NEXT epic, not the current one)
    **And** this check is performed in `watcher/deps.rs` as part of the existing pre-gate dependency resolution

12. **Given** `epic-X-retrospective` is set to `optional` (the initial state for all retrospective entries)
    **When** the watcher computes eligible stories
    **Then** `optional` is treated as "gate clear" — it does NOT block the next epic
    **And** only the `review` status (set by the daemon after epic completion) blocks the next epic
    **And** this ensures backward compatibility: existing projects with `optional` retro entries for completed epics continue working without any manual intervention

13. **Given** the human has reviewed the report and sets `epic-X-retrospective: done` in sprint-status.yaml
    **When** the next poll cycle runs
    **Then** stories from epic X+1 become eligible (subject to their own dependencies)
    **And** the daemon logs the gate clearance via `tracing::info!`

### Notification

14. **Given** the epic gate has been activated
    **When** notifications are enabled
    **Then** the daemon sends a notification (Telegram) with:
    - Epic number and name (parsed from the epic entry in sprint-status.yaml or the epics file heading, with fallback to `"Epic {X}"` if not parseable)
    - Link to the MR (if PR creation succeeded)
    - Summary: number of stories reviewed, pass/fail of cargo check/test/clippy
    - Clear instruction: "Set `epic-X-retrospective: done` in sprint-status.yaml to continue"
    **And** notification failures are non-blocking (logged but do not halt the pipeline)

### New LLM Role

15. **Given** the `LlmRole` enum exists with `Dev`, `Review`, `Supervisor`
    **When** the `EpicReview` variant is added
    **Then** `LlmConfig` gains an `epic_review: LlmRoleConfig` field with `#[serde(default)]`
    **And** if the field is absent from `bmad-bot.yaml`, it defaults to cloning the `review` role config at runtime (zero-config backward compatibility)
    **And** `AgentFactory::config_for_role(LlmRole::EpicReview)` resolves to the epic_review config
    **And** `AgentFactory::build()` handles the new role identically to `Review` (same provider construction paths)
    **And** `BotSecrets::validate_for_config()` includes the epic_review role in its provider validation loop

### Safety Nets

16. **Given** the epic review session is running
    **When** the agent is exploring the codebase
    **Then** a `MAX_EPIC_REVIEW_TURNS` constant (200) limits the maximum chat turns
    **And** the `ShutdownFlag` is checked between turns (same as ReviewRunner)
    **And** if the turn limit is reached, the daemon captures whatever output the agent has produced so far and treats it as the report (partial report is better than no report)

17. **Given** the epic review session encounters a transient error (network timeout, 429, 500)
    **When** the error is caught
    **Then** the session is retried up to `MAX_SESSION_RETRIES` (same constant as ReviewRunner) with a fresh agent
    **And** `is_retryable_review_error()` (from review/mod.rs) is reused for error classification

18. **Given** the daemon crashes during the epic review (before sprint-status is updated)
    **When** the daemon restarts and re-polls
    **Then** it detects the epic is complete and `epic-X-retrospective` is still `optional`
    **And** it re-runs the epic review from scratch (idempotent — the report file is overwritten)
    **And** this is acceptable because the epic review has no WAL — it is a stateless, repeatable analysis

## Tasks / Subtasks

- [x] Task 1: Add `LlmRole::EpicReview` + config plumbing (AC: #15)
  - [x] 1.1 Add `EpicReview` variant to `LlmRole` enum in `llm/agent_factory.rs`
  - [x] 1.2 Add `epic_review: LlmRoleConfig` to `LlmConfig` in `config/mod.rs` with `#[serde(default)]`
  - [x] 1.3 Implement `Default` for `LlmRoleConfig` (empty strings) and add runtime fallback: if `epic_review.provider` is empty, clone from `review` config
  - [x] 1.4 Update `config_for_role()` match arm
  - [x] 1.5 Update `BotSecrets::validate_for_config()` to include epic_review role (skip validation if provider is empty — it will inherit review's provider which is already validated)
  - [x] 1.6 Update `LlmRole::Display` impl
  - [x] 1.7 Add unit tests for new role (display, config resolution, default fallback, empty-provider-skips-validation)
- [x] Task 2: Epic completion detection in pipeline (AC: #1, #2, #3)
  - [x] 2.1 Add `fn detect_epic_completion(sprint_status_path: &Path, completed_story: &StoryInfo) -> Option<u32>` to `pipeline.rs` — re-reads sprint-status.yaml from disk (fresh), returns `Some(epic_num)` if all stories in the epic are done
  - [x] 2.2 Add `fn has_retrospective_entry(sprint_status: &SprintStatusFile, epic_num: u32) -> bool` — checks if `epic-{X}-retrospective` key exists
  - [x] 2.3 Call detection after `process_story()` returns `Completed` in `process_eligible_stories()`, AFTER the safety net sprint-status commit
  - [x] 2.4 Add unit tests for detection logic (all done, partial, no retro entry, single-story epic, fresh-read-not-stale)
- [x] Task 3: Gate enforcement in watcher (AC: #11, #12, #13)
  - [x] 3.1 Add `fn compute_retrospective_gates(statuses: &[(String, String)]) -> HashMap<u32, String>` to `watcher/deps.rs` — maps epic_num → retro status for all `epic-X-retrospective` entries
  - [x] 3.2 In the eligible story filtering logic, add retrospective gate check: for story in epic N, verify that `epic-(N-1)-retrospective` is NOT `review` (i.e., `done`, `optional`, or absent all clear the gate — ONLY `review` blocks)
  - [x] 3.3 Add unit tests for gate blocking: `review` blocks next epic, `done` allows, `optional` allows (backward compat!), absent allows, `optional` on incomplete epic allows
- [x] Task 4: Autonomous review session runner (AC: #4, #5, #6, #7, #8, #16, #17)
  - [x] 4.1 Create `src/review/epic.rs` — `EpicReviewRunner` struct (mirrors `ReviewRunner` pattern)
  - [x] 4.2 Implement `build_epic_review_preamble()` — loads architect.md persona (extract `<persona>` block only) + project-context.md + structured review instructions + English language override
  - [x] 4.3 Implement `build_epic_review_prompt()` — instructs agent to use tools to load epic definition and story files (NOT inlined), provides report structure template with `<<EPIC_REVIEW_REPORT_START>>`/`<<EPIC_REVIEW_REPORT_END>>` delimiters
  - [x] 4.4 Implement `run()` method — builds agent via `AgentFactory::build(LlmRole::EpicReview, ...)`, sends prompt, manages chat loop with `MAX_EPIC_REVIEW_TURNS = 200`, detects completion via delimiter or no-tool-call heuristic, supports retry via `is_retryable_review_error()`
  - [x] 4.5 Implement report extraction: scan all agent output for content between `<<EPIC_REVIEW_REPORT_START>>` and `<<EPIC_REVIEW_REPORT_END>>` delimiters; fallback to concatenating all non-tool-call agent messages if delimiters not found
  - [x] 4.6 Save report to `{implementation_artifacts}/epic-{X}-retrospective-report.md`
  - [x] 4.7 Handle failures gracefully: generate minimal failure report with error details, return `EpicReviewOutcome::Failed` with reason
  - [x] 4.8 Add `EpicReviewOutcome` enum: `Completed { report: String, epic_num: u32 }`, `Failed { reason: String, epic_num: u32 }`
- [x] Task 5: Sprint-status update — ON CURRENT BRANCH, BEFORE retro branch (AC: #9)
  - [x] 5.1 Update `epic-X-retrospective` from `optional` to `review` in sprint-status.yaml using existing `update_story_status()` pattern
  - [x] 5.2 Commit with `chore(sprint-status): epic {X} gate activated — awaiting human review`
  - [x] 5.3 Push current branch to remote so gate is visible to watcher on next poll
- [x] Task 6: Branch, commit & MR creation — AFTER sprint-status update (AC: #10)
  - [x] 6.1 Create branch `epic-{X}-retrospective` from `last_completed_branch` (sequential chaining base) via git CLI
  - [x] 6.2 Commit report file on the retro branch
  - [x] 6.3 Push retro branch to remote
  - [x] 6.4 Create MR/PR via `GitProvider::create_pr()` with the Epic Recap section as description (NOT the full report)
  - [x] 6.5 Checkout back to the working branch after MR creation (so pipeline can continue)
- [x] Task 7: Notification (AC: #14)
  - [x] 7.1 Add `notify_epic_gate()` method to `Notifier` trait (with default no-op impl)
  - [x] 7.2 Implement for `TelegramNotifier` — format message with epic info, MR link, review summary, unlock instructions
  - [x] 7.3 Epic name resolution: try parsing from epics.md heading (`## Epic {X}: {title}`), fallback to `"Epic {X}"`
- [x] Task 8: Pipeline integration (AC: #1-18 wired together)
  - [x] 8.1 Add `EpicReviewRunner` to `StoryPipeline` struct (constructed in `StoryPipeline::new()`)
  - [x] 8.2 Wire into `process_eligible_stories()`: after successful `process_story()` AND after safety net sprint-status commit → fresh-read detect_epic_completion → has_retrospective_entry → run epic review → update sprint-status (on current branch, push) → create retro branch/MR → notify
  - [x] 8.3 Ensure the gate check in watcher is active for the next re-poll within the same pipeline run
  - [x] 8.4 Pass `last_completed_branch` to the retro branch creation step
- [x] Task 9: Validation
  - [x] 9.1 `cargo build` — zero errors
  - [x] 9.2 `cargo test` — all existing + new tests pass (1248 tests: 80 lib + 1168 bin)
  - [x] 9.3 `cargo clippy` — zero warnings (fixed 8 collapsible-if lints across pipeline.rs, review/epic.rs, session/agent.rs, session/cleanup.rs, watcher/deps.rs)
  - [x] 9.4 `cargo fmt` — no formatting issues

## Dev Notes

### Triggered By

Production incident (2026-03-06) — daemon looped 4× re-processing 14 already-completed stories (42 wasted LLM sessions, 19 duplicate MRs) due to sprint-status.yaml divergence across parallel branch chains. Root cause fixed (sequential branch chaining), but exposed a deeper problem: no human checkpoint exists between epics. See `architect-brief-epic-gate-retrospective.md` for the full architect brief written by Amelia (Dev Agent).

### Architecture Pattern — Mirrors ReviewRunner

The `EpicReviewRunner` follows the exact same pattern as `ReviewRunner` in `src/review/mod.rs`:
- Struct with `config`, `secrets`, `agent_factory`, `analyzer`, `shutdown`, `mcp_manager`, `ui` fields
- `new()` constructor, `run()` public method with retry loop
- Builds a fresh `BuiltAgent` via `AgentFactory::build()`
- Manages a chat loop with `stream_chat()`
- Returns a typed outcome enum (`EpicReviewOutcome`)
- `MAX_EPIC_REVIEW_TURNS = 200` (higher than ReviewRunner's 100 — epic review explores more)
- `MAX_SESSION_RETRIES` reused from review/mod.rs (same constant or same value)
- `is_retryable_review_error()` reused from review/mod.rs (same function)

**Key differences from ReviewRunner:**
- Uses `LlmRole::EpicReview` instead of `LlmRole::Review`
- Preamble is Architect persona (not Dev persona)
- Tools include `read_file`, `grep`, `find_path`, `list_directory`, `terminal`, `git`, `think` — but NO `edit_file` and NO `ask_supervisor`
- Single prompt → multi-turn autonomous exploration (no `"CR"` command trigger, no BMAD workflow — just a structured review prompt)
- Completion detection via `<<EPIC_REVIEW_REPORT_END>>` delimiter OR no-tool-call heuristic (see section below)
- Output is a markdown report, not a review verdict

[Source: src/review/mod.rs — ReviewRunner pattern to mirror]

### Completion Detection — Dual Strategy

The epic review agent signals completion in two ways (checked in order):

1. **Delimiter detection (primary)**: The prompt instructs the agent to output the final report between `<<EPIC_REVIEW_REPORT_START>>` and `<<EPIC_REVIEW_REPORT_END>>` delimiters. After each chat turn, scan the cumulative agent output for `<<EPIC_REVIEW_REPORT_END>>`. If found, the review is complete.

2. **No-tool-call heuristic (fallback)**: If the agent produces `N` consecutive chat turns (e.g., N=3) with no tool calls, treat the review as complete. This handles cases where the agent finishes its analysis and produces the report without using the delimiters.

3. **Turn limit (safety net)**: If `MAX_EPIC_REVIEW_TURNS` (200) is reached, force-stop and use whatever output has been collected.

The `ResponseAnalyzer` from `session/analyzer.rs` can be reused if it exposes generic pattern matching, or implement a simpler inline check — the epic review has much simpler interaction patterns than the dev-story workflow.

### Report Extraction — Concrete Strategy

After the review session completes (by any of the three mechanisms above):

1. **Scan all agent messages** (concatenated in order) for the `<<EPIC_REVIEW_REPORT_START>>` delimiter
2. **If found**: Extract everything between `<<EPIC_REVIEW_REPORT_START>>` and `<<EPIC_REVIEW_REPORT_END>>` (exclusive of delimiters). This is the report.
3. **If NOT found (fallback)**: Concatenate all agent messages that are NOT tool-call responses (i.e., the agent's "thinking out loud" and final output). Use this as the report. Prefix with a warning: `<!-- WARNING: Report delimiters not found — this is the raw agent output -->`
4. **If the report is empty**: Generate a minimal report: `# Epic {X} Review — No Report Generated\n\nThe review session completed but produced no extractable report. Manual review recommended.`

The MR description uses ONLY the "Epic Recap" section (section 1) from the extracted report. Parse it by finding the first `## Epic Recap` or `#### 1. Epic Recap` heading and taking content until the next `##` or `####` heading. Fallback: first 50 lines of the report.

### Tool Set — Read-Only with Caveats

The epic review agent gets 7 tools (not the full 9):

| Tool | Included | Reason |
|------|----------|--------|
| `read_file` | ✅ | Read source code, story files, docs |
| `grep` | ✅ | Search for patterns across codebase |
| `find_path` | ✅ | Discover file locations |
| `list_directory` | ✅ | Explore project structure |
| `terminal` | ✅ | Run `cargo check`, `cargo test`, `cargo clippy` |
| `git` | ✅ | `git log`, `git diff` for history analysis |
| `think` | ✅ | Structured reasoning during analysis |
| `edit_file` | ❌ | Review should not modify source code |
| `ask_supervisor` | ❌ | No escalation path in autonomous review |

**⚠️ Caveat on "read-only"**: The `git` tool can execute write operations (`git checkout`, `git commit`) and the `terminal` tool can execute arbitrary commands (`sed`, `rm`). The "read-only" constraint is enforced by the PROMPT, not by the tool set. The preamble must include explicit instructions:

```
CRITICAL CONSTRAINTS:
- You are conducting a READ-ONLY review. Do NOT modify any files.
- Do NOT use the git tool for write operations (commit, checkout, branch, reset, stash). Only use: git log, git diff, git status, git show.
- Do NOT use the terminal tool to modify files. Only use it for: cargo check, cargo test, cargo clippy, cargo fmt --check, wc, find, cat.
- Your job is to ANALYZE and REPORT, not to fix.
```

This is a prompt-level constraint, not a structural one. Acceptable trade-off: adding tool-level restrictions would require new tool variants or wrapper logic, which is over-engineering for a review session where the agent has no incentive to modify files.

Use `session/agent.rs::create_base_tools()` (from Story 4.7) as the foundation, then remove `edit_file` and `ask_supervisor` from the tool set. Or construct the tool set inline — whichever is cleaner.

[Source: src/session/agent.rs — create_base_tools() and create_tools_with_supervisor()]

### Preamble Construction

The preamble combines three elements:

1. **System preamble** — Minimal operational instructions (same pattern as `build_preamble()` in `session/agent.rs` but adapted for review context: no BMAD workflow execution, no story development, English language override, read-only constraints)
2. **Architect persona** — Load `_bmad/bmm/agents/architect.md` and extract the `<persona>` block (role, identity, communication_style, principles). Do NOT load the full agent activation flow — no menus, no config loading, no BMAD workflow. Just the persona identity for the LLM to adopt as its analytical lens
3. **Project context** — Load `project-context.md` as implementation rules reference

The preamble tells the agent: "You are Winston (Architect). You are conducting an autonomous post-epic review. Here are the project rules. Analyze the codebase and produce a structured report. You are READ-ONLY — do not modify anything."

### Review Prompt Design — Tool-Based Loading

The initial user message sent to the agent does NOT inline large documents. Instead, it tells the agent where to find them and what to do:

```
## Epic {X} Autonomous Review

You are reviewing Epic {X} after all its stories have been completed.

### Files to Load (use your tools)

1. **Epic definition**: Use `read_file` to load the Epic {X} section from `{planning_artifacts}/epics.md` (search for "## Epic {X}:" heading)
2. **Story files**: Use `find_path` with glob `{implementation_artifacts}/{X}-*` to discover all story files for this epic, then `read_file` each one — focus on Acceptance Criteria, Dev Notes, and Completion Notes
3. **Architecture document**: Use `read_file` to load `{planning_artifacts}/architecture.md` — focus on patterns, decisions, and project structure sections
4. **Source code**: Use `grep`, `find_path`, `list_directory`, and `read_file` to explore the actual implementation

### Your Mission

Produce a comprehensive review report. Structure it with these four sections.
Output the COMPLETE report between delimiters:

<<EPIC_REVIEW_REPORT_START>>
(your full markdown report here)
<<EPIC_REVIEW_REPORT_END>>

#### 1. Epic Recap
- What were the epic's objectives?
- What stories were delivered?
- What was planned vs what was actually built?
- Any scope changes or deviations?

#### 2. Functional Testing Guide
- For each major capability delivered in this epic, provide:
  - A step-by-step testing scenario the human can follow
  - Concrete CLI commands to run (with expected output)
  - Edge cases worth testing manually
  - What "working correctly" looks like
- Base this on ACTUAL implementation (read the code), not just acceptance criteria
- Be specific: file paths, config values, command flags

#### 3. Technical Analysis
- **Pattern Consistency**: Are the same problems solved the same way across all stories? Error handling, logging, naming conventions — uniform?
- **Architecture Adherence**: Does the implementation match architecture.md? Any drift?
- **Technical Debt Inventory**: TODOs, shortcuts, known issues — list them with file locations
- **Cross-Cutting Concerns**: Test coverage gaps, security surface, dependency hygiene
- **Codebase Health**: Run `cargo check`, `cargo test`, `cargo clippy` via terminal tool and report results verbatim

#### 4. Recommendations
- Actionable items for the next epic
- Risks to watch
- Debt to address before it compounds
- Patterns to reinforce or abandon

Use your tools to explore the codebase thoroughly. Read source files, grep for patterns, run build commands. Do not guess — verify.

CRITICAL CONSTRAINTS:
- You are conducting a READ-ONLY review. Do NOT modify any files.
- Do NOT use git for write operations. Only: git log, git diff, git status, git show.
- Do NOT use terminal to modify files. Only: cargo check, cargo test, cargo clippy, cargo fmt --check, wc, find, cat.
```

**Why tool-based loading**: Epic 7 has 10 stories. Each story file is 200-400 lines. Inlining everything would consume 3000-4000+ lines of context window before the agent even starts analyzing code. Tool-based loading lets the agent load what it needs, when it needs it, and skip sections it doesn't need.

### Epic Completion Detection Logic

**CRITICAL: Must re-read sprint-status.yaml from disk.** The `SprintStatusFile` from the poll cycle may be stale — the just-completed story may still show as `in-progress` or `ready-for-dev` in the cached version. The safety net sprint-status commit runs BEFORE detection, so the file on disk is up-to-date.

```rust
/// Detect whether the completed story was the last one in its epic.
///
/// Re-reads sprint-status.yaml from disk (NOT the cached poll version)
/// to ensure the just-completed story's status update is visible.
/// Returns `Some(epic_num)` if all stories in the epic are done.
fn detect_epic_completion(
    sprint_status_path: &Path,
    completed_story: &StoryInfo,
) -> Option<u32> {
    let ssf = match SprintStatusFile::load(sprint_status_path) {
        Ok(ssf) => ssf,
        Err(e) => {
            tracing::warn!(
                action = "epic_completion_check_failed",
                error = %e,
                "Failed to re-read sprint-status for epic completion detection"
            );
            return None;
        }
    };

    let epic_num = completed_story.epic_num;

    // Check all stories in this epic — ALL must be done
    let all_done = ssf.stories().iter()
        .filter(|s| s.epic_num == epic_num)
        .all(|s| s.status == "done");

    if all_done { Some(epic_num) } else { None }
}

/// Check if sprint-status has a retrospective entry for this epic.
fn has_retrospective_entry(ssf: &SprintStatusFile, epic_num: u32) -> bool {
    let key = format!("epic-{epic_num}-retrospective");
    ssf.entries.iter().any(|(k, _)| k == &key)
}
```

This is called in `process_eligible_stories()` right after a successful `process_story()` and AFTER the safety net sprint-status commit. The flow becomes:

```
process_story() → Completed
    → safety net: commit uncommitted sprint-status changes
    → detect_epic_completion(path, &story)  // FRESH READ from disk
        → Some(epic_num)
            → has_retrospective_entry()?
                → yes → run_epic_review()
                       → update sprint-status to "review" (on current branch, commit, push)
                       → create retro branch from last_completed_branch
                       → commit report on retro branch
                       → push retro branch + create MR
                       → checkout back to working state
                       → notify()
                → no  → skip (backward compat)
        → None → continue
    → re_poll_eligible()  // gate now active, next epic blocked
    → next story
```

[Source: src/pipeline.rs#L784-900 — process_eligible_stories() loop where detection inserts]

### Gate Enforcement in Watcher — BACKWARD COMPATIBILITY FIX

**🚨 CRITICAL**: The gate must treat `optional` as "gate clear". ALL existing BMAD projects have `epic-X-retrospective: optional` for every completed epic. If `optional` blocks, deploying this code would freeze every existing project.

The gate logic:
- `optional` → **gate clear** (default state, epic not yet reviewed by daemon)
- `done` → **gate clear** (human has validated the review)
- `review` → **gate BLOCKED** (daemon has activated the gate, awaiting human validation)
- absent → **gate clear** (backward compat, no retro entry)

```rust
/// Check if the retrospective gate for a previous epic allows stories in the next epic.
///
/// Gate statuses:
/// - "optional": gate clear (default/initial state — not yet reviewed)
/// - "done": gate clear (human has validated)
/// - "review": gate BLOCKED (daemon activated gate, awaiting human)
/// - absent: gate clear (no retro entry, backward compat)
fn is_retrospective_gate_clear(
    story_epic: u32,
    retro_gates: &HashMap<u32, String>,
) -> bool {
    if story_epic <= 1 {
        return true; // Epic 1 has no predecessor gate
    }
    match retro_gates.get(&(story_epic - 1)) {
        None => true,                          // No retro entry → no gate
        Some(status) => status != "review",    // ONLY "review" blocks
    }
}
```

**Important**: The gate blocks stories from epic X+1, NOT epic X. If epic 3 just completed and `epic-3-retrospective: review`, stories from epic 4+ are blocked but any remaining epic 3 stories (unlikely but possible) are still eligible.

**Lifecycle**:
```
epic-X-retrospective: optional        ← initial state (all existing projects)
    → daemon detects epic X complete
    → daemon runs autonomous review
    → daemon sets: epic-X-retrospective: review   ← GATE ACTIVATES
    → daemon creates MR + notifies human
    → daemon WAITS (will not process epic X+1 stories)
    → human reviews report
    → human sets: epic-X-retrospective: done       ← GATE CLEARS
    → daemon proceeds to epic X+1
```

[Source: src/watcher/deps.rs — build_full_dependency_map() and the eligible story filtering where gate check inserts]
[Source: src/watcher/mod.rs#L90-108 — StoryInfo::from_key_and_status() already skips retrospective entries]

### Git Flow — Sprint-Status THEN Retro Branch (Fixes Branch Confusion)

**🚨 CRITICAL**: The sprint-status update and the retro branch creation are TWO SEPARATE git operations on TWO SEPARATE branches. The order matters.

**Step 1: Update sprint-status on the current branch**
```
# We're on the last story's branch (or target_branch after sequential chaining)
# Update sprint-status.yaml: optional → review
git add sprint-status.yaml
git commit -m "chore(sprint-status): epic {X} gate activated — awaiting human review"
git push origin HEAD
# ← The watcher now sees the gate on next poll
```

**Step 2: Create retro branch and commit report**
```
# Create retro branch from last_completed_branch (sequential chaining base)
git checkout -b epic-{X}-retrospective {last_completed_branch}
# Add the report file
git add {implementation_artifacts}/epic-{X}-retrospective-report.md
git commit -m "docs(retrospective): epic {X} autonomous review report"
git push origin epic-{X}-retrospective
# ← MR is created from this branch
```

**Step 3: Return to working state**
```
# Checkout back to the branch we were on before (or target_branch)
git checkout {previous_branch}
```

This ensures:
- Sprint-status gate is visible to the watcher immediately (it reads from the local filesystem)
- The retro branch contains ONLY the report (clean diff for the MR)
- The pipeline can continue processing after returning to the working branch

Use `pipeline.rs`'s existing git push pattern (`git push origin <branch>`) and `GitProvider::create_pr()` with `CreatePrParams`. No new git plumbing needed.

[Source: src/pipeline.rs#L440-520 — push_branch() and create_pr() patterns]
[Source: src/git_provider/mod.rs — CreatePrParams struct]

### Sprint-Status Update Pattern

Reuse the existing `session/cleanup.rs::update_story_status()` pattern for updating the retrospective entry. The function does a regex-based find-and-replace on the sprint-status.yaml file, preserving comments and structure.

The update is: find line matching `epic-{X}-retrospective:` and replace the status value with `review`.

Then commit with `chore(sprint-status): epic {X} gate activated — awaiting human review`.

[Source: src/session/cleanup.rs — update_story_status() regex pattern]

### LlmRole::EpicReview — Default Fallback Design

The `LlmRoleConfig` default is empty strings (`provider: ""`, `model: ""`). At runtime, `AgentFactory::config_for_role(LlmRole::EpicReview)` checks if the epic_review config has an empty provider — if so, it falls back to the `review` config. This means:

- **Existing users**: Zero config change required. Epic review uses the same model as code review.
- **Power users**: Can set `epic_review:` in `bmad-bot.yaml` to use a different (possibly stronger reasoning) model for epic-level analysis.

```rust
pub fn config_for_role(&self, role: LlmRole) -> &LlmRoleConfig {
    match role {
        LlmRole::Dev => &self.config.llm.dev,
        LlmRole::Review => &self.config.llm.review,
        LlmRole::Supervisor => &self.config.llm.supervisor,
        LlmRole::EpicReview => {
            if self.config.llm.epic_review.provider.is_empty() {
                &self.config.llm.review // fallback
            } else {
                &self.config.llm.epic_review
            }
        }
    }
}
```

[Source: src/llm/agent_factory.rs#L252-260 — config_for_role() to extend]

### Epic Name Resolution

The notification and MR mention "epic title". This is NOT available from `StoryInfo` or `SprintStatusFile` directly. Resolution strategy:

1. **Try parsing from epics.md**: Use `grep` or simple regex on `{planning_artifacts}/epics.md` to find the heading `## Epic {X}: {title}` and extract `{title}`.
2. **Fallback**: Use `"Epic {X}"` as the title if parsing fails (file not found, heading not matched, etc.).

This is a best-effort convenience — the notification is still useful without the title. Do NOT make epic name resolution a blocking operation.

```rust
fn resolve_epic_title(planning_artifacts: &Path, epic_num: u32) -> String {
    let epics_path = planning_artifacts.join("epics.md");
    let pattern = format!("## Epic {epic_num}:");
    match std::fs::read_to_string(&epics_path) {
        Ok(content) => {
            content.lines()
                .find(|line| line.contains(&pattern))
                .and_then(|line| line.split(':').nth(1))
                .map(|title| title.trim().to_string())
                .unwrap_or_else(|| format!("Epic {epic_num}"))
        }
        Err(_) => format!("Epic {epic_num}"),
    }
}
```

### Notification Format

```
🔍 Epic {X} Review Gate Activated

Epic: {epic_title}
Stories reviewed: {count}
Cargo check: ✅/❌
Cargo test: ✅/❌
Cargo clippy: ✅/❌

Report: {link_to_mr}

➡️ Review the report and set `epic-{X}-retrospective: done` in sprint-status.yaml to continue.
```

If the review session failed, the notification indicates the failure:

```
🔍 Epic {X} Review Gate Activated (⚠️ Review Failed)

Epic: {epic_title}
Stories completed: {count}
Review error: {error_summary}

A minimal failure report has been committed.
Report: {link_to_mr}

➡️ Manual review strongly recommended. Set `epic-{X}-retrospective: done` in sprint-status.yaml to continue.
```

Reuse the existing `Notifier` trait pattern. Add a `notify_epic_gate()` method with a default no-op implementation on the trait (so `NoopNotifier` doesn't need changes). Only `TelegramNotifier` implements the actual send.

[Source: src/notifier/mod.rs — Notifier trait, TelegramNotifier, message formatting patterns]

### Crash Recovery — Explicit Design Choice

The epic review has **no WAL**. This is a conscious design choice:

- The review is **stateless and idempotent** — re-running from scratch produces the same analysis
- The review is **non-destructive** — it only reads code, it doesn't modify the codebase
- Adding WAL for the review would require extending the WAL schema and recovery logic for minimal benefit

If the daemon crashes during epic review:
1. On restart, watcher re-polls sprint-status.yaml
2. The retro entry is still `optional` (sprint-status wasn't updated yet)
3. The daemon detects the epic is complete and re-runs the review from scratch
4. The report file (if partially written) is overwritten

If the daemon crashes AFTER sprint-status update but BEFORE MR creation:
1. On restart, watcher re-polls and sees `epic-X-retrospective: review`
2. The gate is already active — next epic is blocked
3. The report file exists on disk but no MR was created
4. The human can manually review the report file and set `done`
5. On the NEXT epic completion, the flow runs normally

Both scenarios are acceptable. No special recovery logic needed.

### UI Events

Add UI events for the epic review lifecycle, following the patterns established in Epic 10:
- `ui.phase_start("Epic Review")` / `ui.phase_complete("Epic Review")`
- Or use existing `ui.review_start()` / `ui.review_complete()` if they're generic enough

Check what's available in `UiHandle` and `UiRenderer` before adding new methods. The review events from Story 10.4 may already be reusable.

[Source: src/ui/mod.rs — UiHandle public API]
[Source: src/ui/renderer.rs — UiRenderer trait methods]

### What NOT to Change

- **Per-story code review** (`ReviewRunner`) — completely unchanged
- **Branch management** for stories — unchanged (sequential chaining fix is separate and already landed)
- **The BMAD interactive retrospective workflow** (`_bmad/bmm/workflows/4-implementation/retrospective/`) — remains available for manual use, untouched
- **Existing dependency resolution logic** in `watcher/deps.rs` — only EXTEND, don't restructure
- **Agent tools implementations** — no changes to any tool in `src/tools/`
- **BMAD files** — never modify anything under `_bmad/`
- **`is_retryable_review_error()`** — reuse from review/mod.rs, do NOT duplicate

### Anti-Patterns to Avoid

- **Do NOT parse sprint-status.yaml manually with regex for epic completion detection** — use the existing `SprintStatusFile` struct and its `stories()` method which already handles parsing
- **Do NOT hardcode the report template inside Rust code** — keep the report structure in the prompt, not as a Rust format string. The LLM produces free-form markdown guided by the prompt structure
- **Do NOT make the gate optional via config** — the gate activates if and only if `epic-X-retrospective` exists in sprint-status.yaml. No `epic_gate_enabled: bool` config needed. Projects that don't want the gate simply don't add retrospective entries to sprint-status.yaml
- **Do NOT auto-merge the retrospective MR** — it stays open as a tracking artifact. The human may or may not merge it
- **Do NOT add `edit_file` to the review agent** — the review should not modify source code
- **Do NOT block the current epic** — the gate blocks epic X+1, not epic X. If somehow a story in epic X is still eligible after the last story completes (edge case), it should still be processable
- **Do NOT treat `optional` as blocking** — ONLY `review` blocks. `optional` and `done` both clear the gate. This is the #1 backward compatibility requirement.
- **Do NOT use the cached SprintStatusFile for epic completion detection** — always re-read from disk after the safety net commit
- **Do NOT inline large documents in the review prompt** — use tool-based loading to stay within context window limits
- **Do NOT create the retro branch BEFORE updating sprint-status** — the sprint-status update must be on the current working branch and pushed so the watcher sees it

### Previous Story Intelligence (Story 4.7 — Agent Module Centralization)

Story 4.7 centralized tool creation into `session/agent.rs` with `create_base_tools()` and `create_tools_with_supervisor()`. The epic review runner should leverage `create_base_tools()` and then remove `edit_file` and `ask_supervisor` from the returned tool set (or build a custom subset). Check the actual API of `create_base_tools()` — it may return a `ToolSet` or a tuple of individual tools.

Story 4.7 also established `build_preamble()` in `session/agent.rs` — the epic review preamble can follow the same structure but with different content (Architect persona instead of Dev persona, read-only constraints).

[Source: src/session/agent.rs — create_base_tools(), create_tools_with_supervisor(), build_preamble()]

### Previous Story Intelligence (Story 4.5 — LLM Provider Abstraction)

Story 4.5 established the `AgentFactory` + `BuiltAgent` pattern. Adding `LlmRole::EpicReview` follows the exact same extension pattern used for all three existing roles. The `build()` method doesn't branch on role — it branches on provider. The role only determines which `LlmRoleConfig` is used. So adding a new role is purely: new enum variant + new config field + new `config_for_role()` arm.

[Source: src/llm/agent_factory.rs — AgentFactory::build() and LlmRole enum]

### Previous Story Intelligence (Story 4.6 — Post-Implementation Impact Analysis)

Story 4.6 added post-completion steps to `run_session()` in `session/runner.rs`. The epic review is NOT triggered from the session runner — it's triggered from the pipeline level (`process_eligible_stories()`) after `process_story()` returns. This is important: the epic review is a pipeline concern, not a session concern. The session doesn't know about epics.

[Source: src/session/runner.rs — ResponseAction::Completed arm, Step 7/8/9 sequence]

### Validation Notes

- `cargo build` — zero errors
- `cargo test` — all existing tests pass + new tests for: epic completion detection (fresh read), gate enforcement (`optional` allows, `review` blocks, `done` allows), LlmRole::EpicReview config resolution, report extraction (with delimiters, without delimiters, empty)
- `cargo clippy` — zero warnings
- `cargo fmt` — no formatting issues
- Manual verification: after implementation, simulate the flow by examining the code paths (actual E2E testing requires a running daemon with real LLM)

### Project Structure Notes

New file:
```
src/review/
├── mod.rs        # Existing — add `pub mod epic;` re-export
└── epic.rs       # NEW — EpicReviewRunner, EpicReviewOutcome, report extraction, preamble/prompt builders
```

Modified files:
```
src/llm/agent_factory.rs  # LlmRole::EpicReview variant, config_for_role(), Display
src/config/mod.rs          # LlmConfig.epic_review field, Default impl for LlmRoleConfig
src/pipeline.rs            # Epic completion detection (fresh read), review trigger, sprint-status update, branch/MR, epic name resolution
src/watcher/deps.rs        # Retrospective gate check in eligible story filtering (optional=clear, review=blocked)
src/notifier/mod.rs        # notify_epic_gate() trait method + Telegram impl
src/review/mod.rs          # pub mod epic; re-export
```

### References

- [Source: architect-brief-epic-gate-retrospective.md — Full architect brief with incident details and proposed solution]
- [Source: src/review/mod.rs — ReviewRunner pattern to mirror for EpicReviewRunner, MAX_REVIEW_TURNS, MAX_SESSION_RETRIES, is_retryable_review_error()]
- [Source: src/llm/agent_factory.rs#L45-52 — LlmRole enum to extend]
- [Source: src/llm/agent_factory.rs#L252-260 — config_for_role() to extend]
- [Source: src/config/mod.rs#L165-172 — LlmConfig struct to extend]
- [Source: src/pipeline.rs#L784-900 — process_eligible_stories() loop where detection inserts]
- [Source: src/watcher/deps.rs — dependency resolution where gate check inserts]
- [Source: src/watcher/mod.rs#L90-108 — StoryInfo::from_key_and_status() skips retrospectives]
- [Source: src/session/agent.rs — create_base_tools(), build_preamble() patterns]
- [Source: src/session/cleanup.rs — update_story_status() for sprint-status updates]
- [Source: src/session/analyzer.rs — ResponseAnalyzer patterns for completion detection]
- [Source: src/notifier/mod.rs — Notifier trait, TelegramNotifier patterns]
- [Source: src/git_provider/mod.rs — CreatePrParams, GitProvider trait]
- [Source: _bmad/bmm/agents/architect.md — Architect persona to load for review preamble]
- [Source: _bmad-output/project-context.md — Project rules for review agent context]
- [Source: _bmad-output/planning-artifacts/architecture.md — Architecture doc for review reference]
- [Source: _bmad-output/planning-artifacts/epics.md#L728-1026 — Epic 4 definition]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (via Copilot) — Tasks 1-7 completed in prior session (context window exhaustion). Tasks 8-9 completed in recovery session.

### Debug Log References

- Prior session: completed Tasks 1-7 (LlmRole::EpicReview, epic completion detection, gate enforcement, EpicReviewRunner, sprint-status update, branch/MR creation, notification, pipeline integration wiring)
- Recovery session: verified Task 8 already wired, ran Task 9 validation, fixed 8 clippy collapsible-if lints

### Completion Notes List

- All 9 tasks complete. 1248 tests pass (80 lib + 1168 bin), zero clippy errors, zero fmt issues.
- Task 8 (pipeline integration) was fully implemented in the prior session but checkboxes were not marked before context window exhaustion.
- Task 9 validation surfaced 8 clippy `collapsible_if` errors — all fixed by merging nested `if` statements using Rust 2024 `let` chains.
- All existing dead_code warnings are pre-existing (tracked by `#![warn(dead_code)]` FIXME in main.rs) — not introduced by this story.

### Change Log

- Tasks 1-7: Implemented in prior session (all code changes for LlmRole, config, EpicReviewRunner, gate enforcement, pipeline wiring, notification)
- Task 9: Fixed 8 collapsible-if clippy lints across 5 files (pipeline.rs, review/epic.rs, session/agent.rs, session/cleanup.rs, watcher/deps.rs)
- CR fixes: (H1) Exported MAX_SESSION_RETRIES from review/mod.rs and replaced local constant in epic.rs with import; (H2) try_epic_gate() now returns bool — gate activation failures are visible in logs; (M1) git add now uses repo-relative path instead of absolute; (M2) checkout back to base_branch on push failure in create_retro_branch_and_mr(); (M3) detect_epic_completion() guards against empty epic (Iterator::all on empty = true); (M4) added missing session/runner.rs and watcher/mod.rs to File List

### File List

- `src/review/epic.rs` — NEW: EpicReviewRunner, EpicReviewOutcome, preamble/prompt builders, report extraction, failure report generation (Tasks 4, 9, CR)
- `src/review/mod.rs` — MODIFIED: added `pub mod epic;` re-export; exported MAX_SESSION_RETRIES as pub(super) (Task 4, CR)
- `src/llm/agent_factory.rs` — MODIFIED: LlmRole::EpicReview variant, config_for_role(), Display (Task 1)
- `src/config/mod.rs` — MODIFIED: LlmConfig.epic_review field, Default for LlmRoleConfig (Task 1)
- `src/pipeline.rs` — MODIFIED: EpicReviewRunner in StoryPipeline, try_epic_gate() returns bool, create_retro_branch_and_mr() with checkout-back on push failure and relative git-add path, detect_epic_completion() empty-epic guard, has_retrospective_entry(), resolve_epic_title(), push_current_branch(), checkout_branch() (Tasks 2, 5, 6, 8, 9, CR)
- `src/watcher/deps.rs` — MODIFIED: compute_retrospective_gates(), is_retrospective_gate_clear(), gate check in filter_eligible() (Tasks 3, 9)
- `src/notifier/mod.rs` — MODIFIED: EpicGateNotification, notify_epic_gate() trait method + Telegram impl (Task 7)
- `src/session/agent.rs` — MODIFIED: clippy collapsible-if fix (Task 9)
- `src/session/cleanup.rs` — MODIFIED: clippy collapsible-if fix (Task 9)
- `src/session/runner.rs` — MODIFIED: added base_branch_override parameter to run() for sequential branch chaining (Task 8)
- `src/watcher/mod.rs` — MODIFIED: test fixture updated with epic_review LlmRoleConfig field (Task 1)