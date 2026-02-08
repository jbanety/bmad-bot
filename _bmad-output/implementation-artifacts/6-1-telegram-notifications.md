# Story 6.1: Telegram Notifications

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer running BMAD Bot overnight,
I want to receive Telegram notifications summarizing what happened,
So that I know the results without checking GitHub/GitLab manually.

## Acceptance Criteria

1. **Given** a development session completes successfully **When** the notifier module sends a notification **Then** a Telegram message is sent via reqwest direct HTTP call to the Telegram Bot API using the bot token from `.env` **And** the message includes: story ID, status (✅ completed), and a direct link to the PR/MR

2. **Given** a story is blocked or encounters an error **When** the notifier module sends a notification **Then** a Telegram message is sent with: story ID, status (⚠️ blocked or ❌ error), reason for blockage/error, and a link to the PR if one was created **And** the message provides enough context to understand the issue without opening the PR

3. **Given** a full daemon run completes (all eligible stories processed) **When** the run summary is generated **Then** a summary notification is sent with: total stories processed, count by status (completed, blocked, errored), and links to all PRs created

4. **Given** the Telegram API is unavailable or returns an error **When** the notifier attempts to send a message **Then** the failure is logged via `tracing::error!()` with full context **And** the notification failure does NOT block the pipeline — story processing continues normally **And** no retry is attempted for notification failures (non-critical path)

## Functional Requirements Covered

- **FR25:** The daemon can send Telegram notifications with run summaries (stories completed, blocked, errored)
- **FR26:** Notifications include story ID, status, and a link to the PR
- **NFR-INT3:** Telegram API failures do not block the pipeline — logged but do not stop story processing

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [ ] Verify `src/notifier/mod.rs` skeleton exists (currently contains only TODO comment)
- [ ] Verify `TelegramConfig` struct exists in `src/config/mod.rs` with `enabled: bool` and `chat_id: String`
- [ ] Verify `BotSecrets.telegram_bot_token` field exists and is loaded from `TELEGRAM_BOT_TOKEN` env var
- [ ] Verify `BotSecrets::validate_for_config()` already validates telegram token when enabled
- [ ] Verify `build_http_client()` exists in `src/config/mod.rs` and returns `ClientWithMiddleware`
- [ ] Verify `reqwest` (with `json` feature) and `reqwest-middleware` are in `Cargo.toml`
- [ ] Verify `serde`, `serde_json`, `tracing`, `thiserror`, `async-trait` are available

### Task 1: Define Notifier Error Type (`src/notifier/mod.rs`)

- [ ] Define `NotifierError` enum using `thiserror`
  - [ ] `HttpRequest { reason: String }` — network/middleware send failure (store as String, not the original error — matches gitlab.rs pattern)
  - [ ] `ApiError { status: u16, body: String }` — Telegram API returned non-ok response (carry HTTP status code + response body)
  - [ ] `ResponseParse { reason: String }` — deserialization failure (store message as String via `.to_string()`)
  - [ ] `Disabled` — notification attempted but Telegram is disabled in config
- [ ] All variants must produce human-readable error messages via `#[error(...)]`
- [ ] Do NOT use `#[from]` on `reqwest_middleware::Error` or `serde_json::Error` — use `.map_err(|e| NotifierError::Variant { reason: e.to_string() })` inline (same pattern as `GitProviderError` in `src/git_provider/gitlab.rs`)

### Task 2: Define Data Types (`src/notifier/mod.rs`)

- [ ] Define `StoryStatus` enum: `Completed`, `Blocked`, `Error`
  - [ ] Implement `Display` to emit emoji+label: ✅ completed, ⚠️ blocked, ❌ error
- [ ] Define `StoryNotification` struct:
  - [ ] `story_id: String` (e.g. "6.1")
  - [ ] `story_key: String` (e.g. "6-1-telegram-notifications")
  - [ ] `status: StoryStatus`
  - [ ] `pr_url: Option<String>`
  - [ ] `reason: Option<String>` (for blocked/error context)
