# Deferred Work

## Deferred from: code review of story 11.1 (2026-04-15)

- `is_transient_llm_error` in `src/session/runner.rs` still classifies "unauthorized" and "token expired" as transient retry-worthy errors, but the token-refresh recovery mechanism was removed in Story 11.1. These strings are retried with backoff but can no longer recover — should be re-evaluated during Story 11.3 provider cleanup.
- `pipeline.rs` `is_infra_error`/`is_auth_error` still carve out "token expired" as a non-infrastructure, non-auth error (returns `false`), but no recovery mechanism exists after Copilot token refresh removal. Pre-existing functional code not modified in this diff — should be re-evaluated during Story 11.3 provider cleanup.

## Deferred from: code review of story 11.2 (2026-04-15)

- **github-copilot zombie provider:** `VALID_LLM_PROVIDERS` and `resolve_api_key()` still accept `"github-copilot"`, but `AgentFactory::build()` only has arms for `"anthropic"` and `"openai-compatible"`. Config validation passes but runtime crashes with `UnsupportedProvider`. By design — deferred to Story 11.3.
- **Zero test coverage for github-copilot provider:** All test fixtures that previously exercised `"github-copilot"` were rewritten to `"openai-compatible"`. No remaining test exercises the copilot path. Deferred to Story 11.3 (provider removal).
- **Duplicated provider-to-env-var mapping in architect.rs:** `src/supervisor/architect.rs` `new_with_factory()` reimplements the provider → env var mapping (`"anthropic"` → `ANTHROPIC_API_KEY`, etc.) instead of calling the canonical `resolve_api_key()` in `provider.rs`. Pre-existing architectural debt not introduced by this diff.
- **Env-file OPENAI_API_KEY comment misleading for non-OpenAI backends:** `generate_env_file()` emits `OPENAI_API_KEY=` with a comment referencing roles, but gives no indication when the target is a local Ollama/LM Studio endpoint via `base_url`. Documentation/UX improvements deferred to Story 11.5.
- **No integration test that base_url reaches the HTTP client:** `test_agent_factory_build_openai_compatible_with_base_url` only asserts build succeeds, never verifies the client targets the configured URL. Would require mock-server infrastructure to test properly.

## Deferred from: code review of story 11.3 (2026-04-15)

- **Base-URL collection logic duplicated 3× in `collect_config_interactively()`:** The dev, review, and supervisor base_url prompt blocks in `src/cli/mod.rs` are near-identical 12-line stanzas differing only in the prompt string. Extract a shared helper (e.g., `prompt_base_url(role_label, provider)`) to reduce maintenance burden. Code style concern, not a bug — the story spec prescribes per-role prompting.
- **Duplicated provider-to-env-var mapping in `architect.rs`:** `src/supervisor/architect.rs` `new_with_factory()` still reimplements the provider → env var mapping (`"anthropic"` → `ANTHROPIC_API_KEY`, etc.) and manually constructs `BotSecrets` from `std::env::var` calls instead of calling the canonical `resolve_api_key()` in `provider.rs`. Pre-existing architectural debt not introduced by this diff (also noted in 11.2 review).


## Deferred from: code review of story 11.4 (2026-04-16)

- **`unwrap_or_default()` in test code silently swallows malformed JSON arguments:** In `tests/e2e/mcp_playwright.rs`, the pattern `.as_object().cloned().unwrap_or_default()` degrades to an empty map if `.as_object()` returns `None`. Since these are hardcoded JSON object literals, failure is currently impossible — but if anyone refactors the JSON value, the test will silently send empty arguments instead of failing loudly. Consider replacing with `.expect("arguments must be a JSON object")`. Pre-existing pattern not introduced by this diff.


## Deferred from: code review of story 11.5 (2026-04-16)

- **No CI gate or automated quality checks visible:** A commit with a .bak file, one-word message, whitespace-only churn, and contradictory status fields landed without any automated quality gate preventing it. Pre-existing process gap not introduced by this change.


## Deferred from: code review of story 9.3 (2026-04-18)

- **`timeout_secs: 0` is accepted and causes immediate handshake timeout:** `src/mcp/manager.rs:226` builds `Duration::from_secs(config.timeout_secs.unwrap_or(30))` without rejecting a user-supplied `Some(0)`. Zero triggers an immediate `tokio::time::timeout` and every connect fails with `HandshakeTimeout`. Pre-existing validation gap from Story 9.1 — add a `BotConfig::validate` check that rejects `Some(0)`.
- **`name` uniqueness constraint documented but not enforced:** `docs/mcp-servers.md:292` states the `name` field "must be unique across all configured servers", but `BotConfig::validate` never checks for duplicate names. Duplicate entries silently spawn both servers and create undefined tool-name collisions. Pre-existing from Story 9.1.
- **`@playwright/mcp` is not version-pinned in tests or docs:** `args: ["-y", "@playwright/mcp"]` always fetches `latest`. Upstream renamed `browser_screenshot` → `browser_take_screenshot` in recent versions and removed `browser_fill`. Asserted tool names (`browser_navigate`, `browser_click`, `browser_snapshot`) may not exist in future versions. Defer pinning to a follow-up that also aligns the `docs/mcp-servers.md` tool table with the pinned version's actual output.


## Deferred from: code review of story 12.1 (2026-04-17)

- **Recovery paths omit branch reminder:** `drive_activation_and_recover()` and the empty-history recovery path in `run_session()` send initial messages without a `BRANCH REMINDER`. The LLM may attempt branch operations during recovery. Pre-existing pattern — recovery paths never had branch reminders before Story 12.1.
- **Recovery `ch_msg` sent before recovery context summary:** In `drive_activation_and_recover()`, the initial message ("Continue recovery for story file") is sent before the `recovery_message` containing the compressed prior work summary. The skill may begin executing from scratch before receiving recovery context. Pre-existing architectural pattern unchanged by this story.
- **Architect session filename-based skill detection fragility:** The preamble's skill/persona distinction relies on the LLM interpreting the `SKILL.md` filename substring. If the architect file were ever renamed to contain `SKILL.md`, the flow would break. Currently safe — Story 12.4 scope.
