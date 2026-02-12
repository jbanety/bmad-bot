---
type: architect-brief
from: Amelia (Dev Agent)
to: Product Owner
date: '2026-02-12'
subject: 'Architecture Change Request — LLM Provider Abstraction Layer'
related_decision: 'Provider branching in session runner + review runner'
status: ready-for-po
triggered_by: 'Production bug — gpt-5.2-codex via GitHub Copilot proxy rejects /chat/completions endpoint (requires Responses API)'
---

# Architect Brief: LLM Provider Abstraction Layer

## Context

The BMAD Bot daemon supports three LLM providers — Anthropic, OpenAI, and GitHub Copilot — across three roles (dev session, code review, supervisor). Each role builds and runs an agent via rig-core. Because rig's `Chat` trait is not object-safe (associated types, `Self: Sized`), the codebase uses match arms on the provider name to construct concrete agent types.

**Production incident on 2026-02-12** exposed a flaw in the GitHub Copilot branch: the Copilot proxy routes requests to different backends (OpenAI, Anthropic, etc.), and newer OpenAI models like `gpt-5.2-codex` **only support the Responses API** (`/responses`), not the Chat Completions API (`/chat/completions`). The Copilot branch unconditionally uses `.completions_api()`, causing a hard 400 error:

```
model gpt-5.2-codex is not accessible via the /chat/completions endpoint
code: unsupported_api_for_model
```

This is not a one-off bug — OpenAI is migrating models to the Responses API, so more models will require it over time.

## Problem Summary

| Issue | Impact |
|-------|--------|
| Copilot branch always uses Chat Completions API | Newer OpenAI models (gpt-5.2-codex, future models) fail with 400 |
| Provider match arms duplicated in 3+ locations | runner.run(), runner.resume_session(), review.run_inner() each have identical 3-arm matches |
| Adding a provider or API format requires changes in every match site | High maintenance burden, easy to miss a site |
| Agent construction logic mixed with session/review business logic | Violates single-responsibility — runner shouldn't know about Copilot token exchange |
| No central place to handle provider quirks | Retry logic, API format detection, token caching scattered across modules |

### Current Duplication Map

| Location | Match Arms | Lines of Provider-Specific Code |
|----------|------------|-------------------------------|
| `session/runner.rs` → `run()` | anthropic / openai / copilot | ~120 lines |
| `session/runner.rs` → `resume_session()` | anthropic / openai / copilot | ~120 lines |
| `session/runner.rs` → `build_*_agent()` | 3 separate functions | ~150 lines |
| `review/mod.rs` → `run_inner()` | anthropic / openai / copilot | ~120 lines |
| `supervisor/architect.rs` | anthropic / openai / copilot | ~100 lines |
| **Total duplicated** | | **~610 lines** |

## Proposed Change

**Create a `src/llm/agent_factory.rs` module** that centralizes agent construction behind a `BuiltAgent` enum with a unified `stream_chat()` method. Provider selection, API format detection, and Copilot token exchange are handled once, in one place.

### Core Design: `BuiltAgent` Enum

Since rig's `Chat` trait is not object-safe, we use enum dispatch to wrap the concrete agent types:

```rust
pub enum BuiltAgent {
    Anthropic(Agent<anthropic::CompletionModel>),
    OpenAiResponses(Agent<openai::responses_api::ResponsesCompletionModel>),
    OpenAiCompletions(Agent<openai::completion::CompletionModel>),
}

impl BuiltAgent {
    /// Stream a chat message through the built agent, regardless of provider.
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

### Agent Factory

```rust
pub struct AgentFactory { /* config, secrets, copilot_cache */ }

