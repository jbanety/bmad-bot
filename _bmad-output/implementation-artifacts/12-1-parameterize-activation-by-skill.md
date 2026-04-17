# Story 12.1: Parameterize Activation by Skill

Status: done

## Story

As a daemon operator,
I want agent sessions to be activated by loading a BMAD skill (`SKILL.md`) instead of a persona file (`dev.md`),
So that the bot aligns with BMAD v6.2+ skill-based workflows and no longer depends on persona/menu interaction.

## Acceptance Criteria

1. **AC-1: Dev and review callers pass skill paths instead of persona paths**
   - **Given** `src/session/runner.rs` has 4 call sites passing `"_bmad/bmm/agents/dev.md"` to `activate_agent()` (L1084, L1315, L1525, and shallow-WAL path at L567) and `src/review/mod.rs` has 1 call site (L550)
   - **When** this story is implemented
   - **Then** all 4 call sites in `runner.rs` pass `.github/skills/bmad-dev-story/SKILL.md` instead
   - **And** the 1 call site in `review/mod.rs` passes `.github/skills/bmad-code-review/SKILL.md` instead
   - **And** `src/supervisor/architect.rs` is NOT changed — it still loads `_bmad/bmm/agents/architect.md` (persona-based; evaluated in Story 12.4)

2. **AC-2: Post-activation commands removed — `run_session()` accepts `skill_path`**
   - **Given** `run_session()` currently hardcodes `"_bmad/bmm/agents/dev.md"` and sends `"Execute [DS] for story file: {path}"` as the first user message
   - **When** this story is implemented
   - **Then** `run_session()` accepts a `skill_path: &str` parameter forwarded to all 4 `activate_agent()` call sites within it
   - **And** the initial message retains English override and branch reminder but removes the `[DS]` command (exact format in Dev Notes)
   - **And** `drive_activation_and_recover()` and `context_limit_recovery()` also accept and forward `skill_path`
   - **And** `SessionRunner::run()` passes `.github/skills/bmad-dev-story/SKILL.md` as the `skill_path` argument

3. **AC-3: Review runner post-activation command removed**
   - **Given** `src/review/mod.rs` currently sends `"Execute [CR] for story file: {path}"` after activation
   - **When** this story is implemented
   - **Then** the post-activation message is: English override + story file path — no `[CR]` command
   - **And** no branch reminder — review sessions do not commit code
   - **And** the skill is self-directing

4. **AC-4: `build_preamble()` updated — skill instruction ADDED, persona instructions RETAINED**
   - **Given** `build_preamble()` (~L255–258) ends with: "fully embody the agent persona and follow ALL activation instructions", "execute activation steps in order — load configuration files via tools, then greet and display the menu", "wait for user input after displaying the menu"
   - **And** `src/supervisor/architect.rs` calls the SAME `build_preamble()` (L357) and still loads `_bmad/bmm/agents/architect.md` — the Architect multi-turn flow depends on these persona instructions
   - **When** this story is implemented
   - **Then** the persona activation rules are NOT removed — removing them breaks the Architect session (Story 12.4 scope)
   - **And** the following skill instruction is ADDED after the existing persona rules: "When provided a SKILL.md file in context, follow its instructions completely. Use your read_file tool to load any referenced workflow files (e.g., ./workflow.md). The skill is self-contained — execute it autonomously without waiting for user commands."
   - **And** all other preamble content is retained: tool list, tool usage rules, branch management, completion sentinel (`<<BMAD_JOB_DONE>>`), English override, sequential tool workaround for preview models

5. **AC-5: Doc comments updated**
   - **Given** `src/session/agent.rs` module doc at L11–12 says "loads config.yaml, greets user, shows menu" and "Caller sends a menu command (DS for dev, CR for review, CH for supervisor)"
   - **And** `src/llm/agent_factory.rs` `BuiltAgent::activate_agent()` doc at L116 shows `"_bmad/bmm/agents/dev.md"` as the example path
   - **When** this story is implemented
   - **Then** `agent.rs` module doc is updated to describe dual-mode activation (SKILL.md file OR persona file)
   - **And** `agent_factory.rs` doc example is updated to a skill path; note that persona paths remain valid (Architect)

