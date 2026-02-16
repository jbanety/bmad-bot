# Story 4.6: Post-Implementation Impact Analysis on Downstream Stories

Status: review

## Story

As a daemon operator,
I want the agent to analyze and update downstream dependent stories after completing a story,
so that the next agent picking up a dependent story works from accurate assumptions instead of stale specs, reducing wasted tokens, wrong patterns, and rework.

## Acceptance Criteria

1. **Given** the agent has signaled `<<BMAD_JOB_DONE>>` and the final commit (Step 7) has completed, **When** the session runner executes the impact analysis step (Step 8), **Then** it sends an impact analysis prompt to the agent in a dedicated chat turn with full tool access.

2. **Given** the impact analysis prompt is sent, **When** the agent processes it, **Then** it reads `sprint-status.yaml` and identifies stories whose `depends-on` references the completed story (by full key or short key `{epic}-{story}`), **And** it checks subsequent stories in the same epic (document order) as a secondary criterion.

3. **Given** downstream dependent stories are identified, **When** the agent reads their Dev Notes, **Then** it compares the "Previous Story Intelligence" sections against what was actually implemented, **And** it updates only "Previous Story Intelligence" sections where actual implementation deviates from planned assumptions, **And** updates include: what changed vs the original plan, new APIs/patterns/modules to use, obsolete assumptions to discard, **And** updates are idempotent — sections are replaced, not appended.

4. **Given** the completed story introduced new modules or changed interfaces, **When** the agent checks for `architecture.md`, **Then** it verifies the file exists before attempting to read or update it (not all projects have one), **And** it updates architecture references only if new modules or changed interfaces were introduced.

5. **Given** downstream stories or architecture have been updated, **When** the agent commits the changes, **Then** the commit message uses the prefix `docs(stories): update downstream specs after {story_key}`.

6. **Given** no downstream stories need updating, **When** the agent evaluates the impact, **Then** it reports that nothing needs updating and moves on without making changes — it does not invent changes.

7. **Given** the impact analysis chat turn fails (LLM error, timeout, context window exhaustion), **When** the session runner handles the failure, **Then** it proceeds to the PR summary step (Step 9) without error, **And** the story completion is not blocked or marked as failed, **And** the failure is logged via `tracing::warn!`.

8. **Given** the impact analysis step completes (success or skip), **When** the PR summary step (Step 9) executes, **Then** it is aware that an impact analysis commit may have been added to the branch, **And** the PR description reflects both the implementation work and any downstream spec updates.

9. **Given** all changes are complete, **When** validation runs, **Then** `cargo build`, `cargo test`, `cargo clippy`, and `cargo fmt` all pass with zero errors and zero warnings.

## Tasks / Subtasks

