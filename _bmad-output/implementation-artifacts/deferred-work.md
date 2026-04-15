# Deferred Work

## Deferred from: code review of story 11.1 (2026-04-15)

- `is_transient_llm_error` in `src/session/runner.rs` still classifies "unauthorized" and "token expired" as transient retry-worthy errors, but the token-refresh recovery mechanism was removed in Story 11.1. These strings are retried with backoff but can no longer recover — should be re-evaluated during Story 11.3 provider cleanup.
- `pipeline.rs` `is_infra_error`/`is_auth_error` still carve out "token expired" as a non-infrastructure, non-auth error (returns `false`), but no recovery mechanism exists after Copilot token refresh removal. Pre-existing functional code not modified in this diff — should be re-evaluated during Story 11.3 provider cleanup.