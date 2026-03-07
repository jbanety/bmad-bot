---
type: sm-brief
date: 2026-03-05
author: JB (Dev Agent)
status: draft
scope: minor
epic: 1
---

# SM Brief — `--login-telegram` : Assisted Telegram Setup

## 1. Context

Epic 1 (done) delivered `bmad-bot init --copilot-login` : an interactive command that runs the GitHub Copilot OAuth Device Flow and writes `GITHUB_COPILOT_OAUTH_TOKEN` directly into `.env` — zero manual token hunting.

Epic 6 (done) delivered Telegram notifications (`6-1-telegram-notifications`). However, setup remains fully manual:
1. User creates a bot via @BotFather → copies token into `.env`
2. User sends a message to the bot, calls `getUpdates` manually → copies `chat_id` into `bmad-bot.yaml`

This is friction. The pattern already exists to do better.

---

## 2. Problem Statement

To enable Telegram notifications, the user must:
- Know that `TELEGRAM_BOT_TOKEN` goes in `.env` and `chat_id` goes in `bmad-bot.yaml`
- Manually call the Telegram API to discover their own `chat_id`
- Hand-edit both files correctly

One wrong field name or value → silent failure (notifier logs a warning and continues). The user gets no Telegram notifications and may not notice for hours.

There is no guided setup path equivalent to `--copilot-login`.

---

## 3. Proposed Solution

Add `bmad-bot init --login-telegram` : an interactive guided flow that:

1. Prompts the user for their `TELEGRAM_BOT_TOKEN` (or reads it from `.env` if already present)
2. Calls `GET https://api.telegram.org/bot<TOKEN>/getMe` to validate the token → displays the bot name on success
3. Instructs the user: *"Send any message to @<bot_username> in Telegram, then press Enter"*
4. Polls `GET https://api.telegram.org/bot<TOKEN>/getUpdates` (short intervals, up to ~2 min timeout) until a message arrives
5. Extracts `chat_id` from the first received message
6. Writes `TELEGRAM_BOT_TOKEN=<token>` into `.env`
7. Patches `bmad-bot.yaml`: sets `notifications.telegram.enabled: true` and `notifications.telegram.chat_id: "<id>"`
8. Prints a confirmation summary

This mirrors the `--copilot-login` UX exactly. Non-TTY → skip with warning (same guard as copilot).

---

## 4. Acceptance Criteria (draft)

**AC1 — New CLI flag**
`bmad-bot init --login-telegram` is a valid command. Running it without `--copilot-login` does not trigger the Copilot Device Flow.

**AC2 — Token validation**
Given a valid `TELEGRAM_BOT_TOKEN`,
when the user provides it (interactively or via existing `.env`),
then `getMe` is called and the bot's username is displayed.
If `getMe` returns an error (invalid token, network failure), a clear error is shown and the flow aborts.

**AC3 — Chat ID discovery via polling**
Given a validated token,
when the user sends a message to the bot and presses Enter,
then `getUpdates` is polled and the `chat.id` from the first received message is extracted.
If no message is received within 2 minutes, the flow aborts with instructions for manual setup.

**AC4 — `.env` patching**
Given a successfully obtained token and chat_id,
when the flow completes,
then `TELEGRAM_BOT_TOKEN=<token>` is present in `.env` (created if absent, patched if existing — same logic as `run_copilot_login`).

**AC5 — `bmad-bot.yaml` patching**
Given a successfully obtained chat_id,
when the flow completes,
then `bmad-bot.yaml` has `notifications.telegram.enabled: true` and `notifications.telegram.chat_id: "<id>"`.
If `bmad-bot.yaml` does not exist, a descriptive error is shown (init must be run first).

**AC6 — Non-TTY guard**
Given a non-interactive terminal,
when `--login-telegram` is invoked,
then the flow is skipped with a warning message and exits cleanly (no panic, no partial writes).

**AC7 — `getUpdates` offset handling**
Given prior messages may exist in the bot's update queue,
when polling begins,
then the flow calls `getUpdates` with `offset` correctly set to skip already-seen updates (only messages received *after* the flow starts are considered).

**AC8 — Unit tests**
The Telegram setup logic is covered by unit tests using a trait-based HTTP mock (consistent with `CopilotHttpClient` pattern in `src/auth/github_copilot.rs`):
- `test_getme_success` — valid token → bot username extracted
- `test_getme_invalid_token` — HTTP 401 → error returned
- `test_getupdates_message_received` — update present → chat_id extracted
- `test_getupdates_timeout` — no update within limit → timeout error returned
- `test_getupdates_offset_advances` — verify offset increments correctly across polls

---

## 5. Scope & Placement

| Attribute       | Value |
|-----------------|-------|
| Epic            | Epic 1 — Project Foundation & CLI (additive story) |
| Story ID        | `1-7-telegram-assisted-setup` (suggested) |
| Effort estimate | Small — ~1 day. HTTP trait mock pattern already established. YAML patching already done for copilot token. New surface: `getMe` + `getUpdates` polling + yaml write. |
| Dependencies    | Epic 1 done ✅, Epic 6 done ✅. No blockers. |
| Risk            | Low. Additive only. No existing code modified except `src/cli/mod.rs` (new flag + handler). |

---

## 6. Files Expected to Change

| File | Change |
|------|--------|
| `src/cli/mod.rs` | Add `telegram_login: bool` flag to `Commands::Init` ; add `run_telegram_login()` function |
| `src/auth/mod.rs` | Add `pub mod telegram;` |
| `src/auth/telegram.rs` | New — `TelegramSetupClient` trait, `getMe`, `getUpdates`, `run_telegram_setup_flow()` |
| `bmad-bot.yaml.example` | No change needed (field already present) |
| `README.md` | Add `--login-telegram` to CLI Reference and Telegram setup section |
| `_bmad-output/planning-artifacts/epics.md` | Add Story 1.7 under Epic 1 |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | Add `1-7-telegram-assisted-setup: ready-for-dev` under epic-1 |

---

## 7. Out of Scope

- Re-running the Copilot Device Flow from `--login-telegram` (separate flag, separate concern)
- Support for Telegram group chats or channels (individual chat only, same as current implementation)
- Any change to the notification message format (Epic 6 — done)
- Adding Telegram setup to the initial `bmad-bot init` interactive wizard (follow-up, not this story)

---

## 8. Open Questions for SM

1. **Story placement** — Add as Story 1.7 in Epic 1, or create a new mini-epic for "UX improvements"? Epic 1 is `done` in sprint-status — SM to decide if this reopens it or is tracked as a patch story.
2. **YAML patching strategy** — Simple line-by-line string replace (consistent with copilot token patching) or parse + serialize YAML properly? The latter is safer but adds a `serde_yaml` write dependency. SM/Architect to advise.
3. **`getUpdates` offset** — Should the flow call `getUpdates` once at startup to drain the queue and record the current offset, or simply ignore messages older than flow start time? Functionally equivalent but the offset approach is cleaner.