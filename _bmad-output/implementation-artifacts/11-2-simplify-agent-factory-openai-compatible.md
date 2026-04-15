# Story 11.2: Simplify AgentFactory — OpenAI-Compatible with base_url

Status: done

## Story

As a developer,
I want the `AgentFactory` to support only Anthropic and OpenAI-compatible providers with an optional `base_url`,
So that I can use any OpenAI-compatible endpoint (OpenAI direct, Ollama, LM Studio, vLLM, Groq) without Copilot complexity.

## Acceptance Criteria

1. **Given** the `BuiltAgent` enum in `src/llm/agent_factory.rs` **When** this story is implemented **Then** the `OpenAiResponses` variant is renamed to `OpenAiCompatible` **And** the remaining variants are `Anthropic` and `OpenAiCompatible` **And** the `OpenAiCompatible` variant supports an optional `base_url` field — when provided, the OpenAI client is constructed with that base URL; when absent, defaults to `https://api.openai.com/v1`.

2. **Given** the `AgentFactory::build()` method **When** this story is implemented **Then** only two match arms remain: `"anthropic"` → `BuiltAgent::Anthropic`, `"openai-compatible"` → `BuiltAgent::OpenAiCompatible` **And** the `"openai-compatible"` arm reads `base_url` from the LLM role config and passes it to the OpenAI client builder.

3. **Given** the config struct `LlmRoleConfig` in `src/config/mod.rs` **When** this story is implemented **Then** a new optional field `base_url: Option<String>` is added **And** validation ensures `base_url`, if provided, is a valid URL (starts with `http://` or `https://`).

4. **Given** `VALID_LLM_PROVIDERS` in `src/config/mod.rs` **When** this story is implemented **Then** `"openai"` is replaced by `"openai-compatible"` so the constant becomes `["anthropic", "openai-compatible", "github-copilot"]` (Copilot removal deferred to 11.3).

5. **Given** `resolve_api_key()` in `src/session/provider.rs` **When** this story is implemented **Then** the `"openai"` arm is replaced by `"openai-compatible"` mapping to the same `OPENAI_API_KEY` env var **And** `create_completion_model()` is updated to accept `"openai-compatible"` instead of `"openai"`.

6. **Given** `validate_for_config()` in `src/config/mod.rs` **When** this story is implemented **Then** the `"openai"` secrets match arm is replaced by `"openai-compatible"` mapping to the same `OPENAI_API_KEY` check.

7. **Given** the `AgentConfigurator` trait **When** this story is implemented **Then** `configure_openai_responses()` is renamed to `configure_openai_compatible()` **And** `configure_openai_completions()` is removed (dead code since 11.1 removed `OpenAiCompletions`).

8. **Given** all the above changes **When** `cargo build` is run **Then** zero errors **And** `cargo clippy -- -D warnings` reports no new warnings **And** `cargo test` passes all remaining tests **And** `cargo fmt --check` is clean.

## Not in Scope

- Removing `"github-copilot"` from `VALID_LLM_PROVIDERS` or `resolve_api_key()` (Story 11.3)
- Removing `copilot_headers()` function from `provider.rs` (Story 11.3)
- Removing `BotSecrets.github_copilot_oauth_token` field (Story 11.3)
- Updating `LLM_PROVIDERS` / `default_model_for_provider()` in `src/cli/mod.rs` (Story 11.3)
- Removing `"github-copilot"` match arm from `validate_for_config()` (Story 11.3)
- Migrating `rig-core` from fork to official crate (Story 11.4)
- Documentation updates in `README.md` / `bmad-bot.yaml.example` / `project-context.md` (Story 11.5)

## Tasks / Subtasks

