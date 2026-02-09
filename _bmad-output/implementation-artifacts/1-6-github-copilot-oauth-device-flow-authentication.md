# Story 1.6: GitHub Copilot OAuth Device Flow Authentication

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer setting up BMAD Bot,
I want to authenticate with GitHub Copilot via an OAuth Device Flow when I choose `github-copilot` as my LLM provider,
So that I can get a token automatically without manually creating a Personal Access Token, and the daemon can transparently exchange it for short-lived Copilot session tokens at runtime.

## Acceptance Criteria

### Part 1 — Rename `github-models` → `github-copilot` (do this first)

1. **Given** all existing code references to `github-models` and `GITHUB_MODELS_API_KEY` **When** this story is implemented **Then** every occurrence of `github-models` is replaced with `github-copilot` across the following files:
   - `src/cli/mod.rs` — `LLM_PROVIDERS`, `default_model_for_provider`, `generate_env_file`, and all tests referencing `github_models`
   - `src/config/mod.rs` — `VALID_LLM_PROVIDERS`, `BotSecrets.github_models_api_key` → `BotSecrets.github_copilot_oauth_token`, `load()`, `validate_for_config`, and all tests
   - `src/session/provider.rs` — `resolve_api_key` match arm and all tests
   - `src/session/runner.rs` — `run()` match arm for `"github-models"`
   - `src/supervisor/architect.rs` — `env_var_for_provider` match arm and tests
   - `bmad-bot.yaml.example` — provider comments
   - `README.md` — all references to `github-models` provider
   - `_bmad-output/project-context.md` — multi-provider LLM config section and external integration points
   **And** `GITHUB_MODELS_API_KEY` is replaced with `GITHUB_COPILOT_OAUTH_TOKEN` everywhere
   **And** `default_model_for_provider("github-copilot")` returns `"gpt-4o"`
   **And** all existing tests compile and pass with the renamed provider

### Part 2 — OAuth Device Flow in `bmad-bot init`

2. **Given** the LLM provider list in `bmad-bot init` **When** I view the available providers **Then** the options are `anthropic`, `openai`, and `github-copilot`

3. **Given** I select `github-copilot` as an LLM provider for one or more roles during `bmad-bot init` **When** all three LLM role selections (dev, review, supervisor) are complete **Then** the GitHub Copilot OAuth Device Flow is triggered exactly once, regardless of how many roles use `github-copilot` **And** a device code is requested from `https://github.com/login/device/code` with client ID `Iv1.b507a08c87ecfe98` and scope `read:user` **And** the terminal displays the verification URL and user code for me to authorize in my browser **And** the init flow polls `https://github.com/login/oauth/access_token` for the token with the interval specified by GitHub's response

4. **Given** no role uses `github-copilot` as its provider **When** all three LLM role selections are complete **Then** the Device Flow is not triggered at all

5. **Given** I authorize the device in my browser **When** the polling receives a valid access token **Then** the OAuth token is stored in memory and pre-filled as `GITHUB_COPILOT_OAUTH_TOKEN=<token>` in the generated `.env` file **And** the init flow continues normally with remaining configuration steps (notifications, daemon settings)

6. **Given** the Device Flow is polling for authorization **When** GitHub responds with `slow_down` **Then** the polling interval is increased by 2 seconds as per the OAuth spec

7. **Given** the Device Flow is polling for authorization **When** the device code expires (GitHub responds with `expired_token`) **Then** an error message is displayed explaining the code expired **And** `GITHUB_COPILOT_OAUTH_TOKEN=` is written empty in `.env` with a comment instructing the user to re-run init or obtain a token manually **And** the init flow continues without aborting

8. **Given** the Device Flow is polling for authorization **When** I cancel the authorization in the browser (GitHub responds with `access_denied`) **Then** an error message is displayed explaining the authorization was denied **And** `GITHUB_COPILOT_OAUTH_TOKEN=` is written empty in `.env` **And** the init flow continues without aborting

9. **Given** the terminal is not interactive (no TTY) **When** `github-copilot` is configured as a provider **Then** the Device Flow is skipped with a warning message **And** `GITHUB_COPILOT_OAUTH_TOKEN=` is written empty in `.env` with instructions to obtain the token manually