- [ ] Define `RunSummary` struct:
  - [ ] `stories: Vec<StoryNotification>`
  - [ ] `total_processed: usize`
  - [ ] `completed: usize`
  - [ ] `blocked: usize`
  - [ ] `errored: usize`
- [ ] Define internal `TelegramResponse` struct (for deserializing API response):
  - [ ] `ok: bool`
  - [ ] `description: Option<String>`

### Task 3: Define Notifier Trait (`src/notifier/mod.rs`)

- [ ] Define `#[async_trait] pub trait Notifier: Send + Sync`
  - [ ] `async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError>`
  - [ ] `async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError>`
- [ ] Trait is object-safe for future extensibility (Slack, email, etc.)

### Task 4: Implement `TelegramNotifier` (`src/notifier/mod.rs`)

- [ ] Define `TelegramNotifier` struct:
  - [ ] `http_client: ClientWithMiddleware` (from `build_http_client()`)
  - [ ] `bot_token: String` (from `BotSecrets.telegram_bot_token`)
  - [ ] `chat_id: String` (from `TelegramConfig.chat_id`)
- [ ] Implement `TelegramNotifier::new(config: &TelegramConfig, bot_token: String) -> Result<Self, NotifierError>`
  - [ ] Return `NotifierError::Disabled` if `!config.enabled`
  - [ ] Build HTTP client via `build_http_client()`
  - [ ] Store chat_id and bot_token
- [ ] Implement private `async fn send_message(&self, text: &str) -> Result<(), NotifierError>`
  - [ ] POST to `https://api.telegram.org/bot{token}/sendMessage`
  - [ ] Build JSON body manually via `serde_json::to_vec` (see reqwest-middleware pattern below)
  - [ ] Body: `{ "chat_id": "{chat_id}", "text": "{text}", "parse_mode": "HTML" }`
  - [ ] Handle messages > 4096 chars: truncate at 4093 chars and append `"..."` (see Telegram message limit section below)
  - [ ] Parse response via `response.bytes()` + `serde_json::from_slice::<TelegramResponse>()`
  - [ ] If HTTP status is not success → return `NotifierError::ApiError` with status + body text
  - [ ] If `!response.ok` in parsed body → return `NotifierError::ApiError` with description
  - [ ] Log success via `tracing::info!(action = "telegram_send", "Notification sent")`
- [ ] Implement `Notifier` trait for `TelegramNotifier`:
  - [ ] `notify_story`: Format a single-story message and call `send_message`
  - [ ] `notify_run_summary`: Format a run summary message and call `send_message`

### Task 5: Implement Message Formatting & HTML Escaping

- [ ] `fn escape_html(text: &str) -> String`
  - [ ] Replace `&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`
  - [ ] This MUST be applied to all dynamic text inserted into HTML-formatted messages (story keys, error reasons, etc.)
  - [ ] PR URLs go inside `href="..."` attributes and do NOT need HTML escaping (only the display text does)
- [ ] `fn format_story_message(notification: &StoryNotification) -> String`
  - [ ] Include: status emoji + label, story ID, story key (HTML-escaped)
  - [ ] If PR URL present: include clickable link via `<a href="...">PR</a>`
  - [ ] If reason present: include HTML-escaped reason on separate line
  - [ ] Example output:
    ```
    ✅ Story 6.1 completed
    <b>6-1-telegram-notifications</b>
    PR: <a href="https://github.com/org/repo/pull/42">PR #42</a>
    ```
- [ ] `fn format_run_summary(summary: &RunSummary) -> String`
  - [ ] Header line: "🏁 BMAD Bot Run Complete"
  - [ ] Stats: total, completed, blocked, errored counts
  - [ ] List each story with status emoji, HTML-escaped key, and PR link
  - [ ] Example output:
    ```
    🏁 BMAD Bot Run Complete
    📊 3 stories processed: ✅ 2 | ⚠️ 0 | ❌ 1

    ✅ 6-1-telegram-notifications → <a href="https://github.com/org/repo/pull/42">PR #42</a>
    ✅ 6-2-http-retry → <a href="https://github.com/org/repo/pull/43">PR #43</a>
    ❌ 6-3-crash-recovery — Context limit exceeded
    ```

