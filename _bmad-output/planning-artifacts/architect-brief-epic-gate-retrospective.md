---
type: architect-brief
from: Amelia (Dev Agent)
to: Architect & Scrum Master
date: '2026-03-06'
subject: 'Epic Gate — Mandatory Retrospective Review as Pipeline Checkpoint Between Epics'
related_decision: 'Sequential pipeline execution (one story at a time), sprint-status.yaml as source of truth'
status: draft
triggered_by: 'Production incident — daemon looped 4× re-processing 14 already-completed stories (42 wasted LLM sessions, 19 duplicate MRs) due to sprint-status.yaml divergence across parallel branch chains. Root cause fixed (sequential branch chaining), but exposed a deeper problem: no human checkpoint exists between epics, allowing architectural drift, pattern inconsistencies, and infrastructure bugs to compound unchecked across the entire project.'
---

# Architect Brief: Epic Gate — Mandatory Retrospective Review Between Epics

## Context

The BMAD Bot daemon processes stories autonomously: detect eligible → LLM dev session → PR → notify → next story. When an epic completes (all its stories reach `done`), the daemon immediately picks up the next eligible story from the next epic. There is **no pause, no review, no human checkpoint**.

The sprint-status.yaml already contains `epic-X-retrospective: optional` entries for every epic. These are currently inert — the daemon ignores them entirely. The BMAD methodology includes a full retrospective workflow (`_bmad/bmm/workflows/4-implementation/retrospective/`) designed for interactive Party Mode sessions, but nothing is automated.

### The Incident That Triggered This

On 2026-03-06, the daemon ran against the `autoscalp3000` project. It completed 16 stories (epic 1 through epic 4.4) in one pipeline run (~7 hours). When the run finished and the next poll cycle started, the watcher re-read sprint-status.yaml and found 33 "eligible" stories — including 14 that were already completed.

**Root cause:** Stories with parallel dependency chains (e.g., epic 2.x, 3.x, 4.x all depending on 1-5) were branched from the same ancestor. Each branch chain had its own copy of sprint-status.yaml with only its own `done` markers. When the pipeline finished on `story/4-4`, that branch's sprint-status didn't contain the `done` markers for epics 2.x and 3.x.

The daemon then re-processed all those stories — burning tokens, re-running LLM sessions, and failing with HTTP 409 ("merge request already exists") on every PR creation. It looped **4 times** before being manually killed.

**The sequential branch chaining fix** (already implemented) prevents the sprint-status fork. But it only addresses the technical symptom. The systemic problem remains: **the daemon has no circuit breaker between epics**.

## Problem Summary

| Issue | Impact |
|-------|--------|
| No human checkpoint between epics | Architectural drift, pattern inconsistencies, and technical debt compound silently across the entire project |
| Story-level code review is scoped too narrow | Reviews only see the diff of one story — cannot detect cross-cutting issues like duplicated patterns, diverging error handling, or growing coupling |
| No merge synchronization point | Story branches accumulate without being merged to `target_branch`, increasing merge conflict risk and making the codebase state on `target_branch` stale |
| `epic-X-retrospective: optional` is ignored | The infrastructure for the checkpoint exists in sprint-status but the daemon treats it as decoration |
| Infrastructure bugs go undetected for hours | Without a gate, the daemon runs indefinitely — a bug like the sprint-status fork burned tokens for 4+ hours before manual intervention |

## Proposed Solution

### Epic Gate — Automatic Pause After Last Story of an Epic

When the daemon detects that the last story of an epic has reached `done`, it **stops processing new stories** and enters a gate sequence:

#### Phase 1: Detection

After each story completion, the pipeline checks: "Was this the last story of the current epic?" by scanning sprint-status.yaml. If all `X-Y-*` stories for epic X are `done`, the epic is complete.

#### Phase 2: Retrospective Review (Autonomous)

The daemon launches a **global code review session** — distinct from the per-story adversarial review. This is NOT the interactive Party Mode retrospective. It's an autonomous, LLM-driven analysis that examines:

- **Pattern consistency** — Are the same problems solved the same way across all stories in the epic? Are error handling, logging, and naming conventions uniform?
- **Architecture adherence** — Does the implementation match the architecture doc? Any drift?
- **Technical debt inventory** — What shortcuts were taken? What TODOs were left? What's the debt burden for the next epic?
- **Cross-cutting concerns** — Test coverage gaps, security surface, dependency hygiene
- **Codebase health metrics** — Build status (`cargo check`, `cargo test`, `cargo clippy`), module coupling, dead code

This review examines the **full codebase at this point**, not just diffs. The LLM is given the architecture doc, the PRD, and the project-context as reference.

#### Phase 3: Report & Notify

The review produces a structured report saved as `{implementation_artifacts}/epic-{X}-retrospective-report.md`. Key sections:

- Executive summary (pass/fail/concerns)
- Pattern consistency findings
- Architecture drift analysis
- Technical debt inventory
- Recommendations for next epic
- Blocking issues (if any)

If notifications are enabled, the daemon sends the report summary to the human (Telegram).

#### Phase 4: Gate — Wait for Human Validation

The daemon updates sprint-status.yaml:

```yaml
epic-X-retrospective: review    # was: optional
```

The daemon then **does not process any stories from epic X+1 or beyond**. It continues polling but skips all stories whose epic has a retrospective dependency that isn't `done`.

The human reviews the report, optionally runs the interactive Party Mode retrospective for deeper analysis, then manually sets:

```yaml
epic-X-retrospective: done
```

On the next poll cycle, the daemon sees the gate is cleared and proceeds to epic X+1.

## Architecture Impact

### Modules Affected

| Module | Change | Scope |
|--------|--------|-------|
| `pipeline.rs` | Epic completion detection after each story; gate check before processing | Medium |
| `watcher/mod.rs` + `watcher/deps.rs` | Filter stories by retrospective gate status; treat `epic-X-retrospective != done` as blocking for all stories in epic X+1+ | Medium |
| `session/runner.rs` | New `LlmRole::Retrospective` (or reuse `Review`) for the global review session | Small |
| `llm/agent_factory.rs` | Build agent for retrospective review (reuse existing `Review` role config) | Small |
| `notifier/mod.rs` | New `notify_epic_gate` method for retrospective report summary | Small |
| `config/mod.rs` | Optional: `epic_gate_enabled: bool` config (default `true` for automated runs) | Small |
| `sprint-status.yaml` handling | Write `review` status for retrospective entries; read it as blocking gate | Small |

### New Files

| File | Purpose |
|------|---------|
| `src/review/epic.rs` (or `src/retrospective/mod.rs`) | Epic-level review logic: codebase analysis prompt, report generation, metric collection |

### Dependencies

- Reuses existing `AgentFactory` + `BuiltAgent` infrastructure (no new LLM plumbing)
- Reuses existing `UiHandle` for terminal output
- Reuses existing `Notifier` trait for Telegram
- Reuses existing sprint-status read/write utilities

### What Does NOT Change

- Per-story code review workflow (unchanged)
- Branch management (unchanged — the sequential chaining fix is separate)
- The BMAD interactive retrospective workflow (remains available for manual use)
- Agent tools (no new tools needed — the review reads files via standard tools)

## Retrospective Gate Logic — Dependency Model

The retrospective gate integrates into the existing dependency resolution in `watcher/deps.rs`:

```
Epic 1 stories: 1-1, 1-2, ..., 1-N
epic-1-retrospective: optional → review → done

Epic 2 stories: 2-1, 2-2, ..., 2-M
epic-2-retrospective: optional → review → done
```

**Rule:** A story `X-Y` is eligible ONLY IF `epic-(X-1)-retrospective` is `done` (or doesn't exist for epic 1). This is enforced at the watcher pre-gate level, same as story dependencies.

**Edge case — cross-epic dependencies:** If story `3-1` depends on `epic-2` (as declared in sprint-status comments), the gate for epic 2 must also be cleared before `3-1` is eligible. This is already consistent with how epic-level `depends-on` works.

**Edge case — `optional` status:** In automated mode, `optional` is treated as **mandatory** (the daemon sets it to `review` and waits). In manual mode (human running stories interactively), `optional` remains skippable.

## Review Prompt Strategy

The retrospective review is NOT a line-by-line code review. It's a **holistic codebase assessment**. The prompt includes:

1. **Architecture doc** — The intended design
2. **PRD** — The product requirements
3. **Project context** — Coding rules and conventions
4. **Epic definition** — What was supposed to be built
5. **All story files from the epic** — What was actually done, dev notes, decisions
6. **Codebase exploration instructions** — The agent uses `grep`, `read_file`, `list_directory` to examine the actual code

The agent is instructed to focus on **cross-cutting patterns**, not individual bugs (that's what per-story review is for).

## Sprint-Status Lifecycle Update

Current:
```
epic-X-retrospective: optional  →  (ignored by daemon)  →  optional (forever)
```

Proposed:
```
epic-X-retrospective: optional
    → daemon detects epic X complete
    → daemon runs autonomous review
    → daemon writes report
    → daemon sets: epic-X-retrospective: review
    → daemon notifies human
    → daemon WAITS (will not process epic X+1 stories)
    → human reviews report
    → human sets: epic-X-retrospective: done
    → daemon proceeds to epic X+1
```

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Retrospective review burns too many tokens on large codebases | Scope the review to files changed in the epic (via git diff against the pre-epic state) + architecture/pattern analysis on key modules only |
| Human forgets to set `done` → daemon stuck forever | Notification includes clear instructions: "Set `epic-X-retrospective: done` in sprint-status.yaml to continue". Polling logs remind every cycle. |
| Review quality is low (LLM misses real issues) | The gate's primary value is the **pause**, not the review quality. Even a mediocre report forces the human to look at the project state. Over time, the review prompt can be refined. |
| Config complexity | Single boolean `epic_gate_enabled` (default `true`). No other config needed. |

## Relationship to Existing Workflows

- **Per-story code review** (`code-review/instructions.xml`): Unchanged. Continues to run after each story if `code_review_enabled: true`. Scoped to story diff.
- **Interactive retrospective** (`retrospective/instructions.md`): Unchanged. Remains available for manual Party Mode sessions. The human can run it after the autonomous review for deeper analysis.
- **Epic gate review**: NEW. Autonomous. Runs once per epic completion. Produces a report. Acts as a pipeline gate.

## Success Criteria

1. Daemon stops processing after the last story of an epic completes
2. An autonomous review report is generated and saved
3. Human is notified (if notifications enabled)
4. Sprint-status shows `epic-X-retrospective: review`
5. No stories from the next epic are processed until `epic-X-retrospective: done`
6. Gate integrates cleanly with existing dependency resolution (no special-casing in pipeline)
7. The fix is backward-compatible: projects without retrospective entries in sprint-status are unaffected