- [x] Task 1: Add impact analysis prompt and chat turn in `run_session()` (AC: #1, #2, #3, #4, #5, #6)
  - [x] 1.1: Construct the impact analysis prompt string with story key, sprint-status path, planning artifacts path, and scope guard instructions
  - [x] 1.2: Insert the `stream_chat()` call between the final commit block (current Step 7) and the PR summary block (current Step 8, renumbered to Step 9)
  - [x] 1.3: Add `state.add_user_message()` and `state.add_assistant_message()` calls for WAL persistence
  - [x] 1.4: Increment the turn counter for LLM logging consistency

- [x] Task 2: Implement best-effort error handling for impact analysis turn (AC: #7)
  - [x] 2.1: Wrap the `stream_chat()` call in a match with `Ok(r)` / `Err(e)` arms
  - [x] 2.2: On `Err`: log `tracing::warn!(action = "impact_analysis_failed", ...)` and proceed to PR summary
  - [x] 2.3: On `Ok`: log `tracing::info!(action = "impact_analysis_done", ...)` and proceed to PR summary

- [x] Task 3: Renumber PR summary to Step 9 (AC: #8)
  - [x] 3.1: Update the inline comment from `Step 8` to `Step 9`
  - [x] 3.2: No functional change to PR summary logic — only comment renumbering

- [x] Task 4: Update existing tests and add new test (AC: #9)
  - [x] 4.1: Add `test_impact_analysis_prompt_construction` — verify prompt contains story key, sprint-status path, planning artifacts path, and scope guard language
  - [x] 4.2: Verify existing `parse_pr_summary` tests still pass unchanged
  - [x] 4.3: Run full `cargo test`, `cargo clippy`, `cargo fmt --check`

## Dev Notes

### Single File Change — Minimal Blast Radius

This story modifies exactly **one file**: `src/session/runner.rs`. The change is ~40-50 lines of new code inserted between the existing Step 7 (final commit) and Step 8 (PR summary). The pattern is identical to the final commit block — a `stream_chat()` call wrapped in a match with Ok/Err handling.

No new modules, no new tools, no new dependencies, no new error types.

### Exact Insertion Point in runner.rs

The post-completion sequence lives inside `run_session()` in the `ResponseAction::Completed` match arm. Current structure:

```
ResponseAction::Completed => {
    // Step 7: Final commit
    let commit_msg = "Commit ALL uncommitted changes now...";
    // ... stream_chat() call with Ok/Err match ...

    // Step 8: PR summary (RENUMBER TO STEP 9)
    let pr_summary_prompt = format!("STOP. Do NOT use any tools...");
    // ... stream_chat() call with Ok/Err match ...

    // Write decisions file, delete WAL, return SessionOutcome::Completed
}
```

Insert the impact analysis block **between** the final commit block and the PR summary block. The new structure becomes:

```
ResponseAction::Completed => {
    // Step 7: Final commit (unchanged)
    // ... existing code ...

    // ── Step 8: Impact analysis (NEW) ─────────────────────
    // ... new impact analysis block ...

    // ── Step 9: PR summary (RENUMBERED from Step 8) ──────
    // ... existing PR summary code, comment updated ...

    // Write decisions file, delete WAL, return SessionOutcome::Completed
}
```

[Source: src/session/runner.rs#L1524-1665 — ResponseAction::Completed arm with Step 7 and Step 8]

### Impact Analysis Prompt Design

The prompt must be a single, self-contained instruction that gives the agent everything it needs. The agent retains full tool access (unlike the PR summary turn which is text-only).

Key elements the prompt must include:
- The completed story key (`{story_key}`)
- The path to `sprint-status.yaml` (from `self.config` — the implementation artifacts path)
- The path to planning artifacts (for `architecture.md` existence check)
- Explicit instructions to use `depends-on` as the primary discovery criterion
- Same-epic document order as secondary criterion
- Scope guard: **only** update "Previous Story Intelligence" sections and architecture references
- Idempotence: replace sections, do not append
- Commit prefix: `docs(stories): update downstream specs after {story_key}`
- Explicit "do not invent changes" guard
- Explicit "check architecture.md existence before reading" guard

The prompt should reference concrete paths so the agent doesn't guess:
- `sprint-status.yaml` lives at `{implementation_artifacts}/sprint-status.yaml`
- Story files live at `{implementation_artifacts}/` (same directory)
- `architecture.md` lives at `{planning_artifacts}/architecture.md`

These paths are available via `self.config` fields in `SessionRunner`.

### Config Path Resolution

`SessionRunner` holds `config: Arc<BotConfig>`. The relevant config paths:
- `self.config.implementation_artifacts` — resolves to `_bmad-output/implementation-artifacts`
- `self.config.planning_artifacts` — resolves to `_bmad-output/planning-artifacts`

These are already used elsewhere in the codebase (e.g., `write_decisions()` uses implementation_artifacts). Use the same pattern.

[Source: src/session/runner.rs#L207-220 — SessionRunner struct holds Arc<BotConfig>]

### Pattern: Follow the Final Commit Block Exactly

The final commit block (Step 7) at L1531-1564 is the exact template for the impact analysis block:

1. Construct prompt string
2. `state.add_user_message(&prompt);`
3. `let history = state.to_rig_messages();`
4. `log_llm_request("dev-session", turn, prompt_label, history.len());`
5. `match agent.stream_chat(&prompt, history, Some(&self.shutdown)).await { ... }`
6. On Ok: `log_llm_response(...)`, `state.add_assistant_message(&r)`, `state.save(...)`, `tracing::info!(...)`
7. On Err: `log_llm_error(...)`, `tracing::warn!(...)` — proceed anyway

The only difference: the impact analysis turn has tool access (same as final commit), while the PR summary turn does NOT (the prompt explicitly says "Do NOT use any tools"). The impact analysis prompt must NOT include "Do NOT use any tools" — the agent needs tools to read sprint-status, read story files, edit them, and commit.

### Turn Counter Management

The current code uses `turn` for the final commit and `turn + 1` for the PR summary. With the new step:
- Final commit: `turn` (unchanged)
- Impact analysis: `turn + 1` (new)
- PR summary: `turn + 2` (was `turn + 1`)

This affects the `log_llm_request` / `log_llm_response` / `log_llm_error` calls only — cosmetic, for log readability.

### Previous Story Intelligence (Story 4.5 — LLM Provider Abstraction)

Story 4.5 refactored the session runner to use `BuiltAgent` with `stream_chat()` dispatch. The call pattern is:

```rust
agent.stream_chat(&prompt, history, Some(&self.shutdown)).await
```

Where `agent` is `&BuiltAgent`. This is the exact call to use for the impact analysis turn. No special provider handling needed — `BuiltAgent` abstracts it.

[Source: src/session/runner.rs — all stream_chat calls use BuiltAgent pattern after Story 4.5]

### Previous Story Intelligence (Story 5.4 — Enriched PR Description)

Story 5.4 and commit `6450450` established the pattern of enriched post-completion turns with grounded context. The PR summary prompt explicitly reminds the agent of project/story/branch context to prevent hallucination. The impact analysis prompt should follow the same grounding pattern — include concrete paths, story key, and explicit constraints.

[Source: src/session/runner.rs#L1578-1602 — PR summary prompt with context grounding]

### Symmetric Pattern: Pre-Dev Spec Update (FR5-6)

The pre-dev spec update (Story 4.3) has the agent read prior stories BEFORE starting development. This story adds the symmetric post-impl step — propagating FORWARD to downstream stories after completion. Together they form a closed loop:
- **Pre-dev (FR5-6):** Agent reads what previous stories built → updates current story's specs
- **Post-impl (FR43):** Agent reads what it just built → updates downstream stories' specs

### WAL Persistence

Every prompt/response pair must be saved to the WAL via `state.add_user_message()` / `state.add_assistant_message()` / `state.save()`. This ensures crash recovery can replay the impact analysis turn if interrupted. The existing pattern in Steps 7 and 8 already does this — follow it exactly.

### What NOT to Change

- ❌ Do NOT modify `SessionOutcome::Completed` — no new fields needed. The impact analysis commit is just another commit on the branch, captured by the normal git push.
- ❌ Do NOT modify `pipeline.rs` — the pipeline pushes whatever commits are on the branch.
- ❌ Do NOT modify tool implementations — the agent uses existing `read_file`, `edit_file`, `git` tools.
- ❌ Do NOT modify `session/analyzer.rs` — response analysis is unchanged.
- ❌ Do NOT modify `session/cleanup.rs` — the `unblock_dependents` logic is separate and unchanged.
- ❌ Do NOT add new error types — use the existing `Err(e)` from `stream_chat()` directly.
- ❌ Do NOT modify any BMAD files under `_bmad/`.

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code
- ❌ **NO** `println!` or `eprintln!` — use `tracing` with structured fields only
- ❌ **NO** blocking the story completion on impact analysis failure — always proceed
- ❌ **NO** "Do NOT use any tools" in the impact analysis prompt — the agent NEEDS tools
- ❌ **NO** inventing a new chat turn abstraction — follow the existing inline pattern
- ❌ **NO** modifying the `SessionOutcome` enum shape
- ❌ **NO** adding the impact analysis as a separate function — keep it inline in the match arm like Steps 7 and 9

### Project Structure Notes

```
src/
├── session/
│   ├── runner.rs           # MODIFY — insert ~40-50 lines for impact analysis between Step 7 and Step 8 (renumbered to Step 9)
│   └── (all other files)   # UNCHANGED
└── (all other modules)     # UNCHANGED
```

Alignment with project structure is perfect — this touches only `runner.rs` which already owns the post-completion sequence.

### References

- [Source: _bmad-output/planning-artifacts/architect-brief-post-impl-impact-analysis.md] — Full architect brief with problem statement, proposed change, design constraints, and risk analysis
- [Source: _bmad-output/planning-artifacts/architecture.md#Data Flow step 8] — Architecture documentation of the impact analysis step
- [Source: _bmad-output/planning-artifacts/architecture.md#Coherence Validation] — Symmetric pre-dev/post-impl pattern documented
- [Source: _bmad-output/planning-artifacts/prd.md#FR43] — Functional requirement for post-implementation impact analysis
- [Source: _bmad-output/planning-artifacts/epics.md#Story 4.6] — Epic breakdown with acceptance criteria
- [Source: src/session/runner.rs#L1524-1665] — ResponseAction::Completed arm — exact insertion point
- [Source: src/session/runner.rs#L1531-1564] — Step 7 final commit block — template pattern to follow
- [Source: src/session/runner.rs#L1565-1648] — Step 8 PR summary block — renumber to Step 9
- [Source: src/session/runner.rs#L207-220] — SessionRunner struct with Arc<BotConfig>
- [Source: _bmad-output/project-context.md#Pre-Development Spec Update] — Pre-dev spec update documentation (symmetric counterpart)
- [Source: _bmad-output/implementation-artifacts/4-5-llm-provider-abstraction-agent-factory.md#Dev Notes] — Previous story intelligence on BuiltAgent and stream_chat() pattern

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

- `cargo test impact_analysis_prompt_contains` — 6 passed, 0 failed (includes new short-key prompt assertion)
- `cargo build` — passes (repository has pre-existing `dead_code` warnings outside Story 4.6 scope)
- `cargo test` — 853 passed, 0 failed
- `cargo clippy --all-targets --all-features -- -D warnings` — fails due pre-existing repository-wide lint debt unrelated to Story 4.6 (e.g., `src/config/mod.rs`, `src/review/mod.rs`, `src/tools/read_file.rs`, etc.)
- `cargo fmt -- --check` — fails due pre-existing repository-wide formatting drift in unrelated files under `src/` and `tests/`

### Completion Notes List

- Extracted `build_impact_analysis_prompt()` as a public helper for testability while keeping the `stream_chat()` call inline in the `ResponseAction::Completed` arm per Dev Notes guidance.
- Added `derive_short_story_key()` and now inject an explicit short-key value in the impact prompt (`{epic}-{story}`), e.g. `4-6`, so `depends-on` matching guidance is concrete and deterministic.
- Impact analysis prompt includes: story key, sprint-status.yaml path, implementation artifacts path, planning artifacts path (architecture.md), scope guard, idempotent replacement instructions, commit prefix `docs(stories): update downstream specs after {story_key}`, and "do not invent changes" guard.
- Follows the exact pattern of Step 7 (final commit): `state.add_user_message()` → `to_rig_messages()` → `log_llm_request()` → `stream_chat()` match → Ok: log + persist WAL / Err: warn + proceed.
- Added WAL persistence on the Step 8 error path (`state.save(...)`) so the impact prompt turn is durable even when impact analysis fails before assistant response.
- Agent retains full tool access during impact analysis (unlike PR summary which is text-only).
- Turn counter updated: Step 7 = `turn`, Step 8 (impact analysis) = `turn + 1`, Step 9 (PR summary) = `turn + 2`.
- Removed an orphan doc comment (stale `is_transient_llm_error` description) that became visible as a clippy `empty_line_after_doc_comments` error after insertion.
- 7 unit tests cover impact prompt content validation (story key, explicit short key, sprint-status path, planning artifacts path, scope guard, commit prefix, idempotent language).
- All existing `parse_pr_summary` tests pass unchanged.
- AC #9 status: `cargo build` and `cargo test` pass; strict `clippy -D warnings` and `fmt --check` remain blocked by pre-existing repository-wide baseline issues outside Story 4.6 scope.

### Change Log

- Implemented post-implementation impact analysis step (Step 8) in `run_session()` — inserts between final commit (Step 7) and PR summary (renumbered to Step 9). (2026-02-15)
- Post-review hardening pass: injected explicit short-key guidance into impact prompt and persisted WAL on impact-analysis error path; updated validation notes to reflect pre-existing repo-wide clippy/fmt baseline failures outside Story 4.6 scope. (2026-02-16)

### File List

- `src/session/runner.rs` — Added `build_impact_analysis_prompt()` helper, inserted Step 8 impact analysis block in `ResponseAction::Completed` arm, renumbered Step 8 → Step 9 for PR summary, adjusted turn counters from `turn + 1` to `turn + 2`, removed orphan doc comment, added impact-prompt tests, added explicit short-key prompt injection, and persisted WAL on impact-analysis error path.
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — Updated `4-6-post-implementation-impact-analysis` status: `ready-for-dev` → `in-progress` → `review`.
- `_bmad-output/implementation-artifacts/4-6-post-implementation-impact-analysis.md` — Marked all tasks/subtasks complete, updated Dev Agent Record and status, and documented post-review fixes/validation baseline context.
