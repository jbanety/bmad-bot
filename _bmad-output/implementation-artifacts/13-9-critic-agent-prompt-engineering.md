# Story 13.9: Critic Agent — Prompt Engineering & Construction

Status: done

## Story

As a daemon operator,
I want the Story Critic to be an independent vision guardian with its own LLM role and engineered preambles,
so that it provides non-BMAD, vision-anchored critique of stories and decisions using a model optimized for reasoning.

## Acceptance Criteria

1. **Given** the `LlmRole` enum in `src/llm/agent_factory.rs`, **when** this story is implemented, **then** a new `Critic` variant is added with `Display` impl producing `"critic"`, Debug/Clone/Copy/PartialEq/Eq/Hash derived.

2. **Given** the `LlmConfig` struct in `src/config/mod.rs`, **when** this story is implemented, **then** a new `critic: LlmRoleConfig` field is added with `#[serde(default)]` — empty provider falls back to `review` config at runtime (same pattern as `epic_review`).

3. **Given** `AgentFactory::config_for_role()`, **when** called with `LlmRole::Critic`, **then** it returns the `critic` config if the provider is non-empty, otherwise falls back to the `review` config (same fallback pattern as `EpicReview`).

4. **Given** the critic consultation configs in `build_create_story_consultations()` and `build_review_consultations()`, **when** this story is implemented, **then** `role: LlmRole::Review` is replaced with `role: LlmRole::Critic` for all critic consultations.

5. **Given** the critic preamble functions `build_placeholder_critic_preamble()` and `build_review_critic_preamble()`, **when** this story is implemented, **then** both are replaced with engineered preambles that establish the Critic's identity, vision-anchoring role, memory instructions, tool usage rules, and output format (see Dev Notes for full preamble specifications).

6. **Given** the project brief is configured in `bmad-bot.yaml` (or PRD fallback), **when** a Critic consultation is built, **then** the project brief file path is prepended to `context_files` (before critic-memory and story file), or the PRD path from `bmad_paths.planning_artifacts` if no project brief is configured. Resolution logic lives in a `prepare_project_brief_path()` method on `StoryPipeline`.

7. **Given** the `BotConfig` validation in `validate()`, **when** a `critic` LLM role is explicitly configured (non-empty provider), **then** the `critic` role is validated with `validate_llm_role("llm.critic", ...)` — same as `epic_review` conditional validation.

8. **Given** the `BotSecrets::validate_for_config()`, **when** a `critic` LLM role has a non-empty provider, **then** the corresponding API key is validated — same as `epic_review` conditional secret validation.

9. **Given** `bmad-bot.yaml.example`, **when** this story is implemented, **then** a commented `critic` role is added under `llm:` with documentation explaining its purpose and fallback behavior.

10. **Given** the story preamble functions are renamed, **when** this story is implemented, **then** `build_placeholder_critic_preamble()` → `build_story_critic_preamble()` and `build_review_critic_preamble()` keeps its name but gets an engineered rewrite.

## Tasks / Subtasks