### Part 3 — Runtime Copilot Token Exchange and Caching

10. **Given** the daemon starts with `bmad-bot start` and `github-copilot` is configured as a provider **When** `BotSecrets` loads secrets from the environment **Then** `GITHUB_COPILOT_OAUTH_TOKEN` is loaded **And** validation fails with a descriptive error if the token is missing or empty

11. **Given** a session is about to run with the `github-copilot` provider **When** the daemon needs an API token for the LLM client **Then** it exchanges the long-lived OAuth token for a short-lived Copilot session token by calling `GET https://api.github.com/copilot_internal/v2/token` with `Authorization: Bearer {oauth_token}` **And** the response `{ token: string, expires_at: number }` is parsed and cached in memory **And** if the exchange fails (HTTP error, missing fields), the session fails with a descriptive `ProviderError`

12. **Given** a cached Copilot session token exists in memory **When** a new session is about to start **Then** the daemon checks whether the cached token is still valid (with a 5-minute safety margin before expiry) **And** if valid, the cached token is reused without making a new exchange request **And** if expired or within the safety margin, a fresh token is obtained via the exchange endpoint

13. **Given** a valid Copilot session token has been obtained **When** the token contains a `proxy-ep=<host>` field (semicolon-delimited key-value pairs) **Then** the base URL is derived by extracting the `proxy-ep` value, stripping the protocol, replacing `proxy.` prefix with `api.`, and prepending `https://` **And** if no `proxy-ep` is found, the default base URL `https://api.individual.githubcopilot.com` is used

14. **Given** the Copilot session token and derived base URL are resolved **When** the agent is built **Then** it uses the OpenAI-compatible client with the dynamically derived base URL and the Copilot session token (not the OAuth token) as the API key

### Part 4 — Module Structure and New Files

15. **Given** the new `src/auth/` module **When** I inspect the project structure **Then** the following files exist:
    - `src/auth/mod.rs` — `pub mod github_copilot;`
    - `src/auth/github_copilot.rs` — Device Flow functions (`request_device_code()`, `poll_for_access_token()`, `run_device_flow()`) and Copilot token exchange functions (`exchange_copilot_token()`, `derive_base_url_from_token()`, `CopilotTokenCache` struct with `resolve()` method)
    - `src/main.rs` — `mod auth;` added

16. **Given** the auth module depends on HTTP calls **When** the module is designed **Then** HTTP calls are abstracted behind an `async` trait (e.g. `CopilotHttpClient`) to enable deterministic mocking in unit tests, consistent with the project's existing mock patterns (no external mock crate required)

### Part 5 — Unit Tests

17. **Given** the `src/auth/github_copilot.rs` module **When** I inspect the unit tests **Then** the following tests exist with trait-based HTTP mocks (no real network calls):

*Device Flow tests:*
- `test_request_device_code_success` — mock HTTP 200 with valid JSON, verify parsed fields
- `test_request_device_code_http_error` — mock HTTP 500, verify error
- `test_request_device_code_missing_fields` — mock HTTP 200 with incomplete JSON, verify error
- `test_poll_authorization_pending_then_success` — mock sequential responses (`authorization_pending` × N, then `access_token`), verify final token
- `test_poll_slow_down_increases_interval` — verify interval increases by 2 seconds on `slow_down` response
- `test_poll_expired_token_returns_error` — mock `expired_token`, verify error
- `test_poll_access_denied_returns_error` — mock `access_denied`, verify error

*Token exchange tests:*
- `test_exchange_copilot_token_success` — mock valid exchange response, verify token and expiry parsed
- `test_exchange_copilot_token_http_error` — mock HTTP 401/403, verify error
- `test_exchange_copilot_token_missing_fields` — mock incomplete response, verify error
- `test_copilot_token_cache_returns_cached_when_valid` — verify no HTTP call when cache is fresh
- `test_copilot_token_cache_refreshes_when_expired` — verify HTTP call when cache is stale
- `test_derive_base_url_from_proxy_ep` — verify `proxy.example.com` → `https://api.example.com`
- `test_derive_base_url_fallback_when_no_proxy_ep` — verify default `https://api.individual.githubcopilot.com`
- `test_derive_base_url_strips_protocol_from_proxy_ep` — verify `https://proxy.foo.bar` → `https://api.foo.bar`

