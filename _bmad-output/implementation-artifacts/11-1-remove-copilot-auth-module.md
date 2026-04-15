# Story 11.1: Remove Copilot Auth Module

Status: ready-for-dev

## Story

As a maintainer,
I want all GitHub Copilot authentication code removed from the codebase,
So that the project no longer carries ~1,350 lines of OAuth Device Flow, token exchange, and caching code that is no longer needed.

## Acceptance Criteria

1. **Given** the `src/auth/github_copilot.rs` module exists (~1,350 lines) **When** this story is implemented **Then** the entire `src/auth/` directory is deleted (`mod.rs` and `github_copilot.rs`).

2. **Given** `src/main.rs` declares `mod auth;` **When** this story is implemented **Then** `mod auth;` is removed and the `copilot_login` branch in the `Init` match arm is removed.

3. **Given** multiple modules import from `crate::auth::github_copilot` **When** this story is implemented **Then** all references to `CopilotTokenCache`, `CopilotHttpClient`, `ReqwestCopilotHttpClient`, `exchange_copilot_token()`, `derive_base_url_from_token()`, `run_device_flow()`, `request_device_code()`, `poll_for_access_token()` are removed from the entire codebase.

4. **Given** `src/cli/mod.rs` contains the `copilot-login` subcommand **When** this story is implemented **Then** the `copilot-login` subcommand is removed from the clap CLI definition, the `run_copilot_login()` function is deleted, and the interactive Copilot Device Flow trigger during `bmad-bot init` is removed.

5. **Given** `src/llm/agent_factory.rs` owns a `CopilotTokenCache` and has Copilot-specific build logic **When** this story is implemented **Then** the `copilot_cache` field, `resolve_copilot_session()` method, `copilot_requires_responses_api()` function, the `"github-copilot"` match arm in `build()`, `BuiltAgent::OpenAiCompletions` variant, and the `copilot_headers` import are all deleted.

6. **Given** `src/session/runner.rs` has Copilot token-refresh retry logic **When** this story is implemented **Then** `is_token_expired_error()`, `MAX_TOKEN_REFRESHES`, the `token_refreshes` counter, and all 6 retry branches (confirmed via `grep`) are removed.

7. **Given** `src/review/mod.rs` and `src/review/epic.rs` have their own local Copilot token-refresh logic **When** this story is implemented **Then** `is_token_expired_error()`, `MAX_TOKEN_REFRESHES`, the token refresh branches, and all related tests are removed from both review files.

8. **Given** all the above removals **When** `cargo build` is run **Then** the project compiles with zero errors **And** `cargo clippy -- -D warnings` reports zero warnings **And** `cargo test` passes all remaining tests.

## Not in Scope

- Adding `base_url` support to `OpenAiCompatible` provider (Story 11.2)
- Restructuring the `BuiltAgent` enum variants for the two-provider model beyond removing `OpenAiCompletions` (Story 11.2)
- Updating `VALID_LLM_PROVIDERS` in `src/config/mod.rs` (Story 11.3)
- Updating `LLM_PROVIDERS` / `default_model_for_provider` in `src/cli/mod.rs` (Story 11.3)
- Removing `BotSecrets.github_copilot_oauth_token` field (Story 11.3)
- Removing the `"github-copilot"` string arms in `src/session/provider.rs` (`resolve_api_key`, `create_completion_model`) (Story 11.3)
- Removing `copilot_headers()` function body from `src/session/provider.rs` — it is `pub` and won't trigger dead_code; its caller in `agent_factory.rs` is removed here, but the function itself is cleaned in Story 11.3
- Migrating `rig-core` from fork to official crate (Story 11.4)
- Documentation updates in `README.md` / `bmad-bot.yaml.example` (Story 11.5)

> **Compilation boundary:** Anything that *imports from* `crate::auth::github_copilot` must be fixed here for the code to compile. String literals like `"github-copilot"` in match arms, `BotSecrets.github_copilot_oauth_token` field references, and `VALID_LLM_PROVIDERS` containing `"github-copilot"` are not compilation errors and are deferred to 11.3.

