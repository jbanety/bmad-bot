# Story 11.3: Clean Provider Routing, Config & Secrets

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer configuring BMAD Bot,
I want the provider list, secrets, and config to reflect only Anthropic and OpenAI (with optional `base_url`),
So that there is no residual Copilot configuration anywhere in the codebase.

## Acceptance Criteria

1. **Given** `src/session/provider.rs` **When** this story is implemented **Then** the `"github-copilot"` match arm in `resolve_api_key()` is removed **And** `copilot_headers()` function is deleted entirely **And** `create_completion_model()` no longer references `"github-copilot"` **And** the `ProviderError::UnsupportedProvider` display string no longer lists `github-copilot` **And** the doc comment on `resolve_api_key` no longer references `github-copilot`.

2. **Given** `src/config/mod.rs` **When** this story is implemented **Then** `BotSecrets.github_copilot_oauth_token` field is removed from the struct definition **And** its `load()` env-var line is removed **And** `VALID_LLM_PROVIDERS` is updated to `["anthropic", "openai"]` **And** the `"github-copilot"` match arm in `validate_for_config()` is removed **And** the `LlmRoleConfig` doc comment is updated to list only `"anthropic"` and `"openai"`.

3. **Given** `src/cli/mod.rs` **When** this story is implemented **Then** `LLM_PROVIDERS` is updated to `["anthropic", "openai"]` **And** the `"github-copilot"` arm in `default_model_for_provider()` is removed **And** `default_model_for_provider("openai")` returns `"gpt-4.1"` (updated from the stale `"gpt-4o"` Copilot-era default, as specified in the epics AC) **And** the interactive init flow prompts for an optional `base_url` for **both** `"openai"` and `"anthropic"` providers (since 11.2 wired `base_url` into both client builders) **And** the collected `base_url` is stored in the `LlmRoleConfig` struct literal (trimmed, trailing slash stripped, `None` if empty).

4. **Given** `src/supervisor/architect.rs` **When** this story is implemented **Then** the `"github-copilot"` arm in the `env_var` match inside `new_with_factory()` is removed **And** the `github_copilot_oauth_token` conditional block in the `BotSecrets` construction is removed **And** the `ArchitectSessionError::UnsupportedProvider` display string no longer lists `github-copilot`.

5. **Given** all Copilot-related unit tests in the above modules **When** this story is implemented **Then** those tests are removed and remaining tests are updated to remove the `github_copilot_oauth_token` field from all `BotSecrets` struct literals.

6. **Given** `BotSecrets.github_copilot_oauth_token` is removed **When** `cargo build` is run **Then** the compiler identifies every remaining struct literal that populated this field **And** each is fixed by removing the field **And** the project compiles with zero errors.

7. **Given** all changes are complete **When** verification is run **Then** `cargo build` succeeds with zero errors **And** `cargo test` passes all tests (except the one known pre-existing failure: `test_build_context_limit_recovery_message_contains_all_sections`) **And** `cargo fmt --check` is clean **And** `grep -rn "github.copilot\|copilot_headers\|GITHUB_COPILOT" src/` returns zero results. **Note:** non-Rust files `bmad-bot.yaml.example` (L28) and `README.md` (L257) still reference `"github-copilot"` — these are intentionally deferred to Story 11.5 and are excluded from this grep scope.

## Not in Scope

- Migrating `rig-core` fork to official crate (Story 11.4)
- Documentation updates in `README.md`, `bmad-bot.yaml.example`, `project-context.md` (Story 11.5)
- Adding a `url` crate dependency for `base_url` validation — the scheme + non-empty host check from 11.2 is sufficient
- Touching `Cargo.toml` — no dependency changes in this story
- Modifying `base_url` validation logic in `config/mod.rs` — already implemented correctly in 11.2

## Tasks / Subtasks

