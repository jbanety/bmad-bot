# Story 4.5: LLM Provider Abstraction Layer (AgentFactory + BuiltAgent)

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want all LLM provider construction centralized behind an `AgentFactory` with a `BuiltAgent` enum,
So that provider selection, API format detection, and Copilot token exchange happen in one place, eliminating ~610 lines of duplication and fixing the Copilot Responses API bug.

> **Triggered by:** Production incident (2026-02-12) — `gpt-5.2-codex` via GitHub Copilot proxy rejects `/chat/completions` endpoint (requires Responses API). See `architect-brief-llm-provider-abstraction.md` for full rationale.

## Acceptance Criteria

1. **Given** the `llm` module exists with `context.rs` and `logging.rs`
   **When** the `agent_factory.rs` module is created
   **Then** it defines a `BuiltAgent` enum with variants: `Anthropic(Agent<anthropic::CompletionModel>)`, `OpenAiResponses(Agent<openai::responses_api::ResponsesCompletionModel>)`, `OpenAiCompletions(Agent<openai::completion::CompletionModel>)`
   **And** `BuiltAgent` implements a `stream_chat()` method that delegates to `streaming_chat()` via match dispatch

2. **Given** the `AgentFactory` struct is initialized with `BotConfig`, `BotSecrets`, and `CopilotTokenCache`
   **When** `AgentFactory::build(role, preamble, tools)` is called
   **Then** it resolves the provider and model for the given `LlmRole` (Dev, Review, Supervisor)
   **And** it resolves the API key from secrets
   **And** it constructs the appropriate `BuiltAgent` variant based on provider:
   - `"anthropic"` → `BuiltAgent::Anthropic`
   - `"openai"` → `BuiltAgent::OpenAiResponses`
   - `"github-copilot"` → exchanges OAuth token for session token, then selects API format per model

3. **Given** the provider is `"github-copilot"`
   **When** `AgentFactory::build()` determines the API format
   **Then** `copilot_requires_responses_api(model)` is called — a hardcoded heuristic that matches known OpenAI model families (`gpt-*`, `o1-*`, `o3-*`, `codex`)
   **And** matched models use the Responses API (`BuiltAgent::OpenAiResponses`)
   **And** all other models (Claude, Mistral, unknown) **fallback to Completions API** (`BuiltAgent::OpenAiCompletions`) — the safe default
   **And** this logic is not configurable — API format is a deterministic property of the provider behind the model

4. **Given** the `AgentFactory` is created
   **When** `session/runner.rs` is refactored
   **Then** the 3 `build_*_agent()` methods (`build_anthropic_agent`, `build_openai_agent`, `build_copilot_agent`) are removed
   **And** all provider match arms in `run()` and `resume_session()` are replaced with a single `agent_factory.build(LlmRole::Dev, ..)` call
   **And** `run_session()` accepts `&BuiltAgent` directly and uses `BuiltAgent::stream_chat()` instead of the generic `streaming_chat()`

5. **Given** the `AgentFactory` is created
   **When** `review/mod.rs` is refactored
   **Then** the provider match in `run_inner()` is replaced with `agent_factory.build(LlmRole::Review, ..)`

6. **Given** the `AgentFactory` is created
   **When** `supervisor/architect.rs` is refactored
   **Then** the provider match in `AnswerProvider::ask()` is replaced with `agent_factory.build(LlmRole::Supervisor, ..)`

7. **Given** the `AgentFactory` is created
   **When** `pipeline.rs` is updated
   **Then** `StoryPipeline` receives an `AgentFactory` instance instead of individual provider configs
   **And** it passes the factory to `SessionRunner` and `ReviewRunner`

8. **Given** the refactoring is complete
   **When** unit tests are written
   **Then** `copilot_requires_responses_api()` is tested with known model names (gpt-4o, o1-mini, o3-pro, gpt-5.2-codex, claude-sonnet-4-20250514, mistral-large) verifying correct API format selection
   **And** `AgentFactory::build()` error handling is tested (missing API key, invalid provider name)
   **And** `BuiltAgent::stream_chat()` dispatch is verified for each variant

