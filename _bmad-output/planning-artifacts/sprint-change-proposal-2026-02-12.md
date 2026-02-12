# Sprint Change Proposal — LLM Provider Abstraction Layer (AgentFactory + BuiltAgent)

| Field              | Value                                                        |
| ------------------ | ------------------------------------------------------------ |
| **Date**           | 2026-02-12                                                   |
| **Author**         | JB (Product Manager facilitation)                            |
| **Scope**          | Minor — Direct implementation by dev team                    |
| **Effort**         | Medium                                                       |
| **Risk**           | Low                                                          |
| **Status**         | ✅ Approved                                                  |

---

## 1. Issue Summary

### Problem Statement

The BMAD Bot daemon supports three LLM providers — Anthropic, OpenAI, and GitHub Copilot — across three roles (dev session, code review, supervisor). Because rig-core's `Chat` trait is not object-safe (associated types, `Self: Sized`), the codebase uses match arms on the provider name to construct concrete agent types. This pattern is duplicated across **5 call sites** totaling **~610 lines** of near-identical provider-specific code.

A **production incident on 2026-02-12** exposed a critical flaw: the GitHub Copilot branch unconditionally uses the Chat Completions API (`/chat/completions`), but newer OpenAI models like `gpt-5.2-codex` — routed through the Copilot proxy — **only support the Responses API** (`/responses`). The result is a hard 400 error that blocks all dev sessions using these models.

This is not a one-off bug. OpenAI is progressively migrating models to the Responses API, so more models will require it over time.

### Discovery Context

Production incident during an active dev session. The daemon attempted to launch a rig agent session using `gpt-5.2-codex` via the GitHub Copilot proxy. The Copilot branch in `session/runner.rs` unconditionally called `.completions_api()`, causing the OpenAI backend to reject the request.

### Evidence

- **Error response:** `"model gpt-5.2-codex is not accessible via the /chat/completions endpoint — code: unsupported_api_for_model"`
- **Duplication map:** Provider match arms duplicated in `session/runner.rs` (run + resume + 3 build methods: ~390 lines), `review/mod.rs` (~120 lines), `supervisor/architect.rs` (~100 lines)
- **OpenAI migration trend:** Newer models (gpt-5.2-codex, future releases) are Responses API only
- **Architect brief:** `architect-brief-llm-provider-abstraction.md` — full technical analysis and proposed solution approved by Architect

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact | Detail |
| ---- | ------ | ------ |
| Epic 4: Autonomous Development Session | ⚠️ Moderate | Code refactoring in `session/runner.rs` — remove 3 `build_*_agent()` methods and all provider match arms. Add new story 4.5. |
| Epic 3: Intelligent Supervision | ⚠️ Moderate | Code refactoring in `supervisor/architect.rs` — replace provider match with `AgentFactory::build()`. |
| Epic 5: Code Review & PR Delivery | ⚠️ Moderate | Code refactoring in `review/mod.rs` — replace provider match with `AgentFactory::build()`. |
| Epic 7: Integration Tests | ℹ️ Low | Not started yet. Story 7.4 (pipeline tests) specs should reference `AgentFactory` when created. No immediate action. |
| Epic 1, 2, 6, 8 | ❌ None | No interface or behavioral changes affect these epics. |

### Story Impact

| Story | Status | Change Required |
| ----- | ------ | --------------- |
| 3.2 (LLM Fallback with Project Context) | done | Code refactored — `supervisor/architect.rs` uses `AgentFactory::build(LlmRole::Supervisor, ..)` instead of provider match |
| 4.1 (Rig Tools Implementation) | done | No change — tools are provider-agnostic |
| 4.2 (Agent Session Setup & Chat Loop) | done | Code refactored — `session/runner.rs` uses `AgentFactory::build(LlmRole::Dev, ..)` instead of 3 `build_*_agent()` methods |
| 4.3 (Pre-Development & Branch Management) | done | No change — branch management is provider-agnostic |
| 4.4 (Git CLI Migration) | done | No change — git operations unaffected |
| **4.5 (NEW — LLM Provider Abstraction)** | **ready-for-dev** | **New story — extract AgentFactory + BuiltAgent, fix Copilot Responses API bug** |
| 5.2 (Automated Code Review Session) | done | Code refactored — `review/mod.rs` uses `AgentFactory::build(LlmRole::Review, ..)` |
| 5.4 (Enriched PR Description) | review | No change — PR description logic is provider-agnostic |
| 7.x (Integration Tests) | blocked/not started | Specs adjusted when stories are created — no immediate action |

### Artifact Conflicts

