---
title: 'Console phase result summaries and critic status'
type: 'refactor'
created: '2026-04-29'
status: 'done'
baseline_commit: '2ac8b57'
context:
  - '_bmad-output/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Console output during pipeline execution shows tool calls and LLM activity in real-time (which is good), but when a phase completes, the result is a bare duration with no meaningful summary. The user can't tell if the review found issues, whether the critic was triggered, or what the outcome was. The code review critic phase is invisible — no indication of skipped vs triggered.

**Approach:** Add one-line result summaries to phase completions and story completions. Track review findings count and critic trigger status through the pipeline, and surface them in the completion lines. Introduce `phase_complete_with_result()` to the renderer trait. Add explicit critic status output (skipped with reason or triggered with answer count).

## Boundaries & Constraints

**Always:**
- Keep all real-time output (tool calls, SDK events, LLM turns) — don't filter execution activity
- `NullRenderer` stays untouched
- New trait method must have a default impl to avoid breaking `NullRenderer`
- Plain mode gets equivalent summaries (ASCII)

**Ask First:**
- If the approach requires changing how the pipeline tracks findings count across phases

**Never:**
- Don't suppress SDK/LLM activity during execution — the user wants to see what the agent is doing live
- Don't switch to a TUI framework
- Don't change the indicatif spinner model

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Review complete, 0 findings | phase completes, no findings posted | `● Code Review [40s] — clean` | N/A |
| Review complete, 3 findings | phase completes, 3 findings extracted | `● Code Review [40s] — 3 findings` | N/A |
| Review failed | session failed | `✗ Code Review — LLM timeout` (existing behavior, unchanged) | N/A |
| Critic triggered, 2 answers | consultation_complete with 2 findings | `● Review critic [18s] — 2 answers` | N/A |
| Critic not triggered | no consultation_start for critic | `● Review critic — skipped (0 decision-needed)` at end of review phase | N/A |
| Create story complete | session completed | `● Create Story [47s] — story created` | N/A |
| Dev session complete | session completed | `● Dev Session [2m13s] — completed` | N/A |
| Story complete with PR | story_complete with url | `● Story 1-2 complete [3m12s] → https://...` | N/A |

</frozen-after-approval>

## Code Map

- `src/ui/renderer.rs` -- Add `phase_complete_with_result()` method with default impl
- `src/ui/mod.rs` -- UiHandle delegation for new method
- `src/ui/console.rs` -- ConsoleRenderer implementation: append result to completion line
- `src/pipeline.rs` -- Pass result summaries to phase_complete calls for review, create, dev. Add critic skipped output when review completes without critic trigger.

## Tasks & Acceptance

**Execution:**
- [x] `src/ui/renderer.rs` -- Add `fn phase_complete_with_result(&self, phase_name: &str, duration: Duration, result: &str)` with default impl delegating to `phase_complete()`.
- [x] `src/ui/mod.rs` -- Add delegation in UiHandle for the new method.
- [x] `src/ui/console.rs` -- Implement `phase_complete_with_result`: same as `phase_complete` but appends ` — {result}` after the duration bracket. Format: `● Phase Name [duration] — result`.
- [x] `src/pipeline.rs` -- In `run_review_pipeline()`: after code review session completes, compute findings count from `extract_review_report_from_story()` result and call `phase_complete_with_result("Code Review", duration, "N findings")` or `"clean"` if none.
- [x] `src/pipeline.rs` -- In `run_review_pipeline()`: after code review completes, if critic consultation was NOT triggered during the review session, emit `ui.phase_complete_with_result("Review critic", Duration::ZERO, "skipped (0 decision-needed)")`. If it WAS triggered, the existing `consultation_complete` already shows the count.
- [x] `src/pipeline.rs` -- In `run_create_pipeline()`: call `phase_complete_with_result("Create Story", duration, "story created")` on success.
- [x] `src/pipeline.rs` -- In `run_dev_pipeline()`: call `phase_complete_with_result("Dev Session", duration, "completed")` on success.
- [x] `src/ui/console.rs` -- No separate test needed — `phase_complete_with_result` follows the exact same pattern as `phase_complete` (take spinner, format line). Covered by compilation + manual verification.

**Acceptance Criteria:**
- Given a review pipeline with no findings, when review phase completes, then console shows `● Code Review [Ns] — clean`
- Given a review pipeline where critic is not triggered, when review phase ends, then console shows `● Review critic — skipped (0 decision-needed)`
- Given a review pipeline where critic triggers with 2 findings, then `consultation_complete` already shows `● Review critic [18s] — 2 answers`
- Given existing phase_complete calls without result, when rendering, then output is unchanged (default delegation)

## Spec Change Log

## Design Notes

**Target output for review pipeline:**
```
◉ Story 1-2 — Rate limit widget display
  ● Push Branch [1s]
  ● Create PR [0s]
  ◉ Code Review [codex/o4-mini]
      ● Read src/components/widget.tsx
        └ 142 lines
      └ turn 1 — analyzed rate limit display logic
      ● Edit src/components/widget.tsx
    ● Adversarial review [31s] — 2 findings
    ● Review critic — skipped (0 decision-needed)
  ● Code Review [40s] — 2 findings
  ● Notification [0s]
● Story 1-2 complete → https://gitlab.com/.../merge_requests/4
```

**Critic status tracking:** The pipeline already knows if consultations were configured and which ones fired (via `fired` set in `SdkConsultationRunner` / `check_consultation_triggers`). The "skipped" line is emitted by the pipeline when review completes and no critic consultation was triggered — it's a pipeline-level decision, not a renderer concern.

## Verification

**Commands:**
- `cargo build` -- expected: clean compilation
- `cargo test` -- expected: all existing + new tests pass
- `cargo clippy` -- expected: no new warnings

**Manual checks:**
- Run daemon against a test project and verify phase completion lines include result summaries
