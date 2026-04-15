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