| Artifact | Conflict | Detail |
| -------- | -------- | ------ |
| **PRD** (`prd.md`) | ⚠️ Update needed | FR39 states Copilot uses "the Completions API" — now conditional per model (Responses API for OpenAI models, Completions API fallback for others) |
| **Architecture** (`architecture.md`) | ✅ Already updated | Decision 8 added, module structure updated, data flow updated, external integration points updated, module communication map updated |
| **Project Context** (`project-context.md`) | ✅ Already updated | AgentFactory section added, Multi-Provider LLM Config rewritten, module structure updated, last-updated date reflects change |
| **Epics** (`epics.md`) | ⚠️ Update needed | Add Story 4.5 to Epic 4. Update FR Coverage Map. |
| **Sprint Status** (`sprint-status.yaml`) | ⚠️ Update needed | Add story `4-5-llm-provider-abstraction-agent-factory` with status `ready-for-dev` |
| **Code** (`src/`) | ⚠️ Update needed | Create `src/llm/agent_factory.rs`, refactor `session/runner.rs`, `review/mod.rs`, `supervisor/architect.rs`, update `pipeline.rs` |
| UI/UX | N/A | CLI daemon — no UI |

### Technical Impact

- **New file:** `src/llm/agent_factory.rs` — `BuiltAgent` enum, `AgentFactory` struct, `copilot_requires_responses_api()` heuristic
- **Refactored files:** `session/runner.rs` (remove ~390 lines of provider match arms), `review/mod.rs` (remove ~120 lines), `supervisor/architect.rs` (remove ~100 lines)
- **Updated files:** `src/llm/mod.rs` (add `pub mod agent_factory`), `pipeline.rs` (pass `AgentFactory` to `StoryPipeline`)
- **Absorbed:** `session/provider.rs` functions (`resolve_api_key`, `copilot_headers`) absorbed into `AgentFactory`
- **No new dependencies** — uses existing rig types and project structs
- **No interface changes** — `SessionRunner`, `ReviewRunner`, `Notifier`, `GitProvider` traits unchanged
- **No removed dependencies** — rig-core providers still used, just constructed in one place

---

## 3. Recommended Approach

### Selected Path: Direct Adjustment

Add a single new story (4.5) to Epic 4. Extract the `AgentFactory` + `BuiltAgent` abstraction, fix the Copilot Responses API bug, and eliminate provider match arm duplication. Update PRD FR39 and epics document.

### Rationale

- **Fixes a production-blocking bug** — gpt-5.2-codex and future OpenAI models via Copilot are completely broken without this
- **Future-proof** — hardcoded API format detection per model/provider, with safe Completions API fallback for unknown models
- **Eliminates ~610 lines of duplication** — single construction site replaces 5 match sites
- **No interface changes** — pure internal refactoring, all module contracts preserved
- **Proven pattern** — enum dispatch is idiomatic Rust when trait objects are unavailable (same approach as rig itself)
- **Architect-approved** — full technical brief reviewed and approved, architecture docs already updated

### Alternatives Considered

| Option | Verdict | Reason |
| ------ | ------- | ------ |
| Quick fix (just add Responses API to Copilot branch) | ❌ Not recommended | Fixes the immediate bug but leaves ~610 lines of duplication. Adding a provider or fixing another quirk still requires changes in 5 sites |
| Rollback | ❌ Not viable | Components are correct; provider construction needs centralization, not removal |
| MVP Review | ❌ Not applicable | No scope change — internal refactoring with zero user-facing changes |

---

## 4. Detailed Change Proposals

### 4.1 New Story: Epic 4, Story 4.5 — LLM Provider Abstraction Layer (AgentFactory + BuiltAgent)

**Story:**

As a daemon operator,
I want all LLM provider construction centralized behind an `AgentFactory` with a `BuiltAgent` enum,
So that provider selection, API format detection, and Copilot token exchange happen in one place, eliminating duplication and fixing the Copilot Responses API bug.

**Acceptance Criteria:**

**Given** the `llm` module exists with `context.rs` and `logging.rs`
**When** the `agent_factory.rs` module is created
**Then** it defines a `BuiltAgent` enum with variants: `Anthropic(Agent<anthropic::CompletionModel>)`, `OpenAiResponses(Agent<openai::responses_api::ResponsesCompletionModel>)`, `OpenAiCompletions(Agent<openai::completion::CompletionModel>)`
**And** `BuiltAgent` implements a `stream_chat()` method that delegates to `streaming_chat()` via match dispatch

**Given** the `AgentFactory` struct is initialized with `BotConfig`, `BotSecrets`, and `CopilotTokenCache`
**When** `AgentFactory::build(role, preamble, tools)` is called
**Then** it resolves the provider and model for the given `LlmRole` (Dev, Review, Supervisor)
**And** it resolves the API key from secrets
**And** it constructs the appropriate `BuiltAgent` variant based on provider:
  - `"anthropic"` → `BuiltAgent::Anthropic`
  - `"openai"` → `BuiltAgent::OpenAiResponses`
  - `"github-copilot"` → exchanges OAuth token for session token, then selects API format per model

