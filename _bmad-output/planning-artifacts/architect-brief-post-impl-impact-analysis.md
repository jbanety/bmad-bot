---
type: architect-brief
from: Amelia (Dev Agent)
to: Architect + Product Owner
date: '2026-02-15'
subject: 'Feature Request — Post-Implementation Impact Analysis Step in Session Runner'
related_decision: 'Session runner post-completion sequence (Steps 7-9)'
status: ready-for-review
triggered_by: 'Story 7-1 completed without updating downstream stories 7-2 through 7-10 — their Dev Notes still reference assumptions invalidated by actual implementation'
---

# Architect Brief: Post-Implementation Impact Analysis

## Problem

When the daemon completes a story, downstream stories that depend on it may contain **stale assumptions** in their specs, Dev Notes, and Tasks/Subtasks. The current post-completion sequence does not verify or propagate implementation reality to upcoming stories.

### Concrete Example

Story `7-1-integration-test-infrastructure-fixtures` completed and introduced specific test helpers, fixture patterns, and module structure. Stories `7-2` through `7-10` all `depends-on: 7-1` and will be unblocked automatically. However, their "Previous Story Intelligence" sections still reference the **planned** infrastructure — not what was **actually built**. The agent starting `7-2` will work from outdated assumptions, leading to wasted tokens, wrong patterns, and rework.

This also applies to `architecture.md` when an implementation introduces new modules, changes interfaces, or deviates from documented patterns.

## Current Post-Completion Sequence

After the agent signals `<<BMAD_JOB_DONE>>`, the session runner (`src/session/runner.rs`) executes:

| Step | Action | Tool Access | Purpose |
|------|--------|-------------|---------|
| 7 | Final commit | ✅ Yes | Commit any uncommitted changes |
| 8 | PR summary | ❌ No (text only) | Generate `<pr-summary>` for PR description |

There is **no step** between completion and PR summary that evaluates downstream impact.

## Proposed Change

Insert a new **Step 8 — Impact Analysis** between the current Step 7 (final commit) and Step 8 (PR summary, renumbered to Step 9). This is a regular chat turn where the agent **retains full tool access**.

### New Sequence

| Step | Action | Tool Access | Purpose |
|------|--------|-------------|---------|
| 7 | Final commit | ✅ Yes | Commit uncommitted work |
| **8** | **Impact analysis** | **✅ Yes** | **Evaluate and update downstream stories + architecture** |
| 9 | PR summary | ❌ No | Generate PR description (renumbered) |

### What the Impact Analysis Step Does

The agent receives a prompt instructing it to:

1. **Read `sprint-status.yaml`** and identify stories whose `depends-on` references the completed story (by full key or short key `{epic}-{story}`)
2. **Check subsequent stories in the same epic** (document order)
3. **For each downstream story file**, read its Dev Notes (especially "Previous Story Intelligence") and Tasks/Subtasks
4. **Compare** what was actually implemented against what downstream stories assume
5. **Update** each affected story's "Previous Story Intelligence" section with:
   - What changed vs the original plan
   - New APIs, patterns, or modules the downstream story should use
   - Obsolete assumptions to discard
6. **Optionally update `architecture.md`** if new modules or changed interfaces were introduced — but only if the file exists (not all projects have one)
7. **Commit** any changes with a descriptive message: `docs(stories): update downstream specs after {story_key}`
8. If nothing needs updating, say so and move on — **do not invent changes**

### Design Constraints

- **Best-effort, non-blocking**: If the impact analysis turn fails (LLM error, timeout), the session proceeds to PR summary and completion. No story should fail because of this step.
- **Agent-driven, not daemon-driven**: The daemon sends the prompt; the agent uses its existing tools (`read_file`, `edit_file`, `git`) to do the actual work. No new tools or daemon logic required beyond the prompt and chat turn.
- **Architecture doc is optional**: The prompt must reference the planning artifacts path but not assume `architecture.md` exists. The agent should check for its existence before attempting to read/update it.
- **Scope guard**: The agent must only update "Previous Story Intelligence" in Dev Notes and architecture references — not rewrite tasks, ACs, or other story sections.

## Impact on Existing Code

### Files to Modify

| File | Change |
|------|--------|
| `src/session/runner.rs` | Add impact analysis chat turn after Step 7, renumber PR summary to Step 9. ~40-50 lines of new code following the same pattern as existing steps. |

### Files NOT Modified

- No changes to the BMAD workflow (`instructions.xml`) — this is daemon-level orchestration
- No changes to tools, agent factory, or pipeline
- No new dependencies

### Risks

- **Token cost**: One additional LLM turn per story. Bounded by the sprint-status size and number of downstream stories. For most stories, the agent reads 2-5 files and either updates or skips. Estimated cost: 2-8k tokens.
- **False updates**: The agent could update a story unnecessarily. Mitigated by explicit prompt instruction: "only update what genuinely needs it" and "do not invent changes."
- **Context window**: After a long implementation session, adding another tool-using turn pushes closer to context limits. Mitigated by the best-effort pattern — if it fails, we proceed.

## Story Suggestion for PM

This is a small, self-contained change — likely a subtask or a micro-story rather than a full epic story. Suggested scope:

**Title:** Post-Implementation Impact Analysis on Downstream Stories

**Acceptance Criteria:**
- [ ] After story completion (Step 7 final commit), the session runner sends an impact analysis prompt to the agent
- [ ] The agent can read sprint-status.yaml and identify dependent stories (both short key `7-1` and full key formats)
- [ ] The agent reads downstream story files and updates "Previous Story Intelligence" when implementation deviates from assumptions
- [ ] The agent checks for `architecture.md` existence before attempting updates
- [ ] Changes are committed with a `docs(stories):` prefix
- [ ] If the impact analysis turn fails, the session proceeds to PR summary without error
- [ ] PR summary is renumbered to Step 9 and continues to function identically

**Dependencies:** None — all infrastructure (tools, session runner, sprint-status parsing) already exists.

**Estimated effort:** Small — single file change, ~50 lines, no new modules or tools.

## Related Context

- **Pre-Development Spec Update** (documented in `project-context.md`): The agent already reviews previous stories *before* starting a new one. This proposal adds the symmetric *post*-implementation step — propagating forward instead of catching up.
- **`unblock_dependents`** (`src/session/cleanup.rs`): The pipeline already transitions `blocked → ready-for-dev` after completion. The impact analysis step ensures those newly unblocked stories have accurate specs when they're picked up.
- **PR summary hallucination fix** (commit `6450450`): The enriched PR summary prompt demonstrates the pattern — a dedicated post-completion chat turn with grounded context. The impact analysis step follows the same pattern but with tool access.