6. **AC-6: Compilation and tests pass**
   - **Given** all changes above
   - **When** `cargo build` and `cargo test` are run
   - **Then** zero compilation errors and zero new warnings
   - **And** `test_build_preamble_contains_activation_rules` (~L759) still passes — persona instructions retained, assertion `preamble.contains("activation instructions")` still holds
   - **And** new tests for skill instruction presence are added and pass

## Tasks / Subtasks

- [x] Task 1: Update `build_preamble()` and doc comments in `src/session/agent.rs` (AC: #4, #5)
  - [x] 1.1 Update module-level doc (L1–12): replace step 4 and step 5 with dual-mode description covering both skill activation (autonomous execution) and persona activation (embody persona, await commands)
  - [x] 1.2 In `build_preamble()` (~L255–258), ADD after the existing "wait for user input" line: "When provided a SKILL.md file in context, follow its instructions completely. Use your read_file tool to load any referenced workflow files (e.g., ./workflow.md). The skill is self-contained — execute it autonomously without waiting for user commands." — do NOT remove the preceding persona rules
  - [x] 1.3 Verify preamble still contains: tool list, tool usage rules, branch management, completion sentinel, English override, sequential tool workaround, AND the retained persona rules
  - [x] 1.4 Update `activate_agent()` function doc (~L658–659): replace "processes the activation steps (loads config.yaml via tools, reads the story file, shows the greeting and menu)" with dual-mode language

- [x] Task 2: Update `run_session()` call chain in `src/session/runner.rs` (AC: #1, #2)
  - [x] 2.1 Add `skill_path: &str` parameter to `run_session()` signature (~L1271)
  - [x] 2.2 Replace ALL 4 hardcoded `"_bmad/bmm/agents/dev.md"` strings (use `grep _bmad/bmm/agents/dev.md src/session/runner.rs` to locate all):
    - Normal activation path (~L1315) — replace with `skill_path`
    - Empty-history recovery inside `run_session()` (~L1525) — replace with `skill_path`
    - `drive_activation_and_recover()` (~L1084) — replace with `skill_path`
    - Shallow-WAL path in `recover_from_wal()` (~L567) — forward via struct field (see 2.9)
  - [x] 2.3 Replace the initial_message (~L1389–1401) containing `"Execute [DS]"`: retain English override and branch reminder lines, change last line from `"Execute [DS] for story file: {}"` to `"Story file: {}"` — no DS command
  - [x] 2.4 Same change in recovery initial_message (~L1586–1589): remove `DS` from the message, keep English override prefix
  - [x] 2.5 Update `drive_activation_and_recover()` (~L1063): add `skill_path: &str`; replace `"_bmad/bmm/agents/dev.md"` with `skill_path`; replace `ch_msg` (~L1110) containing `"Execute [CH]"` with `"IMPORTANT: ALL communication MUST be in English regardless of config file settings. Continue recovery for story file: {story_specs_path}"`
  - [x] 2.6 Update `context_limit_recovery()` (~L972): add `skill_path: &str`, forward to `drive_activation_and_recover()`
  - [x] 2.7 `build_agent_for_role()` (~L799) only builds the agent, does not activate — no `skill_path` needed there
  - [x] 2.8 Update `SessionRunner::run()` (public entry point, ~L650): pass `".github/skills/bmad-dev-story/SKILL.md"` as `skill_path` to `run_session()`
  - [x] 2.9 **Design choice for `recover_from_wal()`:** add `skill_path: String` field to the `SessionRunner` struct, set to `".github/skills/bmad-dev-story/SKILL.md"` in `SessionRunner::new()`, access as `&self.skill_path` everywhere internally — avoids threading the parameter through `recover_from_wal()` and its callers in `pipeline.rs`

- [x] Task 3: Update `src/review/mod.rs` (AC: #1, #3)
  - [x] 3.1 Replace `"_bmad/bmm/agents/dev.md"` (~L550) with `".github/skills/bmad-code-review/SKILL.md"`
  - [x] 3.2 Replace initial_message (~L579) containing `"Execute [CR] for story file: {}"` with `"IMPORTANT: ALL communication MUST be in English regardless of config file settings. Story file: {story_specs_path}"` — no `[CR]`, no branch reminder (review sessions do not commit code)

- [x] Task 4: Update doc comment in `src/llm/agent_factory.rs` (AC: #5)
  - [x] 4.1 Update `BuiltAgent::activate_agent()` doc (~L114–122): change example from `"_bmad/bmm/agents/dev.md"` to `.github/skills/bmad-dev-story/SKILL.md`; add note that persona paths (e.g., `_bmad/bmm/agents/architect.md`) remain valid for the Architect session

- [x] Task 5: Update tests (AC: #6)
  - [x] 5.1 `test_build_preamble_contains_activation_rules` (~L759): assertions `preamble.contains("<context><files>")` and `preamble.contains("activation instructions")` still pass because persona rules are retained — add a comment noting dual-mode intent
  - [x] 5.2 Add `test_build_preamble_contains_skill_instructions`: assert `preamble.contains("SKILL.md")` and `preamble.contains("workflow.md")`
  - [x] 5.3 Add `test_build_preamble_retains_persona_rules`: assert `preamble.contains("activation instructions")` holds (Architect compatibility guard — must never regress)
  - [x] 5.4 Grep for any test asserting `"Execute [DS]"`, `"Execute [CR]"`, or `"Execute [CH]"` in session/review tests — update expected strings to new no-command format

- [x] Task 6: Verify (AC: #6)
  - [x] 6.1 `cargo build` — zero errors
  - [x] 6.2 `cargo test` — all tests pass (1131 baseline + new tests from Task 5)
  - [x] 6.3 `cargo clippy` — zero new warnings

## Dev Notes

### Epic 12 Context

Epic 12 replaces persona/menu activation with BMAD v6.2+ skill-based sessions. This is story 12.1 — it swaps the file loaded into `ContextBuilder`. The XML activation mechanism (Zed-style context injection as first user message) is completely unchanged.

Epic 12 parallel branches:
- **Skill activation:** 12.1 → 12.2 (this story → simplify ResponseAnalyzer)
- **SpawnAgent:** 12.3 → 12.4 (new tool → universal registration)
- Both converge at 12.5 (tests)

This story does NOT touch `ResponseAnalyzer` — that is Story 12.2 scope.

### ⚠️ CRITICAL: Architect Session Uses the Shared `build_preamble()`

`src/supervisor/architect.rs` calls the SAME `build_preamble()` function (verified at L357: `let preamble = build_preamble(&mcp_tool_names, supervisor_model)`) and still loads `_bmad/bmm/agents/architect.md` via `activate_agent()`. The Architect multi-turn flow (activate → CH → load context → answer) depends on the persona activation instructions in the preamble.

**If persona instructions are REMOVED from `build_preamble()`, the Architect session breaks.**

**Solution:** ADD skill instructions alongside; do NOT remove persona instructions. Story 12.4 evaluates migrating `ArchitectSession` to `spawn_agent` — at that point persona instructions may be removed.

**DO NOT change `src/supervisor/architect.rs`** in this story.

### ⚠️ CRITICAL: Only Three Things Change

`activate_agent()` already accepts `agent_relative_path` as a parameter. `ContextBuilder` is untouched. The only changes are:

1. **What file** is passed to `activate_agent()` — SKILL.md instead of persona
2. **What initial message** is sent after activation — story context only, no `[DS]`/`[CR]`/`[CH]`
3. **What the preamble says** — skill instruction ADDED; persona instructions RETAINED

### ⚠️ WARNING: Line Numbers Will Drift

All line numbers are from the current unmodified codebase. After each edit within a file, subsequent numbers shift. **Use the grep patterns in the tables below** to re-locate targets — do not trust line numbers after the first edit in a file.

### Skill File Path Resolution

Skill paths follow the same relative-from-project-root convention as persona paths. `activate_agent()` joins: `Path::new(project_root).join(agent_relative_path)`. So `.github/skills/bmad-dev-story/SKILL.md` resolves correctly — same mechanics as `_bmad/bmm/agents/dev.md`. If the skill file is missing, `ContextBuilder::add_file_from_disk()` returns an error propagated through `activate_agent()` — same behavior as a missing persona file, no special handling needed.

### Exact Initial Message Format After Skill Activation

Dev session (replaces `"Execute [DS] for story file: {}"` in `run_session()`):

```
IMPORTANT: ALL communication MUST be in English regardless of config file settings.
BRANCH REMINDER: You are already on branch `{branch_name}`. Do NOT create, checkout, or switch branches — the daemon manages branch lifecycle. Just commit your work on the current branch.
Story file: {specs_path}
```

Review session (replaces `"Execute [CR] for story file: {}"` in `review/mod.rs`):

```
IMPORTANT: ALL communication MUST be in English regardless of config file settings. Story file: {specs_path}
```

Recovery session (replaces `"Execute [CH]"` in `drive_activation_and_recover()`):

```
IMPORTANT: ALL communication MUST be in English regardless of config file settings. Continue recovery for story file: {specs_path}
```

### Exact Call Sites to Modify (Pre-Located)

**`src/session/runner.rs`** — 4 persona path strings + 2 post-activation messages:

| Approx. Line | Grep Pattern | Current | Change To |
|---|---|---|---|
| L567 | `"Activation-only WAL"` comment above | calls `run_session()` | forward `skill_path` / use `&self.skill_path` |
| L1084 | `"context_limit_activation_failed"` in nearby error | `"_bmad/bmm/agents/dev.md"` in `drive_activation_and_recover` | `skill_path` |
| L1110 | `ch_msg =` in `drive_activation_and_recover` | `"...Execute [CH]"` | `"...Continue recovery for story file: ..."` |
| L1315 | `ui.llm_request_content("dev"` above | `"_bmad/bmm/agents/dev.md"` (normal activation) | `skill_path` |
| L1389–1401 | `Execute [DS]` | `"Execute [DS] for story file: {}"` | `"Story file: {}"` (no DS) |
| L1525 | `ui.llm_request_content("recovery"` above | `"_bmad/bmm/agents/dev.md"` (empty-history recovery) | `skill_path` |
| L1586–1589 | `DS for story file` | `"...DS for story file: {}"` | `"...Story file: {}"` (no DS) |

**`src/session/runner.rs`** — functions needing `skill_path` in signature:

| Function | Approx. Line | Action |
|---|---|---|
| `run_session()` | L1271 | Add `skill_path: &str` param |
| `drive_activation_and_recover()` | L1063 | Add `skill_path: &str`, forward to `activate_agent` |
| `context_limit_recovery()` | L972 | Add `skill_path: &str`, forward to `drive_activation_and_recover` |
| `SessionRunner::run()` | ~L650 | Pass `".github/skills/bmad-dev-story/SKILL.md"` as `skill_path` |
| `recover_from_wal()` | ~L490 | See Task 2.9 — add `SessionRunner` struct field |

**`src/review/mod.rs`**:

| Approx. Line | Grep Pattern | Current | Change To |
|---|---|---|---|
| L550 | `"review"` label in `activate_agent` call | `"_bmad/bmm/agents/dev.md"` | `".github/skills/bmad-code-review/SKILL.md"` |
| L579 | `Execute [CR]` | `"Execute [CR] for story file: {}"` | `"Story file: {}"` (no CR, no branch reminder) |

**`src/session/agent.rs`**:

| Approx. Line | Grep Pattern | Change |
|---|---|---|
| L1–12 | `//! 4. The agent processes` | Update module doc — dual-mode activation |
| L255–258 | `Wait for user input after displaying the menu` | ADD skill instruction AFTER existing line (do NOT remove) |
| L658–659 | `processes the activation steps` | Update `activate_agent()` doc — dual-mode |

**`src/llm/agent_factory.rs`**:

| Approx. Line | Grep Pattern | Change |
|---|---|---|
| L116 | `_bmad/bmm/agents/dev.md` in doc comment | Update example to skill path; note persona paths still valid |

### Full `skill_path` Propagation Call Chain

```
SessionRunner::run(story, base_branch_override)
  └─ run_session(agent, story, ..., skill_path)
       ├─ activate_agent(..., skill_path, ...)          ← normal path (~L1315)
       ├─ activate_agent(..., skill_path, ...)          ← empty-history recovery (~L1525)
       ├─ context_limit_recovery(state, story, ..., skill_path)
       │    └─ drive_activation_and_recover(agent, ..., skill_path)
       │         ├─ activate_agent(..., skill_path, ...) ← recovery (~L1084)
       │         └─ run_session(agent, ..., skill_path)  ← recursive Box::pin (~L1236)
       └─ [via recover_from_wal()] run_session(agent, ..., skill_path) ← shallow WAL (~L567)
```

Recommended: add `skill_path: String` field to `SessionRunner` struct (set to `".github/skills/bmad-dev-story/SKILL.md"` in `SessionRunner::new()`). Access as `&self.skill_path` internally. The review runner does not need this — `review/mod.rs` hardcodes the review skill path directly.

### Skill File Paths (Verified Exist)

- Dev session: `.github/skills/bmad-dev-story/SKILL.md`
- Code review: `.github/skills/bmad-code-review/SKILL.md`

### Previous Epic Intelligence (Epic 11)

All 5 stories done. Key facts:
- Provider name is `"openai"` (NOT `"openai-compatible"`)
- rig-core 0.35 from crates.io, `serde_yml` (not `serde_yaml`)
- `BuiltAgent`: 2 variants — `Anthropic` and `OpenAiCompatible`
- `AgentFactory::build()` takes `LlmRole`, `&preamble`, `tools`
- 1131 tests passing, 1 pre-existing failure

### Git Intelligence

Last 5 commits:
- `5496b07` docs(epic-11): complete story 11.5
- `1941956` feat(epic-11): migrate rig-core (Story 11.4)
- `5746a62` feat(epic-11): remove Copilot provider (Story 11.3)
- `43c1a5a` feat(epic-11): add base_url support (Story 11.2)
- `07a3b0f` feat(epic-11): remove Copilot auth module (Story 11.1)

### Anti-Patterns to Avoid

- **DO NOT remove persona instructions from `build_preamble()`** — `architect.rs` uses the same function. ADD skill instruction; REMOVING persona rules breaks the Architect.
- **DO NOT change `src/supervisor/architect.rs`** — persona activation is intentional until Story 12.4.
- **DO NOT change `ContextBuilder`** — XML wrapping is unchanged; only the file path changes.
- **DO NOT modify `ResponseAnalyzer`** — Story 12.2 scope.
- **DO NOT create `skill_config` in `BotConfig`** — skill paths are code constants, not config.
- **DO NOT change `AgentFactory`** — provider construction unchanged.
- **DO NOT add `spawn_agent` tool** — Story 12.3 scope.
- **DO NOT remove the English override** from any post-activation message.
- **DO NOT remove the branch reminder** from the dev session initial message — omit it only from the review session (review sessions do not commit code).

### Project Structure Notes

Files modified (no new files, no deleted files):
- `src/session/agent.rs` — `build_preamble()` text (ADD skill instruction), module/function doc comments
- `src/session/runner.rs` — `skill_path` parameter threading or `SessionRunner` field; 4 activation call sites; 2 post-activation messages
- `src/review/mod.rs` — 1 activation path, 1 post-activation message
- `src/llm/agent_factory.rs` — 1 doc comment example

### References

- [Source: _bmad-output/planning-artifacts/epics.md § Story 12.1 (L2953–2985)]
- [Source: _bmad-output/planning-artifacts/epics.md § Epic 12 Summary (L3118–3137)]
- [Source: _bmad-output/planning-artifacts/architecture.md § Decision 5 + Amendment (L323–396)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md § Epic 12 (L197–215)]
- [Source: _bmad-output/project-context.md § Framework-Specific Rules (L38–109)]
- [Source: src/session/agent.rs § build_preamble() (L199–262) — ADD skill instruction, RETAIN persona rules]
- [Source: src/session/agent.rs § module doc (L1–12) — update to dual-mode]
- [Source: src/session/agent.rs § activate_agent() (L670–731)]
- [Source: src/session/runner.rs § run_session() (L1271)]
- [Source: src/session/runner.rs § drive_activation_and_recover() (L1063)]
- [Source: src/session/runner.rs § context_limit_recovery() (L972)]
- [Source: src/session/runner.rs § SessionRunner::run() (~L650)]
- [Source: src/session/runner.rs § shallow-WAL call (L567)]
- [Source: src/review/mod.rs § activate_agent (L550)]
- [Source: src/review/mod.rs § "Execute [CR]" (L579)]
- [Source: src/supervisor/architect.rs § build_preamble() usage (L357) — DO NOT CHANGE]
- [Source: src/supervisor/architect.rs § ARCHITECT_AGENT_PATH (L30) — DO NOT CHANGE]
- [Source: src/llm/agent_factory.rs § BuiltAgent::activate_agent() doc (L114–122)]

## Dev Agent Record

### Agent Model Used

anthropic/claude-sonnet-4-6

### Debug Log References

No debug issues encountered.

### Completion Notes List

- Implemented Task 2.9 (struct field approach) instead of threading `skill_path` as a function parameter through `run_session()`, `drive_activation_and_recover()`, and `context_limit_recovery()`. Added `skill_path: String` to `SessionRunner` struct, initialized in `SessionRunner::new()`. All 4 activation call sites in `runner.rs` access it as `&self.skill_path`.
- The `ch_msg` in `drive_activation_and_recover()` changed from a `&str` literal to a dynamically-formatted `String` (includes story path). Updated all downstream call sites to pass `&ch_msg` for `&str` parameters and moved ownership at final use.
- Zero new warnings introduced. The 2 pre-existing `cargo clippy` errors in `src/session/branch.rs` (untouched) remain; zero new clippy issues from this story.
- 1133 tests passing (baseline 1131 + 2 new tests: `test_build_preamble_contains_skill_instructions`, `test_build_preamble_retains_persona_rules`). 1 pre-existing test failure (`test_build_context_limit_recovery_message_contains_all_sections`) unchanged.
- `src/supervisor/architect.rs` was NOT changed — Architect persona activation preserved as required.

### Change Log

- 2026-04-17: Story 12.1 implemented — parameterize activation by skill (anthropic/claude-sonnet-4-6)

### File List

- `src/session/agent.rs` — module doc (dual-mode), `build_preamble()` (skill instruction added), `activate_agent()` doc (dual-mode), 3 new tests
- `src/session/runner.rs` — `SessionRunner` struct (`skill_path` field), `SessionRunner::new()` (field init), 4 activation call sites replaced, 2 initial messages updated, `drive_activation_and_recover()` ch_msg updated
- `src/review/mod.rs` — activation path updated to code-review skill, initial message updated (no `[CR]`)
- `src/llm/agent_factory.rs` — `activate_agent()` doc example updated

### Review Findings

- [x] [Review][Defer] Recovery paths omit branch reminder [src/session/runner.rs] — deferred, pre-existing (recovery paths never had branch reminders before this change)
- [x] [Review][Defer] Recovery ch_msg sent before recovery_message context summary [src/session/runner.rs] — deferred, pre-existing (activation → initial msg → recovery summary sequence unchanged)
- [x] [Review][Defer] Architect session filename-based skill detection fragility [src/supervisor/architect.rs] — deferred, Story 12.4 scope