**Given** the provider is `"github-copilot"`
**When** `AgentFactory::build()` determines the API format
**Then** `copilot_requires_responses_api(model)` is called — a hardcoded heuristic that matches known OpenAI model families (`gpt-*`, `o1-*`, `o3-*`, `codex`)
**And** matched models use the Responses API (`BuiltAgent::OpenAiResponses`)
**And** all other models (Claude, Mistral, unknown) **fallback to Completions API** (`BuiltAgent::OpenAiCompletions`) — the safe default
**And** this logic is not configurable — API format is a deterministic property of the provider behind the model

**Given** the `AgentFactory` is created
**When** `session/runner.rs` is refactored
**Then** the 3 `build_*_agent()` methods (`build_anthropic_agent`, `build_openai_agent`, `build_copilot_agent`) are removed
**And** all provider match arms in `run()` and `resume_session()` are replaced with a single `agent_factory.build(LlmRole::Dev, ..)` call
**And** `run_session()` accepts `&BuiltAgent` directly and uses `BuiltAgent::stream_chat()` instead of the generic `streaming_chat()`

**Given** the `AgentFactory` is created
**When** `review/mod.rs` is refactored
**Then** the provider match in `run_inner()` is replaced with `agent_factory.build(LlmRole::Review, ..)`

**Given** the `AgentFactory` is created
**When** `supervisor/architect.rs` is refactored
**Then** the provider match is replaced with `agent_factory.build(LlmRole::Supervisor, ..)`

**Given** the `AgentFactory` is created
**When** `pipeline.rs` is updated
**Then** `StoryPipeline` receives an `AgentFactory` instance instead of individual provider configs
**And** it passes the factory to `SessionRunner` and `ReviewRunner`

**Given** the refactoring is complete
**When** unit tests are written
**Then** `copilot_requires_responses_api()` is tested with known model names (gpt-4o, o1-mini, o3-pro, gpt-5.2-codex, claude-sonnet-4-20250514, mistral-large) verifying correct API format selection
**And** `AgentFactory::build()` error handling is tested (missing API key, invalid provider name)
**And** `BuiltAgent::stream_chat()` dispatch is verified for each variant

**Given** all changes are complete
**When** validation runs
**Then** `cargo build`, `cargo test`, `cargo clippy`, and `cargo fmt` all pass with zero errors and zero warnings

**Technical Notes:**
- Follows the same pattern as Story 4.4 (git CLI migration): production incident → architect brief → cross-cutting refactoring story
- `session/provider.rs` functions (`resolve_api_key`, `copilot_headers`) are absorbed into `AgentFactory` — `provider.rs` may be removed or reduced to re-exports
- `streaming_chat()` may be moved from `session/dev_agent.rs` to `llm/` or re-exported, since `BuiltAgent::stream_chat()` delegates to it
- The `BuiltAgent` enum must be updated if rig adds new provider types — acceptable trade-off (rare, compile-time concern)
- See `architect-brief-llm-provider-abstraction.md` for full technical rationale and before/after code examples

**Dependencies:** None — all prerequisite code (session, review, supervisor, auth) is already implemented and stable.

---

### 4.2 PRD Update: FR39 — Copilot API Format

**Section:** Functional Requirements > CLI & Configuration > FR39

**OLD:**