impl AgentFactory {
    /// Build a BuiltAgent for the given role (dev, review, supervisor).
    pub async fn build(
        &self,
        role: LlmRole,           // Dev | Review | Supervisor
        preamble: &str,
        tools: ToolSet,
    ) -> Result<BuiltAgent, ProviderError> {
        let (provider, model) = self.config_for_role(role);
        let api_key = resolve_api_key(provider, &self.secrets)?;

        match provider {
            "anthropic" => { /* build Anthropic agent */ }
            "openai" => { /* build OpenAI Responses API agent */ }
            "github-copilot" => {
                let (token, url) = self.resolve_copilot(api_key).await?;
                if copilot_requires_responses_api(model) {
                    /* build with Responses API client */
                } else {
                    /* fallback: Completions API (safe default for unknown models) */
                }
            }
        }
    }
}
```

### Copilot API Format Detection

```rust
/// Determine whether a model proxied via GitHub Copilot requires the OpenAI Responses API.
///
/// GitHub Copilot is a proxy that routes to multiple backends (OpenAI, Anthropic, etc.).
/// OpenAI models require the Responses API — Chat Completions returns 400 for newer models.
/// All other models (Claude, Mistral, etc.) use Chat Completions through the proxy.
///
/// This is hardcoded, not configurable — the API format is a deterministic property
/// of the provider behind the model, not a user preference.
///
/// Fallback: Chat Completions API (safe default — works for all non-OpenAI models).
fn copilot_requires_responses_api(model: &str) -> bool {
    let m = model.to_lowercase();
    // Known OpenAI model families that require Responses API
    m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-") || m.contains("codex")
}
```

This is hardcoded by design. The API format is a fact about the provider behind the model — OpenAI models require Responses API, everything else uses Chat Completions. The fallback to Completions API is the safe default: unknown models go through Completions, which works for all non-OpenAI backends. The inverse (defaulting to Responses API) would break non-OpenAI models. When OpenAI introduces new model name patterns, add them to the match — it's a one-liner.

### Activation Helper

The `activate_agent()` and `streaming_chat()` functions in `dev_agent.rs` already work generically. The `BuiltAgent` enum delegates to these via match dispatch, so no changes are needed to the streaming infrastructure.

## Before / After — Session Runner

### Before (current)

```rust
// session/runner.rs — run()
let outcome = match provider.as_str() {
    "anthropic" => {
        match self.build_anthropic_agent(story, &api_key, model, slot, log) {
            Ok(agent) => self.run_session(&agent, story, provider, model, ...).await,
            Err(e) => SessionOutcome::Failed { ... },
        }
    }
    "openai" => {
        match self.build_openai_agent(story, &api_key, model, slot, log) {
            Ok(agent) => self.run_session(&agent, story, provider, model, ...).await,
            Err(e) => SessionOutcome::Failed { ... },
        }
    }
    "github-copilot" => {
        let (token, url) = self.resolve_copilot(&api_key).await?;
        match self.build_copilot_agent(story, &token, model, &url, slot, log) {
            Ok(agent) => self.run_session(&agent, story, provider, model, ...).await,
            Err(e) => SessionOutcome::Failed { ... },
        }
    }
    other => SessionOutcome::Failed { error: "Unsupported provider" },
};
```

This pattern is repeated 3× in runner and 1× in review.

### After (proposed)

```rust
// session/runner.rs — run()
let agent = match self.agent_factory.build(LlmRole::Dev, &preamble, tools).await {
    Ok(a) => a,
    Err(e) => return SessionOutcome::Failed { error: e.to_string(), ... },
};
let outcome = self.run_session(&agent, story, ...).await;
```

One line to build. One line to run. No provider branching in session/review code.

## Scope of Change

### New Files

| File | Purpose |
|------|---------|
| `src/llm/agent_factory.rs` | `BuiltAgent` enum, `AgentFactory` struct, `copilot_needs_responses_api()` |

### Modified Files

| File | Change |
|------|--------|
| `src/llm/mod.rs` | Add `pub mod agent_factory` |
| `src/session/runner.rs` | Remove `build_anthropic_agent`, `build_openai_agent`, `build_copilot_agent`; replace match arms in `run()` and `resume_session()` with `AgentFactory::build()` |
| `src/session/dev_agent.rs` | Move `streaming_chat()` to `llm/` or re-export (used by `BuiltAgent::stream_chat`) |
| `src/review/mod.rs` | Remove provider match in `run_inner()`; use `AgentFactory::build()` |
| `src/supervisor/architect.rs` | Same pattern — replace match with factory |
| `src/pipeline.rs` | Pass `AgentFactory` to `StoryPipeline` instead of individual runner configs |

### Not Modified

| File | Reason |
|------|--------|
| `src/session/analyzer.rs` | Response analysis is provider-agnostic |
| `src/session/state.rs` | WAL state stores provider name (string), not agent types |
| `src/git_provider/` | Unrelated — git hosting, not LLM providers |
| `src/tools/` | Tools are provider-agnostic (injected into any agent) |

## Benefits

| Benefit | Detail |
|---------|--------|
| **Fixes gpt-5.2-codex bug** | Copilot branch correctly selects Responses vs Completions API per model |
| **Future-proof** | Known OpenAI model families matched explicitly; unknown models fall back safely to Completions API. Adding a new OpenAI model family is a one-liner in `copilot_requires_responses_api()` |
| **~610 lines of duplication eliminated** | Single match in factory replaces 5 match sites |
| **Adding a provider = 1 file change** | Add variant to `BuiltAgent`, add arm to factory. No changes in runner/review/supervisor |
| **Copilot token caching centralized** | `AgentFactory` owns the `CopilotTokenCache` — no more passing it around |
| **Testable in isolation** | Factory can be unit-tested without running a full session |
| **Cleaner separation of concerns** | Runner handles session logic. Factory handles provider plumbing. |

## Trade-offs

| Trade-off | Mitigation |
|-----------|------------|
| Enum dispatch adds one match per `stream_chat` call | Negligible overhead — one branch per LLM call (which takes seconds) |
| `BuiltAgent` enum must be updated when rig adds providers | Acceptable — rig provider additions are rare and this is already a compile-time concern |
| Model name heuristic for API format detection | Hardcoded by design — API format is a deterministic property of the provider, not a user config. Fallback to Completions API is the safe default. New OpenAI model families are a one-liner addition |
| `run_session` currently generic over `A: Chat + StreamingChat` | Must be updated to accept `&BuiltAgent` directly and use `BuiltAgent::stream_chat()` instead of the generic `streaming_chat()` |

## Dependency Changes

### No new dependencies

- `BuiltAgent` uses existing rig types
- `AgentFactory` uses existing `BotConfig`, `BotSecrets`, `CopilotTokenCache`

### No removed dependencies

- rig-core providers (anthropic, openai) are still used — just constructed in one place

## Suggested Story Breakdown

### Option A: Single Story (recommended — focused refactor)

**Story: Extract LLM provider abstraction layer with BuiltAgent enum**

Tasks:
1. Create `src/llm/agent_factory.rs` with `BuiltAgent` enum and `stream_chat()` dispatch
2. Add `AgentFactory` struct with `build()` method and Copilot API format detection
3. Add `copilot_needs_responses_api()` with model name heuristic
4. Refactor `session/runner.rs` — remove 3 `build_*_agent` methods, replace match arms with factory
5. Refactor `review/mod.rs` — replace provider match with factory
6. Refactor `supervisor/architect.rs` — replace provider match with factory
7. Update `StoryPipeline` to use `AgentFactory`
8. Unit tests for `BuiltAgent::stream_chat()` dispatch, `copilot_needs_responses_api()`, factory error handling
9. Integration verification — `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt`
10. Update `project-context.md` with new module structure

### Option B: 2 Stories (if PO prefers incremental)

**Story 1: BuiltAgent enum + AgentFactory + Copilot fix**
Create the abstraction, fix the Copilot Responses API bug, refactor `session/runner.rs`.

**Story 2: Propagate factory to review + supervisor**
Refactor `review/mod.rs` and `supervisor/architect.rs` to use the factory.

## Architecture Document Impact

- **Module Structure:** Add `src/llm/agent_factory.rs` to the project structure diagram
- **LLM Provider Design:** Document `BuiltAgent` enum dispatch pattern as the canonical way to interact with LLM providers
- **GitHub Copilot:** Document dual API format support (Responses API for OpenAI models, Completions API for others)
- **project-context.md:** Update "rig Agent + Tool Calling" section to reference `AgentFactory` as the entry point

## References

- **Production incident:** `gpt-5.2-codex` via Copilot → 400 `unsupported_api_for_model` (2026-02-12)
- **rig Responses API docs:** `openai::Client` defaults to Responses API; `.completions_api()` switches to Chat Completions
- **Current duplication sites:** `src/session/runner.rs` (L700-820), `src/review/mod.rs` (L290-410), `src/supervisor/architect.rs`
- **Copilot proxy behavior:** Routes to backend provider, supports both Chat Completions and Responses API formats depending on model
- **OpenAI migration trend:** Newer models (gpt-5.2-codex, future releases) are Responses API only — `/chat/completions` returns 400