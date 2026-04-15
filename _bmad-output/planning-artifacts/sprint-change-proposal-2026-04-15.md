---
type: sprint-change-proposal
date: 2026-04-15
project: bmad-bot
author: JB
status: approved
triggered_by: BMAD v6.2+ skill migration, pipeline expansion, Copilot removal
scope: major
---

# Sprint Change Proposal — BMAD Bot Pipeline Evolution

## 1. Issue Summary

### Problem Statement

The BMAD Bot was architected around BMAD's persona + menu interaction model: the daemon loads a persona file (`dev.md`) via Zed-style XML context injection, sends menu commands (`"DS"`, `"CR"`), and auto-responds to interactive prompts via `ResponseAnalyzer`. Starting with BMAD v6.2, the framework has migrated to a **skill-based model** (`SKILL.md` + `workflow.md`) where workflows are self-contained and require no persona loading or menu-driven interaction.

In addition, the current pipeline only handles stories that are already `ready-for-dev`. It does not create stories, validate them adversarially, or provide an independent product/technical vision check before development. This results in a disconnected workflow where story creation and quality assurance happen outside the bot.

Finally, the GitHub Copilot provider support introduces significant complexity (a rig fork, OAuth Device Flow, token exchange, custom streaming compat) that is no longer needed.

### Context

- The project is **functionally complete** — all 10 epics are done (Epic 7 Integration Tests dropped in favor of real-repo testing)
- 41 BMAD skills are available in `.github/skills/`
- The current bot successfully runs the full dev → review → PR pipeline using the persona model
- The change aligns the bot with BMAD's current architecture and expands its autonomous capabilities

### Evidence

- BMAD v6.2+ skills (`bmad-create-story`, `bmad-dev-story`, `bmad-code-review`, `bmad-review-adversarial-general`) are all skill-based with `SKILL.md` entry points
- Skills are designed for autonomous execution — `bmad-create-story` explicitly states "ZERO USER INTERVENTION", `bmad-dev-story` runs in a single uninterrupted session
- The rig fork (`jbanety/rig`, branch `fix/copilot-streaming-compat`) exists solely for 4 Copilot streaming compatibility commits — all irrelevant without the Copilot provider
- The Zed-style XML context injection mechanism (`ContextBuilder`) already works for loading any MD file — switching from persona to skill requires changing the file path, not the mechanism

---

## 2. Impact Analysis

### Epic Impact

| Epic | Status | Impact |
|------|--------|--------|
| **Epic 1** — Foundation & CLI | Done | 🟡 Story 1.6 (Copilot OAuth) code to remove. Init command extended with project brief prompt. |
| **Epic 2** — Watcher & Deps | Done | 🟡 Watcher extended to detect `backlog` stories. Route by status. |
| **Epic 3** — Supervision | Done | 🟢 Supervisor stays. Architect session may migrate to spawn_agent pattern. |
| **Epic 4** — Dev Session | Done | 🟡 Activation switches from persona to skill. `"DS"` command removed. |
| **Epic 5** — Code Review & PR | Done | 🟡 Activation switches to skill. Critic consultation for decision-needed findings. |
| **Epic 6** — Notifications & Resilience | Done | 🟡 WAL extended with `pipeline_phase`. |
| **Epic 7** — Integration Tests | Ready-for-dev | 🗑️ **Dropped** — testing directly on real repos. |
| **Epic 8** — Surgical Tooling | Done | 🟢 No impact — tools are activation-agnostic. |
| **Epic 9** — MCP | Done | 🟢 No impact — MCP tools registered at agent build time regardless of skill. |
| **Epic 10** — Terminal UI | Done | 🟡 New UI events for create/adversarial/critic phases. |

### Artifact Conflicts

#### PRD

**Functional Requirements to modify:**