> **FR39:** The user can authenticate with GitHub Copilot via OAuth Device Flow during `bmad-bot init` to automatically obtain an LLM access token, and the daemon can transparently exchange it for short-lived Copilot session tokens at runtime. The Copilot provider uses the Completions API (distinct from OpenAI's Responses API) with required IDE-specific headers

**NEW:**

> **FR39:** The user can authenticate with GitHub Copilot via OAuth Device Flow during `bmad-bot init` to automatically obtain an LLM access token, and the daemon can transparently exchange it for short-lived Copilot session tokens at runtime. The Copilot provider is a proxy to multiple backends — API format is hardcoded per model: known OpenAI model families (`gpt-*`, `o1-*`, `o3-*`, `codex`) use the Responses API, all other models fallback to the Completions API (safe default for non-OpenAI backends). Required IDE-specific headers are included in all Copilot requests

**Rationale:** The original FR39 stated Copilot uses "the Completions API" unconditionally. This was accurate at the time of writing but became incorrect when OpenAI began migrating models to the Responses API. The Copilot proxy routes to different backends, so the API format depends on the model — it's a deterministic property, not a blanket rule.

---

### 4.3 Epics Update: Add Story 4.5 + FR Coverage Map

**Section:** Epic 4 > After Story 4.4

Add Story 4.5 (full text from section 4.1 above) after Story 4.4 in the epics document.

**Section:** FR Coverage Map

**ADD:**

> - FR42: Epic 4 (Story 4.5) — Centralize LLM provider construction via AgentFactory with BuiltAgent enum dispatch. Hardcoded API format per provider/model. Fixes Copilot Responses API bug for OpenAI models.

**Section:** PRD Functional Requirements

**ADD (new FR):**

> **FR42:** The daemon centralizes all LLM provider construction via an `AgentFactory` that returns a `BuiltAgent` with unified `stream_chat()` dispatch. API format selection is hardcoded per provider and model — not configurable. GitHub Copilot API format is determined by model name heuristic with Completions API as the safe fallback.

---

### 4.4 Sprint Status Update

**File:** `sprint-status.yaml`

**ADD** under `epic-4` section:

```
  4-5-llm-provider-abstraction-agent-factory: ready-for-dev # depends-on: 4-2
```

**Note:** Story 4.5 should be processed **before** Epic 7 Story 7.1 to ensure integration tests are written against the final provider abstraction.

---

## 5. Implementation Handoff

### Change Scope: Minor

Direct implementation by development team. No backlog reorganization or strategic replan needed.

### Action Plan

| # | Action | Owner | Priority | Artifact |
| - | ------ | ----- | -------- | -------- |
| 1 | Create Story 4.5 implementation artifact | SM | 🔴 Critical | `4-5-llm-provider-abstraction-agent-factory.md` |
| 2 | Implement `src/llm/agent_factory.rs` (BuiltAgent + AgentFactory + copilot heuristic) | Dev | 🔴 Critical | `src/llm/agent_factory.rs` |
| 3 | Refactor `session/runner.rs` — remove build methods, use AgentFactory | Dev | 🔴 Critical | `src/session/runner.rs` |
| 4 | Refactor `review/mod.rs` — use AgentFactory | Dev | 🔴 Critical | `src/review/mod.rs` |
| 5 | Refactor `supervisor/architect.rs` — use AgentFactory | Dev | 🔴 Critical | `src/supervisor/architect.rs` |
| 6 | Update `pipeline.rs` — pass AgentFactory to StoryPipeline | Dev | 🔴 Critical | `src/pipeline.rs` |
| 7 | Unit tests for copilot heuristic, factory build, BuiltAgent dispatch | Dev | 🟡 Important | `src/llm/agent_factory.rs` |
| 8 | Update PRD FR39 + add FR42 | PM | 🟡 Important | `prd.md` |
| 9 | Add Story 4.5 to epics + update FR Coverage Map | PM/SM | 🟡 Important | `epics.md` |
| 10 | Update `sprint-status.yaml` with story 4-5 | SM | 🟡 Important | `sprint-status.yaml` |
| 11 | Validate: `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt` | Dev | 🟢 Standard | — |

### Success Criteria

- [ ] `gpt-5.2-codex` via GitHub Copilot works without 400 error (Responses API used)
- [ ] Non-OpenAI models via Copilot (e.g., Claude) still work (Completions API fallback)
- [ ] `AgentFactory::build()` is the single entry point for all agent construction (dev, review, supervisor)
- [ ] No provider-specific code remains in `session/runner.rs`, `review/mod.rs`, or `supervisor/architect.rs`
- [ ] ~610 lines of duplicated provider match arms eliminated
- [ ] `copilot_requires_responses_api()` correctly identifies OpenAI model families
- [ ] All existing tests pass — zero regressions
- [ ] Architecture and project-context documents reflect the new pattern (already done)
- [ ] PRD FR39 updated, FR42 added
- [ ] Epics document includes Story 4.5
- [ ] Sprint status includes `4-5-llm-provider-abstraction-agent-factory: ready-for-dev`

### Sequencing Note

Story 4.5 should be implemented **before** Epic 7 Story 7.1 (Integration Test Infrastructure). This ensures:
1. The Copilot bug is fixed immediately (production-blocking)
2. Integration tests are written against the final `AgentFactory` abstraction, not the deprecated provider match pattern
3. No throwaway test code

---

## References

- **Architect Brief:** `architect-brief-llm-provider-abstraction.md` — full technical analysis, BuiltAgent design, before/after code, scope of change
- **Production Incident:** 2026-02-12 — `gpt-5.2-codex` via Copilot → 400 `unsupported_api_for_model`
- **Architecture Decision 8:** Added to `architecture.md` — LLM Provider Abstraction pattern
- **Previous Sprint Change Proposal:** `sprint-change-proposal-2026-02-11.md` — Pipeline reordering (approved, for format reference)
- **Similar precedent:** Story 4.4 (Git CLI Migration) — same pattern: production incident → architect brief → cross-cutting refactoring story