### Task 6: Implement `NoopNotifier` (`src/notifier/mod.rs`)

- [ ] Define `NoopNotifier` struct (unit struct, no fields)
- [ ] Implement `Notifier` trait — both methods log via `tracing::debug!()` and return `Ok(())`
- [ ] This is used when `telegram.enabled = false`

### Task 7: Factory Function (`src/notifier/mod.rs`)

- [ ] `pub fn create_notifier(config: &NotificationConfig, secrets: &BotSecrets) -> Box<dyn Notifier>`
  - [ ] If `config.telegram.enabled` and token is available → `Box::new(TelegramNotifier::new(...))`
  - [ ] Else → `Box::new(NoopNotifier)` with `tracing::info!("Telegram notifications disabled")`
- [ ] Factory never fails — worst case returns NoopNotifier with a warning log

### Task 8: Unit Tests

- [ ] `test_story_status_display_completed` — verify ✅ emoji
- [ ] `test_story_status_display_blocked` — verify ⚠️ emoji
- [ ] `test_story_status_display_error` — verify ❌ emoji
- [ ] `test_escape_html_special_chars` — verify `<`, `>`, `&` are escaped
- [ ] `test_escape_html_no_change_for_safe_text` — verify plain text passes through
- [ ] `test_format_story_message_completed_with_pr` — verify message includes PR link
- [ ] `test_format_story_message_blocked_with_reason` — verify HTML-escaped reason included
- [ ] `test_format_story_message_error_no_pr` — verify graceful handling when no PR
- [ ] `test_format_story_message_escapes_html_in_reason` — verify `<timeout>` in reason doesn't break HTML
- [ ] `test_format_run_summary_mixed_statuses` — verify counts and per-story lines
- [ ] `test_format_run_summary_all_completed` — verify happy path
- [ ] `test_format_run_summary_truncation_long_message` — verify messages > 4096 chars are handled
- [ ] `test_noop_notifier_returns_ok` — verify NoopNotifier doesn't error
- [ ] `test_noop_notifier_story_returns_ok` — verify NoopNotifier story notification
- [ ] `test_telegram_notifier_new_disabled` — verify returns Disabled error when not enabled
- [ ] `test_telegram_notifier_send_sync` — verify `TelegramNotifier` is `Send + Sync`
- [ ] `test_create_notifier_disabled_returns_noop` — verify factory when disabled
- [ ] `test_create_notifier_enabled_returns_telegram` — verify factory when enabled (mock-friendly)
- [ ] All tests use mocked data — NO real Telegram API calls

### Task 9: Integration Verification

- [ ] `cargo check` — 0 errors
- [ ] `cargo test` — all existing + new tests pass, 0 regressions
- [ ] `cargo clippy` — 0 new warnings
- [ ] `cargo fmt` — clean
- [ ] All public items have `///` doc comments

## Dev Notes

### Previous Story Intelligence

**From Story 5.3 (GitLab Merge Request Support) — most recent implementation:**
- Agent model: Claude Opus 4.6
- Key learning: `reqwest_middleware::RequestBuilder` does NOT expose `.json()` method — must use manual `serde_json::to_vec()` + `Content-Type: application/json` header + `.body()` instead
- Key learning: `response.json()` is also unavailable on `reqwest::Response` returned through middleware — must use `response.bytes()` + `serde_json::from_slice()` instead
- Key learning: Error types use `{ reason: String }` fields mapped via `.map_err(|e| ... { reason: e.to_string() })` — NOT `#[from]` wrappers
- Pattern: Struct with `new()` constructor that validates inputs and builds HTTP client via `build_http_client()`
- Pattern: HTTP error responses read via `response.text().await.unwrap_or_default()` before mapping
- Pattern: Unit tests cover constructor success/failure, Send+Sync, error mapping, serialization
- All 488 existing tests passed after Story 5.3