| FR | Current | Change |
|----|---------|--------|
| FR1 | Detect `ready-for-dev` stories | Extend to `backlog` + all pipeline phases |
| FR8 | BMAD dev agent persona via Zed-style XML context | Replace with skill invocation (same mechanism, different file) |
| FR10 | Execute full BMAD `dev-story` workflow | Still true, via skill invocation |
| FR11 | Session language override via preamble | Verify skills handle language via config |
| FR39 | Copilot OAuth Device Flow | **REMOVE** |
| FR42 | AgentFactory with Copilot logic | Simplify (remove Copilot, keep Anthropic + OpenAI-compatible) |

**Functional Requirements to add:**

| New FR | Description |
|--------|------------|
| FR50 | Invoke `bmad-create-story` skill to create story files from `backlog` stories |
| FR51 | Invoke `bmad-review-adversarial-general` skill for story critique, with findings fed back to active session |
| FR52 | Launch a Story Critic agent with persistent cross-story memory, project brief as vision anchor, and extended thinking for independent product/technical review |
| FR53 | Pipeline executes a linear flow per story with daemon-orchestrated consultations: create-story session (with adversarial + critic consultations) → dev-story → code-review (with critic consultation for decision-needed findings) |
| FR54 | Epic review (Winston) reads `deferred-work.md` and its own code analysis to propose pre-epic debt stories |
| FR55 | `spawn_agent` tool available to all agent sessions for LLM-initiated sub-agent delegation |
| FR56 | OpenAI-compatible provider with optional `base_url` for any OpenAI-compatible endpoint (Ollama, LM Studio, vLLM, etc.) |

**FR to remove:** FR39 (Copilot OAuth)

**Executive Summary and MVP Scope sections** require rewriting to reflect the skill-based model and expanded pipeline.

#### Architecture

| Decision | Impact |
|----------|--------|
| **D1** — Supervisor Hybrid (Chat Loop + ask_supervisor) | 🟡 Chat loop simplifies (less auto-responding needed with skills). ask_supervisor tool stays. |
| **D2** — Sprint-Status Mutation (Daemon Reads, Agent Writes) | 🟡 Still valid. Daemon tracks pipeline phase internally (WAL), not in sprint-status. |
| **D3** — WAL File for Crash Recovery | 🟡 Add `pipeline_phase` field. Each phase = own WAL, cleared on phase completion. |
| **D5** — XML Context Activation | 🔴 **Obsolete as written** — persona activation no longer applies. Mechanism (ContextBuilder + Zed XML) stays, target file changes from persona to SKILL.md. |
| **D8** — BuiltAgent Enum + AgentFactory | 🟡 Remove `OpenAiCompletions` variant, `CopilotTokenCache`, Copilot logic. Add `base_url` support for OpenAI-compatible. Add `LlmRole::Critic`. |

**New architectural patterns needed:**

| Pattern | Description |
|---------|------------|
| **Daemon-Orchestrated Consultation** | Pipeline pauses an active session, launches a fresh agent (adversarial/critic), feeds results back as a message to the paused session. Same mechanics as `spawn_agent` tool but triggered by the daemon, not the LLM. |
| **Persistent Critic Memory** | `critic-memory.md` file accumulates observations, decisions, and rationale across all stories. Each Critic invocation = fresh agent loading brief + memory. Project brief provided at `bmad-bot init`. |

#### Other Artifacts

| Artifact | Impact |
|----------|--------|
| `project-context.md` | Update: agent activation, Copilot references, AgentFactory |
| `Cargo.toml` | Fork rig → official `rig-core` crates.io (verify `rmcp` feature) |
| `bmad-bot.yaml.example` | Remove `github-copilot` provider, add `base_url` option, add `critic` config, add `project_brief` path |
| `README.md` | Remove Copilot references, document OpenAI-compatible + base_url |

### Technical Impact