- [x] **Task 1: Remove `github_copilot_oauth_token` from `BotSecrets`** (AC: #2, #6)
  - [x] 1.1 Delete the `github_copilot_oauth_token: Option<String>` field and its doc comment from the `BotSecrets` struct definition (`config/mod.rs` ~L542–543)
  - [x] 1.2 Delete the `github_copilot_oauth_token: std::env::var("GITHUB_COPILOT_OAUTH_TOKEN").ok()` line from `BotSecrets::load()` (~L566)
  - [x] 1.3 Run `cargo build` — the compiler will list every `BotSecrets { ... }` struct literal that still populates the removed field. Fix each one (see Exhaustive BotSecrets Literal Inventory below for all 19 known sites)

- [x] **Task 2: Clean `src/config/mod.rs`** (AC: #2)
  - [x] 2.1 Update `VALID_LLM_PROVIDERS` from `["anthropic", "openai", "github-copilot"]` to `["anthropic", "openai"]` (~L304)
  - [x] 2.2 Update `LlmRoleConfig.provider` doc comment — remove `"github-copilot"`, list only `"anthropic"` and `"openai"` (~L183)
  - [x] 2.3 Delete the `"github-copilot"` match arm in `validate_for_config()` (~L609–620). After deletion, any config with `provider: github-copilot` will hit the `_ => {}` wildcard then fail `VALID_LLM_PROVIDERS` validation with `InvalidProvider` — this is the correct behavior
  - [x] 2.4 Delete `test_config_github_copilot_provider_accepted` test (~L1265–1270)
  - [x] 2.5 In `test_config_reasoning_effort_per_role` (~L1379–1410): before changing the `provider: github-copilot` YAML literals to `provider: openai`, verify the test's assertion — if it was specifically testing that reasoning_effort is **ignored** on a Copilot/Completions-API provider, changing to `"openai"` (Responses API, where reasoning_effort is applied) alters the test's intent. If the test only validates that the YAML deserializes without error, the provider change is safe. After confirming, replace both `provider: github-copilot` entries (~L1388, L1392) with `provider: openai`
  - [x] 2.6 Scan for any other YAML fixtures in `config/mod.rs` tests that set `provider: github-copilot` and expect validation to succeed — run `grep -n "github-copilot" src/config/mod.rs` before editing to get the full list. Each such fixture must either be deleted or updated to `provider: openai`
  - [x] 2.7 Remove `github_copilot_oauth_token` from all 8 `BotSecrets` test fixtures (lines ~L854, L872, L891, L1052, L1072, L1093, L1114, L1130)

- [x] **Task 3: Clean `src/session/provider.rs`** (AC: #1)
  - [x] 3.1 Delete the `"github-copilot"` match arm in `resolve_api_key()` (~L60–63)
  - [x] 3.2 Remove `| "github-copilot"` from the match pattern in `create_completion_model()` (~L101) → `"anthropic" | "openai" => {}`
  - [x] 3.3 Delete the entire `copilot_headers()` function (~L113–136). The `use http::{HeaderMap, HeaderValue}` at L122 is function-scoped inside the body — it disappears with the function. There is no module-level `use http::...` import in `provider.rs`; no additional import cleanup is needed
  - [x] 3.4 Update `ProviderError::UnsupportedProvider` display string to `"Supported: anthropic, openai"` (~L22)
  - [x] 3.5 Remove the `"github-copilot"` bullet from the `resolve_api_key` doc comment (~L55)
  - [x] 3.6 Remove `github_copilot_oauth_token` from `secrets_with_all_keys()` and `empty_secrets()` test helpers (~L148, L160)
  - [x] 3.7 Delete `test_resolve_api_key_github_copilot` test (~L234–239)
  - [x] 3.8 Delete `test_create_completion_model_github_copilot_returns_key` test (~L315–326)
  - [x] 3.9 Remove `github_copilot_oauth_token` from `test_resolve_api_key_empty_string_returns_error` fixture (~L261)

- [x] **Task 4: Clean `src/cli/mod.rs`** (AC: #3)
  - [x] 4.1 Update `LLM_PROVIDERS` from `["anthropic", "openai", "github-copilot"]` to `["anthropic", "openai"]` (~L127)
  - [x] 4.2 Delete the `"github-copilot" => "gpt-4o"` arm in `default_model_for_provider()` (~L143)
  - [x] 4.3 Update `"openai" => "gpt-4o"` to `"openai" => "gpt-4.1"` in `default_model_for_provider()` (~L142). This is explicitly specified in the epics AC and updates the stale Copilot-era default to the current recommended OpenAI model
  - [x] 4.4 Add optional `base_url` interactive prompt in the init flow for **both** `"openai"` and `"anthropic"` roles (11.2 wired `base_url` into both client builders). After the model prompt for each role, if provider is `"openai"` or `"anthropic"`, call `prompt_base_url(provider)` — see Critical Implementation section for the extracted helper
  - [x] 4.5 Extract the URL trimming logic into a small private helper `parse_base_url_input(raw: &str) -> Option<String>` that trims whitespace and strips trailing slash. Add unit tests for this helper (see Tests to ADD). Use the helper in the init flow to collect `base_url` per role
  - [x] 4.6 Wire the collected `base_url` into the `LlmRoleConfig` struct literals in the init flow (~L600–617), replacing the hardcoded `base_url: None`

- [x] **Task 5: Clean `src/supervisor/architect.rs`** (AC: #4)
  - [x] 5.1 Delete the `"github-copilot" => "GITHUB_COPILOT_OAUTH_TOKEN"` arm in the `env_var` match (~L181)
  - [x] 5.2 Delete the entire `github_copilot_oauth_token: if provider == "github-copilot" { ... }` block in `BotSecrets` construction (~L212–216)
  - [x] 5.3 After deleting the copilot block, use `.clone()` on `api_key` in both the `anthropic` and `openai` conditionals — this is the simplest approach, avoids field-order fragility, and has negligible cost on a one-time init path. The final struct looks like: `anthropic_api_key: if provider == "anthropic" { Some(api_key.clone()) } else { ... }`, `openai_api_key: if provider == "openai" { Some(api_key.clone()) } else { ... }` — no ownership juggling required
  - [x] 5.4 Update `ArchitectSessionError::UnsupportedProvider` display string — remove `or 'github-copilot'` (~L75)

- [x] **Task 6: Clean remaining test fixtures across other files** (AC: #5, #6)
  - [x] 6.1 `src/llm/agent_factory.rs` — update `make_test_config()`: change supervisor provider from `"github-copilot"` to `"openai"` (~L644); remove `github_copilot_oauth_token` from both `BotSecrets` struct literals (~L678, L689); update `test_agent_factory_config_for_role_supervisor` assertion from `"github-copilot"` to `"openai"` (~L734)
  - [x] 6.2 `src/notifier/mod.rs` — remove `github_copilot_oauth_token: None` from both `BotSecrets` test fixtures (~L906, L937)
  - [x] 6.3 `src/review/epic.rs` — change `provider: "github-copilot"` to `provider: "openai"` in the supervisor fixture of `make_test_config()` (~L1096). **Note:** this file has zero `github_copilot_oauth_token` fields in any `BotSecrets` literal — no struct literal update is needed here, only the provider string
  - [x] 6.4 `src/review/mod.rs` — remove `github_copilot_oauth_token: None` from test `BotSecrets` fixture (~L1002)
  - [x] 6.5 `src/session/runner.rs` — remove `github_copilot_oauth_token` from the `BotSecrets` literal in `make_runner_test_config()` (~L2315)
  - [x] 6.6 `src/session/state.rs` — change the test provider string from `"github-copilot"` to `"openai"` in the `SessionState::new()` call (~L421)

- [x] **Task 7: Verify zero Copilot references remain in `src/`** (AC: #7)
  - [x] 7.1 `grep -rn "github.copilot\|copilot_headers\|GITHUB_COPILOT" src/` — must return zero results. (Non-Rust files `bmad-bot.yaml.example` and `README.md` still contain references; this is expected and deferred to Story 11.5)
  - [x] 7.2 `cargo build` — zero errors
  - [x] 7.3 `cargo test` — all tests pass (except pre-existing `test_build_context_limit_recovery_message_contains_all_sections`)
  - [x] 7.4 `cargo fmt --check` — clean
  - [x] 7.5 `cargo clippy -- -D warnings` — pre-existing warnings from `main.rs` `#![warn(dead_code)] // FIXME` are known; ensure no NEW warnings are introduced

### ⚠️ Recommended Task Execution Order

Execute tasks in this order to leverage the compiler for exhaustive detection:

1. **Task 1** (remove `BotSecrets` field) — `cargo build` will immediately flag every struct literal that needs updating
2. **Task 2** (config/mod.rs) — fix compilation errors in this file + semantic changes
3. **Task 3** (provider.rs) — fix compilation errors + delete dead code
4. **Task 4** (cli/mod.rs) — fix compilation errors + add `base_url` prompt
5. **Task 5** (architect.rs) — fix compilation errors
6. **Task 6** (remaining files) — fix all remaining test fixture compilation errors
7. **Task 7** (verification) — final sweep

## Dev Notes

### Epic 11 Context

Epic 11 is a linear chain: **11.1 → 11.2 → 11.3 → 11.4 → 11.5**. Story 11.1 (done) removed the auth module (~1,950 lines deleted). Story 11.2 (done) restructured the `AgentFactory` for the two-provider model with `base_url` support. This story (11.3) cleans up all remaining vestigial `"github-copilot"` string references, the `BotSecrets` field, and downstream config/routing code. Story 11.4 migrates the rig fork to the official crate. Story 11.5 updates documentation.

**Providers after this story:** Anthropic (`"anthropic"`), OpenAI (`"openai"`) with optional `base_url`.

### Critical Design Divergence: Provider Name is `"openai"`, NOT `"openai-compatible"`

The original epics file specifies `VALID_LLM_PROVIDERS = ["anthropic", "openai-compatible"]` and `default_model_for_provider("openai-compatible") → "gpt-4.1"`. However, during Story 11.2's code review, the provider rename from `"openai"` to `"openai-compatible"` was **reverted** as a design decision:

> *"fix(config): revert provider string rename — keep `"openai"` as canonical identifier, `base_url` is the real feature"*

**The canonical provider name is `"openai"` throughout the codebase.** The `base_url` field is the mechanism for supporting any OpenAI-compatible endpoint (Ollama, LM Studio, vLLM, Groq) — the provider string doesn't change.

### Breaking Config Change Warning

Removing `"github-copilot"` from `VALID_LLM_PROVIDERS` is a **breaking change**: any existing `bmad-bot.yaml` that sets `provider: github-copilot` will fail validation at daemon startup with `InvalidProvider`. This is intentional — Copilot is no longer supported. This is a development-phase project with no external user base. Consider whether the `InvalidProvider` error message is informative enough; if not, add a migration hint in `validate_llm_role()`:

```
// Example improvement (optional but helpful):
// Change the generic "unrecognised provider" error to name Copilot specifically:
// "provider 'github-copilot' was removed; use 'openai' with an optional base_url"
```

This is a nice-to-have, not required — but document it in your completion notes if you implement it.

### Current State After 11.2

What was already done in 11.1 and 11.2 (DO NOT re-do):
- `src/auth/` directory — deleted entirely (11.1)
- `BuiltAgent::OpenAiCompletions` variant — removed (11.1)
- `CopilotTokenCache`, `resolve_copilot_session()`, `copilot_requires_responses_api()` — removed (11.1)
- All token-refresh retry logic in `runner.rs`, `review/mod.rs`, `review/epic.rs` — removed (11.1)
- `copilot-login` CLI subcommand — removed (11.1)
- `BuiltAgent::OpenAiResponses` renamed to `OpenAiCompatible` (11.2)
- `AgentConfigurator` trait cleaned up — `configure_openai_completions` removed, `configure_openai_responses` renamed to `configure_openai_compatible` (11.2)
- `base_url: Option<String>` added to `LlmRoleConfig` with validation (11.2)
- `base_url` wired into **both** Anthropic and OpenAI client builders in `AgentFactory::build()` (11.2)
- `copilot_headers` import removed from `agent_factory.rs` (11.1)

What remains and is in scope for THIS story:
- `BotSecrets.github_copilot_oauth_token` field — **remove from struct + load() + all 19 struct literals**
- `VALID_LLM_PROVIDERS` containing `"github-copilot"` — **remove**
- `"github-copilot"` match arms in `validate_for_config()`, `resolve_api_key()`, `create_completion_model()`, `architect.rs` — **remove**
- `copilot_headers()` function in `provider.rs` — **delete**
- `LLM_PROVIDERS` and `default_model_for_provider()` in `cli/mod.rs` — **clean up + update default model**
- `base_url` interactive prompt in init flow — **add for both providers**
- All `"github-copilot"` string literals in test fixtures across 10 files — **update or remove**

### Full Change Map

| File | Action | Scope |
|------|--------|-------|
| `src/config/mod.rs` | Remove `github_copilot_oauth_token` field/load, update `VALID_LLM_PROVIDERS`, update doc comment, delete `validate_for_config` copilot arm, update 8 test fixtures, delete 1 test, update 1+ tests | Major |
| `src/session/provider.rs` | Delete `copilot_headers()`, remove copilot match arms, update error string, update doc comment, update 3 test fixtures, delete 2 tests | Major |
| `src/cli/mod.rs` | Update `LLM_PROVIDERS`, update `default_model_for_provider`, add `base_url` prompt for both providers, add `parse_base_url_input` helper + tests | Moderate |
| `src/supervisor/architect.rs` | Remove copilot match arm, remove copilot secrets conditional, update error string | Moderate |
| `src/llm/agent_factory.rs` | Update supervisor provider in `make_test_config()`, remove `github_copilot_oauth_token` from 2 test fixtures, update 1 test assertion | Minor (tests only) |
| `src/notifier/mod.rs` | Remove `github_copilot_oauth_token` from 2 test fixtures | Minor (tests only) |
| `src/review/epic.rs` | Change supervisor provider string in test fixture (no BotSecrets field to remove) | Minor (tests only) |
| `src/review/mod.rs` | Remove `github_copilot_oauth_token` from 1 test fixture | Minor (tests only) |
| `src/session/runner.rs` | Remove `github_copilot_oauth_token` from 1 test fixture | Minor (tests only) |
| `src/session/state.rs` | Change test provider string from `"github-copilot"` to `"openai"` | Minor (tests only) |

### Critical Implementation: `base_url` Interactive Prompt in Init Flow

Story 11.2 wired `base_url` into **both** the Anthropic and OpenAI client builders. The init flow must therefore prompt for `base_url` for both providers. Extract a private helper to keep the logic testable:

```rust
/// Parses a raw base_url input from stdin.
/// Returns None if empty, Some(trimmed + trailing-slash-stripped) otherwise.
fn parse_base_url_input(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.trim_end_matches('/').to_string())
    }
}
```

Use it in the per-role init prompt:

```rust
let base_url = if provider == "openai" || provider == "anthropic" {
    print!("  base_url (optional, press Enter for default): ");
    std::io::Write::flush(&mut std::io::stdout())?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    parse_base_url_input(&input)
} else {
    None
};
```

**Key rules:**
- Trailing slash is stripped — rig appends paths like `/chat/completions` directly to `base_url`
- Empty input → `None` (rig uses its internal default for each provider)
- No URL validation here — `validate_llm_role()` in `config/mod.rs` handles it at load time
- The prompt is per-role, not global — different roles may use different endpoints (e.g., supervisor on a local Ollama instance, dev on OpenAI direct)

### Critical Implementation: `architect.rs` Secrets Construction Simplification

After removing the `github_copilot_oauth_token` conditional, the `BotSecrets` construction becomes two-provider clean. Use `.clone()` on `api_key` for both arms — simpler, avoids ownership field-order fragility, and costs nothing on a one-time init path:

```rust
let secrets = Arc::new(crate::config::BotSecrets {
    anthropic_api_key: if provider == "anthropic" {
        Some(api_key.clone())
    } else {
        std::env::var("ANTHROPIC_API_KEY").ok()
    },
    openai_api_key: if provider == "openai" {
        Some(api_key.clone())
    } else {
        std::env::var("OPENAI_API_KEY").ok()
    },
    github_token: std::env::var("GITHUB_TOKEN").ok(),
    gitlab_token: std::env::var("GITLAB_TOKEN").ok(),
    telegram_bot_token: std::env::var("TELEGRAM_BOT_TOKEN").ok(),
});
```

### `copilot_headers()` Deletion — Import Hygiene

**Verified:** the `use http::{HeaderMap, HeaderValue}` at `provider.rs` L122 is a function-scoped `use` statement inside the `copilot_headers()` body. It disappears with the function — no orphaned module-level import remains. No additional import cleanup is needed in `provider.rs`.

### Compilation-Driven Development Strategy

This story benefits from a **compiler-first approach**: removing `github_copilot_oauth_token` from the `BotSecrets` struct definition will cause Rust to error on every `BotSecrets { ... }` struct literal that still populates the field.

**Step 1:** Delete the field from the struct + `load()` → `cargo build`
**Step 2:** Fix every compilation error the compiler reports
**Step 3:** Handle semantic changes (match arms, function deletions, test updates)
**Step 4:** Run `grep -rn "github.copilot\|GITHUB_COPILOT\|copilot_headers" src/` to catch any string literals the compiler can't detect

### Anti-Patterns to Avoid

1. **DO NOT touch `Cargo.toml`** — the rig-core fork migration is Story 11.4
2. **DO NOT rename `"openai"` to `"openai-compatible"`** — the provider rename was reverted in 11.2; the canonical name is `"openai"`
3. **DO NOT add a `url` crate dependency** — the scheme + non-empty host check from 11.2 is sufficient
4. **DO NOT modify `base_url` validation logic in `config/mod.rs`** — it was correctly implemented in 11.2
5. **DO NOT modify `AgentFactory::build()`** — it was fully updated in 11.2; this story only touches config/routing/secrets
6. **DO NOT comment out code** — delete it entirely; Rust's dead_code detection will catch stale references
7. **DO NOT add `todo!()` or `unimplemented!()` stubs** — this story is cleanup + one small feature
8. **DO NOT forget the trailing slash strip** in `parse_base_url_input` — rig appends path segments directly to `base_url`
9. **DO NOT validate `base_url` in the init flow** — config validation at load time handles it
10. **DO NOT leave any `"github-copilot"` or `GITHUB_COPILOT` string anywhere in `src/`** — the final grep must return zero results (non-Rust files are out of scope for this story)
11. **DO NOT remove unrelated tests to fix failures** — the pre-existing failure in `test_build_context_limit_recovery_message_contains_all_sections` is known and unrelated
12. **DO NOT scope the `base_url` init prompt to `"openai"` only** — `base_url` is wired into both Anthropic and OpenAI builders; both providers must offer the prompt

### Exhaustive BotSecrets Literal Inventory

Every `BotSecrets { ... }` struct literal must have its `github_copilot_oauth_token` field removed. **Known locations (19 total):**

| # | File | ~Line | Test/Function |
|---|------|-------|---------------|
| 1 | `config/mod.rs` | L563 | `BotSecrets::load()` (production) |
| 2 | `config/mod.rs` | L854 | `test_secrets_validate_for_config_missing_anthropic_key` |
| 3 | `config/mod.rs` | L872 | `test_secrets_validate_for_config_missing_github_token` |
| 4 | `config/mod.rs` | L891 | `test_secrets_validate_for_config_telegram_not_required_when_disabled` |
| 5 | `config/mod.rs` | L1052 | `test_secrets_struct_construction` |
| 6 | `config/mod.rs` | L1072 | `test_secrets_validate_missing_required_key` |
| 7 | `config/mod.rs` | L1093 | `test_secrets_validate_missing_github_token` |
| 8 | `config/mod.rs` | L1114 | `test_secrets_validate_passes_when_all_present` |
| 9 | `config/mod.rs` | L1130 | `test_secrets_validate_telegram_token_required_when_enabled` |
| 10 | `session/provider.rs` | L148 | `secrets_with_all_keys()` |
| 11 | `session/provider.rs` | L160 | `empty_secrets()` |
| 12 | `session/provider.rs` | L261 | `test_resolve_api_key_empty_string_returns_error` |
| 13 | `llm/agent_factory.rs` | L678 | test fixture (with `Some(...)`) |
| 14 | `llm/agent_factory.rs` | L689 | test fixture (with `None`) |
| 15 | `notifier/mod.rs` | L906 | test fixture |
| 16 | `notifier/mod.rs` | L937 | test fixture |
| 17 | `review/mod.rs` | L1002 | test fixture |
| 18 | `session/runner.rs` | L2315 | `make_runner_test_config()` |
| 19 | `supervisor/architect.rs` | L201 | `new_with_factory()` (production) |

> **Note:** `src/review/epic.rs` does **not** contain any `BotSecrets` struct literal with `github_copilot_oauth_token` — only a provider string needs updating there. Line numbers are approximate; use `cargo build` errors + `grep -rn "github_copilot_oauth_token" src/` to confirm the complete list before editing.

### Test Changes Summary

**Tests to DELETE:**
- `config/mod.rs`: `test_config_github_copilot_provider_accepted`
- `session/provider.rs`: `test_resolve_api_key_github_copilot`, `test_create_completion_model_github_copilot_returns_key`

**Tests to UPDATE (provider string change):**
- `config/mod.rs`: `test_config_reasoning_effort_per_role` — verify test intent first (see Task 2.5), then replace `provider: github-copilot` with `provider: openai` in YAML literal
- `config/mod.rs`: any other YAML fixture with `provider: github-copilot` — run `grep -n "github-copilot" src/config/mod.rs` to find all
- `llm/agent_factory.rs`: `make_test_config()` supervisor provider `"github-copilot"` → `"openai"`; `test_agent_factory_config_for_role_supervisor` assertion updated
- `review/epic.rs`: `make_test_config()` supervisor provider `"github-copilot"` → `"openai"`
- `session/state.rs`: `SessionState::new()` test provider `"github-copilot"` → `"openai"`

**Tests to UPDATE (field removal only):**
- All 19 `BotSecrets` struct literals in the inventory above — remove the `github_copilot_oauth_token` line

**Tests to ADD:**

```rust
// In src/cli/mod.rs, inside #[cfg(test)] mod tests:

#[test]
fn test_parse_base_url_input_empty_returns_none() {
    assert!(parse_base_url_input("").is_none());
    assert!(parse_base_url_input("   ").is_none());
}

#[test]
fn test_parse_base_url_input_strips_trailing_slash() {
    assert_eq!(
        parse_base_url_input("http://localhost:11434/v1/"),
        Some("http://localhost:11434/v1".to_string())
    );
}

#[test]
fn test_parse_base_url_input_trims_whitespace() {
    assert_eq!(
        parse_base_url_input("  https://api.openai.com/v1  "),
        Some("https://api.openai.com/v1".to_string())
    );
}

#[test]
fn test_parse_base_url_input_no_trailing_slash_unchanged() {
    assert_eq!(
        parse_base_url_input("http://localhost:11434/v1"),
        Some("http://localhost:11434/v1".to_string())
    );
}
```

**Verification sequence:**
```
grep -rn "github.copilot\|copilot_headers\|GITHUB_COPILOT" src/   # must be empty
cargo build 2>&1
cargo test 2>&1
cargo fmt --check 2>&1
```

### WAL/Session Recovery Note

`SessionState` in `session/state.rs` stores `provider: String` in the WAL file. This is metadata only (used for logging/display) — session recovery reconstructs the agent via `AgentFactory::build()` which reads from the live config, not the WAL. No WAL migration is needed. The test string change at L421 is cosmetic — it updates the test to use a valid provider name.

### Previous Story Intelligence (11.2)

Key learnings from Story 11.2 review:
- **`base_url` wired into BOTH providers** — Anthropic's `ClientBuilder` supports `base_url` via rig-core's generic builder interface. The init flow must reflect this.
- **Zombie provider eliminated here** — `"github-copilot"` passed config validation (`VALID_LLM_PROVIDERS`) but `build()` returned `UnsupportedProvider`. This story kills the zombie by removing it from the allowlist.
- **Pre-existing clippy failures** — `cargo clippy -- -D warnings` was already failing before Epic 11 (dead_code/unused_imports in `main.rs`). Not introduced by this epic.
- **Pre-existing test failure** — `test_build_context_limit_recovery_message_contains_all_sections` in `runner.rs` was already failing. Ignore it.
- **Deferred: duplicated provider-to-env-var mapping** — `architect.rs` `new_with_factory()` duplicates the `provider → env_var` logic that also lives in `provider.rs` `resolve_api_key()`. Pre-existing architectural debt, not in scope.
- **Deferred: `OPENAI_API_KEY=` in env-file generation** — Misleading label for Ollama/LM Studio users who don't use an OpenAI key. Documentation scope (Story 11.5).

### Git Intelligence

Recent commits:
- `43c1a5a` — feat(epic-11): add base_url support to AgentFactory for both providers (Story 11.2)
- `07a3b0f` — feat(epic-11): remove GitHub Copilot auth module (Story 11.1)
- `1eab695` — chore(planning): create story 11.1 remove copilot auth module
- `778d60a` — docs(architecture): amend decisions for sprint change proposal 2026-04-15

The codebase is in a clean post-11.2 state.

### Project Structure Notes

- No new files or directories are created
- No files are deleted (only code within existing files)
- After this story, zero references to `github-copilot`, `GITHUB_COPILOT`, or `copilot_headers` remain in `src/`
- `BotSecrets` struct shrinks by one field; all serialization/construction becomes simpler
- `cli/mod.rs` gains one private helper function (`parse_base_url_input`) with unit tests

### References

- [Source: _bmad-output/planning-artifacts/epics.md § Story 11.3 (L2866–2898)]
- [Source: _bmad-output/planning-artifacts/epics.md § Epic 11 Summary (L2928–2949)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md § Epic 11 (L165–197)]
- [Source: _bmad-output/implementation-artifacts/11-2-simplify-agent-factory-openai-compatible.md § Vestigial Copilot References (L318–341)]
- [Source: _bmad-output/implementation-artifacts/11-2-simplify-agent-factory-openai-compatible.md § Provider Name Transition Strategy (L309–316)]
- [Source: _bmad-output/implementation-artifacts/11-2-simplify-agent-factory-openai-compatible.md § Review Findings — Deferred (L507–513)]
- [Source: _bmad-output/implementation-artifacts/11-1-remove-copilot-auth-module.md § Not in Scope items deferred to 11.3]
- [Source: _bmad-output/project-context.md § Multi-Provider LLM Config (L99–112)]
- [Source: _bmad-output/project-context.md § Testing Rules (L112–121)]
- [Source: _bmad-output/project-context.md § Code Quality & Style Rules (L121–169)]
- [Source: _bmad-output/planning-artifacts/architecture.md § External Integration Points — LLM Providers]

## Dev Agent Record

### Agent Model Used

Claude Sonnet 4.6 (claude-sonnet-4-5)

### Debug Log References

No debug log entries — implementation proceeded without blockers.

### Completion Notes List

- ✅ **Task 1**: Removed `github_copilot_oauth_token` field from `BotSecrets` struct definition and `load()`. Used compiler-driven approach — `cargo build` flagged all 19 struct literal sites exhaustively.
- ✅ **Task 2**: Cleaned `src/config/mod.rs` — `VALID_LLM_PROVIDERS` now `["anthropic", "openai"]`, doc comment updated, `validate_for_config()` copilot arm deleted, 8 test fixtures updated, `test_config_github_copilot_provider_accepted` deleted, `test_config_reasoning_effort_per_role` updated (test only validates deserialization + validation, safe to change providers to `openai`).
- ✅ **Task 3**: Cleaned `src/session/provider.rs` — `copilot_headers()` function deleted entirely (function-scoped `use http::{...}` disappeared with it, no orphaned imports), `resolve_api_key()` and `create_completion_model()` copilot arms removed, error string updated, 2 tests deleted, 3 fixtures updated.
- ✅ **Task 4**: Cleaned `src/cli/mod.rs` — `LLM_PROVIDERS` updated, `default_model_for_provider("openai")` now returns `"gpt-4.1"` (updated from stale `"gpt-4o"`), `parse_base_url_input` helper extracted with 4 unit tests, `base_url` interactive prompt added per-role for both `"openai"` and `"anthropic"` providers (using `dialoguer::Input` with empty default for consistency with existing pattern), base_url wired into all 3 `LlmRoleConfig` struct literals. The 6-tuple `(review_provider, review_model, review_base_url, supervisor_provider, supervisor_model, supervisor_base_url)` handles both `same_for_all` and separate-provider flows.
- ✅ **Task 5**: Cleaned `src/supervisor/architect.rs` — copilot arm removed from `env_var` match, `github_copilot_oauth_token` conditional block removed from `BotSecrets` construction, `api_key.clone()` used in both `anthropic` and `openai` conditionals (negligible cost on one-time init path), `UnsupportedProvider` error string updated.
- ✅ **Task 6**: Cleaned 6 remaining files — supervisor provider `"github-copilot"` → `"openai"` in `agent_factory.rs` and `review/epic.rs`, `github_copilot_oauth_token` field removed from 7 test fixtures across `agent_factory.rs`, `notifier/mod.rs`, `review/mod.rs`, `session/runner.rs`, test provider string updated in `session/state.rs`.
- ✅ **Task 7**: All verification checks passed — `grep` returns zero results (exit code 1), `cargo build` zero errors, `cargo test` 1131 passed / 1 pre-existing failure, `cargo fmt --check` clean.

### Change Log

- Removed `BotSecrets.github_copilot_oauth_token` field from struct, `load()`, and all 19 struct literals across 8 files (Date: 2026-04-15)
- Updated `VALID_LLM_PROVIDERS` and `LLM_PROVIDERS` to `["anthropic", "openai"]` — Copilot no longer a valid provider (Date: 2026-04-15)
- Deleted `copilot_headers()` function from `session/provider.rs` (Date: 2026-04-15)
- Removed `"github-copilot"` match arms from `resolve_api_key()`, `create_completion_model()`, `validate_for_config()`, `architect.rs` env_var match (Date: 2026-04-15)
- Updated `default_model_for_provider("openai")` from `"gpt-4o"` to `"gpt-4.1"` (Date: 2026-04-15)
- Added `parse_base_url_input()` helper with 4 unit tests in `cli/mod.rs` (Date: 2026-04-15)
- Added per-role `base_url` interactive prompt in `collect_config_interactively()` for both `"openai"` and `"anthropic"` providers (Date: 2026-04-15)
- Updated all `UnsupportedProvider` error display strings in `provider.rs` and `architect.rs` (Date: 2026-04-15)
- Deleted 3 Copilot-specific tests: `test_config_github_copilot_provider_accepted`, `test_resolve_api_key_github_copilot`, `test_create_completion_model_github_copilot_returns_key` (Date: 2026-04-15)

### File List

- `src/config/mod.rs` — removed `github_copilot_oauth_token` field/load/validate arm, updated `VALID_LLM_PROVIDERS` and doc comment, updated 8 test fixtures, deleted 1 test, updated `test_config_reasoning_effort_per_role`
- `src/session/provider.rs` — deleted `copilot_headers()`, removed copilot arms, updated error string and doc comment, updated 3 test fixtures, deleted 2 tests
- `src/cli/mod.rs` — updated `LLM_PROVIDERS`, updated `default_model_for_provider` (removed copilot arm, gpt-4o→gpt-4.1 for openai), added `parse_base_url_input` helper + 4 tests, added per-role `base_url` prompt, wired base_url into BotConfig construction
- `src/supervisor/architect.rs` — removed copilot env_var arm, removed copilot BotSecrets conditional, updated UnsupportedProvider error string
- `src/llm/agent_factory.rs` — changed supervisor provider to `"openai"` in `make_test_config()`, removed `github_copilot_oauth_token` from 2 test secrets fixtures, updated supervisor assertion
- `src/notifier/mod.rs` — removed `github_copilot_oauth_token` from 2 test fixtures
- `src/review/epic.rs` — changed supervisor provider to `"openai"` in `make_test_config()`
- `src/review/mod.rs` — removed `github_copilot_oauth_token` from 1 test fixture
- `src/session/runner.rs` — removed `github_copilot_oauth_token` from `make_test_secrets()`
- `src/session/state.rs` — changed test provider from `"github-copilot"` to `"openai"`
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — status updated: `ready-for-dev` → `review` for `11-3-clean-provider-routing-config-secrets`

### Review Findings

- [x] [Review][Patch] `parse_base_url_input` returns `Some("")` for slash-only input — emptiness check must happen after `trim_end_matches('/')`, not before [src/cli/mod.rs:149-156]
- [x] [Review][Defer] Base-URL collection logic duplicated 3× in `collect_config_interactively()` — extract a shared helper for the dialoguer prompt + parse pattern [src/cli/mod.rs] — deferred, code style not a bug; spec prescribes per-role prompting
- [x] [Review][Defer] `architect.rs` manually constructs `BotSecrets` from `std::env::var` calls, duplicating provider→env_var mapping from `provider.rs` [src/supervisor/architect.rs:197-211] — deferred, pre-existing architectural debt (acknowledged in story spec)