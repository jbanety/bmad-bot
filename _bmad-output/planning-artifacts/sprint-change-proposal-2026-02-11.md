# Sprint Change Proposal — Pipeline Reordering: PR Before Code Review

| Field              | Value                                                        |
| ------------------ | ------------------------------------------------------------ |
| **Date**           | 2026-02-11                                                   |
| **Author**         | JB (Product Manager facilitation)                            |
| **Scope**          | Minor — Direct implementation by dev team                    |
| **Effort**         | Low                                                          |
| **Risk**           | Low                                                          |
| **Status**         | ✅ Approved                                                  |

---

## 1. Issue Summary

### Problem Statement

The `StoryPipeline::process_story()` orchestration in `src/pipeline.rs` creates the Pull Request **after** the automated code review completes. This ordering is contrary to the natural developer workflow where you dev → push → create PR → request review. The PR should be the **vehicle** for the review, not its consequence.

### Discovery Context

Identified during architectural review of the pipeline flow in Epic 5 (Code Review & Pull Request Delivery). All stories in Epic 5 are currently in `review` status.

### Evidence

- **Code** (`src/pipeline.rs` L180–408): `process_story()` runs review before push and PR creation
- **PRD FR20**: *"the review agent can post its review as a comment on the PR"* — implies the PR already exists when the review posts
- **PRD FR24**: *"When code review is disabled, the daemon proceeds directly to PR creation after the development session"* — consistent with PR coming right after session
- **Architecture Data Flow** (points 8–9): documents review → PR creation, the inverted order
- **Real-world workflow**: no developer creates a PR after receiving a review — the PR is where the review happens

### Additional Gap Identified

When `code_review_enabled: false`, there is no automated review step. In the current flow, the PR is still created, but the reordering makes the intent clearer: the PR exists immediately after the dev session, enabling **human review** even when automated review is disabled. This closes a visibility gap.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact | Detail |
| ---- | ------ | ------ |
| Epic 5: Code Review & PR Delivery | ⚠️ Moderate | Story 5.2 AC #4 wording. Epic description. Pipeline orchestration code. |
| Epic 7: Integration Tests | ⚠️ Moderate | Story 7.4 ACs describe the old flow order. |
| Epic 1–4, 6, 8 | ❌ None | No interface or behavioral changes affect these epics. |

### Story Impact

| Story | Status | Change Required |
| ----- | ------ | --------------- |
| 5.1 (Git Provider Trait & GitHub PR Creation) | review | None — trait and implementation unchanged |
| 5.2 (Automated Code Review Session) | review | AC #4 rewording: PR exists before review completes |
| 5.3 (GitLab Merge Request Support) | review | None — same trait, different provider |
| 7.4 (Pipeline Orchestration Integration Tests) | blocked | ACs must reflect new flow order |

### Artifact Conflicts

| Artifact | Conflict | Detail |
| -------- | -------- | ------ |
| **PRD** (`prd.md`) | ✅ None — supports change | FR20 and FR24 already imply PR-before-review |
| **Architecture** (`architecture.md`) | ⚠️ Update needed | Data Flow points 8–9: reorder PR creation before review |
| **Epics** (`epics.md`) | ⚠️ Update needed | Epic 5 description, Story 5.2 ACs, Story 7.4 ACs |
| **README** (`README.md`) | ⚠️ Update needed | Pipeline order (L26–34), ASCII diagram (L61–65), detail section (L384–387) |
| **Story 5.2 artifact** (`5-2-*.md`) | ⚠️ Update needed | AC #4 wording |
| **Story 7.4 artifact** (`7-4-*.md`) | ⚠️ Update needed | ACs flow description |
| **Code** (`src/pipeline.rs`) | ⚠️ Update needed | Reorder phases in `process_story()` |
| **Tests** (`src/pipeline.rs` mod tests) | ⚠️ Review needed | Verify/adapt existing tests |
| UI/UX | N/A | CLI daemon — no UI |

### Technical Impact