### Git Intelligence (Last 5 Commits)

1. `cdc25c3` feat(git-provider): implement GitLabProvider with full GitProvider trait support
2. `dea1232` feat(review): implement automated code review session runner
3. `cf29058` feat(git-provider): implement GitProvider trait and GitHub PR creation
4. `deb6639` docs(stories): create story 5-3 GitLab merge request support
5. `1cbed78` docs(stories): validate story 5-2 — fix critical design gaps

**Patterns established:**
- Trait + implementation pattern (see `GitProvider` trait in `src/git_provider/mod.rs`)
- Factory function pattern (`create_provider()` dispatches based on config string)
- Per-module `thiserror` error enums with `{ reason: String }` fields
- All public items have `///` doc comments
- Tests inline in `#[cfg(test)] mod tests`

### Core Design — TelegramNotifier via reqwest-middleware

The notifier follows the same architectural pattern as `GitProvider`:
- A `Notifier` trait defines the interface
- `TelegramNotifier` implements it for Telegram
- `NoopNotifier` provides a silent fallback when disabled
- A factory function returns `Box<dyn Notifier>` based on config

The HTTP client comes from `build_http_client()` which already includes retry middleware (3 retries, exponential backoff). Per NFR-INT3, notification failures are non-blocking. The caller (session/daemon layer) MUST wrap notifier calls and swallow errors:

```rust
// Caller pattern — daemon/session layer:
if let Err(e) = notifier.notify_story(&notification).await {
    tracing::error!(action = "notification_failed", error = %e, story_id = %id, "Telegram notification failed — continuing");
}
```

### Telegram Bot API — Endpoint Details

**Base URL:** `https://api.telegram.org/bot<token>/METHOD_NAME`

**sendMessage endpoint:**
- Method: `POST`
- URL: `https://api.telegram.org/bot{bot_token}/sendMessage`
- Content-Type: `application/json`
- Body parameters:
  - `chat_id` (String) — Required — The chat ID from config
  - `text` (String) — Required — Message text, **max 4096 characters**
  - `parse_mode` (String) — Optional — Use `"HTML"` for bold/italic/links
- Response: `{ "ok": true/false, "result": { "message_id": ... }, "description": "error msg" }`
- HTML tags supported: `<b>bold</b>`, `<i>italic</i>`, `<a href="url">link</a>`, `<code>mono</code>`
- Special characters `<`, `>`, `&` in text content MUST be HTML-escaped

### Telegram Message Length Limit — CRITICAL

⚠️ **Telegram sendMessage has a hard 4096-character limit on the `text` field.**

If the formatted message exceeds 4096 characters (possible for run summaries with many stories), the API will return an error. Strategy:

- In `send_message()`, check `text.len() > 4096` before sending
- If exceeded, truncate to 4093 characters and append `"..."` as a suffix
- Log a `tracing::warn!(action = "telegram_truncated", original_len = text.len(), "Message truncated to 4096 char Telegram limit")`
- This is acceptable because run summaries are informational — the human can check GitHub/GitLab for full details

### HTML Escaping — CRITICAL

⚠️ **All dynamic text inserted into HTML-formatted messages MUST be escaped.**

Telegram's HTML parser will reject or misinterpret messages containing raw `<`, `>`, or `&` in text content. Error messages like `"Connection <timeout> after 30s"` will break the entire notification.

Provide a helper function:

```rust
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
```

Apply `escape_html()` to: story keys, error reasons, any user-facing text. Do NOT apply to HTML tags you generate yourself (`<b>`, `<a href="...">`) or to URLs inside `href` attributes.

### reqwest-middleware HTTP Pattern — Verified Reference