9. **Given** all changes are complete
   **When** validation runs
   **Then** `cargo build`, `cargo test`, `cargo clippy`, and `cargo fmt` all pass with zero errors and zero warnings

## Tasks / Subtasks

- [ ] Task 0: Prerequisite Verification (AC: all)
  - [ ] Verify all existing tests pass before refactoring: `cargo test`
  - [ ] Read and understand Architecture Decision 8 in `architecture.md`
  - [ ] Read the architect brief: `_bmad-output/planning-artifacts/architect-brief-llm-provider-abstraction.md`
  - [ ] Identify all provider match arm sites via: `grep -rn "match provider" src/` and `grep -rn "build_anthropic_agent\|build_openai_agent\|build_copilot_agent" src/`
  - [ ] Confirm the 5 match sites: `runner.rs::run()`, `runner.rs::resume_session()`, `runner.rs::build_*_agent()` (×3), `review/mod.rs::run_inner()`, `supervisor/architect.rs::ask()`

- [ ] Task 1: Create `src/llm/agent_factory.rs` — BuiltAgent enum (AC: #1)
  - [ ] Define `BuiltAgent` enum with 3 variants: `Anthropic`, `OpenAiResponses`, `OpenAiCompletions`
  - [ ] Implement `stream_chat(&self, prompt, history, shutdown) -> Result<String, PromptError>` via match dispatch
  - [ ] Each arm delegates to the existing `streaming_chat()` function (currently in `session/dev_agent.rs`)
  - [ ] Add `/// doc comments` on enum, variants, and `stream_chat()`

- [ ] Task 2: Create `AgentFactory` struct with `build()` method (AC: #2)
  - [ ] Define `AgentFactory { config: Arc<BotConfig>, secrets: Arc<BotSecrets>, copilot_cache: Mutex<CopilotTokenCache> }`
  - [ ] Define `LlmRole` enum: `Dev`, `Review`, `Supervisor`
  - [ ] Implement `AgentFactory::new(config, secrets)` — creates `CopilotTokenCache::new()` internally
  - [ ] Implement `async fn build(&self, role: LlmRole, preamble: &str, tools: ToolSet) -> Result<BuiltAgent, ProviderError>`
  - [ ] Inside `build()`: resolve provider/model from `config.llm.{role}`, resolve API key via `resolve_api_key()`, then match on provider to construct agent
  - [ ] Move `resolve_api_key()` from `session/provider.rs` into `agent_factory.rs` (or re-export)
  - [ ] Move `copilot_headers()` from `session/provider.rs` into `agent_factory.rs` (or re-export)

- [ ] Task 3: Add `copilot_requires_responses_api()` heuristic (AC: #3)
  - [ ] Implement the function: `fn copilot_requires_responses_api(model: &str) -> bool`
  - [ ] Match logic: `m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-") || m.contains("codex")`
  - [ ] Case-insensitive: `let m = model.to_lowercase();`
  - [ ] Add doc comment explaining this is hardcoded by design, with fallback rationale

- [ ] Task 4: Refactor `session/runner.rs` — remove build methods, use AgentFactory (AC: #4)
  - [ ] Remove `build_anthropic_agent()` (L840–885)
  - [ ] Remove `build_openai_agent()` (L888–934)
  - [ ] Remove `build_copilot_agent()` (L941–990)
  - [ ] Remove `copilot_cache` field from `SessionRunner` struct (L217) — moved to `AgentFactory`
  - [ ] Remove `resolve_copilot_session()` method (L252–284) — absorbed into `AgentFactory`
  - [ ] Add `agent_factory: Arc<AgentFactory>` field to `SessionRunner`
  - [ ] Update `SessionRunner::new()` to accept `AgentFactory`
  - [ ] Refactor `run()` (L607–837): replace the 3-arm provider match (L731–830) with single `self.agent_factory.build(LlmRole::Dev, &preamble, tools).await`
  - [ ] Refactor `resume_session()` (L327–599): replace the 3-arm provider match (L487–594) with single `self.agent_factory.build(LlmRole::Dev, &preamble, tools).await`
  - [ ] Update `run_session()` signature: change generic `<A, M>` to accept `&BuiltAgent` directly
  - [ ] Replace `streaming_chat(agent, prompt, history, shutdown)` calls inside `run_session()` with `agent.stream_chat(prompt, history, shutdown)`
  - [ ] Update `context_limit_recovery()` and `drive_activation_and_recover()` similarly — they also call `streaming_chat()`
  - [ ] Update `summarize_history()` — also calls `streaming_chat()` with the agent

- [ ] Task 5: Refactor `review/mod.rs` — use AgentFactory (AC: #5)
  - [ ] Add `agent_factory: Arc<AgentFactory>` field to `ReviewRunner` (L149–158)
  - [ ] Update `ReviewRunner::new()` to accept `AgentFactory`
  - [ ] Refactor `run_inner()` (L251–420): replace the 3-arm provider match (L295–407) with single `self.agent_factory.build(LlmRole::Review, &preamble, tools).await`
  - [ ] Update `drive_review_session()` — change generic `<A, M>` to accept `&BuiltAgent`
  - [ ] Replace `streaming_chat()` calls with `agent.stream_chat()`
  - [ ] Remove `ReviewError::ApiKeyMissing` and `ReviewError::UnsupportedProvider` — these are now handled by `ProviderError` inside AgentFactory
  - [ ] Map `ProviderError` to `ReviewError::ProviderInit` at the `build()` call site

- [ ] Task 6: Refactor `supervisor/architect.rs` — use AgentFactory (AC: #6)
  - [ ] NOTE: `ArchitectSession` has a different pattern — it builds its own agent per question with only a `ReadFile` tool (not the full 9-tool set). The `AgentFactory` must support this.
  - [ ] Option A: Add an `AgentFactory::build_with_tools()` or pass tools as parameter (already the design)
  - [ ] Option B: `ArchitectSession` creates its own `BuiltAgent` using a lightweight factory call
  - [ ] Replace the 3-arm match in `AnswerProvider::ask()` (L354–434) with `agent_factory.build(LlmRole::Supervisor, &preamble, tools).await`
  - [ ] Update `drive_conversation()` — change generic `<A, M>` to accept `&BuiltAgent`
  - [ ] Replace `streaming_architect_chat()` calls with `agent.stream_chat()` — note this function (L33–74) is a local version of `streaming_chat` specific to architect; evaluate if `BuiltAgent::stream_chat()` can replace it
  - [ ] Remove `api_key` and `provider` fields from `ArchitectSession` struct (L164–175) — replaced by `AgentFactory`
  - [ ] Update `ArchitectSession::new()` to accept `Arc<AgentFactory>` instead of resolving provider/key internally
  - [ ] Remove `env_var_for_provider()` helper (L178–187) — `resolve_api_key` in factory handles this

- [ ] Task 7: Update `pipeline.rs` — pass AgentFactory to StoryPipeline (AC: #7)
  - [ ] Create `AgentFactory` in `StoryPipeline::new()` (L155–175)
  - [ ] Wrap in `Arc<AgentFactory>` and pass to `SessionRunner::new()` and `ReviewRunner::new()`
  - [ ] The `copilot_cache` no longer needs to exist on `SessionRunner` — it's inside `AgentFactory`

- [ ] Task 8: Update `src/llm/mod.rs` (AC: #1)
  - [ ] Add `pub mod agent_factory;`
  - [ ] Re-export key types: `pub use agent_factory::{AgentFactory, BuiltAgent, LlmRole};`

- [ ] Task 9: Handle `session/provider.rs` fate (AC: #4)
  - [ ] Evaluate what remains in `provider.rs` after `resolve_api_key()` and `copilot_headers()` move to `agent_factory.rs`
  - [ ] If only `ProviderError` and `create_completion_model()` remain → keep `provider.rs` with just `ProviderError` (still used by `create_tools()`)
  - [ ] If `ProviderError` moves to `agent_factory.rs` → `provider.rs` can be deleted
  - [ ] Ensure no broken imports across the codebase

- [ ] Task 10: Unit tests (AC: #8)
  - [ ] Test `copilot_requires_responses_api()` — positive cases: `"gpt-4o"`, `"gpt-5.2-codex"`, `"o1-mini"`, `"o3-pro"`, `"GPT-4o"` (case insensitive), `"some-codex-model"`
  - [ ] Test `copilot_requires_responses_api()` — negative cases: `"claude-sonnet-4-20250514"`, `"mistral-large"`, `"unknown-model"`, `""`
  - [ ] Test `LlmRole` enum: verify `config_for_role()` returns correct provider/model for each role
  - [ ] Test `AgentFactory` error: missing API key → `ProviderError::MissingApiKey`
  - [ ] Test `AgentFactory` error: unsupported provider → `ProviderError::UnsupportedProvider`
  - [ ] Verify existing tests in `session/runner.rs`, `review/mod.rs`, `supervisor/architect.rs` still pass after refactor

- [ ] Task 11: Final verification (AC: #9)
  - [ ] `cargo build` — zero errors
  - [ ] `cargo test` — all tests pass, zero regressions
  - [ ] `cargo clippy` — zero warnings
  - [ ] `cargo fmt --check` — clean
  - [ ] `grep -rn "build_anthropic_agent\|build_openai_agent\|build_copilot_agent" src/` — zero results
  - [ ] `grep -rn "match provider" src/session/runner.rs src/review/mod.rs src/supervisor/architect.rs` — zero provider match arms remain

## Dev Notes

### ⚠️ Cross-Cutting Refactoring — Touches 6+ Files Across 4 Modules

This story is a **cross-cutting refactoring** that touches code originally established by multiple stories:
- `src/session/runner.rs` — Story 4.2 (Epic 4) — SessionRunner with 3 build methods + 2 match sites
- `src/session/provider.rs` — Story 4.2 (Epic 4) — `resolve_api_key()`, `copilot_headers()`, `ProviderError`
- `src/review/mod.rs` — Story 5.2 (Epic 5) — ReviewRunner with 1 match site
- `src/supervisor/architect.rs` — Story 3.2 (Epic 3) — ArchitectSession with 1 match site
- `src/pipeline.rs` — Story 5.1 (Epic 5) — StoryPipeline construction
- `src/llm/mod.rs` — Story 4.2 area — module root (currently has `context` and `logging`)

After this story, **zero provider match arms** should remain outside `agent_factory.rs`.

### Triggered By: Production Incident (2026-02-12)

The daemon's Copilot branch in `session/runner.rs` unconditionally called `.completions_api()`, but the OpenAI backend for `gpt-5.2-codex` requires the Responses API (`/responses`). Error: `"model gpt-5.2-codex is not accessible via the /chat/completions endpoint — code: unsupported_api_for_model"`.

OpenAI is progressively migrating models to the Responses API, making this a recurring problem without the abstraction.

### Previous Story Intelligence (Story 4.2 — Agent Session Setup & Chat Loop)

**Story 4.2** established the `SessionRunner` with these key patterns:
- `SessionRunner` struct (L207–220): holds `config`, `secrets`, `copilot_cache`, `shutdown`
- `run()` method (L607–837): branch setup → resolve API key → 3-arm provider match → `run_session()`
- `resume_session()` method (L327–599): WAL recovery → git verification → 3-arm provider match → `run_session()`
- `build_anthropic_agent()` (L840–885): builds `Agent<anthropic::completion::CompletionModel>`
- `build_openai_agent()` (L888–934): builds `Agent<openai::responses_api::ResponsesCompletionModel>`
- `build_copilot_agent()` (L941–990): builds `Agent<openai::completion::CompletionModel>` via `.completions_api()` ← **THE BUG**
- `build_preamble()` (L1000–1002): returns operational preamble via `dev_agent::build_preamble()`
- `create_tools()` (L1005–1041): creates 8 tool instances (7 custom + ask_supervisor)
- `run_session()` (L1722–2366): generic over `<A: Chat + StreamingChat<M, ...>, M: CompletionModel>` — this signature MUST change to accept `&BuiltAgent`
- `streaming_chat()` lives in `session/dev_agent.rs` — generic function used by `run_session()`, `drive_activation_and_recover()`, `summarize_history()`, `context_limit_recovery()`

**Critical insight — `run_session()` generics:** The current signature is:
```
async fn run_session<A, M>(&self, agent: &A, story: &StoryInfo, ...) -> SessionOutcome
where
    A: Chat + StreamingChat<M, M::StreamingResponse>,
    M: CompletionModel + 'static,
    M::StreamingResponse: Clone + Unpin + GetTokenUsage,
```
After refactoring, this becomes `async fn run_session(&self, agent: &BuiltAgent, story: &StoryInfo, ...) -> SessionOutcome` — no generics needed. The `BuiltAgent::stream_chat()` method handles the dispatch internally.

### Previous Story Intelligence (Story 3.2 — LLM Fallback with Project Context)

**Story 3.2** established `ArchitectSession` with a **different tool set**:
- `ArchitectSession` struct (L164–175): holds `agent_file_content`, `provider`, `model`, `api_key`, `project_root`
- `ArchitectSession::new()` (L195–242): reads architect.md, resolves provider/key, stores everything
- `AnswerProvider::ask()` (L354–434): 3-arm match → builds agent with ONLY `ReadFile` tool (not the full 9-tool set)
- `drive_conversation()` (L261–350): generic over `<A: Chat + StreamingChat<M, ...>>` — 3 turns: "CH" → "Load project context" → question
- `streaming_architect_chat()` (L33–74): local streaming function (similar to `streaming_chat` but with different error handling — no shutdown flag, fixed max turns)

**Key difference from SessionRunner/ReviewRunner:** The architect agent uses only 1 tool (`ReadFile`), has its own preamble (the full architect.md content), and uses a separate streaming function. The `AgentFactory::build()` must accept arbitrary tools, not assume the 9-tool set.

### Previous Story Intelligence (Story 5.2 — Automated Code Review Session)

**Story 5.2** established `ReviewRunner` with the same 3-arm pattern:
- `ReviewRunner` struct (L149–158): holds `config`, `secrets`, `analyzer`, `shutdown`
- `run_inner()` (L251–420): resolve API key → 3-arm provider match → `drive_review_session()`
- `create_tools()` (L423–458): creates the same 8 tools as SessionRunner
- `drive_review_session()` (L470–665): generic over `<A: Chat + StreamingChat<M, ...>>` — drives the CR workflow
- Uses `streaming_chat()` from `session/dev_agent.rs` (same function as SessionRunner)

### BuiltAgent Design Pattern (from Architecture Decision 8)

The `BuiltAgent` enum dispatch pattern is documented in Architecture Decision 8:

```
pub enum BuiltAgent {
    Anthropic(Agent<anthropic::CompletionModel>),
    OpenAiResponses(Agent<openai::responses_api::ResponsesCompletionModel>),
    OpenAiCompletions(Agent<openai::completion::CompletionModel>),
}

impl BuiltAgent {
    pub async fn stream_chat(
        &self,
        prompt: impl Into<Message> + Send,
        history: Vec<Message>,
        shutdown: Option<&ShutdownFlag>,
    ) -> Result<String, PromptError> {
        match self {
            Self::Anthropic(a) => streaming_chat(a, prompt, history, shutdown).await,
            Self::OpenAiResponses(a) => streaming_chat(a, prompt, history, shutdown).await,
            Self::OpenAiCompletions(a) => streaming_chat(a, prompt, history, shutdown).await,
        }
    }
}
```

Each variant wraps a concrete rig `Agent<M>` type. The `stream_chat()` method delegates to the existing `streaming_chat()` generic function, which works for any `A: Chat + StreamingChat<M, ...>`.

### Copilot API Format Detection — Hardcoded by Design

```
fn copilot_requires_responses_api(model: &str) -> bool {
    let m = model.to_lowercase();
    m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-") || m.contains("codex")
}
```

- **Anthropic direct** → Messages API (always) — rig handles natively
- **OpenAI direct** → Responses API (always) — rig default with `openai::Client`
- **GitHub Copilot** → proxy, API format depends on model behind:
  - Known OpenAI families → Responses API (`BuiltAgent::OpenAiResponses`)
  - **Everything else → Completions API** (`BuiltAgent::OpenAiCompletions`) — safe fallback
- No `api_format` config — the API format is a deterministic property of the provider, not a user preference
- New OpenAI model families are a one-liner addition to the heuristic

### `streaming_chat()` Location Decision

`streaming_chat()` currently lives in `src/session/dev_agent.rs`. It is a generic function used by:
- `session/runner.rs` — `run_session()`, `drive_activation_and_recover()`, `summarize_history()`, `context_limit_recovery()`
- `review/mod.rs` — `drive_review_session()`

Since `BuiltAgent::stream_chat()` delegates to this function, and `BuiltAgent` lives in `llm/agent_factory.rs`, the function should ideally be accessible from the `llm` module. Options:
1. **Move to `llm/agent_factory.rs`** — keeps everything in one place but may feel odd as it's a generic util
2. **Move to `llm/mod.rs`** as a public function — clean, `llm` module owns all LLM interaction
3. **Keep in `session/dev_agent.rs` and import** — minimal change, cross-module dependency
4. **Re-export from `llm/`** — alias

Recommendation: **Option 3** (keep in place, import). It works today, avoids churn. The `BuiltAgent` in `llm/agent_factory.rs` imports it via `use crate::session::dev_agent::streaming_chat;`.

### `streaming_architect_chat()` in supervisor/architect.rs

`streaming_architect_chat()` (L33–74) is a local function similar to `streaming_chat()` but with differences:
- No `shutdown` parameter (supervisor sessions are short)
- Fixed `MAX_TURNS = 50` constant
- Slightly different error handling

After refactoring, `BuiltAgent::stream_chat()` can replace this IF we make `shutdown` optional (`Option<&ShutdownFlag>` — which it already is in the proposed design). The architect caller passes `None` for shutdown.

### ToolSet Type for AgentFactory::build()

The `AgentFactory::build()` needs to accept tools. However, rig's agent builder uses a variadic `.tool(t)` pattern — there's no `ToolSet` collection type that can be passed generically.

**Solution approach:** The factory builds the rig client and returns a builder, or the factory accepts a closure that configures tools on the builder. Alternatively, the factory can take the pre-built tools tuple and register them inside.

**Practical approach from the architect brief:** The factory takes `preamble` and `tools` where tools is the tuple from `create_tools()`. Since each call site (session, review, supervisor) creates different tool sets, the factory should accept a closure:

```
pub async fn build<F>(&self, role: LlmRole, preamble: &str, configure_tools: F) -> Result<BuiltAgent, ProviderError>
where
    F: FnOnce(AgentBuilder) -> AgentBuilder,
```

This way each caller provides its own tool registration logic while the factory handles provider selection and client construction.

### Duplication Map — What Gets Deleted

| Location | Current Lines | After Refactor |
|----------|--------------|----------------|
| `session/runner.rs` → `run()` match arms | L731–830 (~100 lines) | ~5 lines (single `build()` call) |
| `session/runner.rs` → `resume_session()` match arms | L487–594 (~107 lines) | ~5 lines |
| `session/runner.rs` → `build_anthropic_agent()` | L840–885 (~45 lines) | Deleted |
| `session/runner.rs` → `build_openai_agent()` | L888–934 (~46 lines) | Deleted |
| `session/runner.rs` → `build_copilot_agent()` | L941–990 (~49 lines) | Deleted |
| `session/runner.rs` → `resolve_copilot_session()` | L252–284 (~32 lines) | Deleted (moved to factory) |
| `review/mod.rs` → `run_inner()` match arms | L295–407 (~112 lines) | ~5 lines |
| `supervisor/architect.rs` → `ask()` match arms | L370–430 (~60 lines) | ~5 lines |
| **Total removed** | **~551 lines** | |
| **New `agent_factory.rs`** | | **~200 lines** (enum + factory + heuristic + tests) |
| **Net reduction** | | **~350 lines** |

### Error Type Evolution

`ProviderError` currently lives in `session/provider.rs`. After refactoring:
- `ProviderError` moves to `llm/agent_factory.rs` (or stays in `provider.rs` and is re-exported)
- `ReviewError::ApiKeyMissing` and `ReviewError::UnsupportedProvider` can be removed — the factory returns `ProviderError`, which the caller maps to `ReviewError::ProviderInit`
- `ArchitectSessionError::ApiKeyMissing` and `ArchitectSessionError::UnsupportedProvider` similarly can be reduced — map `ProviderError` to `ArchitectSessionError::ProviderInit`

### Anti-Patterns to Avoid

- ❌ **NO** provider match arms outside `agent_factory.rs` — the whole point is centralization
- ❌ **NO** `api_format` config option — API format is hardcoded per provider/model
- ❌ **NO** defaulting Copilot to Responses API — the safe fallback is Completions API (breaks nothing)
- ❌ **NO** `unwrap()` or `expect()` in production code — only in tests
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** changing the LLM-facing tool APIs — tools are provider-agnostic, untouched
- ❌ **NO** modifying any file under `_bmad/` — daemon is read-only consumer
- ❌ **NO** modifying `sprint-status.yaml` — daemon reads only
- ❌ **NO** changing `SessionOutcome`, `ReviewOutcome`, or `StoryInfo` shapes — these are consumer contracts
- ❌ **NO** making `streaming_chat()` or `streaming_architect_chat()` non-generic — `BuiltAgent::stream_chat()` wraps them, it doesn't replace them
- ❌ **NO** adding a `dyn Chat` or `Box<dyn Chat>` — rig's `Chat` trait is not object-safe, that's why we use enum dispatch
- ❌ **NO** action multiplexing on `BuiltAgent` — each enum variant is a distinct concrete type, not a string-based switch

### Scope Boundaries

**IN SCOPE:**
- `src/llm/agent_factory.rs` — NEW: `BuiltAgent` enum, `AgentFactory` struct, `LlmRole` enum, `copilot_requires_responses_api()`
- `src/llm/mod.rs` — Add `pub mod agent_factory`, re-exports
- `src/session/runner.rs` — Remove 3 `build_*_agent()` methods, remove 2 provider match sites, remove `copilot_cache` field, remove `resolve_copilot_session()`, update `run_session()` signature from generic to `&BuiltAgent`
- `src/session/provider.rs` — Move `resolve_api_key()` and `copilot_headers()` to factory (or keep and re-export)
- `src/review/mod.rs` — Remove provider match in `run_inner()`, update `drive_review_session()` signature
- `src/supervisor/architect.rs` — Remove provider match in `ask()`, update `drive_conversation()` signature, update `ArchitectSession` struct
- `src/pipeline.rs` — Create `AgentFactory` in `new()`, pass to runners
- All associated unit tests across affected files

**OUT OF SCOPE — do NOT implement:**
- Changes to tool implementations (`src/tools/*`) — tools are provider-agnostic
- Changes to `session/analyzer.rs` — response analysis is provider-agnostic
- Changes to `session/state.rs` — WAL stores provider name as string, unchanged
- Changes to `session/branch.rs` — branch management is provider-agnostic
- Changes to `git_provider/` — git hosting, not LLM providers
- Changes to `notifier/` — notification is provider-agnostic
- Changes to `watcher/` — story detection is provider-agnostic
- Changes to `config/` — config structure unchanged (provider/model strings)
- Integration/E2E tests (Epic 7)
- Any changes to BMAD files under `_bmad/`
- Documentation updates (architecture.md, project-context.md) — already done by Architect

### Project Structure Notes

Files modified by this story:

```
src/
├── llm/
│   ├── mod.rs              # MODIFY — add pub mod agent_factory + re-exports
│   ├── agent_factory.rs    # NEW — BuiltAgent enum, AgentFactory, LlmRole, copilot heuristic
│   ├── context.rs          # UNCHANGED
│   └── logging.rs          # UNCHANGED
├── session/
│   ├── runner.rs           # MAJOR REFACTOR — remove 3 build methods, 2 match sites, update run_session() signature
│   ├── provider.rs         # MODIFY/REDUCE — move resolve_api_key + copilot_headers to factory
│   ├── dev_agent.rs        # UNCHANGED (streaming_chat stays here)
│   ├── analyzer.rs         # UNCHANGED
│   ├── branch.rs           # UNCHANGED
│   ├── cleanup.rs          # UNCHANGED
│   ├── escalation.rs       # UNCHANGED
│   ├── state.rs            # UNCHANGED
│   └── mod.rs              # UNCHANGED
├── supervisor/
│   ├── architect.rs        # REFACTOR — remove match, update struct, update drive_conversation()
│   ├── mod.rs              # UNCHANGED
│   ├── rules.rs            # UNCHANGED
│   ├── read_tool.rs        # UNCHANGED
│   └── decisions.rs        # UNCHANGED
├── review/
│   └── mod.rs              # REFACTOR — remove match, update drive_review_session()
├── pipeline.rs             # MODIFY — create AgentFactory, pass to runners
└── main.rs                 # UNCHANGED
```

### References

- [Source: _bmad-output/planning-artifacts/architect-brief-llm-provider-abstraction.md] — Full technical rationale, BuiltAgent design, before/after code, scope of change
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 8] — LLM Provider Abstraction architecture decision
- [Source: _bmad-output/planning-artifacts/architecture.md#L921-931] — Project directory structure with `src/llm/` module
- [Source: _bmad-output/planning-artifacts/architecture.md#L1032-1035] — Data Flow step 4 referencing AgentFactory
- [Source: _bmad-output/planning-artifacts/architecture.md#L1045-1047] — External Integration Points for LLM providers
- [Source: _bmad-output/project-context.md#Multi-Provider LLM Config] — AgentFactory + BuiltAgent documentation
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-02-12.md] — Approved sprint change proposal
- [Source: src/session/runner.rs#L207-220] — SessionRunner struct definition
- [Source: src/session/runner.rs#L607-837] — run() method with provider match arms at L731-830
- [Source: src/session/runner.rs#L327-599] — resume_session() with provider match arms at L487-594
- [Source: src/session/runner.rs#L840-990] — 3 build_*_agent() methods to delete
- [Source: src/session/runner.rs#L252-284] — resolve_copilot_session() to move to factory
- [Source: src/session/runner.rs#L1722-2366] — run_session() with generic signature to change
- [Source: src/session/provider.rs] — ProviderError, resolve_api_key(), copilot_headers(), create_completion_model()
- [Source: src/review/mod.rs#L149-158] — ReviewRunner struct definition
- [Source: src/review/mod.rs#L251-420] — run_inner() with provider match arms at L295-407
- [Source: src/review/mod.rs#L470-665] — drive_review_session() with generic signature
- [Source: src/supervisor/architect.rs#L164-175] — ArchitectSession struct definition
- [Source: src/supervisor/architect.rs#L354-434] — AnswerProvider::ask() with provider match arms at L370-430
- [Source: src/supervisor/architect.rs#L261-350] — drive_conversation() with generic signature
- [Source: src/supervisor/architect.rs#L33-74] — streaming_architect_chat() local streaming function
- [Source: src/supervisor/architect.rs#L178-187] — env_var_for_provider() to remove
- [Source: src/pipeline.rs#L130-175] — StoryPipeline struct and new() constructor
- [Source: src/llm/mod.rs] — Current module: `pub mod context; pub mod logging;`
- [Source: src/session/dev_agent.rs] — streaming_chat() function used by BuiltAgent::stream_chat()

## Dev Agent Record

### Agent Model Used

_To be filled by Dev Agent_

### Debug Log References

_To be filled by Dev Agent_

### Completion Notes List

_To be filled by Dev Agent — one entry per task with key decisions and changes made_

### Change Log

_To be filled by Dev Agent — date + summary of implementation_

### File List

_To be filled by Dev Agent — all files created/modified with brief description_