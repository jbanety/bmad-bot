---
title: 'Move post-session pipeline steps from SessionRunner to pipeline level'
type: 'refactor'
created: '2026-04-29'
status: 'in-progress'
baseline_commit: '0f7c3a0'
context:
  - '_bmad-output/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `SessionRunner` (API mode) contains 4 post-session pipeline steps (Final Commit, Dirty Worktree Cleanup, Impact Analysis, PR Summary) that SDK sessions don't get. This means SDK sessions don't commit uncommitted work, don't analyze downstream impacts, and don't generate PR summaries. The pipeline has two divergent code paths instead of one.

**Approach:** Extract the 4 post-session steps from `SessionRunner` and move them to `pipeline.rs`, after each `run_session()` → Completed. Commit uses a dedicated `utility` LLM role (configurable, defaults to a cheap model) in a one-shot session with the diff as context — produces descriptive conventional commit messages. Impact Analysis and PR Summary dispatch through `SessionRuntime` as short follow-up sessions. `SessionRunner` returns immediately after the main chat loop signals completion.

## Boundaries & Constraints

**Always:**
- New `utility` LLM role in config — optional, defaults to review config if empty (same fallback pattern as `epic_review` and `critic`)
- Commit step: `git add -A`, capture `git diff --cached`, one-shot LLM for message, `git commit -m "{msg}"`. No-op if nothing to commit.
- Impact Analysis and PR Summary dispatch through `SessionRuntime` (API or SDK, transparent)
- `SessionRunner` must become a pure chat-loop executor — no post-completion LLM turns
- WAL deletion stays after all post-session steps complete

**Ask First:**
- If Impact Analysis should be skipped entirely for create/review phases (currently only runs after dev)

**Never:**
- Don't change the main chat loop logic in `SessionRunner` (turns, consultations, response analysis)
- Don't duplicate post-session logic per runtime — one path in pipeline.rs

</frozen-after-approval>

## Code Map

- `src/config/mod.rs` -- Add `utility: LlmRoleConfig` to `LlmConfig` with empty-provider default (falls back to `review`)
- `src/llm/agent_factory.rs` -- Add `LlmRole::Utility` variant, wire `resolve_role_config` fallback
- `src/runtime/mod.rs` -- Add `Utility` to `resolve_role_config()`
- `src/session/runner.rs` -- Remove Steps 7-9 (Final Commit, Dirty Cleanup, Impact Analysis, PR Summary) from `ResponseAction::Completed` branch
- `src/pipeline.rs` -- Add `post_session_commit()`, `post_session_impact_analysis()`, `post_session_pr_summary()` methods
- `src/session/cleanup.rs` -- `get_dirty_files()` already exists, reuse from pipeline

## Tasks & Acceptance

**Execution:**
- [ ] `src/config/mod.rs` -- Add `utility: LlmRoleConfig` field to `LlmConfig` with `#[serde(default)]`. Update `_test_minimal()` and validation.
- [ ] `src/llm/agent_factory.rs` -- Add `LlmRole::Utility` variant. Update `config_for_role()`.
- [ ] `src/runtime/mod.rs` -- Add `LlmRole::Utility` to `resolve_role_config()`: falls back to `review` config when provider is empty.
- [ ] `src/runtime/sdk.rs` -- Add `LlmRole::Utility` to `config_for_role()` in SdkRuntime (same fallback).
- [ ] `src/session/runner.rs` -- Remove Steps 7-9 from `ResponseAction::Completed`. Keep: `completion_detected()`, `write_decisions()`, WAL delete, return `Completed` with `pr_context: None, pr_how_to_test: None, pr_additional_info: None`.
- [ ] `src/pipeline.rs` -- Add `async fn post_session_commit(&self, story: &StoryInfo)`: run `git add -A`, check `git diff --cached --stat` for changes, if dirty: capture `git diff --cached` (truncated to ~4K), one-shot LLM call for commit message generation (API via `AgentFactory` with `LlmRole::Utility`, SDK via subprocess — same `resolve_role_config` + `is_sdk_provider` pattern), then `git commit -m "{msg}"`. If nothing to commit, no-op.
- [ ] `src/pipeline.rs` -- Add `async fn post_session_impact_analysis(&self, story: &StoryInfo)`: dispatch short session via `session_runtime.run_session()` with impact analysis prompt. Best-effort, failure is non-blocking.
- [ ] `src/pipeline.rs` -- Add `async fn post_session_pr_summary(&self, story: &StoryInfo) -> PrSummary`: if `SessionOutcome::Completed.pr_context` is already set (SDK skill may produce one), use it. Otherwise dispatch short session for PR summary generation.
- [ ] `src/pipeline.rs` -- In `run_dev_pipeline()` after Completed: call `post_session_commit()` → `post_session_impact_analysis()` → `post_session_pr_summary()`. Wire PR summary into PR creation.
- [ ] `src/pipeline.rs` -- In `run_create_pipeline()` after Completed: call `post_session_commit()`.
- [ ] `src/pipeline.rs` -- In `run_review_pipeline()` after Completed: call `post_session_commit()`.
- [ ] Update tests affected by SessionRunner return value changes and new config field.

**Acceptance Criteria:**
- Given `utility` role configured with `haiku`/`o4-mini`, when post_session_commit runs on dirty worktree, then the commit message is descriptive and conventional-commits compliant
- Given `utility` role not configured (empty provider), when commit runs, then it falls back to `review` role config
- Given an SDK session that leaves uncommitted files, when pipeline post-session runs, then files are committed with a meaningful message
- Given an API session (where agent already committed), when post_session_commit runs, then it's a no-op (nothing to commit)
- Given SessionRunner after refactor, when ResponseAction::Completed fires, then it returns immediately without sending additional LLM turns

## Spec Change Log

## Design Notes

**Utility role rationale:** Commit messages, impact analysis, and PR summaries are lightweight LLM tasks. They don't need the dev model (Opus/GPT-5). A dedicated `utility` role lets the user configure a cheap, fast model (Haiku, o4-mini) for these mechanical tasks. Falls back to `review` config when unconfigured — zero breaking change for existing configs.

**Commit flow:**
```
git add -A
diff=$(git diff --cached | head -c 4096)
if [ -n "$diff" ]; then
  role_config = resolve_role_config(Utility)
  if role_config.is_sdk_provider():
    msg = sdk_oneshot(role_config, prompt + diff)  # subprocess
  else:
    msg = api_oneshot(role_config, prompt + diff)   # AgentFactory
  git commit -m "$msg"
fi
```

**All post-session LLM calls (commit, impact, PR summary)** use the same dual-runtime dispatch pattern: check `resolve_role_config(Utility).is_sdk_provider()` → SDK subprocess or API `AgentFactory`. Same code path regardless of what runtime the main session used — the `utility` role has its own provider config.

## Verification

**Commands:**
- `cargo build` -- expected: clean compilation
- `cargo test` -- expected: all existing + new tests pass
- `cargo clippy` -- expected: no new warnings

**Manual checks:**
- Run daemon with SDK provider, verify uncommitted changes are committed with descriptive message
- Run daemon with API provider, verify no double-commit (SessionRunner no longer commits)
- Configure `utility` role with haiku, verify cheap model is used for commit/impact/summary