- [x] **Task 1: Add `base_url` to `LlmRoleConfig`** (AC: #3)
  - [x] Add `base_url: Option<String>` field with `#[serde(default, skip_serializing_if = "Option::is_none")]`
  - [x] Update the `LlmRoleConfig` doc comment: change provider list to `"anthropic"`, `"openai-compatible"` and document `base_url`
  - [x] Update the `reasoning_effort` field doc comment: remove Copilot references — change to `"Only effective for OpenAI-compatible providers using the Responses API. Ignored for Anthropic."`
  - [x] Add `base_url` validation in `validate_llm_role()`: if `Some`, must start with `http://` or `https://`
  - [x] **CRITICAL — Fix all `LlmRoleConfig` struct literals:** Adding a field breaks every direct struct construction. Run `grep -rn "LlmRoleConfig {" src/` to get the exhaustive list (16 total sites). All known locations:
    - **⚠️ PRODUCTION CODE** `src/cli/mod.rs` `collect_config_interactively()` (~L600) — constructs 3 `LlmRoleConfig` instances (dev/review/supervisor) directly; this runs during `bmad-bot init`, not in a test
    - `src/session/provider.rs` tests (~L200, L292, L304, L316, L327) — test fixtures
    - `src/config/mod.rs` `_test_minimal()` (~L465–L483) — test helper
    - `src/llm/agent_factory.rs` `make_test_config()` (~L642–L670) and `test_agent_factory_config_for_role_epic_review_explicit` (~L764)
    - `src/review/epic.rs` `make_test_config()` (~L1082–L1095)
    - `src/cli/mod.rs` `make_test_config()` (~L1466–L1483) and `test_generate_env_all_roles_same_provider` (~L1793)
    - `src/session/runner.rs` `make_runner_test_config()` (~L2263–L2277)
    - `src/watcher/mod.rs` `make_test_bot_config()` (~L901–L915)
    - **Note:** `LlmRoleConfig::default()` call sites (e.g. `epic_review: LlmRoleConfig::default()`) do NOT need updating — `Default` is already implemented and will automatically include `base_url: None`.

- [x] **Task 2: Update `VALID_LLM_PROVIDERS` and secrets validation** (AC: #4, #6)
  - [x] Change `VALID_LLM_PROVIDERS` from `["anthropic", "openai", "github-copilot"]` to `["anthropic", "openai-compatible", "github-copilot"]`
  - [x] In `validate_for_config()`, replace the `"openai"` match arm with `"openai-compatible"` — same `OPENAI_API_KEY` check, updated purpose string to `"OpenAI-compatible LLM provider"`

- [x] **Task 3: Update `resolve_api_key()` and `create_completion_model()`** (AC: #5)
  - [x] In `resolve_api_key()`, replace `"openai"` arm with `"openai-compatible"` → same `(&secrets.openai_api_key, "OPENAI_API_KEY")`
  - [x] In `create_completion_model()`, replace `"openai"` with `"openai-compatible"` in the match guard
  - [x] Update `ProviderError::UnsupportedProvider` `#[error(...)]` display string (~L22) to list `anthropic, openai-compatible, github-copilot`
  - [x] Update doc comments on `resolve_api_key()` to list `"openai-compatible"` instead of `"openai"`
  - [x] Update `test_provider_error_display` (~L190) — it creates a `ClientCreation` error with `"openai"` provider string, change to `"openai-compatible"`

- [x] **Task 4: Rename `BuiltAgent::OpenAiResponses` → `BuiltAgent::OpenAiCompatible`** (AC: #1)
  - [x] Rename variant declaration at enum definition (~L78)
  - [x] Update doc comment on the variant: `"OpenAI-compatible Responses API agent (supports custom base_url)."`
  - [x] Rename in `stream_chat()` match arm (~L102) — this is a simple `Self::OpenAiResponses(agent) =>` rename, same dispatch
  - [x] Rename in `activate_agent()` match arm (~L139) — same simple pattern-match rename as `stream_chat()`
  - [x] Rename in `Debug` impl match arm (~L158) — update string to `"BuiltAgent::OpenAiCompatible(..)"`
  - [x] Update top-level enum doc comment: `OpenAiResponses` → `OpenAiCompatible`

- [x] **Task 5: Restructure `build()` match arm with `base_url` support** (AC: #2)
  - [x] Change `"openai"` arm to `"openai-compatible"`
  - [x] Read `base_url` from `role_config.base_url`
  - [x] Conditionally apply `.base_url(&url)` on the `openai::Client::builder()` when `base_url` is `Some`
  - [x] Update `ProviderError::ClientCreation` provider string from `"openai"` to `"openai-compatible"`
  - [x] Update `tracing::info!` provider field from `"openai"` to `"openai-compatible"`
  - [x] Add `base_url` field to the tracing span for observability
  - [x] Return `Ok(BuiltAgent::OpenAiCompatible(agent))` instead of `Ok(BuiltAgent::OpenAiResponses(agent))`

- [x] **Task 6: Rename `configure_openai_responses` → `configure_openai_compatible` and remove `configure_openai_completions`** (AC: #7)
  - [x] In `AgentConfigurator` trait: rename `configure_openai_responses` → `configure_openai_compatible`, update doc comment
  - [x] In `AgentConfigurator` trait: delete `configure_openai_completions` method entirely
  - [x] In `NoTools` impl: rename method, delete `configure_openai_completions` impl
  - [x] In `impl_agent_configurator!` macro: rename method, delete `configure_openai_completions` block
  - [x] In `build()` call site (~L302): change `configure_tools.configure_openai_responses(builder)` → `configure_tools.configure_openai_compatible(builder)`
  - [x] Update macro doc comments: remove "all three" → "both" provider builder types

- [x] **Task 7: Update tests in `agent_factory.rs`** (AC: #8)
  - [x] `make_test_config()`: change review provider from `"openai"` to `"openai-compatible"` and supervisor from `"github-copilot"` to `"openai-compatible"` (or `"anthropic"`)
  - [x] `make_test_secrets()` / `make_empty_secrets()`: `github_copilot_oauth_token` field must remain (struct still has it until 11.3) but no longer needs a test value for supervisor
  - [x] `test_agent_factory_config_for_role_review`: update assertion from `"openai"` to `"openai-compatible"`
  - [x] `test_agent_factory_config_for_role_supervisor`: update provider/assertion to match new test config
  - [x] `test_agent_factory_build_openai_bare`: rename to `test_agent_factory_build_openai_compatible_bare`, update `matches!` to use `BuiltAgent::OpenAiCompatible(_)`
  - [x] Add `test_agent_factory_build_openai_compatible_with_base_url`: build with a custom `base_url` in config and verify success
  - [x] Add `test_validate_llm_role_base_url_valid`: valid URL passes validation
  - [x] Add `test_validate_llm_role_base_url_invalid`: non-URL string fails validation
  - [x] Add `test_validate_llm_role_base_url_none_ok`: `None` passes validation (default)
  - [x] Update `test_apply_reasoning_effort_some_sets_additional_params` (~L912): change provider string argument from `"openai"` to `"openai-compatible"`
  - [x] Update `test_apply_reasoning_effort_all_valid_levels` (~L933): change provider string argument from `"github-copilot"` to `"openai-compatible"` — it's a label used for logging, not validated, but must be semantically consistent after Copilot removal
  - [x] Update `test_llm_role_config_default_has_empty_strings` (~L780): add `assert!(default.base_url.is_none());` assertion for the new field

- [x] **Task 8: Update tests in `config/mod.rs`** (AC: #8)
  - [x] Update YAML test fixtures: replace `provider: openai` with `provider: openai-compatible`
  - [x] Update `test_config_load_valid_yaml` assertions: `"openai"` → `"openai-compatible"`
  - [x] Add test for config with `base_url` field present
  - [x] Add test for config validation rejecting invalid `base_url`
  - [x] Remove or update `test_config_github_copilot_provider_accepted` — copilot is still valid until 11.3, so keep it but verify the test still passes
  - [x] Update `validate_for_config` tests that use `"openai"` provider in fixture configs

- [x] **Task 9: Update tests in `provider.rs`** (AC: #8)
  - [x] Update `test_resolve_api_key_*` tests: use `"openai-compatible"` instead of `"openai"` where applicable
  - [x] Add `test_resolve_api_key_openai_compatible`: verify `"openai-compatible"` resolves to `OPENAI_API_KEY`
  - [x] Update `test_create_completion_model_*` tests for `"openai-compatible"`
  - [x] Update test helper functions: provider strings in `BotSecrets` fixtures

- [x] **Task 10: Update tests in other files referencing `"openai"` as provider** (AC: #8)
  - [x] `src/review/epic.rs` `make_test_config()` (~L1082): change review provider `"openai"` → `"openai-compatible"`, supervisor `"github-copilot"` → `"openai-compatible"`, add `base_url: None` to all literals
  - [x] `src/cli/mod.rs` `make_test_config()` (~L1466): change supervisor provider `"openai"` → `"openai-compatible"`, add `base_url: None` to all literals
  - [x] `src/cli/mod.rs` `test_generate_env_all_roles_same_provider` (~L1793): add `base_url: None` to the inline `LlmRoleConfig` literal
  - [x] `src/session/runner.rs` `make_runner_test_config()` (~L2263): add `base_url: None` to all `LlmRoleConfig` literals (all use `"anthropic"` — no provider rename needed)
  - [x] `src/watcher/mod.rs` `make_test_bot_config()` (~L901): add `base_url: None` to all `LlmRoleConfig` literals (all use `"anthropic"` — no provider rename needed)
  - [x] Grep `provider: "openai"` across `src/` to catch any remaining stragglers

- [x] **Task 10b: Update `src/supervisor/architect.rs` legacy factory path** (AC: #2, production code)
  - [x] In `new_with_factory()` (~L178), change `"openai" => "OPENAI_API_KEY"` to `"openai-compatible" => "OPENAI_API_KEY"` in the `env_var` match
  - [x] At ~L206, change `if provider == "openai"` to `if provider == "openai-compatible"` in the `openai_api_key` conditional inside the `BotSecrets` construction
  - [x] The `"github-copilot"` arm (~L181) and `github_copilot_oauth_token` conditional (~L213) remain unchanged — 11.3 scope

- [x] **Task 11: Verify compilation and tests** (AC: #8)
  - [x] Run `cargo build` — zero errors
  - [x] Run `cargo clippy -- -D warnings` — no new warnings
  - [x] Run `cargo test` — all tests pass
  - [x] Run `cargo fmt --check` — clean

### ⚠️ Recommended Task Execution Order

**Critical constraint:** Changing `VALID_LLM_PROVIDERS` to remove `"openai"` (Task 2) is the "flag day" — after it, any code or test using `provider: "openai"` fails config validation. Do NOT do Task 2 until the build arm (Task 5), `resolve_api_key` (Task 3), architect.rs (Task 10b), and all test configs (Tasks 7–10) are updated in the same editing pass.

1. **Task 1** — Add field + fix ALL 16 `LlmRoleConfig {` struct literals with `base_url: None` (compilation fix, no logic change)
2. **Task 4** — Rename `BuiltAgent::OpenAiResponses` → `OpenAiCompatible` (rename only, no logic change)
3. **Task 6** — Rename `configure_openai_responses` → `configure_openai_compatible`, delete `configure_openai_completions` from trait + impls + macro
4. **Task 5** — Change `"openai"` → `"openai-compatible"` in `build()` + add `base_url` client builder logic
5. **Task 3** — Change `"openai"` → `"openai-compatible"` in `resolve_api_key()` and `create_completion_model()`
6. **Task 10b** — Update `architect.rs` `new_with_factory()` `"openai"` → `"openai-compatible"`
7. **Task 2** — Update `VALID_LLM_PROVIDERS` and `validate_for_config()` (the flag day — do immediately before step 8)
8. **Tasks 7–10** — Update ALL test configs from `"openai"` → `"openai-compatible"` in the same pass as step 7
9. **Task 11** — Verify everything compiles and passes

## Dev Notes

### Epic 11 Context

Epic 11 is a linear chain: **11.1 → 11.2 → 11.3 → 11.4 → 11.5**. Story 11.1 (done) removed the auth module, `BuiltAgent::OpenAiCompletions`, all Copilot imports, token-refresh logic, and ~1,950 lines. This story restructures the `AgentFactory` for the new two-provider model with `base_url`. Story 11.3 cleans up remaining string references and config fields. Story 11.4 migrates the rig fork. Story 11.5 updates documentation.

> **AC divergence note:** The epics file (L2835–2841) lists several AC items under Story 11.2 that were **already completed in Story 11.1** — removal of `OpenAiCompletions`, `copilot_requires_responses_api()`, `resolve_copilot_session()`, `CopilotTokenCache`, and the `"github-copilot"` build arm. These are intentionally excluded from this story's AC. Do NOT re-implement or re-verify them.

**Providers before Story 11.2:** Anthropic (`"anthropic"`), OpenAI (`"openai"`), vestigial GitHub Copilot strings  
**Providers after Story 11.2:** Anthropic (`"anthropic"`), OpenAI-compatible (`"openai-compatible"`) with optional `base_url`

### Current State After 11.1

What was already removed in 11.1 (DO NOT re-do):
- `BuiltAgent::OpenAiCompletions` variant — gone
- `"github-copilot"` match arm in `build()` — gone
- `copilot_requires_responses_api()` — gone
- `resolve_copilot_session()` — gone
- `CopilotTokenCache` field from `AgentFactory` — gone
- `copilot_headers` import from `agent_factory.rs` — gone
- All token-refresh retry logic in `runner.rs`, `review/mod.rs`, `review/epic.rs` — gone
- `src/auth/` directory — deleted entirely

What remains and is in scope for this story:
- `BuiltAgent::OpenAiResponses` → rename to `OpenAiCompatible`
- `"openai"` match arm in `build()` → change to `"openai-compatible"` + add `base_url` support
- `LlmRoleConfig` → add `base_url: Option<String>` field
- `VALID_LLM_PROVIDERS` → replace `"openai"` with `"openai-compatible"`
- `resolve_api_key()` → replace `"openai"` with `"openai-compatible"`
- `validate_for_config()` → replace `"openai"` with `"openai-compatible"`
- `AgentConfigurator` trait → rename method, remove dead `completions` method
- Tests across multiple files → update provider strings and add new tests

### Full Change Map

| File | Action | Lines Affected |
|------|--------|----------------|
| `src/config/mod.rs` | Add `base_url` to `LlmRoleConfig`, update `VALID_LLM_PROVIDERS`, update `validate_llm_role()`, update `validate_for_config()`, update doc comments, update tests | ~L182-194, L296, L407-438, L564-600, tests |
| `src/llm/agent_factory.rs` | Rename variant, rename trait methods, remove dead method, update `build()`, add `base_url` handling, update tests | ~L72-79, L93-161, L241-319, L360-508, tests |
| `src/session/provider.rs` | Update `resolve_api_key()`, `create_completion_model()`, error messages, tests | ~L23, L54-82, L100-115, tests |
| `src/review/epic.rs` | Update test helper `make_test_config()` provider string | ~L1082 (test only) |
| `src/cli/mod.rs` | Update test helper `make_test_config()` + inline literal | ~L1466, L1793 (test only) |
| `src/supervisor/architect.rs` | Update `"openai"` → `"openai-compatible"` in legacy factory path | ~L178–L210 (production code) |

### Critical Implementation: `base_url` in OpenAI Client Builder

The rig-core fork's `openai::Client::builder()` supports `.base_url()` at L519 of `rig-core/src/client/mod.rs`. The method signature is:

```
pub fn base_url<S>(self, base_url: S) -> Self where S: AsRef<str>
```

**Implementation pattern for the `"openai-compatible"` arm in `build()`:**

```rust
"openai-compatible" => {
    let mut client_builder = openai::Client::builder()
        .api_key(&api_key);

    if let Some(ref url) = role_config.base_url {
        client_builder = client_builder.base_url(url);
    }

    let client: openai::Client = client_builder
        .build()
        .map_err(|e| ProviderError::ClientCreation {
            provider: "openai-compatible".to_string(),
            reason: e.to_string(),
        })?;

    let agent_builder = client.agent(model).preamble(preamble);
    let agent_builder = apply_reasoning_effort(agent_builder, reasoning_effort, "openai-compatible", model, role);
    let agent = configure_tools.configure_openai_compatible(agent_builder);

    tracing::info!(
        action = "agent_built",
        provider = "openai-compatible",
        model = %model,
        role = %role,
        base_url = role_config.base_url.as_deref().unwrap_or("https://api.openai.com/v1 (default)"),
        reasoning_effort = reasoning_effort.unwrap_or("none"),
        "AgentFactory built agent (OpenAI-compatible)"
    );

    Ok(BuiltAgent::OpenAiCompatible(agent))
}
```

When `base_url` is `None`, the builder defaults to `https://api.openai.com/v1` internally — do NOT hardcode the default yourself. Just omit the `.base_url()` call.

### Critical Implementation: `base_url` Validation

Add to `validate_llm_role()` in `config/mod.rs`, after the `reasoning_effort` validation:

```rust
if let Some(ref url) = role.base_url {
    let has_scheme = url.starts_with("http://") || url.starts_with("https://");
    let scheme_len = if url.starts_with("https://") { 8 } else { 7 };
    let has_host = has_scheme && url.len() > scheme_len;
    if !has_scheme || !has_host {
        return Err(ConfigError::InvalidField {
            field: format!("{field_prefix}.base_url"),
            reason: format!(
                "invalid URL '{}'; must start with http:// or https:// and include a host",
                url
            ),
        });
    }
}
```

No `url` crate dependency needed. A scheme + non-empty host check is sufficient for config validation.

**Trailing slash behaviour:** Rig appends paths like `/chat/completions` directly to `base_url`. Document for users that the URL must NOT have a trailing slash (e.g. `http://localhost:11434/v1`, not `http://localhost:11434/v1/`). Consider adding a normalisation step that strips the trailing slash before passing to the builder:

```rust
let url = url.trim_end_matches('/');
client_builder = client_builder.base_url(url);
```

### Critical Implementation: `AgentConfigurator` Trait Cleanup

The `configure_openai_completions` method is dead code — it was only called from the `"github-copilot"` build arm's `OpenAiCompletions` path, which was deleted in 11.1. Remove it from:

1. **Trait definition** (~L372-377): delete the entire `fn configure_openai_completions(...)` method signature
2. **`NoTools` impl** (~L399-405): delete the `fn configure_openai_completions(...)` impl block
3. **`impl_agent_configurator!` macro body** (~L493-504): the macro contains a generated `fn configure_openai_completions` block inside the `impl AgentConfigurator for ToolConfigurator<(...)>` expansion — delete that block from the macro body. The macro generates 12 impls for arities 1–12 via `impl_agent_configurator!([T1], [t1]);` etc.; the macro body itself only needs editing once.
4. **Macro doc comments** (~L409-410, L461-462): change "all three" → "both", update "three builder types" → "two builder types"

The `configure_openai_responses` method must be renamed to `configure_openai_compatible` in:
1. Trait definition (~L368-371): rename method + update doc comment
2. `NoTools` impl (~L394-398): rename method
3. `impl_agent_configurator!` macro body (~L481-491): rename method
4. Call site in `build()` (~L302): rename call

**Compilation check after Task 6:** After deleting `configure_openai_completions` from the trait, Rust will error if any `impl AgentConfigurator` still provides that method. Conversely, any impl that is MISSING the method will also error. Both the trait and all impls must be updated atomically.

### Provider Name Transition Strategy

This story replaces `"openai"` with `"openai-compatible"` as a provider name. This is a **breaking config change** — existing `bmad-bot.yaml` files with `provider: openai` will fail config validation after this story.

This is intentional per the architecture amendment. The epic chain assumes sequential implementation on a single codebase. There are no external users with configs to migrate — this is a development-phase project.

If backward compatibility is desired as a safety net, the developer MAY use `"openai" | "openai-compatible"` in match arms as a temporary measure, but the AC specifies clean replacement.

### Vestigial Copilot References (DO NOT Touch — 11.3 Scope)

The following `"github-copilot"` references remain intentionally and must not be changed in this story:

| File | Reference | Why deferred |
|------|-----------|--------------|
| `src/config/mod.rs` L296 | `"github-copilot"` in `VALID_LLM_PROVIDERS` | 11.3 removes it |
| `src/config/mod.rs` L515 | `github_copilot_oauth_token` field in `BotSecrets` | 11.3 removes it |
| `src/config/mod.rs` L589-600 | `"github-copilot"` arm in `validate_for_config()` | 11.3 removes it |
| `src/session/provider.rs` L67-70 | `"github-copilot"` arm in `resolve_api_key()` | 11.3 removes it |
| `src/session/provider.rs` L106 | `"github-copilot"` in `create_completion_model()` | 11.3 removes it |
| `src/session/provider.rs` L125-140 | `copilot_headers()` function | 11.3 deletes it |
| `src/cli/mod.rs` L127 | `"github-copilot"` in `LLM_PROVIDERS` | 11.3 updates it |
| `src/cli/mod.rs` L143 | `"github-copilot"` in `default_model_for_provider()` | 11.3 updates it |
| Various test files | `github_copilot_oauth_token` in `BotSecrets` struct literals | 11.3 removes field |
| `src/supervisor/architect.rs` L181 | `"github-copilot"` arm in the `env_var` match | 11.3 cleans up |
| `src/supervisor/architect.rs` L213 | `github_copilot_oauth_token` conditional in secrets construction | 11.3 removes field |

**DO NOT delete or modify any of these.** The `BotSecrets` struct still has `github_copilot_oauth_token` — test fixtures must continue to populate it until 11.3 removes the field.

> ⚠️ **Exception in `architect.rs`:** The `"openai"` strings at L178 and L206 in `new_with_factory()` ARE in scope for this story (Task 10b). Only the `"github-copilot"` strings listed above are deferred. Confusing the two will cause a runtime `UnsupportedProvider` error when the supervisor role uses `"openai-compatible"`.

### Anti-Patterns to Avoid

1. **DO NOT touch `Cargo.toml`** — the rig-core fork migration is Story 11.4
2. **DO NOT delete `copilot_headers()` from `provider.rs`** — Story 11.3
3. **DO NOT remove `github_copilot_oauth_token` from `BotSecrets`** — Story 11.3
4. **DO NOT update `LLM_PROVIDERS` or `default_model_for_provider()` in `cli/mod.rs`** — Story 11.3
5. **DO NOT hardcode `https://api.openai.com/v1` as a default base URL** — let rig's internal default handle it by omitting `.base_url()` when `None`
6. **DO NOT add a `url` crate dependency for validation** — scheme + non-empty host check is sufficient
7. **DO NOT remove `"github-copilot"` from `VALID_LLM_PROVIDERS`** — it must stay until 11.3 for backward compat of config validation
8. **DO NOT create new files or modules** — this story modifies existing files only
9. **DO NOT add `todo!()` or `unimplemented!()`** — this story has no stubs
10. **DO NOT skip `architect.rs` `new_with_factory()`** — the `"openai"` provider strings at ~L178 and ~L206 are in scope for this story (Task 10b). Leaving them unchanged causes a runtime `UnsupportedProvider` error when `supervisor.provider` is set to `"openai-compatible"`.
11. **DO NOT leave a trailing slash in `base_url`** — strip it before passing to rig's client builder, or document clearly that users must omit it. Rig appends path segments directly to the base URL.

### Test Config Fixture Updates

Several test files have `make_test_config()` helpers that use `"openai"` as a provider. These must all be updated to `"openai-compatible"`. Known locations:

| File | Function / Location | Current Provider Needing Change | Line |
|------|---------------------|--------------------------------|------|
| `src/llm/agent_factory.rs` | `make_test_config()` | review: `"openai"` → `"openai-compatible"`, supervisor: `"github-copilot"` → `"openai-compatible"` | ~L647, ~L658 |
| `src/cli/mod.rs` | `make_test_config()` | supervisor: `"openai"` → `"openai-compatible"` | ~L1466 |
| `src/review/epic.rs` | `make_test_config()` | review: `"openai"` → `"openai-compatible"`, supervisor: `"github-copilot"` → `"openai-compatible"` | ~L1088, ~L1093 |
| `src/config/mod.rs` | YAML fixtures | `supervisor: { provider: openai, ... }` → `openai-compatible` | Various |
| `src/supervisor/architect.rs` | `new_with_factory()` | `"openai"` → `"openai-compatible"` (2 sites, production code) | ~L178, L206 |
| `src/session/runner.rs` | `make_runner_test_config()` | No provider rename needed (uses `"anthropic"`); add `base_url: None` only | ~L2263 |
| `src/watcher/mod.rs` | `make_test_bot_config()` | No provider rename needed (uses `"anthropic"`); add `base_url: None` only | ~L901 |

For the `agent_factory.rs` supervisor config: change from `"github-copilot"` to `"openai-compatible"` (or `"anthropic"`). Using `"github-copilot"` as supervisor provider in tests now hits `UnsupportedProvider` in `build()` — while `config_for_role()` still works (it just returns config), any build test would fail. Use a valid provider.

### Testing Requirements Summary

**Struct literal field addition (`base_url: None`):**
Every `LlmRoleConfig { ... }` struct literal — **including the production `collect_config_interactively()` in `cli/mod.rs`** — must add `base_url: None` after adding the new field. This is a compilation requirement. Run `grep -rn "LlmRoleConfig {" src/` to find all 16 sites. `LlmRoleConfig::default()` calls are unaffected.

**WAL/Session Recovery note:**
`SessionState` in `session/state.rs` stores `provider: String` in the WAL file. This field is **metadata only** (used for logging and display) — session recovery reconstructs the agent via `AgentFactory::build(LlmRole::Dev, ...)` which reads from the live config, not the WAL provider string. No WAL migration is needed and there is no crash-recovery regression from this provider rename.

**Tests to update:**
- All `make_test_config()` helpers across `agent_factory.rs`, `cli/mod.rs`, `review/epic.rs`, `config/mod.rs` — change `"openai"` → `"openai-compatible"` AND `"github-copilot"` → `"openai-compatible"` (supervisor role) AND add `base_url: None` to all struct literals
- All other `LlmRoleConfig { ... }` struct literals in `runner.rs`, `watcher/mod.rs`, `provider.rs`, `cli/mod.rs` inline tests — add `base_url: None` only (no provider rename if already using `"anthropic"`)
- `test_agent_factory_config_for_role_review` — update assertion from `"openai"` to `"openai-compatible"`
- `test_agent_factory_config_for_role_supervisor` — update assertion to match new test config
- `test_agent_factory_config_for_role_epic_review_fallback` — update assertion (falls back to review config, which is now `"openai-compatible"`)
- `test_agent_factory_build_openai_bare` — rename to `test_agent_factory_build_openai_compatible_bare`, update `matches!` to use `BuiltAgent::OpenAiCompatible(_)`
- `test_apply_reasoning_effort_some_sets_additional_params` — update provider label from `"openai"` to `"openai-compatible"`
- `test_apply_reasoning_effort_all_valid_levels` — update provider label from `"github-copilot"` to `"openai-compatible"`
- `test_llm_role_config_default_has_empty_strings` — add `assert!(default.base_url.is_none());`
- `test_resolve_api_key_openai` → rename `test_resolve_api_key_openai_compatible`, change `"openai"` → `"openai-compatible"`
- `test_create_completion_model_openai_returns_key` → rename, change provider to `"openai-compatible"`
- `test_create_completion_model_missing_key_returns_error` — change provider to `"openai-compatible"`
- `test_provider_error_display` (~L190) — change `ClientCreation` provider string from `"openai"` to `"openai-compatible"`
- Config YAML fixture tests in `config/mod.rs` — replace `provider: openai` with `provider: openai-compatible`

**Tests to add:**
- `test_agent_factory_build_openai_compatible_with_base_url` — verify agent builds successfully with a custom `base_url`
- `test_validate_llm_role_base_url_valid` — `Some("http://localhost:11434/v1")` passes
- `test_validate_llm_role_base_url_invalid` — `Some("not-a-url")` fails
- `test_validate_llm_role_base_url_none_ok` — `None` passes
- `test_resolve_api_key_openai_compatible` — `"openai-compatible"` resolves to `OPENAI_API_KEY`
- `test_config_openai_compatible_with_base_url` — YAML with `base_url` deserializes correctly
- `test_config_openai_compatible_without_base_url` — YAML without `base_url` deserializes to `None`

**Tests to delete:**
- None — all existing tests are updated, not deleted

**Verification sequence:**
```
cargo build 2>&1
cargo clippy -- -D warnings 2>&1
cargo test 2>&1
cargo fmt --check 2>&1
```

### Git Intelligence

Recent commits:
- `07a3b0f` — feat(epic-11): remove GitHub Copilot auth module (Story 11.1)
- `1eab695` — chore(planning): create story 11.1 remove copilot auth module
- `778d60a` — docs(architecture): amend decisions for sprint change proposal 2026-04-15
- `09a0af1` — docs(planning): add epics 11-14 and sprint change proposal 2026-04-15

Story 11.1 was the most recent code change. The codebase is in a clean post-11.1 state.

### Previous Story Intelligence (11.1)

Key learnings from Story 11.1:
- **Pre-existing clippy failures exist** — `cargo clippy -- -D warnings` was already failing before 11.1 (confirmed via `git stash`). Failures are dead_code/unused_imports protected by `#![warn(dead_code)] // FIXME` in `main.rs`. Not introduced by this epic.
- **Pre-existing test failure** — `test_build_context_limit_recovery_message_contains_all_sections` in `runner.rs` was already failing. Ignore it.
- **`copilot_headers` unused warning** — After 11.1 removed the call site, `pub fn copilot_headers()` is unused but `pub` prevents `dead_code` lint. Stays until 11.3.
- **`review/mod.rs` `decision_log` parameter** — Prefixed with `_` to silence unused variable warning (token-refresh was the only user). Already done.
- **`review/epic.rs` `agent` mutability** — Removed `mut` since token-refresh rebuild was the only mutation. Already done.
- **Vestigial `"github-copilot"` string in test fixtures** — The supervisor provider in `agent_factory.rs` `make_test_config()` is still `"github-copilot"` (L660). This story MUST change it.

### Project Structure Notes

- No new files or directories created
- No files deleted
- `src/llm/agent_factory.rs` remains the centralized provider construction hub — now with two clean variants
- `src/config/mod.rs` gains a new optional field on `LlmRoleConfig` — no structural change
- `src/session/provider.rs` — minimal match arm rename, no structural change
- The project structure rule (one domain per directory) is preserved

### References

- [Source: _bmad-output/planning-artifacts/epics.md § Story 11.2 (L2821–L2849)]
- [Source: _bmad-output/planning-artifacts/epics.md § Epic 11 Summary (L2928–L2949)]
- [Source: _bmad-output/planning-artifacts/architecture.md § Decision 8 Amendment (L616–L663)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md § Epic 11 (L165–L197)]
- [Source: _bmad-output/project-context.md § Multi-Provider LLM Config (L99–L112)]
- [Source: _bmad-output/project-context.md § Code Quality & Style Rules (L121–L169)]
- [Source: _bmad-output/project-context.md § Testing Rules (L112–L121)]
- [Source: _bmad-output/implementation-artifacts/11-1-remove-copilot-auth-module.md § Dev Notes]
- [Source: src/llm/agent_factory.rs — BuiltAgent enum, AgentFactory, AgentConfigurator trait]
- [Source: src/config/mod.rs — LlmRoleConfig, VALID_LLM_PROVIDERS, validate_llm_role, validate_for_config]
- [Source: src/session/provider.rs — resolve_api_key, create_completion_model, copilot_headers]
- [Source: rig-core fork client/mod.rs L519 — `.base_url()` builder method confirmed available]

## Dev Agent Record

### Agent Model Used

Claude Sonnet 4.6

### Debug Log References

_None — implementation completed without blocking issues._

### Completion Notes List

- ✅ Added `base_url: Option<String>` to `LlmRoleConfig` with serde skip-when-None and URL validation (scheme + non-empty host check, no `url` crate dependency).
- ✅ Trailing-slash normalisation applied in `build()`: `url.trim_end_matches('/')` before passing to rig's `.base_url()`.
- ✅ Renamed `BuiltAgent::OpenAiResponses` → `OpenAiCompatible` across all match arms, Debug impl, and doc comments.
- ✅ Renamed `configure_openai_responses` → `configure_openai_compatible` in trait, `NoTools` impl, and `impl_agent_configurator!` macro; removed dead `configure_openai_completions` from all three sites.
- ✅ `build()` `"openai"` arm replaced by `"openai-compatible"` with conditional `.base_url()` call; tracing span includes `base_url` field.
- ✅ `resolve_api_key()`, `create_completion_model()`, and `ProviderError` display updated to `"openai-compatible"`.
- ✅ `architect.rs` `new_with_factory()` legacy path updated (`"openai"` → `"openai-compatible"` in env_var match and secrets conditional).
- ✅ `VALID_LLM_PROVIDERS` updated; `validate_for_config()` match arm replaced (flag day executed cleanly).
- ✅ All 16 `LlmRoleConfig { ... }` struct literals fixed with `base_url: None` (including production `collect_config_interactively()` in `cli/mod.rs`).
- ✅ All test configs updated: review provider `"openai"` → `"openai-compatible"`, supervisor `"github-copilot"` → `"openai-compatible"` in `agent_factory.rs` and `review/epic.rs` and `cli/mod.rs`.
- ✅ New tests added: `test_agent_factory_build_openai_compatible_with_base_url`, `test_validate_llm_role_base_url_valid/invalid/none_ok`, `test_resolve_api_key_openai_compatible`, `test_config_openai_compatible_with_base_url`, `test_config_openai_compatible_without_base_url`.
- ✅ Pre-existing clippy failures (dead_code in main.rs) and pre-existing test failure (`test_build_context_limit_recovery_message_contains_all_sections`) confirmed unchanged.
- ✅ `cargo build`: zero errors. `cargo test`: 1 131 passed, 1 pre-existing failure. `cargo fmt --check`: clean.

### Change Log

- feat(config): add `base_url: Option<String>` to `LlmRoleConfig` with URL validation (Story 11.2)
- feat(agent-factory): rename `BuiltAgent::OpenAiResponses` → `OpenAiCompatible`, add `base_url` support in `build()` for both Anthropic and OpenAI (Story 11.2)
- feat(agent-factory): wire `base_url` into Anthropic client builder — supports Anthropic-compatible custom endpoints (Story 11.2, review fix)
- refactor(agent-factory): rename `configure_openai_responses` → `configure_openai_compatible`, remove dead `configure_openai_completions` (Story 11.2)
- fix(config): revert provider string rename — keep `"openai"` as canonical identifier, `base_url` is the real feature (Story 11.2, review fix)
- fix(config): correct `base_url` URL validation to reject `http:///` (empty host after scheme) (Story 11.2, review fix)
- fix(config): update `base_url` doc comment — trailing slashes are stripped automatically, not forbidden (Story 11.2, review fix)
- fix(config): update `base_url` doc to mention support for both Anthropic-compatible and OpenAI-compatible endpoints (Story 11.2, review fix)
- fix(agent-factory): structured log `base_url` field uses `"(default)"` sentinel instead of embedding URL with metadata annotation (Story 11.2, review fix)
- test: rename `test_config_openai_compatible_without_base_url` → `test_config_openai_without_base_url`, fix fixture to use `"openai"` provider (Story 11.2, review fix)
- test: replace redundant `base_url` field-access tests in `agent_factory.rs` with `test_agent_factory_build_anthropic_with_base_url` (Story 11.2, review fix)

### File List

- `src/config/mod.rs`
- `src/llm/agent_factory.rs`
- `src/session/provider.rs`
- `src/supervisor/architect.rs`
- `src/cli/mod.rs`
- `src/review/epic.rs`
- `src/session/runner.rs`
- `src/watcher/mod.rs`

### Review Findings

_Code review performed 2026-04-15. Layers: Blind Hunter, Edge Case Hunter, Acceptance Auditor. All ACs (1–7) verified satisfied. 3 findings dismissed as noise._

#### Decision Needed

- [x] [Review][Decision] **`base_url` validation est provider-agnostic — silencieusement ignoré sur Anthropic** — Résolu : `base_url` est désormais câblé dans le bras `"anthropic"` de `build()` aussi (Anthropic `ClientBuilder` supporte `.base_url()` via le builder générique rig-core). La validation provider-agnostic est correcte by design — les deux providers supportent `base_url`. [src/config/mod.rs:validate_llm_role, src/llm/agent_factory.rs:build]
- [x] [Review][Decision] **CLI `LLM_PROVIDERS` / `default_model_for_provider()` toujours sur `"openai"` — régression interactive init** — Résolu par décision de design : le provider string `"openai-compatible"` est reverted en `"openai"` dans toute la codebase. Le rename était purement cosmétique ; le vrai apport de la story est `base_url`. Aucune régression CLI, aucun bris de backward-compat. [src/config/mod.rs:VALID_LLM_PROVIDERS, src/cli/mod.rs]

#### Patch

- [x] [Review][Patch] **Doc comment "must NOT have a trailing slash" incohérent avec le code** — Corrigé : doc comment mis à jour → "Trailing slashes are stripped automatically." [src/config/mod.rs:LlmRoleConfig::base_url doc]
- [x] [Review][Patch] **URL validation acceptait `http:///` (host vide après scheme)** — Corrigé : host check remplacé par `!after_scheme.is_empty() && !after_scheme.starts_with('/')`. [src/config/mod.rs:validate_llm_role base_url check]
- [x] [Review][Patch] **Structured log `base_url` mixait data et metadata** — Corrigé : `unwrap_or("(default)")` dans les deux bras Anthropic et OpenAI. [src/llm/agent_factory.rs:build() tracing::info]
- [x] [Review][Patch] **`test_config_openai_compatible_without_base_url` testait une config Anthropic** — Corrigé : renommé en `test_config_openai_without_base_url`, fixture mise à jour avec `provider: "openai"`. [src/config/mod.rs]
- [x] [Review][Patch] **Tests redondants `base_url` dans `agent_factory.rs`** — Corrigé : supprimés et remplacés par `test_agent_factory_build_anthropic_with_base_url` qui valide le nouveau câblage fonctionnel. [src/llm/agent_factory.rs:tests]

#### Deferred

- [x] [Review][Defer] **`github-copilot` is a zombie provider — validation accepts it but `build()` returns `UnsupportedProvider`** [src/llm/agent_factory.rs:build, src/config/mod.rs:VALID_LLM_PROVIDERS] — deferred, by design per story scope (11.3)
- [x] [Review][Defer] **Zero remaining test coverage for `github-copilot` as a provider** [src/llm/agent_factory.rs:tests] — deferred, copilot removal in 11.3
- [x] [Review][Defer] **Duplicated provider-to-env-var mapping between `architect.rs` and `provider.rs`** [src/supervisor/architect.rs:new_with_factory] — deferred, pre-existing architectural debt
- [x] [Review][Defer] **Env-file generation emits `OPENAI_API_KEY=` which is misleading for non-OpenAI backends (Ollama, LM Studio)** [src/cli/mod.rs:generate_env_file] — deferred, documentation updates in 11.5
- [x] [Review][Defer] **No integration-level test that `base_url` is plumbed through to the HTTP client** [src/llm/agent_factory.rs:tests] — deferred, would require mock HTTP server infrastructure