## Tasks / Subtasks

- [ ] **Task 1: Delete `src/auth/` directory** (AC: #1)
  - [ ] Delete `src/auth/github_copilot.rs` (~1,351 lines including ~680 lines of tests)
  - [ ] Delete `src/auth/mod.rs` (3 lines: doc comment + `pub mod github_copilot;`)

- [ ] **Task 2: Clean `src/main.rs`** (AC: #2)
  - [ ] Remove `mod auth;` declaration
  - [ ] Remove `copilot_login: bool` from `Commands::Init` destructure
  - [ ] Remove the `if copilot_login { cli::run_copilot_login().await?; }` branch
  - [ ] The `Init` match arm simplifies to calling `cli::run_init().await?` directly

- [ ] **Task 3: Clean `src/cli/mod.rs`** (AC: #4)
  - [ ] Remove `use crate::auth::github_copilot::{self, ReqwestCopilotHttpClient};` import
  - [ ] Remove `copilot_login: bool` field from `Commands::Init` variant definition (the `#[arg(long)]` block)
  - [ ] Remove entire `copilot_oauth_token` block in `run_init()` — the `let copilot_oauth_token = { ... }` block that checks `uses_copilot` and runs the Device Flow
  - [ ] Remove the `println!` hint pointing users to `bmad-bot init --copilot-login`
  - [ ] Delete the `run_copilot_login()` function entirely
  - [ ] Remove `copilot_oauth_token` parameter from `generate_env_file()` signature — new signature is `fn generate_env_file(config: &BotConfig) -> ...`
  - [ ] Remove `"github-copilot"` block inside `generate_env_file()` that writes `GITHUB_COPILOT_OAUTH_TOKEN`
  - [ ] Update the **one production call site** of `generate_env_file` in `run_init()` (line ~321) — remove the `copilot_oauth_token.as_deref()` argument
  - [ ] Update **all ~12 test call sites** of `generate_env_file` — remove the second argument from every call (they all pass `None` or a token literal). Test call sites are at approximately lines 1825, 1832, 1839, 1846, 1853, 1860, 1869, 1877, 1887, 1909, 1926, 1949
  - [ ] Delete Copilot-specific tests: `test_generate_env_excludes_github_copilot_token`, `test_generate_env_copilot_token_prefilled`, `test_generate_env_copilot_token_empty_when_none`, `test_default_model_for_provider_github_copilot`

- [ ] **Task 4: Clean `src/llm/agent_factory.rs`** (AC: #5)
  - [ ] Remove `use crate::auth::github_copilot::{CopilotHttpClient, CopilotTokenCache, ReqwestCopilotHttpClient};` import
  - [ ] Remove `copilot_headers` from the provider import — change `use crate::session::provider::{ProviderError, copilot_headers};` to `use crate::session::provider::ProviderError;`
  - [ ] Remove `copilot_cache: std::sync::Mutex<CopilotTokenCache>` field from the `AgentFactory` struct
  - [ ] Remove `copilot_cache: std::sync::Mutex::new(CopilotTokenCache::new())` from `AgentFactory::new()`
  - [ ] Delete the entire `"github-copilot" => { ... }` match arm in `AgentFactory::build()` (the arm that calls `resolve_copilot_session`, constructs the OpenAI client with `copilot_headers()`, and returns `BuiltAgent::OpenAiCompletions`)
  - [ ] Delete `resolve_copilot_session()` method
  - [ ] Delete `copilot_requires_responses_api()` function and all its associated tests (~6 test functions)
  - [ ] **Remove `BuiltAgent::OpenAiCompletions` variant** — it is confirmed Copilot-only (its only constructor is inside the `"github-copilot"` build arm just deleted; with `#![deny(dead_code)]` the compiler will reject it). Remove the variant definition, its `stream_chat` match arm, and its `Debug` match arm
  - [ ] Update module-level doc comment to remove references to "Copilot token exchange" and "Copilot API Format Detection"
  - [ ] Update `BuiltAgent` enum doc comment to remove the `OpenAiCompletions` variant description

- [ ] **Task 5: Clean `src/session/runner.rs`** (AC: #6)
  - [ ] Delete `is_token_expired_error()` function
  - [ ] Delete `MAX_TOKEN_REFRESHES` constant
  - [ ] Remove `token_refreshes` counter variable declaration (at the top of `run_session`)
  - [ ] Remove all **6 confirmed** token-refresh retry branches — run `grep -n "is_token_expired_error" src/session/runner.rs` to locate them all before editing; confirmed at approximately lines 1344, 1474, 1871, 2049, 2205, 2427
  - [ ] For each branch: remove only the `if is_token_expired_error(...) && token_refreshes < MAX_TOKEN_REFRESHES { ... }` block; leave the surrounding error handling intact
  - [ ] Update `SessionRunner` struct doc comment — remove "Copilot token cache" reference
  - [ ] Update `resume_session` inline comment — remove "Copilot token exchange" reference
  - [ ] Delete tests: `test_is_token_expired_error_exact_copilot_message`, `test_is_token_expired_error_simple`, `test_is_token_expired_error_false_for_other_auth_errors`, `test_is_token_expired_error_false_for_transient_errors`, `test_is_token_expired_error_false_for_context_limit`

- [ ] **Task 6: Clean `src/review/mod.rs`** (AC: #7)
  - [ ] Delete `is_token_expired_error()` function (defined locally at ~line 69, independent from the auth module)
  - [ ] Delete `MAX_TOKEN_REFRESHES` constant (~line 49) and its doc comment referencing `crate::session::runner::MAX_TOKEN_REFRESHES`
  - [ ] Remove the token-refresh retry branch in the review chat loop (~line 842): the `if is_token_expired_error(&error_str) && token_refreshes < MAX_TOKEN_REFRESHES { ... }` block
  - [ ] Remove `token_refreshes` counter variable declaration (~line 645)
  - [ ] Remove the broken intra-doc link `[`crate::auth::CopilotTokenCache`]` in the `is_token_expired_error` doc comment (the type no longer exists)
  - [ ] Delete tests: `test_is_token_expired_error_exact_copilot_message`, `test_is_token_expired_error_simple`, `test_is_token_expired_error_false_for_other_auth_errors`, `test_is_token_expired_error_false_for_transient_errors`, `test_max_token_refreshes_is_reasonable`

- [ ] **Task 7: Clean `src/review/epic.rs`** (AC: #7)
  - [ ] Remove `is_token_expired_error` and `MAX_TOKEN_REFRESHES` from the `use super::{ ... }` import at line 40
  - [ ] Remove the token-refresh retry branch at ~line 354: `if is_token_expired_error(&err_str) && token_refreshes < MAX_TOKEN_REFRESHES { ... }` and the subsequent agent-rebuild block
  - [ ] Remove `token_refreshes` counter variable declaration (~line 304) and all `token_refreshes += 1` / `token_refreshes as u32` usages
  - [ ] Update doc comment at ~line 57 referencing `MAX_TOKEN_REFRESHES` separately from session retries

- [ ] **Task 8: Update doc comments in adjacent files** (AC: #8 — prevents clippy doc-link warnings)
  - [ ] `src/session/agent.rs` — update doc comment at ~line 267 that references "All providers (Anthropic, OpenAI, GitHub Copilot)"
  - [ ] `src/pipeline.rs` — update doc comment at ~line 188 that says "owns secrets + Copilot token cache"; update doc comments at ~lines 2222, 2226, 2254, 2259 that reference "Copilot token refresh issue" (these describe the `is_infra_error`/`is_auth_error` classification logic — rephrase as generic token expiry)
  - [ ] `src/supervisor/architect.rs` — update doc comment at ~line 144 that references "Copilot token"

- [ ] **Task 9: Verify compilation and tests** (AC: #8)
  - [ ] Run `grep -rn "crate::auth" src/` — must return zero results
  - [ ] Run `grep -rn "CopilotTokenCache\|copilot_headers\|run_device_flow\|request_device_code\|poll_for_access_token\|derive_base_url_from_token\|exchange_copilot_token" src/` — must return zero results
  - [ ] Run `cargo build` — zero errors
  - [ ] Run `cargo clippy -- -D warnings` — zero warnings
  - [ ] Run `cargo test` — all remaining tests pass; if unexpected failures appear, see Rollback Guidance below
  - [ ] Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Epic 11 Context

Epic 11 is a linear chain: **11.1 → 11.2 → 11.3 → 11.4 → 11.5**. This story is the foundational deletion — it removes the auth module and everything that directly or logically depends on it. Subsequent stories restructure the `AgentFactory` for the new two-provider model with `base_url` (11.2), clean provider routing and config strings (11.3), migrate the rig fork (11.4), and update end-user docs (11.5).

**Providers before Epic 11:** Anthropic, OpenAI, GitHub Copilot.  
**Providers after Epic 11:** Anthropic, OpenAI-compatible (with optional `base_url`).

### Full Blast Radius

| File | Action | Compilation blocker? |
|------|--------|----------------------|
| `src/auth/github_copilot.rs` | **DELETE** (~1,351 lines) | Yes — primary target |
| `src/auth/mod.rs` | **DELETE** (3 lines) | Yes — module declaration |
| `src/main.rs` | Remove `mod auth;` + copilot branch | Yes |
| `src/cli/mod.rs` | Remove import, flag, functions, env gen, ~12 test call sites | Yes |
| `src/llm/agent_factory.rs` | Remove imports, cache field, build arm, `copilot_headers` import, `OpenAiCompletions`, functions, tests | Yes |
| `src/session/runner.rs` | Remove `is_token_expired_error`, `MAX_TOKEN_REFRESHES`, 6 retry branches, tests | Clippy `-D warnings` |
| `src/review/mod.rs` | Remove local `is_token_expired_error`, `MAX_TOKEN_REFRESHES`, retry branch, broken doc link, tests | Clippy `-D warnings` |
| `src/review/epic.rs` | Remove import of removed symbols, retry branch, counter | Clippy `-D warnings` |
| `src/session/agent.rs` | Update doc comment | No |
| `src/pipeline.rs` | Update doc comments | No |
| `src/supervisor/architect.rs` | Update doc comment | No |

**Estimated total removal: ~1,900 lines.**

Files confirmed clean (no compilation issues for 11.1):
- `src/session/provider.rs` — `copilot_headers()` is `pub fn` so no `dead_code` lint; string arms (`"github-copilot"`) and `BotSecrets` field usage are not compilation errors → cleaned in 11.3
- `src/session/state.rs` — `"github-copilot"` is test string data only
- `src/supervisor/architect.rs` — `"github-copilot"` string match arms and `github_copilot_oauth_token` field reference stay (field not removed until 11.3)
- `src/notifier/mod.rs` — `github_copilot_oauth_token: None` in test structs stays (field not removed until 11.3)
- `src/pipeline.rs` — Copilot references are doc comments and test string literals in error classification tests, not compilation issues

### Critical Implementation Order

Follow this sequence to minimise intermediate compilation errors:

1. **Delete `src/auth/`** — the compiler now shows every dependent that needs fixing
2. **Remove `mod auth;` from `src/main.rs`**
3. **Fix `src/llm/agent_factory.rs`** — remove both imports, cache field, build arm, helper functions, `OpenAiCompletions` variant, tests
4. **Fix `src/cli/mod.rs`** — remove import, command, init flow, env gen signature, all call sites, tests
5. **Fix `src/session/runner.rs`** — remove token refresh logic and tests
6. **Fix `src/review/mod.rs`** — remove local token refresh machinery and tests
7. **Fix `src/review/epic.rs`** — remove import of removed symbols and retry branch
8. **Update doc comments** in `agent.rs`, `pipeline.rs`, `architect.rs`
9. **Run `cargo build`** — iterate on any remaining errors
10. **Run `cargo clippy -- -D warnings`** — catch unused imports, dead code
11. **Run `cargo test`** — verify nothing broke
12. **Run `cargo fmt`** — formatting pass

### `BuiltAgent::OpenAiCompletions` — Mandatory Removal

This variant is **confirmed Copilot-only**. Grep confirms its only constructor is at `agent_factory.rs:439` inside the `"github-copilot"` build arm:

```bmad-bot/src/llm/agent_factory.rs#L439
Ok(BuiltAgent::OpenAiCompletions(agent))
```

Once the `"github-copilot"` arm is deleted, `OpenAiCompletions` is never constructed. Rust's `#![deny(dead_code)]` fires on unconstructed enum variants → compilation error. **Remove it unconditionally.** Locations to clean:

- The variant declaration (enum definition)
- The `stream_chat` match arm handling `OpenAiCompletions`
- The `Debug` impl match arm for `OpenAiCompletions`
- The doc comment describing the variant

### `copilot_headers` — Import Must Go, Function Can Stay

`copilot_headers()` is defined as `pub fn` in `src/session/provider.rs`. It is:
- **Imported** in `agent_factory.rs` at the top: `use crate::session::provider::{ProviderError, copilot_headers};`
- **Called** inside the `"github-copilot"` build arm (two call sites: one for the Responses API path, one for the Completions API path)

After deleting the build arm, the `copilot_headers` name becomes an unused import → `unused_imports` lint → clippy `-D warnings` failure.

**Fix:** Change the import in `agent_factory.rs` from:
```bmad-bot/src/llm/agent_factory.rs#L23
use crate::session::provider::{ProviderError, copilot_headers};
```
to:
```/dev/null/agent_factory_fixed.rs#L1
use crate::session::provider::ProviderError;
```

The `copilot_headers()` function body in `src/session/provider.rs` is `pub` and will not trigger `dead_code`. It is deferred to Story 11.3 which does the full provider.rs cleanup.

### Token Refresh Removal Pattern (`runner.rs` and `review/`)

The token refresh logic follows a consistent pattern. For each occurrence, remove the entire `if is_token_expired_error(...)` block. The surrounding retry/error-handling code (transient error retries, backoff, etc.) must remain untouched.

**Confirmed call sites in `src/session/runner.rs`** (verified via grep):
- `is_token_expired_error` at lines 1344, 1474, 1871, 2049, 2205, 2427 — exactly 6

**Call sites in `src/review/`** (verified via grep):
- `src/review/mod.rs` line 842 — 1 call site
- `src/review/epic.rs` line 354 — 1 call site (imports function from `super`)

Before editing `runner.rs`, run `grep -n "is_token_expired_error" src/session/runner.rs` to confirm all 6 locations. The line numbers above are accurate as of the last commit but will shift as you edit; always grep to confirm before each removal.

The `token_refreshes` counter in `runner.rs` is declared once at the top of `run_session` and incremented inside each retry branch. Remove the declaration once all 6 branches are gone.

### `generate_env_file` — Signature Change and All Call Sites

**Current signature:**
```bmad-bot/src/cli/mod.rs#L782-784
fn generate_env_file(
    config: &BotConfig,
    copilot_oauth_token: Option<&str>,
```

**New signature:**
```/dev/null/cli_mod_new.rs#L1-2
fn generate_env_file(
    config: &BotConfig,
```

**Production call site (1):** In `run_init()` at approximately line 321:
```bmad-bot/src/cli/mod.rs#L321
let env_content = generate_env_file(&config, copilot_oauth_token.as_deref())?;
```
Remove the second argument.

**Test call sites (~12):** All calls in the `#[cfg(test)]` block at approximately lines 1825, 1832, 1839, 1846, 1853, 1860, 1869, 1877, 1887, 1909, 1926, 1949. Every one passes either `None` or `Some("gho_test_token_123")` as the second argument. Remove the second argument from all of them. Run `grep -n "generate_env_file" src/cli/mod.rs` to get the exact list before editing.

### `review/mod.rs` — Local Token Refresh Machinery

`src/review/mod.rs` has its **own local definitions** — it does not import from `crate::auth`. Deleting `src/auth/` will not cause a compilation error here. However, the code is Copilot-specific dead logic:

- `MAX_TOKEN_REFRESHES` constant (~line 49) with doc referencing `crate::session::runner::MAX_TOKEN_REFRESHES`
- `is_token_expired_error()` function (~line 69) with a doc comment containing broken intra-doc link `[crate::auth::CopilotTokenCache]`
- Token refresh branch in the review chat loop (~line 842)
- `token_refreshes` counter (~line 645)

Tests to delete in `review/mod.rs`: `test_is_token_expired_error_exact_copilot_message`, `test_is_token_expired_error_simple`, `test_is_token_expired_error_false_for_other_auth_errors`, `test_is_token_expired_error_false_for_transient_errors`, `test_max_token_refreshes_is_reasonable`.

`src/review/epic.rs` imports `is_token_expired_error` and `MAX_TOKEN_REFRESHES` from `super` (review/mod.rs) at line 40. After removing them from mod.rs, the import in epic.rs becomes a compilation error. Fix: remove those two names from the `use super::{ ... }` import, then remove the retry branch at ~line 354 and the `token_refreshes` counter at ~line 304.

### Anti-Patterns to Avoid

1. **DO NOT comment out code** — delete it entirely. `#![deny(dead_code)]` enforces this.
2. **DO NOT add `todo!()` or `unimplemented!()` stubs** — this story is pure deletion, no new features.
3. **DO NOT touch `Cargo.toml`** — the rig-core fork migration is Story 11.4. The fork compiles fine without the auth module.
4. **DO NOT update `VALID_LLM_PROVIDERS` or `LLM_PROVIDERS` constants** — that is Story 11.3 scope. Do not replace `"github-copilot"` with `"openai-compatible"` here; that change would make the `init` flow offer a provider name that config validation (unchanged in 11.1) would reject, creating a regression.
5. **DO NOT update `BotSecrets.github_copilot_oauth_token` field** — Story 11.3.
6. **DO NOT update string match arms in `src/session/provider.rs`** — Story 11.3.
7. **DO NOT delete `copilot_headers()` function from `src/session/provider.rs`** — it is `pub`, won't trigger dead_code, and is cleaned in 11.3. Only remove its import from `agent_factory.rs`.

### Known Remaining Debt (Not Blocking)

- `reqwest` may have now-unused features that were only exercised by `ReqwestCopilotHttpClient`. Cargo won't error on this, but it is worth noting for Story 11.4 when `Cargo.toml` is reviewed.
- `src/session/provider.rs::copilot_headers()` remains as a dead-but-public function until 11.3.
- String literals `"github-copilot"` persist in `supervisor/architect.rs`, `session/provider.rs`, `session/state.rs` test data, `pipeline.rs` test data — all cleaned in 11.3.

### Rollback Guidance for Unexpected Test Failures

If `cargo test` reveals failures in modules not touched by this story:

1. **Check for hidden `crate::auth` usage** — run `grep -rn "crate::auth\|use.*auth::" src/` to find any missed import.
2. **Check for indirect dependencies** — some modules may re-export auth types via `pub use`. Verify none of the remaining modules have a `pub use` chain pointing into auth.
3. **Check test fixture structs** — `BotSecrets { ... }` struct literals in tests across `config/`, `notifier/`, `session/`, `review/` may fail to compile if any field was accidentally removed. In 11.1, `github_copilot_oauth_token` field stays; only in 11.3 is it removed.
4. **If a non-Copilot test fails** — check whether it was exercising a `match` arm that covered `BuiltAgent::OpenAiCompletions`. All such arms must be removed now that the variant is gone.
5. **Do not remove unrelated tests to fix failures** — investigate the root cause first.

### Testing Requirements Summary

**Tests deleted as part of this story:**
- All `~680` lines of tests in `src/auth/github_copilot.rs` — deleted with the file
- `copilot_requires_responses_api` test functions in `agent_factory.rs` (~6 tests)
- In `src/cli/mod.rs`: `test_generate_env_excludes_github_copilot_token`, `test_generate_env_copilot_token_prefilled`, `test_generate_env_copilot_token_empty_when_none`, `test_default_model_for_provider_github_copilot`
- In `src/session/runner.rs`: `test_is_token_expired_error_exact_copilot_message`, `test_is_token_expired_error_simple`, `test_is_token_expired_error_false_for_other_auth_errors`, `test_is_token_expired_error_false_for_transient_errors`, `test_is_token_expired_error_false_for_context_limit`
- In `src/review/mod.rs`: `test_is_token_expired_error_exact_copilot_message`, `test_is_token_expired_error_simple`, `test_is_token_expired_error_false_for_other_auth_errors`, `test_is_token_expired_error_false_for_transient_errors`, `test_max_token_refreshes_is_reasonable`

**Tests to update (signature change, not deletion):**
- All ~12 `generate_env_file(...)` call sites in `src/cli/mod.rs` tests — remove second argument

**Verification sequence:**
```/dev/null/verify.sh#L1-6
grep -rn "crate::auth" src/                    # must be empty
grep -rn "CopilotTokenCache\|copilot_headers\|run_device_flow" src/  # must be empty
cargo build 2>&1
cargo clippy -- -D warnings 2>&1
cargo test 2>&1
cargo fmt --check 2>&1
```

### Git Intelligence

Recent commits:
- `778d60a` — docs(architecture): amend decisions for sprint change proposal 2026-04-15
- `09a0af1` — docs(planning): add epics 11-14 and sprint change proposal 2026-04-15
- `1e08b26` — bmad upgrade
- `974f8c7` — fix(watcher): prevent burst polling after long pipeline runs

All epics 1–10 are done. The codebase is stable. This is a clean starting point for Epic 11 with no outstanding story branches to worry about.

### Project Structure Notes

- After this story, `src/auth/` directory no longer exists
- No new files or directories are created
- `src/llm/agent_factory.rs` remains the centralized provider construction hub; it simply loses the Copilot variant
- The project structure rule (one domain per directory) is preserved

### References

- [Source: _bmad-output/planning-artifacts/epics.md § Story 11.1 (L2801–L2821)]
- [Source: _bmad-output/planning-artifacts/epics.md § Epic 11 Summary (L2928–L2949)]
- [Source: _bmad-output/project-context.md § Multi-Provider LLM Config (L99–L112)]
- [Source: _bmad-output/project-context.md § Code Quality & Style Rules (L121–L169)]
- [Source: _bmad-output/project-context.md § Testing Rules (L112–L121)]
- [Source: src/auth/github_copilot.rs — primary deletion target]
- [Source: src/llm/agent_factory.rs — CopilotTokenCache owner, copilot_headers import, OpenAiCompletions]
- [Source: src/cli/mod.rs — copilot-login command, init Device Flow, generate_env_file]
- [Source: src/session/runner.rs — is_token_expired_error ×6, MAX_TOKEN_REFRESHES]
- [Source: src/review/mod.rs — independent is_token_expired_error, MAX_TOKEN_REFRESHES, retry branch]
- [Source: src/review/epic.rs — imported token refresh symbols, retry branch]
- [Source: src/session/provider.rs — copilot_headers pub fn (stays), string arms (stays until 11.3)]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### Change Log

### File List