| Area | Detail |
|------|--------|
| `src/auth/` | **Delete entirely** (~1350 lines) |
| `src/llm/agent_factory.rs` | Remove Copilot branch, simplify BuiltAgent enum, add base_url support |
| `src/session/agent.rs` | Parameterize activation (skill path instead of persona path), remove hardcoded "DS"/"CR" commands |
| `src/session/runner.rs` | Support pause/consult/resume pattern for daemon-orchestrated consultations |
| `src/session/analyzer.rs` | Simplify: remove persona/menu auto-response patterns |
| `src/session/provider.rs` | Remove Copilot routing, clean secrets |
| `src/pipeline.rs` | Major refonte: 3 session types (create+consult, dev, review+consult), backlog→done state machine |
| `src/watcher/` | Extend eligible stories to include `backlog` status |
| `src/review/mod.rs` | Switch to skill-based activation |
| `src/review/epic.rs` | Extend prompt: read deferred-work.md, propose pre-epic stories |
| `src/tools/spawn_agent.rs` | **New** — Zed-inspired sub-agent tool for all sessions |
| `src/critic/` | **New module** — Critic memory system, agent construction, prompt engineering |
| `src/cli/mod.rs` | Project brief prompt at init, remove Copilot from provider list |

---

## 3. Recommended Approach

### Selected Path: New Epics (11–14) post-completion

The existing 10 epics are done and stable. Rather than modifying completed epics, the changes are structured as **4 new epics** sequenced by dependency:

```
Epic 11 (independent)     → Copilot Removal & Provider Simplification
Epic 12 (depends on 11)   → Skill-Based Sessions & SpawnAgent Tool
Epic 13 (depends on 12)   → Multi-Phase Pipeline & Story Critic
Epic 14 (depends on 13)   → Epic Review Enhancement & Deferred Work
```

**Rationale:**
- **Low risk** — existing functionality is preserved; new epics build on a stable foundation
- **Incremental delivery** — each epic is independently valuable and testable
- **Clean separation** — no retroactive changes to completed epics (except code cleanup in Epic 11)
- **Epic 7 dropped** — integration tests will be done directly on real repos, which provides better validation than mocked tests

### Effort and Risk

| Epic | Effort | Risk | Timeline Impact |
|------|--------|------|-----------------|
| 11 — Copilot Cleanup | Low | 🟢 Low — pure deletion | ~1 sprint |
| 12 — Skill Sessions + SpawnAgent | Medium | 🟢 Low — mechanism stays, file changes | ~1 sprint |
| 13 — Pipeline + Critic | High | 🟡 Medium — prompt engineering, new patterns | ~2-3 sprints |
| 14 — Epic Review Enhancement | Low | 🟢 Low — prompt extension | ~1 sprint |

---

## 4. Detailed Change Proposals

### Epic 11: Copilot Removal & Provider Simplification

**Objective:** Remove all Copilot code, switch to official `rig-core`, simplify to Anthropic + OpenAI-compatible providers.

| Story | Description | Scope |
|-------|------------|-------|
| **11-1** | Delete `src/auth/` module entirely (Copilot OAuth, token exchange, cache) | ~1350 lines removed |
| **11-2** | Simplify `AgentFactory`: remove `github-copilot` branch, `BuiltAgent::OpenAiCompletions` variant, `CopilotTokenCache`. Keep 2 variants: `Anthropic` + `OpenAiCompatible`. Expose optional `base_url` in config for OpenAI-compatible provider. | `llm/agent_factory.rs` |
| **11-3** | Clean provider routing: `session/provider.rs`, `config/mod.rs` (secrets), `cli/mod.rs` (provider list → `anthropic`, `openai-compatible`). Add optional `base_url` per LLM role in config YAML. | Multi-file |
| **11-4** | Migrate `Cargo.toml`: fork `jbanety/rig` → official `rig-core` on crates.io. Verify `rmcp` feature availability. Validate compilation + all tests pass. | `Cargo.toml`, `Cargo.lock` |
| **11-5** | Update docs: `project-context.md`, `bmad-bot.yaml.example`, `README.md` — remove Copilot, document OpenAI-compatible with `base_url`. | Docs |