- **Code change is isolated** to `StoryPipeline::process_story()` in `src/pipeline.rs`
- **No interface changes** — `GitProvider`, `ReviewRunner`, `SessionRunner`, `Notifier` traits unchanged
- **One addition**: a second `push_branch()` call after review to push review fix commits to the PR
- **No new dependencies** or infrastructure changes

---

## 3. Recommended Approach

### Selected Path: Direct Adjustment

Reorder the phases within `process_story()` and update documentation. No rollback, no MVP scope change.

### Rationale

- **Surgical change** — one function to reorder, same components, same interfaces
- **PRD alignment** — FR20 and FR24 already describe the target behavior
- **Low effort** — the same calls in a different order, plus one additional push
- **Low risk** — isolated to pipeline orchestration, no ripple effects on other modules
- **Developer intuition** — the new flow matches every developer's mental model
- **Closes a gap** — enables human review via PR even when automated review is off

### Alternatives Considered

| Option | Verdict | Reason |
| ------ | ------- | ------ |
| Rollback | ❌ Not viable | Components are correct; only orchestration order is wrong |
| MVP Review | ❌ Not applicable | No scope change needed |

---

## 4. Detailed Change Proposals

### 4.1 Code: `src/pipeline.rs` — `process_story()` Reordering

**Scope:** `SessionOutcome::Completed` match arm (L198–320)

**OLD flow (Completed case):**

```
Phase 1 — Dev Session
Phase 2 — Code Review (optional)
Phase 3 — Push branch
Phase 4 — Create PR
Phase 5 — Post review comment on PR (if review report exists)
Phase 6 — Notify
```

**NEW flow (Completed case):**

```
Phase 1 — Dev Session
Phase 2 — Push branch
Phase 3 — Create PR (with story description + supervisor decisions)
Phase 4 — Code Review (optional)
Phase 5 — Push branch again (to update PR with review fix commits)
Phase 6 — Post review comment on PR (if review report exists)
Phase 7 — Notify
```

**Key detail:** Phase 5 (second push) is needed because the review agent may commit fixes locally after the initial push. These commits must be pushed so the PR reflects the complete state including review fixes. If the review made no commits, the push is a no-op.

**Failed/Escalated cases:** No change needed. Failed stories already push + create failure PR without review. Escalated stories don't create PRs.

---

### 4.2 Documentation: `architecture.md` — Data Flow

**Section:** Data Flow (L1024–1038)

**OLD (points 8–9):**

> 8. **Session end:** If `code_review_enabled`, `review/ReviewRunner` launches a new rig agent session [...] review report is captured in `ReviewOutcome::Completed { report }` and later posted as a PR comment by the orchestrator via `GitProvider::add_comment()`. Review failures are non-blocking.
> 9. **PR creation:** `git_provider/` creates PR (GitHub or GitLab) with agent-written description + Supervisor Decisions section

**NEW (points 8–10):**

> 8. **Push & PR creation:** `pipeline.rs` pushes the story branch to remote, then `git_provider/` creates PR (GitHub or GitLab) with agent-written description + Supervisor Decisions section. PR is immediately visible for human review.
> 9. **Code review (optional):** If `code_review_enabled`, `review/ReviewRunner` launches a new rig agent session [...] review report is captured in `ReviewOutcome::Completed { report }`. The pipeline pushes any review fix commits to update the PR, then posts the review report as a comment via `GitProvider::add_comment()`. Review failures are non-blocking — the PR already exists.
> 10. **Notification:** `notifier/` sends Telegram message with story status + PR link

---

### 4.3 Documentation: `epics.md` — Epic 5 & Story 5.2

**Section:** Epic 5 description (L863–866)

**OLD:**

> The daemon optionally launches a code review via a separate LLM after the dev session, with fixes in separate commits and review posted as a PR comment. It creates a Pull Request on GitHub or GitLab with an agent-written description [...]

**NEW:**

> The daemon creates a Pull Request on GitHub or GitLab with an agent-written description immediately after the dev session. It then optionally launches a code review via a separate LLM, with fixes in separate commits pushed to update the PR and review posted as a PR comment [...]

