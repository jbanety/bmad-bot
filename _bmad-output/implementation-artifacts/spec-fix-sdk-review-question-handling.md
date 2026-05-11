---
title: 'Fix SDK review question handling — disable AskUserQuestion, robustify decision-needed detection'
type: 'bugfix'
created: '2026-05-11'
status: 'done'
baseline_commit: 'ed36171'
context:
  - '_bmad-output/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** When Claude Code runs code review, it calls its native `AskUserQuestion` tool to ask about `decision-needed` findings. This tool is blocked in `-p` (non-interactive) mode, causing the session to exit with the question as completion text instead of the review summary. The existing `has_decision_needed` detection then fails because it pattern-matches the completion text, which no longer contains "decision-needed" — only the fallback question. The critic consultation never runs.

**Approach:** Disable `AskUserQuestion` via `--disallowedTools` so Claude Code finishes the review normally (completion text = review summary with `decision-needed` tags). Add a fallback detection path that checks the story file for decision-needed findings when completion text detection misses.

## Boundaries & Constraints

**Always:** Both detection paths (completion text + story file) must agree — if either finds decision-needed, the consultation runs. Existing Codex review flow must remain unaffected.

**Ask First:** Any change to the consultation prompt or critic behavior.

**Never:** Modify the consultation/critic logic itself — it already works. No new LLM calls for detection. No changes to the supervisor module.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Claude Code review with decision-needed | AskUserQuestion disabled, review completes normally | Completion text has "decision-needed", consultation runs | N/A |
| Claude Code review, findings in story file but not in completion | Completion text lacks pattern, story file has `decision-needed` | Fallback detection triggers consultation | N/A |
| Review with no decision-needed | Clean review, no decision items | No consultation triggered, flow unchanged | N/A |
| Codex review | Codex doesn't use AskUserQuestion | No behavioral change | N/A |

</frozen-after-approval>

## Code Map

- `src/runtime/sdk_claude.rs:240-251` -- `build_claude_code_config`: CLI args for fresh Claude Code sessions
- `src/runtime/sdk_claude.rs:285-296` -- `build_claude_code_resume_config`: CLI args for resume sessions
- `src/pipeline/mod.rs:1183-1194` -- `has_decision_needed` detection logic (completion text)
- `src/pipeline/mod.rs:1131-1180` -- `has_findings_in_completion` + story file check
- `src/pipeline/mod.rs:1195-1211` -- consultation trigger conditional
- `src/pipeline/mod.rs:1776-1825` -- `build_review_consultations` (already correct)

## Tasks & Acceptance

**Execution:**
- [x] `src/runtime/sdk_claude.rs` -- Add `--disallowedTools` `AskUserQuestion` to both `build_claude_code_config` and `build_claude_code_resume_config` args vecs -- `AskUserQuestion` is a human-interactive tool, has no purpose in bot/non-interactive mode regardless of role (dev, review, critic)
- [x] `src/pipeline/mod.rs` -- After existing `has_decision_needed` completion text check, add fallback: read the story file and search for `decision-needed` (case-insensitive) in a `### Review Findings` section. Set `has_decision_needed = true` if found.
- [x] `src/runtime/sdk_claude.rs` -- Add unit tests for `--disallowedTools` presence in both config builders
- [x] `src/pipeline/mod.rs` -- Add unit test for story-file-based decision-needed detection fallback

**Acceptance Criteria:**
- Given any Claude Code session config (start or resume, any role), when the config is built, then args contain `--disallowedTools` and `AskUserQuestion`
- Given a review where completion text lacks "decision-needed" but story file has `decision-needed` in `### Review Findings`, when detection runs, then `has_decision_needed` is true and consultation triggers
- Given a review where neither completion text nor story file has decision-needed, when detection runs, then no consultation triggers

## Verification

**Commands:**
- `cargo test -p bmad-bot` -- expected: all tests pass including new ones
- `cargo clippy -p bmad-bot` -- expected: no warnings

## Suggested Review Order

**AskUserQuestion disable**

- Both CLI config builders now block the human-interactive tool
  [`sdk_claude.rs:249`](../../src/runtime/sdk_claude.rs#L249)

- Same for resume sessions
  [`sdk_claude.rs:297`](../../src/runtime/sdk_claude.rs#L297)

**Decision-needed fallback detection**

- `story_path` hoisted to outer scope for reuse by both detection paths
  [`mod.rs:1131`](../../src/pipeline/mod.rs#L1131)

- Fallback: if completion text misses, check the story file
  [`mod.rs:1197`](../../src/pipeline/mod.rs#L1197)

- New helper parses `### Review Findings` section for `decision-needed`
  [`mod.rs:5019`](../../src/pipeline/mod.rs#L5019)

**Tests**

- Config builder tests verify `--disallowedTools AskUserQuestion` presence
  [`sdk_claude.rs:813`](../../src/runtime/sdk_claude.rs#L813)

- Story file detection: present, absent, no section, missing file
  [`mod.rs:8383`](../../src/pipeline/mod.rs#L8383)
