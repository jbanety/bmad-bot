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


## Deferred from: code review of story 12.3 (2026-04-19)

- **Unbounded `sessions` HashMap growth (no eviction):** `Arc<Mutex<HashMap<String, SubAgentState>>>` has no TTL, LRU, or max-size. A parent LLM that spawns many sub-agents grows the map for the daemon's lifetime. Story 12.4 explicitly owns the lifecycle — the 12.3 module surface has no cleanup API on purpose.
- **No upfront empty-`message` validation:** Parent passing `message: ""` flows through to the provider, which returns 400. Currently surfaced to the parent LLM via `build_error_json` (recoverable). Acceptable for v1; add client-side validation once usage patterns settle.
- **`#![allow(dead_code)]` + public re-export create a permanent silence blanket:** `spawn_agent.rs:1` has a module-wide `allow(dead_code)`, `tools/mod.rs` re-exports with `#[allow(unused_imports)]`. Once Story 12.4 wires the tool into `create_base_tools`, drop these allow attributes and let the compiler police unused code again.
- **Shutdown flag is a stored snapshot (staleness across parent-session rotation):** `SpawnAgentTool::new(..., shutdown: Option<ShutdownFlag>)` captures one flag for the tool's lifetime. If Story 12.4 rotates shutdown flags per parent story, the tool holds a stale reference. Address via 12.4 ownership/lifecycle design.
- **Shutdown race: sub-agent Err re-insert while daemon drops the `sessions Arc`:** On follow-up error, the code re-inserts the original state. If the daemon is concurrently dropping the sessions map (e.g., pipeline teardown), the re-insert is wasted work at best, a surprise panic at worst. Needs 12.4's lifecycle design to clarify.
- **Preamble negative-substring tests are brittle:** `test_build_sub_agent_preamble_excludes_{ask_supervisor,spawn_agent}` use `!preamble.contains(...)` checks. Any defensive bullet added later ("NEVER call spawn_agent") flips the test red even though the intent (don't advertise) still holds. Replace with a parser that extracts the "tools available" line and compares the set.
- **No cross-check between preamble tool names and rig Tool `NAME` constants:**
 `build_sub_agent_preamble` hardcodes `edit_file, read_file, grep, find_path, list_directory, git, terminal`. If any registered tool's `NAME` drifts (e.g., `list_dir`), the LLM attempts a non-existent tool. Add a small integration test that walks the registered tools and asserts each appears in the preamble string.


## Deferred from: code review of story 13.2 (2026-04-22)

- **`review` stories prioritized first by `status_priority()`, guaranteed to fail with placeholder:** `status_priority("review")` returns 0 (highest), so review stories are ordered before ready-for-dev stories. Now that the guard is removed, review stories enter the pipeline first, hit the placeholder error, and waste a notification before actionable ready-for-dev stories. Resolves when Story 13.6 implements the review phase.
- **Duplicated status string literals between `route_story_status` and `is_eligible`:** Both `route_story_status()` in `src/pipeline.rs` and `is_eligible()` in `src/watcher/mod.rs` match `"backlog"`, `"ready-for-dev"`, `"review"` as independent string literals. No shared constant links them. If someone adds a status to one but not the other, stories either enter the pipeline but fall through to Unknown, or are routed but never eligible.


## Deferred from: code review of story 13.4 (2026-04-23)

- **Adversarial trigger regex fragile:** Trigger pattern depends on LLM exact phrasing; plausible variations like "story context is now created" won't match. Design limitation inherited from the consultation mechanism (Story 13.3). [src/pipeline.rs:1290]
- **Critic trigger regex fragile:** Same concern for the critic consultation trigger; agent could say "updated the story based on feedback" and miss the pattern. [src/pipeline.rs:1305]
- **reload_story_info linear scan with no file locking:** Concurrent write to sprint-status.yaml during read could produce corrupt YAML parse. Pre-existing concern for all sprint-status reads across the daemon.
- **Test validates copy of message format, not actual code path:** `test_create_story_initial_message_format` manually reconstructs the format string and checks properties; doesn't call the actual `run_session()` code that builds it. Extracting into a testable function is a non-trivial refactor beyond this story.
- **Fragile string-match skill dispatch:** `skill_path.contains("bmad-create-story")` for initial message branching. Should become a typed enum when more skill types are added. Premature with only 2 skill types.
- **Duplicated preamble content:** `build_create_preamble()` shares ~40 lines with `build_preamble()` (tool rules, communication override, branch management). Future drift guaranteed as the daemon evolves. Extracting shared parts needs design discussion.


## Deferred from: code review of story 13.3 (2026-04-22)

- **`unwrap_or("")` on non-UTF-8 `project_root` in `ConsultationRunner::execute()`:** `self.project_root.to_str().unwrap_or("")` at `src/session/consultation.rs:240` silently degrades to empty string if the path contains non-UTF-8 bytes, causing cryptic `activate_agent` failures. Pre-existing pattern used in `session/agent.rs` and `tools/spawn_agent.rs`.


## Deferred from: code review of story 13.5 (2026-04-23)

- **Watcher entry path does not detect pre-existing PRs on GitLab:** GitLab provider maps HTTP 422 to `BranchNotFound` instead of `DuplicatePr`. When a `review` story re-enters via watcher and a PR already exists, GitLab fails instead of fetching the existing MR. Pre-existing GitLab provider limitation. [src/git_provider/gitlab.rs:214-230]
- **Watcher retry re-runs code review and may post duplicate PR comments:** No tracking of whether code review already completed in a previous pipeline iteration. If daemon crashes post-review but pre-done, the retry re-runs the entire review. Addressed by Story 13.10 (WAL phase tracking).
- **Recovery path (`process_recovered_session`) does not mark `review` on push failure:** Unlike `run_dev_pipeline()` which now marks `review` for retry, the recovery push-failure path at `src/pipeline.rs:2419-2441` leaves the story in `in-progress` limbo until daemon restart. Pre-existing gap not addressed by this story.
- **`update_story_status` regex replaces only first match:** `re.replace()` (not `replace_all()`) updates only the first occurrence of a story key in sprint-status.yaml. Duplicate keys in manually-edited files leave inconsistent state. [src/session/cleanup.rs:288]


## Deferred from: code review of story 13.6 (2026-04-24)

- **Dead code in `review/mod.rs` suppressed with `#[allow(dead_code)]`:** `ReviewRunner`, `ReviewOutcome`, `ReviewError`, `ParsedReviewReport`, and related functions are now dead code after Story 13.6 replaced `ReviewRunner` with `SessionRunner` in the pipeline. Module kept for `EpicReviewRunner` — clean removal deferred to a future cleanup story.
- **Crash recovery always uses `LlmRole::Dev`:** `resume_session()` at `src/session/runner.rs:589` builds the recovery agent with hardcoded `LlmRole::Dev` and the dev preamble, regardless of whether the crashed session was a code review. Story 13.10 (WAL Pipeline Phase Tracking) will add `pipeline_phase` to the WAL for correct role resolution during recovery.
- **`SpawnAgentTool` hardcodes `LlmRole::Dev`:** `build_agent_for_role()` at `src/session/runner.rs:954` always passes `LlmRole::Dev` to `create_spawn_agent_tool`. Sub-agents spawned from non-dev sessions use the wrong provider/model pairing. Pre-existing from Story 12.4.
- **Logging/UI calls hardcode `"dev"` label for all roles:** `run_session()` uses `"dev"` as the label in all `log_llm_request`, `log_llm_response`, and `self.ui.llm_request` calls regardless of actual role. Makes it impossible to distinguish dev vs. review session activity in logs. Story 13.11 (UI events for new phases) is the right place.
- **Stringly-typed skill path dispatch:** `run_session()` branches initial message via `skill_path.contains("bmad-create-story")` / `skill_path.contains("bmad-code-review")`. Should become a typed enum when more skill types are added. Pre-existing pattern extended by this story.
- **Critic preamble prohibits `edit_file` but `ConsultationToolSet::Restricted` still includes it:** Enforcement is prompt-only. Story 13.9 may introduce a `ReadOnly` tool set variant for stricter enforcement.

## Deferred from: code review of story 12.4 (2026-04-20)

- **`build_agent_for_role` hardcodes `LlmRole::Dev` for `SpawnAgentTool`:** `src/session/runner.rs:844` always passes `LlmRole::Dev` to `create_spawn_agent_tool` even though the enclosing function accepts a `role: LlmRole` parameter. All current call sites pass `LlmRole::Dev` so no bug today, but if a future caller passes a different role, sub-agents would use the wrong provider/model pairing.
- **Dev sub-agent state leaks into review session within same `process_story` call:** The `StorySubAgentCleanup` RAII guard wraps the entire `process_story` scope. Sub-agents spawned during the dev phase remain in the shared `sub_agent_sessions` map when the review phase begins. Impact is negligible (UUIDs are opaque and the review LLM has no mechanism to discover them), but a defensive clear between phases would enforce stricter isolation.
- **Sub-agent sessions accumulate across review runner retries:** Each retry in `ReviewRunner::run()` may spawn sub-agents that remain in `sub_agent_sessions`. Memory growth is bounded by `MAX_SESSION_RETRIES=2` and practical sub-agent spawn limits per session, but could be significant if sub-agents carry large conversation histories. `build_sub_agent_preamble` hardcodes `edit_file, read_file, grep, find_path, list_directory, git, terminal`. If any registered tool's `NAME` drifts (e.g., `list_dir`), the LLM attempts a non-existent tool. Add a small integration test that walks the registered tools and asserts each appears in the preamble string.