**Section:** Story 5.2 AC #4 (L920–924)

**OLD:**

> 4. **Given** a PR is created by the orchestrator after the review
>    **When** the `ReviewOutcome::Completed` contains a report
>    **Then** the orchestrator posts the review report as a comment on the PR via `GitProvider::add_comment()`

**NEW:**

> 4. **Given** a PR was already created by the orchestrator before the review
>    **When** the review completes with `ReviewOutcome::Completed` containing a report
>    **Then** the orchestrator pushes any review fix commits to update the PR, then posts the review report as a comment on the PR via `GitProvider::add_comment()`

---

### 4.4 Documentation: `epics.md` — Story 7.4

**Section:** Story 7.4 ACs (L1249–1257)

**OLD:**

> So that I'm confident the orchestration logic correctly chains session → review → PR → notification.

**NEW:**

> So that I'm confident the orchestration logic correctly chains session → PR → review → notification.

AC descriptions should reflect: PR is created before review runs; review comment is posted on existing PR.

---

### 4.5 Implementation Artifact: `5-2-automated-code-review-session.md`

**Section:** AC #4 (L37–41)

Same change as 4.3 above — align AC #4 wording to reflect PR exists before review completes. Add note that review fix commits are pushed to update the existing PR.

---

### 4.6 Implementation Artifact: `7-4-pipeline-orchestration-integration-tests.md`

**Section:** ACs

Update all acceptance criteria to reflect the new ordering: `session → push → PR → review → push #2 → comment → notify`. Specifically:
- Happy path AC: verify `create_pr` is called before `run_review`
- Review-disabled AC: verify PR is created, review not called
- Review-failure AC: verify PR already exists, review failure doesn't prevent PR

---

### 4.7 Documentation: `README.md` — Pipeline Description

**Section:** Table of Contents (L26–34)

**OLD:**

```
  - [6. Code Review](#6-code-review)
  - [7. Pull Request Creation](#7-pull-request-creation)
```

**NEW:**

```
  - [6. Pull Request Creation](#6-pull-request-creation)
  - [7. Code Review](#7-code-review)
```

**Section:** ASCII diagram (L61–65) — Update to show PR Creation before Code Review.

**Section:** Pipeline detail (L384+) — Swap sections 6 and 7. Update section 7 (Code Review) to note that review comments are posted on the existing PR and review fix commits are pushed to update it.

---

## 5. Implementation Handoff

### Change Scope: Minor

Direct implementation by development team. No backlog reorganization or strategic replan needed.

### Action Plan

| # | Action | Owner | Priority | Artifact |
| - | ------ | ----- | -------- | -------- |
| 1 | Reorder `process_story()` phases: Push → PR → Review → Push #2 → Comment → Notify | Dev | 🔴 Critical | `src/pipeline.rs` |
| 2 | Verify/adapt existing unit tests in `pipeline.rs` mod tests | Dev | 🟢 Standard | `src/pipeline.rs` |
| 3 | Update Data Flow section (points 8–10) | PM/SM | 🟡 Important | `architecture.md` |
| 4 | Update Epic 5 description + Story 5.2 ACs + Story 7.4 ACs | PM/SM | 🟡 Important | `epics.md` |
| 5 | Update pipeline order, ASCII diagram, and detail sections | PM/SM | 🟡 Important | `README.md` |
| 6 | Update AC #4 wording | PM/SM | 🟡 Important | `5-2-*.md` |
| 7 | Update ACs for new flow order | PM/SM | 🟡 Important | `7-4-*.md` |

### Success Criteria

- [ ] `process_story()` creates the PR before launching the review
- [ ] Review posts its comment on an already-existing PR
- [ ] A second push after review ensures fix commits are visible on the PR
- [ ] When `code_review_enabled: false`, PR is created immediately after session — enabling human review
- [ ] All documentation (architecture, epics, README, stories) reflects the new flow
- [ ] All existing tests pass with the new ordering
- [ ] No regression in Failed/Escalated story handling