**Dependencies:** None
**Config change example:**

```yaml
llm:
  dev:
    provider: openai-compatible
    model: gpt-4.1
    # base_url: https://api.openai.com/v1  (default, optional)
  review:
    provider: anthropic
    model: claude-sonnet-4-20250514
  supervisor:
    provider: openai-compatible
    model: local-llama
    base_url: http://localhost:11434/v1   # Ollama
```

---

### Epic 12: Skill-Based Sessions & SpawnAgent Tool

**Objective:** Replace persona/menu activation with skill invocation. Add universal `spawn_agent` tool.

**Key insight:** The Zed-style XML context injection mechanism already works. Switching from persona to skill = changing which file gets loaded into `ContextBuilder`. The LLM reads SKILL.md, sees "Follow the instructions in ./workflow.md", and loads it via `read_file` tool autonomously.

| Story | Description | Scope |
|-------|------------|-------|
| **12-1** | Parameterize activation by skill — `activate_agent()` accepts a skill path (e.g., `.github/skills/bmad-dev-story/SKILL.md`) instead of persona `dev.md`. Load only `SKILL.md` via `ContextBuilder`. The LLM discovers and loads workflow files itself via tools. Remove hardcoded post-activation messages (`"DS"`, `"CR"`). | `session/agent.rs`, `session/runner.rs`, `review/mod.rs` |
| **12-2** | Simplify `ResponseAnalyzer` — Remove auto-response patterns for persona menu (confirmations, "Should I proceed?", story selection prompts). Keep: completion detection, error detection, escalation detection. | `session/analyzer.rs` |
| **12-3** | `SpawnAgentTool` — New rig tool inspired by Zed's implementation. Input: `label`, `message`, `session_id` (optional for follow-up). Output: `session_id` + `output` (final message only). Creates fresh agent via `AgentFactory`, runs via `stream_chat()`, returns final message. In-memory session map (`HashMap<SessionId, SubAgentState>`) for follow-ups. | `tools/spawn_agent.rs` (new) |
| **12-4** | Universal `SpawnAgentTool` registration — Added to `create_base_tools()` for all sessions. Evaluate migrating `ArchitectSession` (supervisor tier 2) to use spawn_agent instead of hardcoded multi-turn script. | `session/agent.rs`, `supervisor/architect.rs` |
| **12-5** | Tests — Skill-based activation, `SpawnAgentTool`, simplified analyzer. Remove persona/menu-related tests. | Tests |

**Dependencies:** Epic 11

---

### Epic 13: Multi-Phase Pipeline & Story Critic

**Objective:** The pipeline orchestrates the full story lifecycle from `backlog` to `done` with daemon-orchestrated consultations (adversarial review + Story Critic).

**Pipeline model:**

```
SESSION CREATE-STORY (lives from start to commit)
  │
  ├─ Preamble + SKILL.md → LLM works → story created
  │  (daemon detects completion pattern)
  │
  ├─ await Adversarial agent (fresh, story as input)
  │  → findings
  │
  ├─ Findings as message → create-story session resumes
  │  "apply these corrections" → LLM updates story
  │
  ├─ await Critic agent (fresh, brief + memory + updated story)
  │  → observations + corrections
  │  → updates critic-memory.md
  │
  ├─ Critic findings as message → create-story session resumes
  │  LLM applies → commit
  │
  └─ Session complete

SESSION DEV-STORY (fresh agent)
  └─ implements → commit

SESSION CODE-REVIEW (lives through if decision-needed)
  ├─ Preamble + SKILL.md → LLM works → findings
  ├─ if decision-needed:
  │    ├─ await Critic agent (fresh, brief + memory + findings)
  │    │  → decisions
  │    ├─ Decisions as message → code-review session resumes
  │    │  LLM applies
  │    └─ commit
  └─ Session complete

Push + PR + Notify
```