**⚠️ DO NOT use `.json()` on `reqwest_middleware::RequestBuilder` — it does NOT exist.**
**⚠️ DO NOT use `.json()` on `reqwest::Response` from middleware — it does NOT exist.**

Follow the exact pattern verified in production (`src/git_provider/gitlab.rs` L83-L126):

```rust
// 1. Serialize body manually
let json_body = serde_json::to_vec(&serde_json::json!({
    "chat_id": &self.chat_id,
    "text": text,
    "parse_mode": "HTML",
}))
.map_err(|e| NotifierError::ResponseParse { reason: e.to_string() })?;

// 2. Send with explicit Content-Type header
let response = self.http_client
    .post(&url)
    .header("Content-Type", "application/json")
    .body(json_body)
    .send()
    .await
    .map_err(|e| NotifierError::HttpRequest { reason: e.to_string() })?;

// 3. Check HTTP status — read body for error context
if !response.status().is_success() {
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    return Err(NotifierError::ApiError { status, body });
}

// 4. Parse success response via bytes
let resp_bytes = response.bytes().await
    .map_err(|e| NotifierError::ResponseParse { reason: e.to_string() })?;
let parsed: TelegramResponse = serde_json::from_slice(&resp_bytes)
    .map_err(|e| NotifierError::ResponseParse { reason: e.to_string() })?;

if !parsed.ok {
    return Err(NotifierError::ApiError {
        status: 200,
        body: parsed.description.unwrap_or_default(),
    });
}
```

This is the ONLY correct HTTP pattern for this project. The `reqwest_middleware` crate wraps the underlying `reqwest` client and does not re-expose `.json()` convenience methods.

### Architecture Compliance

| Constraint | Implementation |
|---|---|
| Module location | `src/notifier/mod.rs` |
| Error handling | `NotifierError` enum via `thiserror` — no `anyhow`, no `#[from]` on external errors |
| Error field pattern | `{ reason: String }` mapped via `.map_err(\|e\| ... { reason: e.to_string() })` — matches `GitProviderError` |
| HTTP client | `build_http_client()` from `src/config/mod.rs` (retry middleware included) |
| Logging | `tracing` only — no `println!` or `eprintln!` |
| Secrets | `TELEGRAM_BOT_TOKEN` from `.env` via `BotSecrets` — never hardcoded |
| Non-blocking | Notification failures logged but never propagated to stop pipeline |
| Retry behavior | Transport retries handled transparently by middleware; application-level Telegram errors (bad token, invalid chat_id) are not retried |
| HTML safety | All dynamic text escaped via `escape_html()` before insertion into HTML messages |
| Message limits | Messages truncated to 4096 chars before sending |
| Doc comments | `///` on all public structs, traits, enums, functions |
| Tests | Inline `#[cfg(test)] mod tests` — mock data only, no real API calls |

### Library & Framework Requirements

| Dependency | Version | Purpose | Already in Cargo.toml |
|---|---|---|---|
| `reqwest` | 0.13 | HTTP client for Telegram API | ✅ Yes (with `json` feature) |
| `reqwest-middleware` | 0.5 | Retry middleware wrapper | ✅ Yes |
| `serde` | 1 | Serialization | ✅ Yes |
| `serde_json` | 1 | JSON body construction & response parsing | ✅ Yes |
| `thiserror` | 2 | Typed error enums | ✅ Yes |
| `tracing` | 0.1 | Structured logging | ✅ Yes |
| `async-trait` | 0.1 | Async trait methods | ✅ Yes |

**No new dependencies needed.** Everything is already available.

### File Structure Requirements

**Files to create/modify:**
- `src/notifier/mod.rs` — **OVERWRITE** — Full implementation replacing TODO skeleton

**Files NOT to touch:**
- `src/config/mod.rs` — `TelegramConfig`, `NotificationConfig`, `BotSecrets.telegram_bot_token`, `build_http_client()` already exist
- `src/main.rs` — Module declaration `mod notifier;` already exists
- `Cargo.toml` — All dependencies already present
- Anything under `_bmad/` — Read-only, sacred