- [x] Task 1: Add `LlmRole::Critic` variant (AC: #1, #3)
  - [x] 1.1 Add `Critic` variant to `LlmRole` enum in `src/llm/agent_factory.rs` with doc comment: `/// Story Critic — independent vision guardian for story and decision review.`
  - [x] 1.2 Add `Self::Critic => write!(f, "critic")` arm to `Display` impl
  - [x] 1.3 Add `LlmRole::Critic` arm to `config_for_role()` with empty-provider fallback to `review` config (same pattern as `EpicReview`)

- [x] Task 2: Add `critic` field to `LlmConfig` (AC: #2)
  - [x] 2.1 Add `#[serde(default)] pub critic: LlmRoleConfig` to `LlmConfig` struct in `src/config/mod.rs` with doc comment: `/// Provider + model for the Story Critic (independent vision guardian). Defaults to empty — at runtime, falls back to the review config.`
  - [x] 2.2 Add `critic: LlmRoleConfig::default()` to the `LlmConfig` block in every test config builder (actual function names differ per module):
    - `src/llm/agent_factory.rs` → `make_test_config()` (line ~618)
    - `src/review/epic.rs` → `make_test_config()` (line ~1073)
    - `src/watcher/mod.rs` → `make_test_bot_config()` (line ~932)
    - `src/session/runner.rs` → `make_runner_test_config()` (line ~2538)
    - `src/pipeline.rs` → inside `make_test_pipeline()` (line ~4464)

- [x] Task 3: Update config validation and secrets validation (AC: #7, #8)
  - [x] 3.1 In `BotConfig::validate()` after the `epic_review` validation block: add identical conditional validation for `critic` — `if !self.llm.critic.provider.is_empty() { self.validate_llm_role("llm.critic", &self.llm.critic)?; }`
  - [x] 3.2 In `BotSecrets::validate_for_config()`: add `critic` to the `llm_roles` vec conditionally — `if !config.llm.critic.provider.is_empty() { llm_roles.push(("critic", &config.llm.critic)); }`

- [x] Task 4: Add project brief context injection (AC: #6)
  - [x] 4.1 Add `prepare_project_brief_path(&self) -> Option<String>` method to `StoryPipeline`:
    - If `config.project_brief` is `Some(path)`: apply same security checks as `check_project_brief()` (reject absolute paths, reject `..` components), resolve `Path::new(&project_root).join(path)`, return `Some` if file exists, else log `tracing::warn!` and fall through to PRD fallback
    - PRD fallback: `read_dir(planning_artifacts)`, filter entries to filenames that contain `"prd"` (case-insensitive) AND end with `.md` AND are regular files (not directories), sort matching entries alphabetically, return first match as `Some(absolute_path_string)`, log `tracing::info!("No project brief configured, using PRD as Critic vision anchor")`
    - If neither exists: return `None`, log `tracing::warn!("No project brief or PRD found — Critic will operate without vision anchor")`
  - [x] 4.2 In `run_create_pipeline()`: call `prepare_project_brief_path()` and pass result to `build_create_story_consultations()`
  - [x] 4.3 In `run_review_pipeline()`: call `prepare_project_brief_path()` and pass result to `build_review_consultations()`
  - [x] 4.4 Update `build_create_story_consultations()` signature to accept `project_brief_path: Option<String>` — prepend to critic consultation's `context_files` (before memory and story)
  - [x] 4.5 Update `build_review_consultations()` signature to accept `project_brief_path: Option<String>` — prepend to review-critic consultation's `context_files`

- [x] Task 5: Rewrite preambles and update consultation configs (AC: #4, #5, #10)
  - [x] 5.1 Rename `build_placeholder_critic_preamble()` → `build_story_critic_preamble()` and rewrite with engineered content (see Dev Notes — Story Critic Preamble Specification)
  - [x] 5.2 Rewrite `build_review_critic_preamble()` with engineered content (see Dev Notes — Review Critic Preamble Specification)
  - [x] 5.3 In `build_create_story_consultations()`: update call to use `build_story_critic_preamble()` AND change `role: LlmRole::Review` → `role: LlmRole::Critic` — apply both changes to the critic `ConsultationConfig` in one pass
  - [x] 5.4 In `build_review_consultations()`: change `role: LlmRole::Review` → `role: LlmRole::Critic`
  - [x] 5.5 Update existing preamble tests in `src/pipeline.rs`: rename `test_critic_preamble_contains_memory_instructions` (line ~4973) to `test_story_critic_preamble_contains_identity_and_memory` and update assertions to match new preamble content; update `test_review_critic_preamble_contains_memory_instructions` (line ~4982) assertions to match new preamble content

- [x] Task 6: Update `bmad-bot.yaml.example` (AC: #9)
  - [x] 6.1 Add commented `critic` section under `llm:` after `supervisor:` block and before the `notifications:` section. Exact content:
    ```yaml
    # Story Critic — independent vision guardian (optional).
    # When configured, the Critic uses this provider/model for story reviews
    # and code review decision resolution. Defaults to the review config when absent.
    # Recommended: use a model with strong reasoning capabilities.
    # critic:
    #   provider: anthropic
    #   model: claude-sonnet-4-20250514
    #   # base_url: "https://custom-endpoint.example.com"
    #   # reasoning_effort: high  # for OpenAI models
    ```

- [x] Task 7: Tests (AC: #1-#10)
  - [x] 7.1 `test_llm_role_critic_display` — `LlmRole::Critic.to_string() == "critic"`
  - [x] 7.2 `test_llm_role_critic_debug` — `format!("{:?}", LlmRole::Critic)` contains "Critic"
  - [x] 7.3 `test_llm_role_critic_equality` — `LlmRole::Critic == LlmRole::Critic` and `LlmRole::Critic != LlmRole::Review`
  - [x] 7.4 `test_config_for_role_critic_fallback_to_review` — empty critic provider returns review config
  - [x] 7.5 `test_config_for_role_critic_explicit` — non-empty critic provider returns critic config
  - [x] 7.6 `test_config_critic_serde_default` — YAML without `critic` field deserializes to `LlmRoleConfig::default()`
  - [x] 7.7 `test_config_critic_serde_explicit` — YAML with explicit `critic` section deserializes correctly
  - [x] 7.8 `test_config_validate_critic_valid` — explicit critic config with valid provider passes validation
  - [x] 7.9 `test_config_validate_critic_invalid_provider` — invalid provider in critic config fails validation
  - [x] 7.10 `test_config_validate_critic_empty_skipped` — empty critic provider skips validation (not required)
  - [x] 7.11 `test_secrets_validate_critic_provider_key_required` — non-empty critic provider requires matching API key
  - [x] 7.12 `test_secrets_validate_critic_empty_skipped` — empty critic provider skips secret validation
  - [x] 7.13 `test_build_create_story_consultations_uses_critic_role` — critic consultation uses `LlmRole::Critic`
  - [x] 7.14 `test_build_review_consultations_uses_critic_role` — review-critic consultation uses `LlmRole::Critic`
  - [x] 7.15 `test_build_create_story_consultations_includes_project_brief` — project brief path appears as first element of critic consultation's `context_files`
  - [x] 7.16 `test_build_review_consultations_includes_project_brief` — project brief path appears as first element of review-critic consultation's `context_files`
  - [x] 7.17 `test_prepare_project_brief_path_configured` — returns resolved path when configured and file exists
  - [x] 7.18 `test_prepare_project_brief_path_fallback_prd` — returns PRD path when no project brief configured; verify it matches a file named `prd.md` (not `deprecated-prd.md`)
  - [x] 7.19 `test_prepare_project_brief_path_none` — returns `None` when neither exists
  - [x] 7.20 `test_prepare_project_brief_path_rejects_absolute` — absolute path in `project_brief` config is rejected (returns `None`, falls through to PRD)
  - [x] 7.21 `test_prepare_project_brief_path_rejects_traversal` — path containing `..` is rejected (returns `None`, falls through to PRD)
  - [x] 7.22 `test_prepare_project_brief_path_prd_filters_correctly` — directory containing `prd.md`, `not-a-prd-file.txt`, `deprecated-prd-old.md` returns only the `.md` file containing `prd`
  - [x] 7.23 `test_story_critic_preamble_contains_identity` — preamble contains "independent" and "vision guardian" and "NOT part of the BMAD"
  - [x] 7.24 `test_story_critic_preamble_contains_memory_instructions` — preamble contains "critic-memory.md" and "edit_file" and "overwrite"
  - [x] 7.25 `test_review_critic_preamble_contains_decision_vocabulary` — preamble contains "patch", "defer", "dismiss"
  - [x] 7.26 `test_review_critic_preamble_contains_memory_instructions` — preamble contains "critic-memory.md" and "edit_file"
  - [x] 7.27 Verify `cargo clippy` passes with zero new warnings (existing allowances: `-A clippy::needless_splitn -A clippy::unnecessary_map_or`)
  - [x] 7.28 Verify `cargo test` passes — baseline: 1208 passed, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)

## Dev Notes

### Architecture Compliance

- **Decision 11 (Story Critic):** This story implements the agent construction and prompt engineering. The Critic is an independent vision guardian, NOT part of BMAD. Built via `AgentFactory::build(LlmRole::Critic, ...)`. Fresh agent per invocation — continuity via `critic-memory.md`. [Source: architecture.md#Decision-11]
- **Decision 10 (Consultations):** Integration is through existing `ConsultationConfig` — only `role` and `preamble_override` fields change. No modifications to `ConsultationRunner` or `ConsultationToolSet`. [Source: architecture.md#Decision-10]
- **Decision 8 (LLM Provider Abstraction):** Architecture amendment specifies `LlmRole` should include `Critic`. The `BuiltAgent` enum dispatch and `AgentFactory` are unchanged — only `config_for_role()` gains a new match arm. [Source: architecture.md#Decision-8-Amendment]

### Story Critic Preamble Specification

The `build_story_critic_preamble()` function must produce a preamble that covers these sections:

**Identity & Role:**
```
You are the Story Critic — an independent product and technical vision guardian.

You are NOT part of the BMAD methodology. You are an external advisor brought in to ensure that what is being built aligns with the original project vision.

Your founding document is the project brief (or PRD) loaded in your context. This is your north star — every observation should trace back to whether the story serves the project's stated goals.
```

**Review Mandate (for story review):**
```
## Your Review Mandate

Evaluate the story against these dimensions:
1. **Vision alignment** — Does the story serve the project's stated goals? Does it solve a real user problem described in the brief?
2. **Scope integrity** — Is the story appropriately scoped? Does it include unnecessary work or miss critical requirements?
3. **Architectural coherence** — Do the technical decisions align with the project's architecture and constraints?
4. **Cross-story consistency** — Based on your memory of previous stories, are there contradictions, duplications, or gaps?
5. **Risk identification** — Are there unstated assumptions, missing error handling, or security concerns?

Do NOT review implementation details (code style, variable names, etc.) — that's the adversarial reviewer's job. Focus on whether the RIGHT thing is being built.
```

**Memory Instructions (shared pattern for both preambles):**
```
## Persistent Memory

You have a persistent memory file (critic-memory.md) loaded in your context. It contains your observations from all previous story reviews. Read it carefully — it is your institutional knowledge.

If this is your first invocation, the memory file will contain only a header. This is normal.

After completing your review, update your memory file:
1. Use read_file to read the current content of critic-memory.md
2. Use edit_file in overwrite mode to write the COMPLETE content: all existing content preserved, plus your new observation section appended at the end

Your new section must include:
- Date and story key (e.g., "## Story 4-2 — 2026-04-24")
- Review type: "Story Review"
- Key observations with rationale
- Cross-story patterns you notice (contradictions, emerging themes, recurring concerns)
- Any concerns that should carry forward to future reviews

CRITICAL: Use overwrite mode for edit_file, NOT create mode. The file already exists.
```

**Tool Rules:**
```
## Tools Available

You have access to: read_file, edit_file, grep, find_path, list_directory, think.

- Use read_file to examine files referenced in the story
- Use grep/find_path to verify claims about existing code when needed
- Use think for complex reasoning before forming observations

## CRITICAL: edit_file Restriction

You may ONLY use edit_file on ONE file: critic-memory.md. This is your personal memory file.
NEVER call edit_file on any other file — not source code, not story files, not configuration files.
Any edit_file call targeting a file other than critic-memory.md is a violation of your operating constraints.
When editing critic-memory.md, ALWAYS use overwrite mode (never create mode — the file already exists).

You do NOT have: git, terminal, ask_supervisor, spawn_agent. You are read-only on the codebase except for your own memory file.
```

**Output Format:**
```
## Output Format

Structure your response as:

### Vision Alignment Assessment
[Overall assessment: aligned / partially aligned / misaligned]
[Brief rationale]

### Observations
For each observation:
- **[Category]** Brief title
  - What: description of the concern
  - Why it matters: impact on project vision
  - Suggested correction: specific, actionable change

### Cross-Story Patterns
[Any patterns from your memory that are relevant]

### Summary
[1-2 sentence summary of findings]

Signal completion with <<BMAD_JOB_DONE>> when finished.
```

**Communication:**
```
## Communication
- Respond in English
- Be constructive but direct — flag real concerns, not theoretical ones
- Every observation must reference either the project brief or your accumulated memory
- Quality over quantity — 3 incisive observations are better than 10 shallow ones
```

### Review Critic Preamble Specification

The `build_review_critic_preamble()` function must produce a preamble covering:

**Identity & Role:**
```
You are the Code Review Critic — an independent decision authority for ambiguous code review findings.

You are NOT part of the BMAD methodology. You are an external judge brought in to resolve findings that the code reviewer couldn't classify with confidence.

Your founding document is the project brief (or PRD) loaded in your context. Use it to judge whether findings align with the project's goals and constraints.
```

**Decision Framework:**
```
## Decision Framework

For each [Review][Decision] finding, decide:
- **patch**: The issue is real, the fix is clear and unambiguous. Apply it.
- **defer**: The issue is real but not actionable now — it requires design discussion, impacts other stories, or is out of scope. Leave as a documented action item.
- **dismiss**: The finding is noise, a false positive, stylistic preference, or not a real issue. Remove it.

When deciding, consider:
1. Does this finding relate to the project's core goals (from the brief)?
2. Have you seen this pattern in previous stories (check your memory)?
3. Is the suggested fix safe — could it introduce regressions?
4. Is this finding blocking vs. advisory?
```

**Memory Instructions:**
```
## Persistent Memory

You have a persistent memory file (critic-memory.md) loaded in your context. It contains your observations and decisions from all previous reviews. Read it carefully — it is your institutional knowledge.

If this is your first invocation, the memory file will contain only a header. This is normal.

After completing your review, update your memory file:
1. Use read_file to read the current content of critic-memory.md
2. Use edit_file in overwrite mode to write the COMPLETE content: all existing content preserved, plus your new observation section appended at the end

Your new section must include:
- Date and story key (e.g., "## Story 4-2 — 2026-04-24")
- Review type: "Decision Resolution"
- Decisions made with rationale for each
- References to prior decisions when relevant (e.g., "Consistent with Story 3-1 where we deferred a similar concern")
- Any patterns emerging across reviews

CRITICAL: Use overwrite mode for edit_file, NOT create mode. The file already exists.
```

**Tool Rules:**
```
## Tools Available

You have access to: read_file, edit_file, grep, find_path, list_directory, think.

- Use read_file to examine the story file and findings in detail
- Use grep/find_path to verify claims about existing code when needed
- Use think for complex reasoning before forming decisions

## CRITICAL: edit_file Restriction

You may ONLY use edit_file on ONE file: critic-memory.md. This is your personal memory file.
NEVER call edit_file on any other file — not source code, not story files, not configuration files.
Any edit_file call targeting a file other than critic-memory.md is a violation of your operating constraints.
When editing critic-memory.md, ALWAYS use overwrite mode (never create mode — the file already exists).

You do NOT have: git, terminal, ask_supervisor, spawn_agent. You are read-only on the codebase except for your own memory file.
```

**Output Format:**
```
## Output Format

For each finding:
- **Finding:** [Copy the finding text verbatim]
- **Decision:** patch | defer | dismiss
- **Rationale:** [Why this decision, referencing brief or memory when applicable]

After all decisions, provide a brief summary of patterns observed.

Signal completion with <<BMAD_JOB_DONE>> when finished.
```

**Communication:**
```
## Communication
- Respond in English
- Be decisive — every finding must get a clear verdict
- Reference the project brief or your memory to justify decisions
- When in doubt between defer and dismiss, prefer defer — real issues shouldn't be silenced
```

### Project Brief Context Injection

The `prepare_project_brief_path()` method resolves which vision document to load:

1. If `config.project_brief` is `Some(path)`:
   - **Security checks first** (same as `check_project_brief()`): reject if path is absolute, reject if path contains `..` — log `tracing::warn!` and fall through to PRD fallback
   - Resolve absolute path: `Path::new(&config.bmad_paths.project_root).join(path)`
   - If file exists → return `Some(absolute_path_string)`
   - If file does NOT exist → log `tracing::warn!`, fall through to PRD fallback
2. PRD fallback:
   - `std::fs::read_dir(planning_artifacts)` — iterate entries
   - Filter: `entry.file_type().is_file()` AND filename (lowercased) contains `"prd"` AND filename ends with `.md`
   - Sort matching filenames alphabetically, take the first match
   - Return `Some(absolute_path_string)` of first match
   - Log `tracing::info!("No project brief configured, using PRD as Critic vision anchor")`
3. If neither exists → return `None`, log `tracing::warn!("No project brief or PRD found — Critic will operate without vision anchor")`

**Context file ordering in critic consultation `context_files` vec:**
1. Project brief / PRD (vision anchor — read first)
2. Critic memory (institutional knowledge — read second)
3. Story file / findings (artifact under review — read last)

**Adversarial consultation is NOT modified** — it keeps its existing `context_files: vec![story_file_path]` with no project brief. The adversarial reviewer analyzes content quality (completeness, missing details), not vision alignment. Adding the project brief would dilute its focus and waste context tokens.

### Critical Implementation Details

- **`LlmRole::Critic` fallback is to `review`, NOT `supervisor`.** The Critic benefits from the same provider class as the review agent (reasoning-heavy), not the supervisor (fast pattern matching).
- **`#[serde(default)]` on `critic` field** ensures backward compatibility — existing `bmad-bot.yaml` files without a `critic` section deserialize cleanly with `LlmRoleConfig::default()`.
- **Preambles are standalone functions** (not methods on `StoryPipeline`) — follows existing pattern for `build_adversarial_consultation_preamble()`.
- **No changes to `ConsultationRunner`, `ConsultationConfig`, or `ConsultationToolSet`** — all integration goes through the pipeline's consultation config builders. The `ConsultationToolSet::Restricted` already provides exactly the right tool set for the Critic.
- **No changes to `src/critic/mod.rs`** — the `CriticMemory` module is complete from Story 13.8.
- **The project brief validation in `BotConfig::check_project_brief()` already exists** from Story 13.7 — it logs warnings for invalid paths at startup. The new `prepare_project_brief_path()` does runtime resolution per-invocation with the same validation logic (exists check, relative path).
- **`prepare_project_brief_path()` must NOT crash** if the file disappears between startup validation and runtime — the Critic operates in degraded mode without vision anchor.
- **Large vision document warning:** The project brief (or PRD fallback) is loaded in full by `ContextBuilder::add_file_from_disk()`. A multi-hundred-KB PRD will consume significant context window. This is acceptable — the Critic needs the full vision document for accurate alignment review. The `critic-memory.md` already has a size warning at 50KB (Story 13.8); adding a similar warning for the vision document is out of scope for this story but could be added later if context window issues arise in practice.
- **TOCTOU for project brief:** `prepare_project_brief_path()` checks existence before returning, but the file could be deleted between the check and `ConsultationRunner::build_context_xml()` reading it. Since `build_context_xml` returns `Err(ContextFileNotFound)` for missing files, and this would crash the consultation, the project brief path must NOT be added to `context_files` directly. Instead, re-verify existence in the consultation config builder right before inserting — or accept the tiny race window since file deletion during a running pipeline is exceptional. Document this as a known limitation consistent with the pre-existing TOCTOU pattern for `critic-memory.md` (which uses `ensure_exists()` to minimize the window).
- **Preamble content is the critical deliverable** of this story. The code changes (LlmRole, config field, role swap) are mechanical. The preamble text determines the Critic's effectiveness. Take time to engineer it — this is prompt engineering, not boilerplate.

### Deferred Work Context (from prior reviews)

- **Config nesting (from 13.7 review):** "Consider grouping under a `[critic]` config section when the next critic field is added." This story adds `critic` as a nested `LlmRoleConfig` under `llm:` — that's the natural home for it (alongside `dev`, `review`, `supervisor`, `epic_review`). The `project_brief` and `critic_memory_threshold_kb` remain top-level since they're not LLM-role-specific. No config nesting refactor needed.
- **Critic preamble vs. edit_file (from 13.6 review):** "Critic preamble prohibits edit_file but ConsultationToolSet::Restricted still includes it. Story 13.9 may introduce a ReadOnly tool set variant for stricter enforcement." Decision for this story: keep `Restricted` tool set — the Critic NEEDS `edit_file` for updating critic-memory.md. The preamble must clearly instruct the Critic to ONLY use edit_file on critic-memory.md. A `ReadOnly` variant would break memory updates.

### Source Tree Components to Touch

| File | Change |
|------|--------|
| `src/llm/agent_factory.rs` | Add `LlmRole::Critic` variant, `Display`/`config_for_role()` arms, update `make_test_config()` |
| `src/config/mod.rs` | Add `critic: LlmRoleConfig` to `LlmConfig`, conditional validation + secret validation |
| `src/pipeline.rs` | Rewrite preamble functions, add `prepare_project_brief_path()`, update consultation builders with `LlmRole::Critic` + project brief context, update `make_test_pipeline()`, update existing preamble tests |
| `bmad-bot.yaml.example` | Add commented `critic` LLM config section |
| `src/watcher/mod.rs` | Update `make_test_bot_config()` with `critic: LlmRoleConfig::default()` |
| `src/review/epic.rs` | Update `make_test_config()` with `critic: LlmRoleConfig::default()` |
| `src/session/runner.rs` | Update `make_runner_test_config()` with `critic: LlmRoleConfig::default()` |

### Anti-Patterns to Avoid

- DO NOT modify `ConsultationRunner`, `ConsultationConfig`, or `ConsultationToolSet` — infrastructure is complete
- DO NOT modify `src/critic/mod.rs` — CriticMemory is complete from Story 13.8
- DO NOT add a `ReadOnly` tool set variant — the Critic needs `edit_file` for memory updates
- DO NOT hardcode the project brief path — always resolve from config + filesystem at runtime
- DO NOT use `println!` — daemon code uses `tracing` only
- DO NOT add I/O side effects inside config builder methods — keep them pure, do I/O in phase runners or via `prepare_*` methods
- DO NOT change the consultation `trigger_pattern`, `prompt_template`, or `resume_message_template` — those are correct from Stories 13.4/13.6
- DO NOT add `use glob` or pull in a glob crate for PRD fallback — simple `read_dir` + `contains("prd")` matching on filename is sufficient
- DO NOT make `prepare_project_brief_path()` return an error — it's a best-effort resolution, always returns `Option<String>`

### Previous Story Intelligence (Story 13.8)

- **Test baseline:** 1208 passed, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)
- **Clippy allowances:** `-A clippy::needless_splitn -A clippy::unnecessary_map_or`
- **Config pattern established:** `critic_memory_threshold_kb: Option<u64>` with `#[serde(default, skip_serializing_if)]` for optional fields. For `LlmRoleConfig`, use `#[serde(default)]` without `skip_serializing_if` (same as `epic_review`).
- **Test config helpers:** 5 functions construct `LlmConfig` in tests — need `critic: LlmRoleConfig::default()` added:
  - `src/llm/agent_factory.rs` → `make_test_config()` (line ~618)
  - `src/review/epic.rs` → `make_test_config()` (line ~1073)
  - `src/watcher/mod.rs` → `make_test_bot_config()` (line ~932)
  - `src/session/runner.rs` → `make_runner_test_config()` (line ~2538)
  - `src/pipeline.rs` → inside `make_test_pipeline()` (line ~4464)
  - Note: `src/config/mod.rs` has NO `make_test_config()` function
- **Preamble test pattern:** Story 13.8 added preamble assertion tests in `src/pipeline.rs` — `test_critic_preamble_contains_memory_instructions` (line ~4973) and `test_review_critic_preamble_contains_memory_instructions` (line ~4982). These tests call `build_placeholder_critic_preamble()` and `build_review_critic_preamble()` — they must be updated since the function is renamed and preamble content changes.
- **Critic consultation context_files ordering:** Memory file first, story file second (established in 13.8). This story changes to: project brief first, memory second, story third.
- **`prepare_context_path()` pattern:** `CriticMemory::prepare_context_path()` returns `Option<String>` — same pattern for `prepare_project_brief_path()`.

### Git Intelligence

Recent commits follow pattern: `feat(epic-13): description (Story 13.X)`
- `48f613e` Story 13.8: critic memory system — CriticMemory struct, pipeline integration, preamble memory sections
- `cedf83b` Story 13.7: project_brief config field — BotConfig extension, init prompt, startup validation
- `21761e0` Story 13.6: code-review phase via SessionRunner with critic consultation
- `b68fc0d` Story 13.5: separate dev/review phases in pipeline
- `5f4a497` Story 13.4: create-story phase with adversarial + critic consultations

### Testing Standards

- Framework: `#[tokio::test]` for async, `#[test]` for sync
- Naming: `test_{module}_{behavior}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert
- Use `tempdir` / `tempfile` for filesystem tests
- `NullRenderer` via `UiHandle::null()` for UI in tests
- Preamble assertion tests: use `.contains()` on key phrases — not exact string matching (preambles will evolve)

### Project Structure Notes

- No new files created — all changes are modifications to existing files
- `LlmRole::Critic` lives alongside `Dev`, `Review`, `Supervisor`, `EpicReview` in `src/llm/agent_factory.rs`
- `LlmConfig::critic` lives alongside `dev`, `review`, `supervisor`, `epic_review` in `src/config/mod.rs`
- Preamble functions remain as standalone `fn` in `src/pipeline.rs` (not methods)
- `prepare_project_brief_path()` is a method on `StoryPipeline` (needs access to `self.config`)

### References

- [Source: architecture.md#Decision-11] Story Critic design: independent vision guardian, LlmRole::Critic, extended thinking, restricted tools
- [Source: architecture.md#Decision-10] Daemon-orchestrated consultations: ConsultationConfig, context_files mechanism
- [Source: architecture.md#Decision-8-Amendment] Updated LlmRole enum includes Critic variant
- [Source: pipeline.rs:1414-1497] Current consultation config builders — integration point
- [Source: pipeline.rs:2971-3041] Current preamble functions — replacement targets
- [Source: pipeline.rs:385-411] run_create_pipeline — add project brief path resolution
- [Source: pipeline.rs:1112-1226] run_review_pipeline — add project brief path resolution
- [Source: config/mod.rs:174-213] LlmConfig + LlmRoleConfig structs — add critic field
- [Source: config/mod.rs:392-400] LLM role validation — add critic conditional validation
- [Source: config/mod.rs:639-649] Secret validation — add critic conditional secret check
- [Source: config/mod.rs:496-525] check_project_brief() — reference for path resolution pattern
- [Source: agent_factory.rs:37-57] LlmRole enum — add Critic variant
- [Source: agent_factory.rs:210-222] config_for_role() — add Critic arm with fallback
- [Source: agent_factory.rs:618-675] make_test_config() — add critic field
- [Source: epics.md#Story-13.9] Story requirements and acceptance criteria
- [Source: epics.md#Epic-13-Summary] Epic execution strategy — this story is the prompt engineering challenge

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (claude-opus-4-6)

### Debug Log References

### Completion Notes List

- Added `LlmRole::Critic` variant with Display, Debug, Clone, Copy, PartialEq, Eq, Hash derives and `config_for_role()` fallback to review config
- Added `critic: LlmRoleConfig` field with `#[serde(default)]` to `LlmConfig` — backward compatible deserialization
- Updated 7 test config builders across all modules (agent_factory, config, review/epic, watcher, session/runner, pipeline, cli)
- Added conditional validation for critic role in `BotConfig::validate()` and `BotSecrets::validate_for_config()`
- Added `LlmRole::Critic` match arm to `SessionRunner::run_session()` with same fallback pattern
- Implemented `prepare_project_brief_path()` on `StoryPipeline` — resolves project brief with security checks (reject absolute/traversal paths), falls back to PRD scan in planning_artifacts
- Updated `build_create_story_consultations()` and `build_review_consultations()` to accept `project_brief_path` parameter — prepended as first element of critic context_files (before memory and story)
- Rewrote `build_story_critic_preamble()` (formerly `build_placeholder_critic_preamble()`) with full engineered content: identity, review mandate (5 dimensions), persistent memory instructions, tool rules with edit_file restriction, structured output format, communication guidelines
- Rewrote `build_review_critic_preamble()` with full engineered content: identity, decision framework (patch/defer/dismiss), persistent memory with decision tracking, tool rules, structured output format
- Changed critic consultations from `LlmRole::Review` to `LlmRole::Critic` in both create-story and code-review phases
- Added commented `critic` section to `bmad-bot.yaml.example` with documentation
- 29 new tests added (1237 total passed, 1 pre-existing failure unchanged)

### File List

- `src/llm/agent_factory.rs` — Added `LlmRole::Critic` variant, Display arm, `config_for_role()` arm, 5 new tests, updated existing test assertions, updated `make_test_config()`
- `src/config/mod.rs` — Added `critic: LlmRoleConfig` to `LlmConfig`, conditional validation + secrets validation, updated `_test_minimal()`, 7 new tests
- `src/pipeline.rs` — Added `prepare_project_brief_path()`, rewrote preamble functions, updated consultation builders with project brief + Critic role, updated `make_pipeline_test_config()`, 20 new/updated tests
- `src/session/runner.rs` — Added `LlmRole::Critic` match arm in `run_session()`, updated `make_runner_test_config()`
- `src/review/epic.rs` — Updated `make_test_config()` with critic field
- `src/watcher/mod.rs` — Updated `make_test_bot_config()` with critic field
- `src/cli/mod.rs` — Updated 3 `LlmConfig` initializers with critic field
- `bmad-bot.yaml.example` — Added commented critic LLM config section

### Change Log

- 2026-04-24: Implemented Story 13.9 — Critic Agent prompt engineering and construction. Added LlmRole::Critic with config/validation, engineered story and review critic preambles, project brief context injection with security checks and PRD fallback, updated all consultation configs to use Critic role. 29 new tests (1237 total).

### Review Findings

- [x] [Review][Patch] **PRD fallback alphabetical sort may select wrong file** — Fixed: sort by filename length then alphabetically, so `prd.md` wins over `deprecated-prd-old.md`. [src/pipeline.rs:1462]
- [x] [Review][Patch] **Preamble claims "founding document" when project brief may be absent** — Fixed: preamble text now conditional on `has_vision_document` parameter. [src/pipeline.rs:3062,3152]
- [x] [Review][Patch] **Path rejection tests pass for wrong reason** — Fixed: added `prd.md` fixture to rejection tests and assert PRD fallback is returned (proving rejection occurred). [src/pipeline.rs:tests ~5455,~5515]
- [x] [Review][Patch] **Stale "placeholder preamble" doc comment** — Fixed: updated to "engineered vision guardian preamble". [src/pipeline.rs:1477]
- [x] [Review][Defer] **Symlink escape bypasses path traversal guards** [src/pipeline.rs:1435] — deferred, pre-existing pattern from `check_project_brief()` (Story 13.7). Fix requires canonicalize + containment check across both functions.
- [x] [Review][Defer] **`contains("..")` rejects legitimate filenames containing ".." substring** [src/pipeline.rs:1428, src/config/mod.rs:521] — deferred, pre-existing pattern. Fix: split on path separators before checking for ".." component.
- [x] [Review][Defer] **Test boilerplate duplication in prepare_project_brief_path tests** [src/pipeline.rs:tests ~5272-5530] — deferred, ~240 lines of identical StoryPipeline construction. Should use a shared helper.
- [x] [Review][Defer] **No integration test for `AgentFactory::build()` with `LlmRole::Critic`** [src/llm/agent_factory.rs] — deferred, pre-existing test pattern. No role has a build() integration test in unit tests (requires API credentials).
- [x] [Review][Defer] **Empty model string with valid critic provider passes validation** [src/config/mod.rs:407] — deferred, pre-existing gap in `validate_llm_role()` affecting all roles.