**Daemon-orchestrated consultation pattern:** Same mechanics as `spawn_agent` tool (fresh agent, result returned) but triggered by the daemon, not the LLM. The daemon detects a phase completion pattern, launches a consultation agent, and feeds the result back as a new message to the paused session.

| Story | Description |
|-------|------------|
| **13-1** | **Extension Watcher** — `eligible_stories()` returns `backlog` stories too. Pipeline routes by status: `backlog` → create phase, `ready-for-dev` → dev phase, `review` → code-review phase. Supports resuming after crash. |
| **13-2** | **Pipeline Orchestrator refonte** — Orchestrates main sessions + daemon-orchestrated consultations. Uniform pattern: main session stays alive, consultations are fresh agent calls whose results return as messages. |
| **13-3** | **Consultation mechanism** — In the session runner: detect phase completion pattern, launch external agent via await, send result as new message to the main session. Reusable for create-story and code-review. |
| **13-4** | **Create-Story phase complete** — Session `bmad-create-story` + 2 consultations (adversarial fresh → findings → resume → critic fresh → findings → resume → commit). |
| **13-5** | **Dev phase** — Simple session `bmad-dev-story`. |
| **13-6** | **Code-Review phase with Critic** — Session `bmad-code-review`. If `decision-needed`: consultation critic → decisions → resume → apply. |
| **13-7** | **Config `bmad-bot init` — Project Brief** — New prompt at init for project brief file path. Stored in config YAML as `project_brief`. |
| **13-8** | **Critic Memory System** — Persistent `critic-memory.md` file. Cumulative across all stories. Project brief as founding context. Enriched after every Critic invocation with observations, decisions, rationale. |
| **13-9** | **Critic Agent** — Prompt engineering: product & technical vision guardian. Extended thinking. Loads: project brief + `critic-memory.md` + current artifact (story or findings). Produces structured observations and corrections. Updates memory after each invocation. Independent from BMAD methodology context. |
| **13-10** | **WAL with pipeline_phase** — Add `pipeline_phase` to WAL. On crash recovery: resume at the correct phase. Each phase = own WAL, cleared on phase completion. |
| **13-11** | **UI Events for new phases** — Events for create, adversarial consultation, critic consultation, and phase transitions in `UiRenderer`. |

**Dependencies:** Epic 12
**Story Critic design notes:**

The Critic replicates the workflow JB currently does manually with ChatGPT:
1. Start with the project idea/brief (founding context)
2. Review each story against that original vision
3. Accumulate knowledge across stories ("in story 3 we decided X, so here we should...")
4. Provide decisions for ambiguous findings based on full project history

Implementation: fresh agent each time, but loads `critic-memory.md` (cumulative file) + project brief. Memory grows organically — the Critic reads it and appends its new observations. No structured format enforced — the Critic manages its own memory.

---

### Epic 14: Epic Review Enhancement & Deferred Work

**Objective:** Enrich the epic review (Winston) to process accumulated technical debt and propose pre-epic cleanup stories.

| Story | Description |
|-------|------------|
| **14-1** | **Winston reads `deferred-work.md`** — Extend `EpicReviewRunner` prompt to read the deferred work file, sort by criticality/effort, and integrate analysis into the report. |
| **14-2** | **Pre-epic story generation** — Winston proposes debt/improvement stories from **two sources**: items in `deferred-work.md` AND his own findings from the epic code review. Convention: `X-0-pre-epic-X-{slug}`. Structured output: title, description, estimation, justification. |
| **14-3** | **Inject into sprint-status.yaml** — Pre-epic stories added at the head of the next epic with status `backlog`. The linear pipeline processes them first (document-order topo sort handles this naturally). |
| **14-4** | **Purge processed items** — When a pre-epic story reaches `done`, corresponding items are removed from `deferred-work.md`. Mapping between story and debt items maintained in the story file. |

**Dependencies:** Epic 13 (linear pipeline in place to process pre-epic stories)