### Testing Requirements

All tests inline in `#[cfg(test)] mod tests` at the bottom of `src/notifier/mod.rs`:
- Use `#[tokio::test]` for async tests
- Naming convention: `test_{module}_{behavior}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert
- Mock all external dependencies — NO real Telegram API calls
- Message formatting and HTML escaping tests are pure functions — no mocking needed
- `NoopNotifier` tests verify it returns `Ok(())` without side effects
- Send+Sync compile-time assertions for `TelegramNotifier`

### Anti-Patterns to Avoid

- ❌ Do NOT use `.json()` on `reqwest_middleware::RequestBuilder` — it doesn't exist. Use `serde_json::to_vec()` + `.header("Content-Type", "application/json")` + `.body()`
- ❌ Do NOT use `.json()` on `reqwest::Response` from middleware — use `.bytes()` + `serde_json::from_slice()`
- ❌ Do NOT use `#[from]` to wrap `reqwest_middleware::Error` or `serde_json::Error` — use `.map_err(|e| ... { reason: e.to_string() })` inline
- ❌ Do NOT send dynamic text in HTML mode without escaping `<`, `>`, `&` — it will break Telegram's parser
- ❌ Do NOT send messages > 4096 characters — Telegram will reject them. Truncate first
- ❌ Do NOT use `unwrap()` or `expect()` in production code
- ❌ Do NOT use `println!` or `eprintln!` — use `tracing` only
- ❌ Do NOT hardcode bot token or chat ID
- ❌ Do NOT propagate notification errors to block the pipeline — caller must swallow errors with logging
- ❌ Do NOT use `anyhow` in this module — `thiserror` only
- ❌ Do NOT call real Telegram API in tests
- ❌ Do NOT add new dependencies — everything needed is already available

### Scope Boundaries

**In scope:**
- `Notifier` trait definition
- `TelegramNotifier` implementation (sendMessage via HTTP)
- `NoopNotifier` fallback implementation
- Factory function `create_notifier()`
- Message formatting with HTML escaping (story + run summary)
- Message truncation for 4096-char Telegram limit
- Data types (`StoryNotification`, `RunSummary`, `StoryStatus`)
- `NotifierError` typed error enum
- `escape_html()` helper function
- Unit tests for all public types and functions

**Out of scope:**
- Wiring the notifier into the daemon main loop (future story / daemon orchestrator)
- Slack, email, or other notification channels (future extensibility via trait)
- Editing or deleting messages after sending
- Rich media (photos, documents) — text messages only
- Message splitting for very long summaries (truncation is sufficient)
- Rate limiting on the notifier side (Telegram rate limits are generous for bot-to-user messages)

### Project Structure Notes

After this story, `src/notifier/` contains:
```
src/notifier/
└── mod.rs    # Notifier trait + TelegramNotifier + NoopNotifier + factory + types + escape_html + tests
```

This aligns with the architecture document's project structure specification. The module is already declared in `main.rs` via `mod notifier;`.

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Story 6.1 (L694-L722), FR25/FR26 (L56-L57), NFR-INT3 (L80)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Project Structure (L561-L607), Boundaries (L621-L660), Data Flow (L660-L673), Error Type Pattern (L353-L376), Git Provider Trait Pattern (L479-L510)]
- [Source: _bmad-output/project-context.md — Resilience Rules, Critical Don't-Miss Rules]
- [Source: src/git_provider/gitlab.rs — Verified reqwest-middleware HTTP pattern (L83-L126), error mapping (L213-L229)]
- [Source: src/config/mod.rs — TelegramConfig (L182-L189), NotificationConfig (L175-L178), BotSecrets.telegram_bot_token (L392), build_http_client() (L509-L515)]
- [Source: Telegram Bot API — sendMessage: https://core.telegram.org/bots/api#sendmessage]

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### Change Log

### File List