## Tasks / Subtasks

- [x] Task 0: Rename `github-models` → `github-copilot` across the codebase (AC: #1)
  - [x] 0.1 In `src/cli/mod.rs`: replace `"github-models"` with `"github-copilot"` in `LLM_PROVIDERS`, `default_model_for_provider`, `generate_env_file`, and all test functions containing `github_models`
  - [x] 0.2 In `src/config/mod.rs`: replace `"github-models"` with `"github-copilot"` in `VALID_LLM_PROVIDERS`; rename `BotSecrets.github_models_api_key` → `BotSecrets.github_copilot_oauth_token`; update `load()` to read `GITHUB_COPILOT_OAUTH_TOKEN`; update `validate_for_config` match arm; update all tests
  - [x] 0.3 In `src/session/provider.rs`: update `resolve_api_key` match arm from `"github-models"` → `"github-copilot"` and env var from `GITHUB_MODELS_API_KEY` → `GITHUB_COPILOT_OAUTH_TOKEN`; update all tests
  - [x] 0.4 In `src/session/runner.rs`: rename `"github-models"` match arm in `run()` to `"github-copilot"`
  - [x] 0.5 In `src/supervisor/architect.rs`: update `env_var_for_provider` match arm and tests
  - [x] 0.6 In `bmad-bot.yaml.example`: update provider comments from `"github-models"` to `"github-copilot"`
  - [x] 0.7 In `README.md`: replace all references to `github-models` with `github-copilot` and `GITHUB_MODELS_API_KEY` with `GITHUB_COPILOT_OAUTH_TOKEN`
  - [x] 0.8 In `_bmad-output/project-context.md`: update multi-provider LLM config section and external integration points
  - [x] 0.9 Run `cargo test` — all 599+ existing tests must pass with the renamed provider

- [x] Task 1: Create `src/auth/` module with Device Flow implementation (AC: #3, #5, #6, #7, #8, #15, #16)
  - [x] 1.1 Create `src/auth/mod.rs` with `pub mod github_copilot;`
  - [x] 1.2 Add `mod auth;` to `src/main.rs`
  - [x] 1.3 Create `src/auth/github_copilot.rs` with the following:
  - [x] 1.4 Define `CopilotAuthError` thiserror enum with variants: `DeviceCodeRequestFailed`, `DeviceCodeResponseInvalid`, `AccessTokenPollFailed`, `DeviceCodeExpired`, `AccessDenied`, `TokenExchangeFailed`, `TokenExchangeResponseInvalid`, `UnexpectedError`
  - [x] 1.5 Define `DeviceCodeResponse` struct: `device_code: String`, `user_code: String`, `verification_uri: String`, `expires_in: u64`, `interval: u64`
  - [x] 1.6 Define `DeviceTokenResponse` enum: `Success { access_token, token_type, scope }` | `Pending { error }` for deserialization
  - [x] 1.7 Define `#[async_trait] trait CopilotHttpClient: Send + Sync` with methods `request_device_code(client_id, scope) -> Result<DeviceCodeResponse>` and `poll_access_token(client_id, device_code) -> Result<DeviceTokenResponse>` and `exchange_copilot_token(oauth_token) -> Result<CopilotTokenResponse>`
  - [x] 1.8 Implement `ReqwestCopilotHttpClient` struct implementing the trait with real `reqwest` calls:
    - `request_device_code()`: POST `https://github.com/login/device/code` with `client_id`, `scope` as form body, `Accept: application/json`
    - `poll_access_token()`: POST `https://github.com/login/oauth/access_token` with `client_id`, `device_code`, `grant_type=urn:ietf:params:oauth:grant-type:device_code` as form body, `Accept: application/json`
    - `exchange_copilot_token()`: GET `https://api.github.com/copilot_internal/v2/token` with `Authorization: Bearer {oauth_token}`, `Accept: application/json`
  - [x] 1.9 Implement `pub async fn request_device_code(client: &dyn CopilotHttpClient) -> Result<DeviceCodeResponse, CopilotAuthError>` — calls client, validates all required fields present
  - [x] 1.10 Implement `pub async fn poll_for_access_token(client: &dyn CopilotHttpClient, device_code: &str, interval: u64, expires_in: u64) -> Result<String, CopilotAuthError>` — polling loop with `tokio::time::sleep`, handles `authorization_pending` (keep polling), `slow_down` (interval += 2s), `expired_token` (error), `access_denied` (error)
  - [x] 1.11 Implement `pub async fn run_device_flow(client: &dyn CopilotHttpClient) -> Result<String, CopilotAuthError>` — orchestrates: request device code → display URL + user code → poll → return OAuth token. Uses `COPILOT_CLIENT_ID = "Iv1.b507a08c87ecfe98"` and `COPILOT_SCOPE = "read:user"`
  - [x] 1.12 All public structs, traits, functions, and enum variants have `///` doc comments

- [x] Task 2: Implement Copilot token exchange and caching (AC: #11, #12, #13, #14, #15)
  - [x] 2.1 Define `CopilotTokenResponse` struct: `token: String`, `expires_at: u64` (unix timestamp seconds)
  - [x] 2.2 Implement `pub fn derive_base_url_from_token(token: &str) -> String` — parse `proxy-ep=<value>` from semicolon-delimited token string; strip protocol prefix; replace `proxy.` with `api.`; prepend `https://`; fallback to `DEFAULT_COPILOT_BASE_URL = "https://api.individual.githubcopilot.com"`
  - [x] 2.3 Implement `CopilotTokenCache` struct with fields: `cached_token: Option<String>`, `cached_base_url: Option<String>`, `expires_at: Option<u64>` (unix timestamp ms)
  - [x] 2.4 Implement `CopilotTokenCache::resolve(client: &dyn CopilotHttpClient, oauth_token: &str) -> Result<(String, String), CopilotAuthError>` — checks cache validity (5-minute safety margin), returns cached `(token, base_url)` if valid, otherwise calls `exchange_copilot_token()`, parses response, derives base URL, updates cache, returns `(token, base_url)`
  - [x] 2.5 Implement `fn parse_copilot_token_response(json: serde_json::Value) -> Result<CopilotTokenResponse, CopilotAuthError>` — extract `token` (string) and `expires_at` (number, handle both seconds and milliseconds like openclaw), error if missing
  - [x] 2.6 All public items have `///` doc comments

- [x] Task 3: Integrate Device Flow into `bmad-bot init` (AC: #2, #3, #4, #5, #7, #8, #9)
  - [x] 3.1 In `collect_config_interactively()`: after all three LLM role selections are complete, check if any role uses `"github-copilot"`
  - [x] 3.2 If yes: check `std::io::stdin().is_terminal()` (requires `use std::io::IsTerminal;`)
  - [x] 3.3 If TTY: display header `"── GitHub Copilot Authentication ──"`, instantiate `ReqwestCopilotHttpClient`, call `run_device_flow()`, display verification URL and user code prominently
  - [x] 3.4 If Device Flow succeeds: store the OAuth token in a local variable for later use in `.env` generation
  - [x] 3.5 If Device Flow fails (expired, denied): display error message via `eprintln!` or `tracing::warn!`, set token to empty string, continue init flow
  - [x] 3.6 If not TTY: display warning that Device Flow requires interactive terminal, set token to empty string, continue
  - [x] 3.7 Pass the obtained token (or empty string) to `generate_env_file()` — modify `generate_env_file` signature to accept an optional `copilot_oauth_token: Option<&str>` parameter
  - [x] 3.8 In `generate_env_file()`: when `github-copilot` roles exist, write `GITHUB_COPILOT_OAUTH_TOKEN=<token>` (pre-filled if token obtained, empty with comment if not)

- [x] Task 4: Update `session/runner.rs` and `supervisor/architect.rs` to use token exchange (AC: #14)
  - [x] 4.1 In `session/runner.rs`: update the `"github-copilot"` match arm in `run()` to resolve the Copilot session token and base URL before building the agent. Create a `CopilotTokenCache` instance (or receive one), call `cache.resolve()` with the OAuth token from secrets, use the returned `(session_token, base_url)` to build the OpenAI agent via `build_openai_agent(story, &session_token, model, Some(&base_url), ...)`
  - [x] 4.2 In `supervisor/architect.rs`: similarly update the `"github-copilot"` path in `ArchitectSession::ask()` to exchange the token and derive the base URL before building the rig agent
  - [x] 4.3 Decide on `CopilotTokenCache` lifetime: since sessions are sequential (one at a time), a single cache instance can be held in `SessionRunner` or passed into `run()`. The cache should persist across stories within the same daemon run to avoid unnecessary exchange calls

- [x] Task 5: Write unit tests for Device Flow (AC: #17)
  - [x] 5.1 Create `MockCopilotHttpClient` struct implementing `CopilotHttpClient` trait with configurable responses (use `std::sync::Mutex<Vec<Result<...>>>` for sequential response queues)
  - [x] 5.2 `test_request_device_code_success` — mock returns valid `DeviceCodeResponse`, verify all fields parsed
  - [x] 5.3 `test_request_device_code_http_error` — mock returns error, verify `CopilotAuthError::DeviceCodeRequestFailed`
  - [x] 5.4 `test_request_device_code_missing_fields` — mock returns JSON with missing `user_code`, verify error
  - [x] 5.5 `test_poll_authorization_pending_then_success` — mock returns `authorization_pending` twice then `access_token`, verify final token value (use short/zero sleep intervals in tests)
  - [x] 5.6 `test_poll_slow_down_increases_interval` — mock returns `slow_down`, verify the function respects the increased interval (assert call count or timing)
  - [x] 5.7 `test_poll_expired_token_returns_error` — mock returns `expired_token`, verify `CopilotAuthError::DeviceCodeExpired`
  - [x] 5.8 `test_poll_access_denied_returns_error` — mock returns `access_denied`, verify `CopilotAuthError::AccessDenied`

- [x] Task 6: Write unit tests for token exchange and caching (AC: #17)
  - [x] 6.1 `test_exchange_copilot_token_success` — mock returns valid `{ token, expires_at }`, verify parsed values
  - [x] 6.2 `test_exchange_copilot_token_http_error` — mock returns HTTP 401, verify error
  - [x] 6.3 `test_exchange_copilot_token_missing_fields` — mock returns JSON without `token` field, verify error
  - [x] 6.4 `test_copilot_token_cache_returns_cached_when_valid` — set cache with future expiry, call resolve, verify no HTTP call made (mock call count = 0)
  - [x] 6.5 `test_copilot_token_cache_refreshes_when_expired` — set cache with past expiry, call resolve, verify HTTP call made and new token returned
  - [x] 6.6 `test_derive_base_url_from_proxy_ep` — input `"tid=abc;proxy-ep=proxy.example.com;exp=123"` → output `"https://api.example.com"`
  - [x] 6.7 `test_derive_base_url_fallback_when_no_proxy_ep` — input `"tid=abc;exp=123"` → output `"https://api.individual.githubcopilot.com"`
  - [x] 6.8 `test_derive_base_url_strips_protocol_from_proxy_ep` — input `"proxy-ep=https://proxy.foo.bar"` → output `"https://api.foo.bar"`

- [x] Task 7: Final quality checks (AC: all)
  - [x] 7.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [x] 7.2 Run `cargo clippy` and fix any warnings introduced by this story
  - [x] 7.3 Run `cargo test` and verify ALL tests pass (existing + new, zero regressions)
  - [x] 7.4 Verify all new public items have `///` doc comments
  - [x] 7.5 Verify no `unwrap()` or `expect()` in production code (only in tests)
  - [x] 7.6 Verify no secrets are logged via `tracing` (OAuth tokens, session tokens)
  - [x] 7.7 Count new tests — expect ~15+ new tests in `src/auth/github_copilot.rs` plus updated existing tests

## Dev Agent Record

### Implementation Plan

- Task 0: Bulk rename `github-models` → `github-copilot` across 10+ files using sed + manual fixups for string literals inside error messages and test comments. 599/599 existing tests pass after rename.
- Tasks 1+2: Created `src/auth/github_copilot.rs` (1321 lines) with full Device Flow, token exchange, caching, `CopilotHttpClient` trait for mocking, `ReqwestCopilotHttpClient` for production, and 34 unit tests — all in a single module following project conventions.
- Task 3: Integrated Device Flow into `run_init()` (not `collect_config_interactively()` which is sync) — async network call belongs in async context. Modified `generate_env_file()` to accept `Option<&str>` for pre-filled token.
- Task 4: Added `CopilotTokenCache` as `std::sync::Mutex` field on `SessionRunner` for cross-story persistence. Refactored `resolve_copilot_session()` into a 3-phase pattern (check-cache → exchange-without-lock → store-under-lock) to satisfy clippy's `await_holding_lock` lint. Updated all 4 `"github-copilot"` match arms in runner.rs, architect.rs ask(), and review/mod.rs run_inner().
- Tasks 5+6: All 34 auth tests implemented inline with MockCopilotHttpClient + CountingMockClient wrapper for cache call-count verification.
- Task 7: cargo fmt clean, cargo clippy zero new errors (17 pre-existing dead_code warnings unchanged), 636/636 tests pass, 109 doc comments in new module, zero unwrap/expect in production code, zero token logging.

### Debug Log

- `reqwest` 0.13 requires explicit `form` feature for `.form()` method — added to Cargo.toml.
- `resp.json()` in reqwest 0.13 needs turbofish `.json::<serde_json::Value>()` for type inference inside async trait impls.
- `std::sync::MutexGuard` held across `.await` triggers clippy `await_holding_lock` error — refactored to 3-phase lock pattern (try_get_cached → exchange → store).

### Completion Notes

✅ Story 1.6 implementation complete — 636/636 tests pass (599 existing + 37 new).

Key accomplishments:
- Complete rename of `github-models` → `github-copilot` across entire codebase (12 files)
- OAuth Device Flow (RFC 8628) with `request_device_code()`, `poll_for_access_token()`, `run_device_flow()`
- Copilot token exchange with `CopilotTokenCache` (5-min safety margin, in-memory only)
- Dynamic base URL derivation from session token `proxy-ep` field
- `CopilotHttpClient` trait for deterministic unit testing (no real network calls)
- Integration into `bmad-bot init` (TTY check, graceful failure, pre-filled .env)
- Integration into session runner (4 match arms), architect, and review runner
- 34 new tests in `src/auth/github_copilot.rs` + 2 new tests in `src/cli/mod.rs` + 1 renamed test

## File List

- `src/main.rs` — added `mod auth;`
- `src/auth/mod.rs` — **NEW** — auth module root
- `src/auth/github_copilot.rs` — **NEW** — Device Flow, token exchange, caching, traits, tests
- `src/cli/mod.rs` — renamed github-models → github-copilot, integrated Device Flow into run_init, updated generate_env_file signature, added 2 new tests
- `src/config/mod.rs` — renamed github-models → github-copilot in VALID_LLM_PROVIDERS, BotSecrets field, load(), validate_for_config, all tests
- `src/session/provider.rs` — renamed github-models → github-copilot in resolve_api_key, create_completion_model, ProviderError display, all tests
- `src/session/runner.rs` — renamed github-models → github-copilot, added CopilotTokenCache field to SessionRunner, added resolve_copilot_session(), updated 4 match arms to use token exchange
- `src/session/state.rs` — renamed github-models → github-copilot in test
- `src/supervisor/architect.rs` — renamed github-models → github-copilot, updated env_var_for_provider, updated ask() to use token exchange
- `src/review/mod.rs` — renamed github-models → github-copilot, updated run_inner() to use token exchange
- `src/notifier/mod.rs` — renamed github_models_api_key → github_copilot_oauth_token in tests
- `Cargo.toml` — added `form` feature to reqwest
- `bmad-bot.yaml.example` — renamed github-models → github-copilot
- `README.md` — renamed github-models → github-copilot, GitHub Models → GitHub Copilot
- `_bmad-output/project-context.md` — renamed GitHub Models → GitHub Copilot

## Change Log

- 2026-02-09: Story 1.6 implemented — renamed github-models → github-copilot across codebase, added OAuth Device Flow authentication module, Copilot token exchange with caching, integrated into init/session/review/supervisor flows. 37 new tests added. All 636 tests pass.

## Status

Status: review

## Dev Notes

### Previous Story Intelligence

This story modifies files from multiple previous stories:

**From Story 1.1 (config):**
- `BotSecrets` struct in `src/config/mod.rs` — rename `github_models_api_key` field
- `VALID_LLM_PROVIDERS` const — replace provider string
- `validate_for_config()` — update match arm

**From Story 1.3 (init):**
- `collect_config_interactively()` in `src/cli/mod.rs` — insert Device Flow trigger after LLM role selection
- `generate_env_file()` — modify to accept optional pre-filled token, update env var name
- `LLM_PROVIDERS` const, `default_model_for_provider()` — replace provider string

**From Story 4.2 (session):**
- `SessionRunner::run()` in `src/session/runner.rs` — update `"github-models"` match arm to use token exchange
- `build_openai_agent()` — now receives dynamic base URL from token exchange instead of hardcoded URL

**From Story 3.2 (supervisor):**
- `ArchitectSession` in `src/supervisor/architect.rs` — update `env_var_for_provider()` and agent build for `github-copilot`

### Key Constants

```rust
/// GitHub Copilot OAuth App client ID (public, same as VS Code uses).
const COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

/// OAuth scope for the Device Flow.
const COPILOT_SCOPE: &str = "read:user";

/// GitHub Device Code endpoint.
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";

/// GitHub OAuth access token endpoint.
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

/// Copilot internal token exchange endpoint.
const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";

/// Default Copilot API base URL when proxy-ep is not found in token.
const DEFAULT_COPILOT_BASE_URL: &str = "https://api.individual.githubcopilot.com";

/// Safety margin (in milliseconds) before considering a cached token expired.
const TOKEN_EXPIRY_SAFETY_MARGIN_MS: u64 = 5 * 60 * 1000;
```

### Token Exchange Flow (Runtime)

The OAuth token from the Device Flow is long-lived but cannot be used directly for LLM inference. At runtime:

1. Read `GITHUB_COPILOT_OAUTH_TOKEN` from `.env` (long-lived)
2. Exchange it: `GET https://api.github.com/copilot_internal/v2/token` with `Authorization: Bearer {oauth_token}`
3. Parse response: `{ "token": "<session_token>", "expires_at": <unix_timestamp> }`
4. The session token is a semicolon-delimited string containing `proxy-ep=<host>` for the API base URL
5. Derive base URL: extract `proxy-ep`, replace `proxy.` → `api.`, prepend `https://`
6. Use session token as API key + derived base URL for OpenAI-compatible client
7. Cache in memory with 5-minute safety margin before expiry

### Base URL Derivation Logic

```rust
/// Parse proxy-ep from a Copilot session token and derive the API base URL.
///
/// Token format: "tid=abc123;exp=1234567890;sku=free;proxy-ep=proxy.example.com;st=dotcom;..."
/// 1. Find `proxy-ep=<value>` in semicolon-delimited pairs
/// 2. Strip protocol prefix if present (e.g., "https://proxy.foo.bar" → "proxy.foo.bar")
/// 3. Replace "proxy." prefix with "api." (e.g., "proxy.foo.bar" → "api.foo.bar")
/// 4. Prepend "https://"
/// 5. If no proxy-ep found, return DEFAULT_COPILOT_BASE_URL
```

### Device Flow Display Format

When the Device Flow is triggered during `init`, display:

```
── GitHub Copilot Authentication ──

🔗 To authorize BMAD Bot with GitHub Copilot:

   1. Open: https://github.com/login/device
   2. Enter code: ABCD-1234

⏳ Waiting for authorization...
```

After success:
```
✅ GitHub Copilot authorization successful!
```

After failure:
```
⚠ GitHub Copilot authorization failed: <reason>
  You can set GITHUB_COPILOT_OAUTH_TOKEN manually in .env later.
```

### Error Type Design

```rust
#[derive(Debug, thiserror::Error)]
pub enum CopilotAuthError {
    #[error("Failed to request device code from GitHub: {reason}")]
    DeviceCodeRequestFailed { reason: String },

    #[error("Invalid device code response from GitHub: {reason}")]
    DeviceCodeResponseInvalid { reason: String },

    #[error("Failed to poll for access token: {reason}")]
    AccessTokenPollFailed { reason: String },

    #[error("GitHub device code expired — re-run `bmad-bot init` to authenticate")]
    DeviceCodeExpired,

    #[error("GitHub authorization was denied by the user")]
    AccessDenied,

    #[error("Copilot token exchange failed: HTTP {status}")]
    TokenExchangeFailed { status: u16 },

    #[error("Invalid Copilot token exchange response: {reason}")]
    TokenExchangeResponseInvalid { reason: String },

    #[error("Unexpected error during Copilot authentication: {reason}")]
    UnexpectedError { reason: String },
}
```

### Mock Pattern for Tests

Consistent with the project's existing mock patterns (no external mock crate):

```rust
#[cfg(test)]
struct MockCopilotHttpClient {
    device_code_responses: std::sync::Mutex<Vec<Result<DeviceCodeResponse, CopilotAuthError>>>,
    access_token_responses: std::sync::Mutex<Vec<Result<DeviceTokenResponse, CopilotAuthError>>>,
    exchange_responses: std::sync::Mutex<Vec<Result<CopilotTokenResponse, CopilotAuthError>>>,
}

#[cfg(test)]
impl MockCopilotHttpClient {
    fn new() -> Self { /* empty queues */ }
    fn with_device_code(mut self, resp: Result<DeviceCodeResponse, CopilotAuthError>) -> Self { /* push */ }
    fn with_access_token(mut self, resp: Result<DeviceTokenResponse, CopilotAuthError>) -> Self { /* push */ }
    fn with_exchange(mut self, resp: Result<CopilotTokenResponse, CopilotAuthError>) -> Self { /* push */ }
}

#[async_trait]
#[cfg(test)]
impl CopilotHttpClient for MockCopilotHttpClient {
    async fn request_device_code(&self, _client_id: &str, _scope: &str) -> Result<DeviceCodeResponse, CopilotAuthError> {
        self.device_code_responses.lock().unwrap().remove(0)
    }
    // ... same pattern for other methods
}
```

### Anti-Patterns to Avoid

- **Do NOT log OAuth tokens or session tokens** — use `tracing::info!(action = "copilot_token_exchanged")` without the token value
- **Do NOT store Copilot session tokens on disk** — in-memory cache only (they're short-lived and easily re-fetched)
- **Do NOT hardcode the base URL** — always derive from the session token's `proxy-ep` field with fallback
- **Do NOT call real GitHub APIs in unit tests** — use the `CopilotHttpClient` trait mock exclusively
- **Do NOT block the async runtime** — use `tokio::time::sleep` for polling, not `std::thread::sleep`
- **Do NOT abort `bmad-bot init` on Device Flow failure** — always continue with empty token and clear instructions
- **Do NOT trigger the Device Flow more than once** even if multiple roles use `github-copilot`

### Scope Boundaries

**IN scope:**
- Complete rename of `github-models` → `github-copilot` across the codebase
- OAuth Device Flow implementation in `src/auth/github_copilot.rs`
- Integration into `bmad-bot init` (triggered once after all LLM role selections)
- Copilot token exchange at runtime with in-memory caching
- Dynamic base URL derivation from session token `proxy-ep`
- All unit tests with trait-based mocks
- Update `bmad-bot.yaml.example`, `README.md`, `project-context.md`

**OUT of scope:**
- Dedicated `bmad-bot login` command (future enhancement)
- On-disk token caching (in-memory only for MVP)
- Token refresh during a running session (exchange happens before each session start)
- Copilot-specific model listing or validation
- Changes to `anthropic` or `openai` provider paths

### References

- Story 1.1 (`1-1-project-scaffolding-configuration-validation.md`) — BotConfig, BotSecrets, config validation
- Story 1.3 (`1-3-interactive-init-command.md`) — `collect_config_interactively()`, `generate_env_file()`
- Story 4.2 (`4-2-agent-session-setup-chat-loop.md`) — `SessionRunner::run()`, agent build flow
- Story 3.2 (`3-2-llm-fallback-with-project-context.md`) — `ArchitectSession`, supervisor LLM setup
- [openclaw github-copilot-auth.ts](https://github.com/openclaw/openclaw/blob/main/src/providers/github-copilot-auth.ts) — Device Flow reference implementation
- [openclaw github-copilot-token.ts](https://github.com/openclaw/openclaw/blob/main/src/providers/github-copilot-token.ts) — Token exchange and base URL derivation reference
- [RFC 8628 — OAuth 2.0 Device Authorization Grant](https://datatracker.ietf.org/doc/html/rfc8628) — Protocol specification