---

## 5. Implementation Handoff

### Scope Classification: **Major**

This is a fundamental evolution of the BMAD Bot pipeline touching the interaction model, pipeline orchestration, and adding a new agent type. However, the existing codebase is solid and the changes build incrementally on stable foundations.

### Handoff Plan

| Bloc | Epic | Recipient | Responsibilities |
|------|------|-----------|-----------------|
| **A — Cleanup** | Epic 11 | Developer agent | Delete Copilot code, migrate to official rig-core, simplify AgentFactory |
| **B — Foundation** | Epic 12 | Developer agent | Skill-based activation, SpawnAgent tool, analyzer simplification |
| **C — Core** | Epic 13 | Developer agent + Prompt engineering | Pipeline refonte, consultation pattern, Critic agent design |
| **D — Enhancement** | Epic 14 | Developer agent | Winston prompt extension, sprint-status integration |

### Sequencing

```
Epic 11 ──→ Epic 12 ──→ Epic 13 ──→ Epic 14
(cleanup)   (foundation) (core)      (enhancement)
```

Each epic is independently deployable and testable. Epic 11 can ship immediately — the bot continues working with Anthropic + OpenAI-compatible providers. Epic 12 changes the activation model. Epic 13 is the big one. Epic 14 is polish.

### Success Criteria

- [ ] Bot invokes BMAD skills instead of persona files — no more `dev.md` activation, no more `"DS"`/`"CR"` commands
- [ ] Pipeline handles stories from `backlog` through `done` autonomously
- [ ] Story Critic provides independent review with persistent memory across stories
- [ ] `spawn_agent` tool available in all agent sessions
- [ ] Copilot code fully removed, official `rig-core` in use
- [ ] `deferred-work.md` processed at epic boundaries with pre-epic stories generated
- [ ] OpenAI-compatible provider works with custom `base_url`

### Key Architectural Decisions Made

| Decision | Choice | Rationale |
|----------|--------|-----------|
| N sessions per story (not 1 mega-session) | Each pipeline phase = fresh agent | Isolation, clean context, error containment |
| Skill activation via existing Zed-style XML | Load SKILL.md instead of persona MD | Mechanism already works, minimal code change |
| LLM reads workflow.md itself | No daemon-side workflow loading | Skills are self-contained, LLM discovers via read_file |
| Daemon-orchestrated consultations | Pause session → fresh agent → resume | Same pattern as spawn_agent but daemon-controlled |
| Critic memory via cumulative file | `critic-memory.md` grows across stories | Simple, inspectable, no DB needed |
| Project brief at init | Critic's founding context | Independent from BMAD artifacts |
| Pre-epic stories for debt | `X-0-pre-epic-X-{slug}` convention | Natural integration with linear pipeline |
| Epic 7 dropped | Test on real repos | Better validation than mocked integration tests |
| OpenAI-compatible (not just OpenAI) | Support any OpenAI-compatible endpoint | Ollama, LM Studio, vLLM, Groq, etc. |

---

## 6. Summary

| Dimension | Detail |
|-----------|--------|
| **Issue addressed** | BMAD v6.2+ skill migration, pipeline expansion (story creation + critic), Copilot removal |
| **Change scope** | Major — 4 new epics, 25 stories |
| **Artifacts to update** | PRD (6 FRs modified, 7 added, 1 removed), Architecture (5 decisions impacted, 2 new patterns), project-context.md, Cargo.toml, config examples, README |
| **Epics modified** | Epic 7 dropped. Epics 1-10 code impacted by cleanup (Epic 11) and activation changes (Epic 12). |
| **New epics** | 11 (Copilot Removal), 12 (Skill Sessions + SpawnAgent), 13 (Pipeline + Critic), 14 (Epic Review Enhancement) |
| **Routed to** | Developer agent for implementation, with prompt engineering focus for Story Critic (Epic 13) |

---

*Correct Course workflow complete, JB!*