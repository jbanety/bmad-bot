# Story 6.4: Context Window Limit Recovery

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want the daemon to recover from context window limit errors without losing session progress,
So that long or complex stories can still be completed autonomously.

## Acceptance Criteria

1. **Given** the agent session is active and the chat history has grown large **When** the LLM API returns a context limit error **Then** the error is detected from the provider response in the chat loop **And** the recovery process is initiated (not a crash — a controlled recovery)

2. **Given** a context limit error has been detected **When** the recovery process starts **Then** the full chat_history is read from the in-memory `SessionState` (which mirrors the WAL, already persisted after each turn) **And** the last N exchanges are extracted verbatim as immediate context **And** a separate, fresh LLM call is made (new context, not the exhausted one) to summarize the full chat_history into a compact session summary

3. **Given** the summary has been generated **When** the new session is bootstrapped **Then** a fresh agent is constructed with the same provider/model config and the standard dev preamble + tools **And** the daemon drives the BMAD activation flow as a simulated human: sends "CH" to enter chat mode, then sends "Load the project context" so the agent loads what it needs via its tools (same pattern as Story 3.2 Architect session) **And** the daemon then sends a recovery message containing the session summary, last N verbatim exchanges, and instruction to continue on the current story **And** the session enters direct chat mode (not re-entering the full dev-story workflow pipeline, since checkboxes and Dev Agent Record are already up to date on disk)

4. **Given** the new session is bootstrapped **When** the chat loop resumes **Then** the agent picks up the current task with full awareness of prior work **And** the recovery event is logged via `tracing::info!()` with `action = "context_limit_recovery"`, original history length, and summary length **And** the WAL file is updated with the new (compressed) session state

## Functional Requirements Covered

- **NFR-REL1:** Transient LLM errors recovered with exponential backoff, max 3 retries per call
- **NFR-REL3:** Crash recovery produces clean state — no corrupted branches, no half-committed files
- **Architecture Decision 3:** Session State Persistence — WAL File for Crash & Context Limit Recovery (Recovery Case B)

## Dependencies

- **Story 6.3 (Crash Recovery via Session WAL) MUST be completed first.** This story builds on the `run_session()` refactor from 6.3 (the `Option<SessionState>` parameter) and reuses `RecoveryInfo`, `story_info_from_wal()`, and the WAL infrastructure. Story 6.3 also introduces the pattern of rebuilding agents from WAL state that this story extends.
- **Story 6.2 (HTTP Retry & Error Resilience) MUST be completed first.** This story uses `StoryPipeline` created by 6.2.

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [x] 🚨 **BLOCKING**: Verify `run_session()` in `src/session/runner.rs` accepts `recovered_state: Option<SessionState>` as its last parameter (added by Story 6.3). **If this parameter does NOT exist, STOP — Story 6.3 MUST be implemented first.** This story's recovery design calls `run_session()` recursively with `Some(compressed_state)` for the inner loop, which is impossible without this parameter.
- [x] Verify `SessionState` in `src/session/state.rs` with `save()`, `load()`, `to_rig_messages()`, `add_user_message()`, `add_assistant_message()`, `chat_history: Vec<ChatMessage>`
- [x] Verify `ChatMessage` struct: `role: String`, `content: String` (derives `Clone`)
- [x] Verify `SessionRunner` has `build_anthropic_agent()`, `build_openai_agent()`, `build_preamble()`, `create_tools()`, `state_file_path`
- [x] Verify `resolve_api_key()` in `src/session/provider.rs`
- [x] Verify `EscalationSlot` type alias in `src/supervisor/mod.rs`
- [x] Verify `DecisionLog` in `src/supervisor/decisions.rs`
- [x] Verify `ResponseAnalyzer` in `src/session/analyzer.rs`
- [x] Verify `rig-core` version 0.30 — `agent.chat()` returns `Result<String, PromptError>`, errors surface as `CompletionError::ProviderError(String)` or `CompletionError::ResponseError(String)`
- [x] Verify project-context.md exists at `_bmad-output/project-context.md`

### Task 1: Add Context Limit Error Detection (`src/session/runner.rs`)

- [x] Add helper function `fn is_context_limit_error(error: &str) -> bool`:
  ```rust
  /// Detect context window / token limit errors from LLM provider error strings.
  ///
  /// Each provider returns different error messages. This function checks for
  /// known patterns across Anthropic, OpenAI, and GitHub Models.
  fn is_context_limit_error(error: &str) -> bool {
      let lower = error.to_lowercase();
      // Anthropic patterns
      lower.contains("context_length_exceeded")
          || lower.contains("prompt is too long")
          || lower.contains("maximum context length")
          // OpenAI patterns
          || lower.contains("maximum context length")
          || lower.contains("context_length_exceeded")
          || lower.contains("max_tokens")
          || lower.contains("token limit")
          || lower.contains("context window")
          // Generic patterns
          || lower.contains("too many tokens")
          || lower.contains("input too long")
          || lower.contains("exceeds the model")
  }
  ```
- [x] This is a pure function — easy to test, no dependencies

### Task 2: Modify Chat Loop Error Handling in `run_session()` (`src/session/runner.rs`)

- [x] In the `Err(e)` branch of `agent.chat(&reply, history).await` (around line 626-649):
- [x] BEFORE the existing retry logic, add context limit detection:
  ```rust
  Err(e) => {
      let error_str = e.to_string();

      // Check for context limit error BEFORE retry logic
      if is_context_limit_error(&error_str) {
          tracing::warn!(
              action = "context_limit_detected",
              turn = %turn,
              history_len = %state.chat_history.len(),
              error = %error_str,
              "Context window limit hit — initiating recovery"
          );

          // Remove the user message we just added (it failed)
          state.chat_history.pop();

          // Recovery runs its own inner chat loop to completion via run_session().
          // It returns a terminal SessionOutcome — the current loop exits.
          let outcome = self.context_limit_recovery(
              state, story, provider, model, base_branch,
              escalation_slot.clone(), decision_log.clone(),
              0, // recovery_depth: first recovery attempt
          ).await;

          // Write decisions regardless of outcome
          self.write_decisions(story, &decision_log).await;
          return outcome;  // Exit the current run_session()
      }

      // Existing retry logic for non-context-limit errors continues below...
      retries += 1;
      // ... (rest of existing retry code unchanged)
  }
  ```
- [x] The context limit check MUST come BEFORE the `retries += 1` line — retrying a context limit error is pointless (same history = same error)
- [x] The failed user message MUST be popped from `state.chat_history` before recovery (it was added by `state.add_user_message(&reply)` before the `agent.chat()` call)
- [x] Recovery is a **terminal action** for the current chat loop — the method returns `SessionOutcome` directly and the caller does `return outcome`. The fresh agent runs its own inner loop via `run_session(Some(compressed_state))`.
- [x] Pass `recovery_depth: 0` on first recovery. Inside `context_limit_recovery()`, if another context limit is hit, the inner `run_session()` will call recovery again with `recovery_depth + 1`, up to `MAX_RECOVERY_DEPTH = 3`.

### Task 3: Implement `context_limit_recovery()` Method (`src/session/runner.rs`)

This is the unified recovery method. It summarizes history, builds a fresh agent with the **standard** dev preamble, drives the BMAD activation flow (CH → Load project context) as a simulated human (same pattern as Story 3.2 Architect session), then sends a recovery message and delegates to `run_session(Some(compressed_state))` for the inner chat loop. It returns `SessionOutcome` directly — the caller treats recovery as a terminal action.

**Key design principle (from Story 3.2):** "Treat it like a human" — the agent knows WHAT project files to load and HOW to interpret them. The daemon does NOT inject project context into the preamble. Instead, the daemon drives the standard BMAD activation flow, and the agent loads its own context via its tools.

- [x] Add constants:
  ```rust
  const RECOVERY_KEEP_LAST_EXCHANGES: usize = 10; // 10 exchanges = 20 messages
  const MAX_RECOVERY_DEPTH: usize = 3;
  ```

- [x] Add method signature to `SessionRunner`:
  ```rust
  /// Recover from a context window limit error by summarizing history and
  /// bootstrapping a fresh session following the BMAD activation pattern.
  ///
  /// Architecture Decision 3, Recovery Case B. The method:
  /// 1. Extracts last N exchanges from in-memory state as immediate context
  /// 2. Makes a fresh LLM call to summarize the full history
  /// 3. Builds a fresh agent with the STANDARD dev preamble + all tools
  /// 4. Drives BMAD activation: "CH" → "Load the project context" (agent
  ///    loads what it needs via its tools — same pattern as Story 3.2)
  /// 5. Sends recovery message (summary + last N exchanges + continue instruction)
  /// 6. Builds compressed SessionState with activation turns + recovery message
  /// 7. Calls `run_session()` with `Some(compressed_state)` — reuses the existing
  ///    chat loop instead of duplicating it
  /// 8. Returns the `SessionOutcome` from the inner loop directly
  ///
  /// If `recovery_depth >= MAX_RECOVERY_DEPTH`, returns `SessionOutcome::Failed`
  /// to prevent infinite recursion.
  async fn context_limit_recovery(
      &self,
      state: &SessionState,
      story: &StoryInfo,
      provider: &str,
      model: &str,
      base_branch: &str,
      escalation_slot: EscalationSlot,
      decision_log: DecisionLog,
      recovery_depth: usize,
  ) -> SessionOutcome
  ```

- [x] **Step 0 — Check recovery depth:**
  - [x] If `recovery_depth >= MAX_RECOVERY_DEPTH`:
    ```rust
    tracing::error!(
        action = "context_limit_max_depth",
        depth = %recovery_depth,
        "Max recovery depth reached — aborting"
    );
    return SessionOutcome::Failed {
        story_key: story.story_key.clone(),
        error: format!("Context limit recovery exceeded max depth ({MAX_RECOVERY_DEPTH})"),
        decisions: decision_log.records(),
    };
    ```

- [x] **Step 1 — Extract last N exchanges:**
  - [x] Call `extract_last_exchanges(&state.chat_history, RECOVERY_KEEP_LAST_EXCHANGES)` (see Task 4 helpers)
  - [x] Call `format_exchanges_for_message(&last_exchanges)` to format for the recovery message

- [x] **Step 2 — Summarize full history via fresh LLM call:**
  - [x] Call `self.summarize_history(state, story, provider, model).await`
  - [x] If summarization fails → return `SessionOutcome::Failed` with error detail
  - [x] Log: `tracing::info!(action = "context_limit_summary_generated", original_len = %state.chat_history.len(), summary_len = %summary.len())`

- [x] **Step 3 — Build fresh agent with STANDARD preamble + tools:**
  - [x] Use `self.build_preamble(story)` — the standard dev agent preamble, NOT an enhanced one
  - [x] Resolve API key via `resolve_api_key(provider, &self.secrets)`
  - [x] Match on provider string (same pattern as `run()` and `resume_session()`):
    - [x] `"anthropic"` → build Anthropic client, create agent with **standard preamble** + all 4 tools
    - [x] `"openai"` → build OpenAI client, same pattern
    - [x] `"github-models"` → build OpenAI client with base URL override, same pattern
  - [x] The `Chat` trait is NOT object-safe — `agent` must be built and used within the same match arm. All subsequent steps (4, 5, 6) happen **inside** the match arm.

- [x] **Step 4 — Drive BMAD activation flow (inside provider match arm):**
  This follows the exact same pattern as Story 3.2 (Architect session). The daemon acts as a simulated human driving the agent through its activation steps.
  - [x] Initialize `activation_history: Vec<Message>` as empty
  - [x] **Turn 1 — Enter chat mode:**
    ```rust
    let response = agent.chat("CH", activation_history.clone()).await
        .map_err(|e| format!("Recovery activation CH failed: {e}"))?;
    activation_history.push(Message::user("CH"));
    activation_history.push(Message::assistant(&response));
    // Response is the agent greeting — discard content, keep in history for context
    ```
  - [x] **Turn 2 — Load project context:**
    ```rust
    let response = agent.chat("Load the project context", activation_history.clone()).await
        .map_err(|e| format!("Recovery activation load context failed: {e}"))?;
    activation_history.push(Message::user("Load the project context"));
    activation_history.push(Message::assistant(&response));
    // Agent uses ReadFile/tools to load config, architecture, PRD, etc. — discard response content
    ```
  - [x] If either activation turn fails → return `SessionOutcome::Failed` with error detail
  - [x] Log: `tracing::info!(action = "context_limit_activation_complete", "BMAD activation flow completed for recovery agent")`

- [x] **Step 5 — Build recovery message and compressed SessionState:**
  - [x] Build the recovery message (sent as the last user message in the compressed state):
    ```
    === SESSION RECOVERY — Context Window Limit Reached ===
    Your previous session hit the context window limit. Below is your recovery context:

    === Session Summary ===
    {summary}

    {last_n_exchanges_formatted}

    === Current Story ===
    The story file is at: {specs_path}
    Read this file to see current task checkboxes and progress.
    Continue working directly on the current task. Do NOT restart the workflow from the beginning.
    ```
  - [x] Build compressed `SessionState` (see Task 5 for details):
    - [x] `chat_history` = activation turns (CH + response, Load context + response) + recovery_message as final user message
    - [x] The recovery message is the LAST message (role = user) — `run_session(Some(state))` will detect this and re-send it to the agent (Story 6.3 recovery path: "last msg = user → re-send last user message")
  - [x] Metadata (story_id, branch, provider, model, etc.) preserved from original state

- [x] **Step 6 — Delegate to `run_session()` with compressed state (still inside match arm):**
  ```rust
  // Inside the "anthropic" match arm (openai/github-models are identical pattern):
  // Agent already built in Step 3 with standard preamble + tools
  // Activation turns already completed in Step 4

  // Delegate to run_session() — reuses the existing chat loop
  // run_session sees last msg = user (recovery message) → re-sends it → agent responds → loop continues
  let outcome = self.run_session(
      &agent, story, provider, model, base_branch,
      escalation_slot.clone(), decision_log.clone(),
      Some(compressed_state), // ← recovery state with activation turns + recovery message
  ).await;
  ```
  - [x] IMPORTANT: `EscalationSlot` and `DecisionLog` are the SAME instances from the parent call — decision continuity is preserved across recovery boundary

- [x] **Step 7 — Log and return:**
  - [x] `tracing::info!(action = "context_limit_recovery", depth = %recovery_depth, original_history_len = %state.chat_history.len(), "Context limit recovery delegated to inner run_session()")`
  - [x] Return the `SessionOutcome` from the inner `run_session()` call directly
  - [x] If another context limit is hit inside the inner loop, `run_session()` will call `context_limit_recovery()` again with `recovery_depth + 1` — the depth check in Step 0 prevents infinite recursion

**Design rationale:** By following the BMAD activation pattern (Story 3.2), the agent loads its own project context via its tools — it knows WHAT to load and HOW to interpret it. The daemon does not need to decide which docs are relevant or inject them into the preamble. By delegating to `run_session(Some(compressed_state))`, we reuse ALL existing chat loop logic (completion detection, escalation handling, failure handling, WAL management, turn counting). No code duplication. The fresh agent has a clean context with proper BMAD activation, and the compressed state's `chat_history` contains the activation turns plus the recovery message.

### Task 4: Implement Helper Functions (`src/session/runner.rs`)

- [x] `fn extract_last_exchanges(history: &[ChatMessage], n: usize) -> Vec<ChatMessage>`:
  - [x] Extract the last `n * 2` messages (n exchanges = n user + n assistant messages)
  - [x] If history has fewer messages, return all of them
  - [x] **Odd message count handling:** If history length is odd (e.g., 21 messages — unpaired trailing user message), round DOWN to the nearest even number before slicing. The orphan message is excluded to keep clean user/assistant pairs. Example: history of 21, N=10 → take last 20 messages (10 pairs), the orphan 1st message is dropped.
  - [x] Return cloned messages (`ChatMessage` derives `Clone`)

- [x] `fn format_exchanges_for_message(exchanges: &[ChatMessage]) -> String`:
  - [x] Format as readable text for inclusion in the recovery message:
    ```
    === Recent Conversation (last N exchanges) ===
    User: {content}
    Assistant: {content}
    ...
    ```
  - [x] Truncate individual messages if extremely long (> 2000 chars) with `"... [truncated]"` to keep the recovery message within reasonable bounds

- [x] `async fn summarize_history(&self, state: &SessionState, story: &StoryInfo, provider: &str, model: &str) -> Result<String, String>`:
  - [x] Format the full `state.chat_history` as text (`"User: {content}\nAssistant: {content}\n..."`)
  - [x] Resolve API key via `resolve_api_key(provider, &self.secrets)`
  - [x] Build a minimal agent (no tools) with summarization preamble — match on provider string (same pattern as elsewhere):
    ```rust
    // Anthropic example (OpenAI/GitHub-Models follow same pattern):
    let client = anthropic::Client::builder().api_key(&api_key).build()
        .map_err(|e| format!("Summarization client failed: {e}"))?;
    let agent = client.agent(model)
        .preamble("You are a technical session summarizer. Be concise but comprehensive.")
        .build();
    ```
  - [x] Build the summarization prompt (sent as the user message with empty history):
    ```
    You are summarizing a development session for continuity. The session is implementing
    story {story_key} in a software project. Provide a concise but comprehensive summary
    covering:
    - What tasks have been completed (with file paths and key decisions)
    - What task is currently in progress
    - Any issues encountered and how they were resolved
    - Key code patterns and conventions established
    - Current state of the implementation

    Here is the full conversation history:
    {formatted_full_history}
    ```
  - [x] Call `agent.chat(&summarization_prompt, vec![]).await` — empty history, prompt-only
  - [x] **Fallback on failure:** If the summarization call fails with a context limit error (the history itself is too large for a fresh context), retry with truncated history (last 50% of messages). If that also fails → return `Err("Summarization failed even with truncated history: {e}")`
  - [x] Return the summary string

- [x] `fn build_recovery_message(&self, story: &StoryInfo, summary: &str, formatted_exchanges: &str) -> String`:
  - [x] Assemble the recovery message that will be sent as a user message after BMAD activation:
    ```
    === SESSION RECOVERY — Context Window Limit Reached ===
    Your previous session hit the context window limit. Below is your recovery context:

    === Session Summary ===
    {summary}

    {formatted_exchanges}

    === Current Story ===
    The story file is at: {specs_path}
    Read this file to see current task checkboxes and progress.
    Continue working directly on the current task. Do NOT restart the workflow from the beginning.
    ```
  - [x] This is a plain user message, NOT a preamble — the agent already has its standard preamble and has already loaded project context via the BMAD activation flow (CH → Load project context)
  - [x] Return the message string

### Task 5: Build New Compressed `SessionState` (`src/session/runner.rs`)

- [x] When building the new state after BMAD activation (Step 4 in Task 3), include the activation turns and recovery message:
  ```rust
  // activation_history contains: CH/response + "Load the project context"/response (4 messages)
  // Convert activation_history (Vec<Message>) to Vec<ChatMessage> for SessionState
  let mut compressed_history: Vec<ChatMessage> = activation_history.iter().map(|msg| {
      ChatMessage {
          role: match msg { Message::User(_) => "user", Message::Assistant(_) => "assistant", .. }.to_string(),
          content: msg.content().to_string(),
      }
  }).collect();

  // Add recovery message as the final user message
  // run_session(Some(state)) will detect last msg = user and re-send it
  compressed_history.push(ChatMessage {
      role: "user".to_string(),
      content: recovery_message.clone(),
  });

  let new_state = SessionState {
      story_id: state.story_id.clone(),
      story_key: state.story_key.clone(),
      branch: state.branch.clone(),
      started_at: state.started_at.clone(),  // preserve original start time
      last_activity: chrono::Utc::now().to_rfc3339(),
      provider: state.provider.clone(),
      model: state.model.clone(),
      branch_name: state.branch_name.clone(),
      base_branch: state.base_branch.clone(),
      chat_history: compressed_history,       // activation turns + recovery message (user)
  };
  ```
- [x] The compressed state's `chat_history` contains:
  1. Activation turns: "CH" / response / "Load the project context" / response (4 messages)
  2. Recovery message as the last user message (1 message)
  3. Total: 5 messages — the recovery message is pending (no assistant response yet)
- [x] `run_session(Some(state))` sees the last message is `role = "user"` → pops and re-sends the recovery message → agent responds with its plan to continue → normal chat loop resumes
- [x] New messages from the recovered session are appended normally after this
- [x] The WAL is overwritten with the compressed state — the full history is gone from disk (the summary is captured in the recovery message, and the agent has loaded project context itself)
- [x] This ensures the WAL file stays small even across multiple recovery events

### Task 6: Unit Tests

- [x] `test_is_context_limit_error_anthropic_pattern` — verify detection of `"context_length_exceeded"`
- [x] `test_is_context_limit_error_openai_pattern` — verify detection of `"maximum context length"`
- [x] `test_is_context_limit_error_token_limit` — verify detection of `"token limit"`
- [x] `test_is_context_limit_error_too_many_tokens` — verify detection of `"too many tokens"`
- [x] `test_is_context_limit_error_case_insensitive` — verify `"CONTEXT_LENGTH_EXCEEDED"` matches
- [x] `test_is_context_limit_error_false_for_network_error` — verify `"connection refused"` returns false
- [x] `test_is_context_limit_error_false_for_auth_error` — verify `"invalid api key"` returns false
- [x] `test_is_context_limit_error_false_for_rate_limit` — verify `"rate limit exceeded"` returns false (rate limits should retry, not recover)
- [x] `test_extract_last_exchanges_normal` — verify 10 exchanges from 40-message history returns last 20 messages
- [x] `test_extract_last_exchanges_fewer_than_n` — verify 3-exchange history returns all 6 messages when N=10
- [x] `test_extract_last_exchanges_empty_history` — verify empty history returns empty vec
- [x] `test_extract_last_exchanges_odd_message_count` — verify odd-length history (e.g., 21 messages) rounds DOWN to nearest even count before slicing, dropping the orphan message to keep clean user/assistant pairs
- [x] `test_format_exchanges_for_message_basic` — verify correct "User: / Assistant:" formatting
- [x] `test_format_exchanges_for_message_truncates_long_messages` — verify messages > 2000 chars are truncated
- [x] `test_format_exchanges_for_message_empty` — verify empty exchanges produce reasonable output
- [x] `test_build_recovery_message_contains_all_sections` — verify output contains "SESSION RECOVERY", "Session Summary", "Recent Conversation", "Current Story"
- [x] `test_build_recovery_message_includes_story_path` — verify `specs_path` is in the output
- [x] `test_build_recovery_message_does_not_contain_project_context` — verify output does NOT contain "Project Context" section (project context is loaded by the agent via BMAD activation, not injected in the message)
- [x] `test_compressed_state_contains_activation_turns` — verify `chat_history` starts with "CH" user message and "Load the project context" user message
- [x] `test_compressed_state_last_message_is_recovery` — verify last message in `chat_history` is role "user" containing "SESSION RECOVERY"
- [x] `test_compressed_state_preserves_metadata` — verify `story_id`, `story_key`, `branch`, `provider`, `model` are preserved from original state
- [x] `test_compressed_state_updates_last_activity` — verify `last_activity` is refreshed
- [x] All tests use mocked data — NO real LLM calls, NO real file I/O (except tempdir for WAL)

### Task 7: Integration Verification

- [x] `cargo check` — 0 errors
- [x] `cargo test` — all existing + new tests pass, 0 regressions (574 passed)
- [x] `cargo clippy` — 0 new warnings
- [x] `cargo fmt` — clean
- [x] All public items have `///` doc comments

## Dev Notes

### Previous Story Intelligence

**From Story 6.3 (Crash Recovery via Session WAL) — immediate predecessor, MUST be done first:**
- `run_session()` now accepts `recovered_state: Option<SessionState>` as last parameter
- `RecoveryInfo` struct, `story_info_from_wal()` helper
- `resume_session()` method on `SessionRunner` for crash recovery
- `recover_and_process()` on `StoryPipeline` — single entry point
- WAL is ALWAYS deleted after recovery attempt (prevents infinite loops)
- `SessionState` does NOT implement `Clone` — consume by ownership. But `ChatMessage` DOES implement `Clone`
- Turn counter initialized to `state.chat_history.len() / 2` in recovery mode
- Three edge cases handled: last-msg-assistant, last-msg-user, empty-history

**From Story 6.2 (HTTP Retry & Error Resilience) — MUST be done first:**
- `StoryPipeline` with `process_story()`, `process_eligible_stories()`
- Pipeline handles post-session flow: review → PR → notify
- Layer 3 error handling: session failures create failure PRs, daemon never stops

**From Story 6.1 (Telegram Notifications):**
- `Notifier` trait, `TelegramNotifier`, `NoopNotifier`
- All notification failures are non-blocking

**From Story 4.2 (Agent Session Setup & Chat Loop) — chat loop owner:**
- `run_session()` is a **private** async method with generic `<A: Chat>` — different concrete return types per provider
- `Chat` trait is NOT object-safe — cannot use `Box<dyn Chat>`. Must match on provider string and build concrete type
- Error from `agent.chat()` is `PromptError` (rig 0.30) — `.to_string()` produces the error message containing provider-specific text
- Chat loop retry pattern: on `Err(e)`, increment retries, pop last user message, continue. After MAX_RETRIES=3 → `SessionOutcome::Failed`
- WAL saved after each successful turn via `state.save(&self.state_file_path).await`
- The `state.chat_history.pop()` pattern is already used for retry (remove the failed user message)

**From Story 4.3 (Pre-Development Preparation & Branch Management):**
- `SessionState` fields `branch_name` and `base_branch` with `serde(default)` for backward compat

**From Story 5.2 (Automated Code Review):**
- `ReviewRunner` also uses the `<A: Chat>` generic pattern with provider matching
- Confirms that the "build agent inside match arm" pattern is the established approach

### Git Intelligence (Last 5 Commits)

1. `cd93cc6` docs(stories): create story 6-3 crash recovery via session WAL and update sprint status
2. `a57a125` docs(stories): create story 6-2 HTTP retry and error resilience and update sprint status
3. `97b7c80` docs(stories): create story 6-1 telegram notifications and update sprint status
4. `cdc25c3` feat(git-provider): implement GitLabProvider with full GitProvider trait support
5. `dea1232` feat(review): implement automated code review session runner

### Core Design — Context Window Limit Recovery Flow

Architecture Decision 3, Recovery Case B. This is a controlled, mid-session recovery — NOT a crash. Recovery is a **terminal action** for the current chat loop — it builds a fresh agent, drives the BMAD activation flow, then delegates to a NEW `run_session()` call and returns the outcome.

**Key principle (from Story 3.2):** "Treat it like a human" — the daemon drives the agent through its standard activation flow. The agent loads its own project context via its tools. The daemon does NOT inject project files into the preamble.

```
chat loop running
└── agent.chat(&reply, history).await → Err(e)
    └── is_context_limit_error(&e.to_string())?
        ├── NO → existing retry logic (retries += 1, pop user msg, continue)
        └── YES → context_limit_recovery(state, ..., recovery_depth=0)
            ├── check recovery_depth < MAX_RECOVERY_DEPTH (3)
            │   └── depth exceeded → return SessionOutcome::Failed
            ├── extract last N exchanges from state.chat_history (clone)
            ├── format exchanges for recovery message
            ├── summarize_history() — fresh LLM call with full history
            │   ├── build minimal agent (no tools, just summarization prompt)
            │   ├── call agent.chat(summarization_prompt, [])
            │   └── return summary string
            ├── build fresh agent with STANDARD preamble (build_preamble) + 4 tools
            ├── drive BMAD activation flow (same pattern as Story 3.2):
            │   ├── agent.chat("CH", []) → greeting [discard, keep in history]
            │   ├── agent.chat("Load the project context", history) →
            │   │   agent uses tools to load config, architecture, PRD, etc.
            │   │   [discard response, keep in history]
            │   └── activation_history now has 4 messages (2 exchanges)
            ├── build_recovery_message(summary + last N exchanges + continue instruction)
            ├── build compressed SessionState:
            │   └── chat_history = activation turns + recovery_message (as last user msg)
            ├── call run_session(&agent, ..., Some(compressed_state))
            │   └── run_session sees last msg = user → re-sends recovery message
            │       └── inner loop runs to completion with fresh context
            │           ├── if context limit hit again → recursive recovery(depth+1)
            │           ├── Completed → WAL deleted, return Completed
            │           ├── Escalated → WAL deleted, return Escalated
            │           └── Failed → WAL preserved, return Failed
            └── return SessionOutcome from inner run_session()
                └── caller does `return outcome` — exits the outer loop
```

**Key difference from Story 6.3 (crash recovery):**
- Crash recovery happens at daemon startup — context limit recovery happens mid-session
- Crash recovery rebuilds from WAL on disk — context limit recovery uses in-memory state (WAL is backup)
- Crash recovery re-enters `run_session()` — context limit recovery ALSO re-enters `run_session()` but with a fresh agent and compressed state
- Crash recovery always deletes WAL after — context limit overwrites WAL with compressed state (handled by inner `run_session()`)
- Context limit recovery has a depth limit (`MAX_RECOVERY_DEPTH = 3`) to prevent infinite recursion
- Context limit recovery drives the BMAD activation flow (CH → Load project context) before resuming — the agent loads its own context via tools, just like a human would drive it

### How `agent.chat()` Errors Surface

In rig 0.30, `agent.chat()` returns `Result<String, PromptError>`. The error chain:

1. Provider API returns HTTP 400 with body like `{"error": {"type": "invalid_request_error", "message": "prompt is too long: 204835 tokens > 200000 maximum"}}`
2. rig parses this into `CompletionError::ProviderError(String)` containing the error message
3. This surfaces to `agent.chat()` as a `PromptError` wrapping the `CompletionError`
4. Calling `.to_string()` on the error produces a string containing the provider's error message

The `is_context_limit_error()` function matches against `.to_string()` output — it must cover patterns from all three supported providers (Anthropic, OpenAI, GitHub Models/Azure).

### Why NOT Retry Context Limit Errors

The existing retry logic retries with the SAME history. For context limit errors, the history is too long — retrying will fail with the exact same error. Context limit recovery MUST intercept BEFORE the retry counter to avoid wasting 3 failed retries.

### Summarization Call — Design Details

The summarization call uses a FRESH LLM context (empty history). Key constraints:

1. **Provider:** Use the SAME provider/model as the dev session — ensures consistent quality
2. **Tools:** NO tools needed — summarization is text-only
3. **Preamble:** Simple instruction: "You are summarizing a development session"
4. **Input:** The full `chat_history` formatted as text (user/assistant turns)
5. **Risk:** If the full history is SO large it exceeds context even for summarization → fallback to truncated history (last 50%)
6. **Output:** A structured summary suitable for inclusion in the recovery message

### Token Budget — BMAD Activation + Recovery Message

The recovery approach uses the standard preamble (not enhanced) and drives BMAD activation turns. Token budget:

| Component | Estimated tokens |
|---|---|
| Standard preamble (dev.md + override) | ~2,000-5,000 |
| BMAD activation turns (CH + Load context — 4 messages) | ~2,000-8,000 (varies: agent loads files via tools) |
| Recovery message (summary + last N exchanges + story ref) | ~2,500-12,000 |
| **Total initial context** | **~6,500-25,000** |

This leaves 175K-193K tokens for new conversation (Anthropic 200K window). The BMAD activation turns are slightly more expensive than injecting project-context.md directly into the preamble, because the agent may load additional files it deems relevant. However, this is the correct trade-off: the agent knows WHAT context it needs and loads it through the standard workflow, producing better results than a daemon-curated preamble.

OpenAI models with smaller windows may need fewer kept exchanges — but N=10 is a reasonable default for MVP.

### Design Decision: Chat Trait Not Object-Safe — Implications for Recovery

The `Chat` trait in rig 0.30 is NOT object-safe (`fn chat()` returns `impl Future`). This drives the recovery design:

- Cannot use `Box<dyn Chat>` — must match on provider string and build concrete agent types
- The old agent's context is exhausted after a context limit error — cannot reuse it
- **Solution:** `context_limit_recovery()` builds a fresh agent inside a provider match arm, drives BMAD activation turns (`agent.chat("CH", ...)` and `agent.chat("Load the project context", ...)`) within the same arm, then calls `run_session(Some(compressed_state))` also within that arm. This reuses the existing chat loop and all completion/escalation/failure handling. The method returns `SessionOutcome` directly — the outer loop exits.
- The BMAD activation turns (CH + Load project context) happen on the SAME agent instance that is passed to `run_session()` — rig agents are stateless (history is passed as parameter), so the activation context is carried via the compressed state's `chat_history`
- Recursive recovery (if inner loop also hits context limit) is bounded by `MAX_RECOVERY_DEPTH = 3`

### Error Handling in Context Limit Recovery

| Error | Response | WAL State |
|---|---|---|
| `recovery_depth >= MAX_RECOVERY_DEPTH` | Return `SessionOutcome::Failed` | WAL preserved (original state) |
| API key resolution fails | Return `SessionOutcome::Failed` | WAL preserved (original state) |
| Summarization LLM call fails | Try with truncated history (50%); if still fails → `Failed` | WAL preserved |
| Agent build fails | Return `SessionOutcome::Failed` | WAL preserved |
| Recovery succeeds → inner `run_session()` completes | `Completed` | WAL deleted by inner loop |
| Recovery succeeds → inner `run_session()` escalates | `Escalated` | WAL deleted by inner loop |
| Recovery succeeds → inner `run_session()` fails | `Failed` | WAL preserved by inner loop (with compressed state) |
| Recovery succeeds → inner loop hits context limit again | Recursive `context_limit_recovery(depth+1)` | Handled by inner recursion |

### Architecture Compliance

| Constraint | Implementation |
|---|---|
| No new modules | All code in `src/session/runner.rs` — helper functions + recovery method |
| Error handling | Returns `SessionOutcome` variants — integrates with existing pipeline |
| Error field pattern | `{ reason: String }` for any new error types |
| Sequential execution | Recovery is synchronous within the session — no parallel sessions |
| WAL update | Compressed state overwrites WAL — keeps file small |
| Logging | `tracing` only — structured fields with `action = "context_limit_recovery"` |
| Doc comments | `///` on all new public/private functions |
| Tests | Inline `#[cfg(test)] mod tests` — mock data only |
| No unsafe | No unsafe code |
| BMAD sacred | No modifications to `_bmad/` |

### Existing Code to Reuse (DO NOT Reinvent)

| Component | Location | What to use |
|---|---|---|
| `SessionState` | `src/session/state.rs` | WAL state, `save()`, `to_rig_messages()`, `add_user_message()`, `add_assistant_message()` |
| `ChatMessage` | `src/session/state.rs` | Message format, derives `Clone` |
| `SessionRunner` | `src/session/runner.rs` | `build_anthropic_agent()`, `build_openai_agent()`, `build_preamble()`, `create_tools()`, `run_session()` |
| `resolve_api_key()` | `src/session/provider.rs` | API key resolution |
| `ResponseAnalyzer` | `src/session/analyzer.rs` | Response pattern matching (used by inner loop) |
| `EscalationSlot` | `src/supervisor/mod.rs` | Shared escalation slot (passed through to inner loop) |
| `DecisionLog` | `src/supervisor/decisions.rs` | Decision tracking (shared across recovery boundary) |
| `SessionOutcome` | `src/session/mod.rs` | `Completed`, `Escalated`, `Failed` |
| `run_session()` | `src/session/runner.rs` | Reuse for inner loop after recovery (with `Some(compressed_state)`) |
| Story 3.2 pattern | `src/supervisor/architect.rs` | BMAD activation flow: CH → Load project context → question (same simulated-human pattern) |

⚠️ **Do NOT reimplement any of these.** In particular:
- Do NOT create a new chat loop — reuse `run_session()` with `Some(compressed_state)` for the inner loop
- Do NOT create an "enhanced preamble" that injects project-context.md — the agent loads its own context via the BMAD activation flow (CH → "Load the project context"), same pattern as Story 3.2's Architect session. Use `self.build_preamble()` for the standard preamble.
- Do NOT try to pre-select which project files the agent needs — the agent knows WHAT to load and HOW to interpret it (Story 3.2 principle: "Treat it like a human")
- Do NOT create a new WAL format — overwrite existing WAL with compressed `SessionState`
- Do NOT create a summarization-specific LLM infrastructure — build a minimal temp agent inline (no tools, summarization preamble only)

### Library & Framework Requirements

| Dependency | Version | Purpose | Already in Cargo.toml |
|---|---|---|---|
| `rig-core` | 0.30 | Chat trait, agent building, providers | ✅ Yes |
| `tokio` | latest | Async runtime | ✅ Yes |
| `tracing` | 0.1 | Structured logging | ✅ Yes |
| `serde` / `serde_yml` | latest | WAL serialization | ✅ Yes |
| `chrono` | latest | Timestamps | ✅ Yes |
| `thiserror` | 2 | Error types | ✅ Yes |

**No new dependencies needed.**

### File Structure Requirements

**Files to modify:**
- `src/session/runner.rs` — **MODIFY** — Add `is_context_limit_error()`, `context_limit_recovery()`, `summarize_history()`, `build_recovery_message()`, `extract_last_exchanges()`, `format_exchanges_for_message()`. Modify `run_session()` error handling to intercept context limit errors before retry logic.

**Files NOT to touch:**
- `src/session/state.rs` — WAL infrastructure is complete
- `src/session/mod.rs` — No new public types to export
- `src/session/branch.rs` — Branch operations are complete
- `src/session/analyzer.rs` — Response analysis is complete
- `src/session/provider.rs` — Provider factory is complete
- `src/session/cleanup.rs` — Cleanup operations are complete
- `src/pipeline.rs` — Pipeline unchanged (recovery is invisible to pipeline)
- `src/cli/mod.rs` — CLI unchanged (recovery is invisible to caller)
- `src/config/` — Config is complete
- `src/watcher/` — Watcher is complete
- `src/notifier/` — Notifier is complete
- `src/git_provider/` — Git provider is complete
- `src/review/` — Review runner is complete
- `src/supervisor/` — Supervisor is complete
- `src/tools/` — Agent tools are complete
- `Cargo.toml` — No new dependencies
- Anything under `_bmad/` — Read-only, sacred

### Testing Requirements

All tests inline in `#[cfg(test)] mod tests` at the bottom of `src/session/runner.rs`:
- Use `#[test]` for synchronous tests (`is_context_limit_error`, `extract_last_exchanges`, `format_exchanges_for_preamble`)
- Use `#[tokio::test]` for async tests if needed
- Naming convention: `test_{function}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert
- Mock all external dependencies — NO real LLM calls
- `is_context_limit_error()` and extraction/formatting functions are pure — no mocking needed
- Do NOT test `context_limit_recovery()` end-to-end (requires live LLM) — test the helper functions and error detection thoroughly

### Anti-Patterns to Avoid

- ❌ Do NOT retry context limit errors — they will fail with the same error. Intercept BEFORE retry logic
- ❌ Do NOT use `Box<dyn Chat>` — `Chat` is not object-safe in rig 0.30. Match on provider and build concrete types
- ❌ Do NOT try to swap the agent variable in the outer loop — the original agent's context is exhausted. Delegate to a NEW `run_session()` call with `Some(compressed_state)` and return its outcome
- ❌ Do NOT return `(SessionState, String)` from recovery and `continue` in the loop — the old agent variable is still bound to the exhausted context. Recovery MUST be a terminal action that returns `SessionOutcome`
- ❌ Do NOT inject project-context.md (or any project files) into the preamble or recovery message — the agent loads its own project context via the BMAD activation flow ("CH" → "Load the project context"). The daemon does NOT decide which docs are relevant. Follow the Story 3.2 pattern: "Treat it like a human"
- ❌ Do NOT build an "enhanced preamble" — use the standard `build_preamble()`. Context is loaded by the agent via its tools during BMAD activation, not pre-injected by the daemon
- ❌ Do NOT skip the BMAD activation flow (CH → Load project context) — the agent needs its standard activation to operate correctly with full project awareness
- ❌ Do NOT keep full history in the compressed WAL — only activation turns + recovery message. The summary is in the recovery message, not the preamble
- ❌ Do NOT use a different LLM provider/model for summarization — use the same one for consistency
- ❌ Do NOT send "DS" as the continuation prompt — that restarts the entire workflow. The recovery message is sent as the last user message in the compressed state, and `run_session(Some(state))` re-sends it via Story 6.3's recovery path (last message = user → re-send)
- ❌ Do NOT duplicate the chat loop — reuse `run_session()` with `Some(compressed_state)`
- ❌ Do NOT allow infinite recursive recoveries — cap at `MAX_RECOVERY_DEPTH = 3` via `recovery_depth` parameter
- ❌ Do NOT omit `recovery_depth` from the `context_limit_recovery()` signature — it is required for recursion safety
- ❌ Do NOT use `unwrap()` or `expect()` in production code
- ❌ Do NOT use `println!` or `eprintln!` — use `tracing` only
- ❌ Do NOT use `anyhow` in session modules — `thiserror` only
- ❌ Do NOT add new dependencies

### Scope Boundaries

**In scope:**
- Context limit error detection from provider error strings (`is_context_limit_error()`)
- Interception in `run_session()` error handler — before retry logic
- Chat history summarization via fresh LLM call
- Fresh agent construction with standard preamble and same tools
- BMAD activation flow (CH → "Load the project context") driven by daemon as simulated human (Story 3.2 pattern)
- Recovery message construction with summary + last N exchanges + story reference (sent as user message, NOT in preamble)
- Compressed `SessionState` with activation turns + recovery message
- WAL overwrite with compressed state (handled by inner `run_session()`)
- Inner chat loop via `run_session()` with `Some(compressed_state)` — returns `SessionOutcome`
- Recovery depth limit (`MAX_RECOVERY_DEPTH = 3`) to prevent infinite recursion
- Summarization fallback (truncated history if full history too large)
- Unit tests for detection, extraction, formatting helpers

**Out of scope:**
- Changes to WAL format or `SessionState` struct (already complete)
- Changes to `run_session()` signature beyond what Story 6.3 already did
- Changes to pipeline or CLI (recovery is invisible — contained within `run_session()`)
- Streaming API support (future — rig 0.30 uses request/response)
- Dynamic N calculation based on model context window size (MVP uses fixed N=10; for small-context models like 8K, N=10 may need to be reduced — future enhancement)
- Persistent summary storage (summary lives only in the recovery message of the recovered session)
- Changes to `ResponseAnalyzer` patterns
- New dependencies

### Project Structure Notes

After this story, modified files in the project:
```
src/
├── session/
│   ├── runner.rs       # MODIFIED — add context limit detection, recovery method, helpers
│   ├── mod.rs          # UNCHANGED
│   ├── state.rs        # UNCHANGED
│   ├── analyzer.rs     # UNCHANGED
│   ├── branch.rs       # UNCHANGED
│   ├── cleanup.rs      # UNCHANGED
│   ├── escalation.rs   # UNCHANGED
│   └── provider.rs     # UNCHANGED
├── pipeline.rs         # UNCHANGED
├── cli/
│   └── mod.rs          # UNCHANGED
├── main.rs             # UNCHANGED
├── config/             # UNCHANGED
├── watcher/            # UNCHANGED
├── notifier/           # UNCHANGED
├── git_provider/       # UNCHANGED
├── review/             # UNCHANGED
├── supervisor/         # UNCHANGED
└── tools/              # UNCHANGED
```

### References

- [Source: planning-artifacts/architecture.md — Decision 3: Session State Persistence — WAL File for Crash & Context Limit Recovery (Recovery Case B)]
- [Source: planning-artifacts/architecture.md — Decision 4: Error Propagation — Layered with Bubble-Up]
- [Source: planning-artifacts/architecture.md — Decision 5: Agent Prompt Composition — Load BMAD Agent File Directly]
- [Source: planning-artifacts/prd.md — NFR-REL1: Transient LLM errors recovered with exponential backoff]
- [Source: planning-artifacts/prd.md — NFR-REL3: Crash recovery produces clean state]
- [Source: planning-artifacts/epics.md — Epic 6, Story 6.4: Context Window Limit Recovery]
- [Source: project-context.md — Resilience Rules, Daemon Lifecycle]
- [Source: project-context.md — Session Language Override, Multi-Provider LLM Config]
- [Source: src/session/runner.rs — run_session() chat loop, build_*_agent(), build_preamble(), create_tools()]
- [Source: src/session/runner.rs — Chat error handling at L618-649, retry logic, state.chat_history.pop()]
- [Source: src/session/state.rs — SessionState, ChatMessage (derives Clone), to_rig_messages()]
- [Source: src/session/mod.rs — SessionOutcome, SessionError]
- [Source: src/session/provider.rs — resolve_api_key()]
- [Source: src/session/analyzer.rs — ResponseAnalyzer, ResponseAction]
- [Source: src/supervisor/mod.rs — EscalationSlot type alias]
- [Source: src/supervisor/decisions.rs — DecisionLog]
- [Source: src/config/mod.rs — BotConfig, bmad_paths.output_folder]
- [Source: rig-core 0.30 — CompletionError::ProviderError(String), Chat trait (not object-safe)]
- [Source: implementation-artifacts/6-3-crash-recovery-via-session-wal.md — run_session() refactor, Option<SessionState>, RecoveryInfo]
- [Source: implementation-artifacts/6-2-http-retry-error-resilience.md — StoryPipeline, pipeline flow]

## Dev Agent Record

### Agent Model Used

Claude Opus 4

### Debug Log References

- All 574 tests pass (52 in `session::runner::tests`, including 30 new Story 6.4 tests)
- `cargo check` — 0 errors
- `cargo clippy` — 0 new warnings (all pre-existing)
- `cargo fmt` — clean

### Completion Notes List

- Task 0: All 10 prerequisites verified — `run_session()` accepts `Option<SessionState>`, `SessionState`/`ChatMessage` structs confirmed, all dependencies in place, rig-core 0.30 confirmed.
- Task 1: Added `is_context_limit_error()` free function with patterns for Anthropic (`context_length_exceeded`, `prompt is too long`), OpenAI (`maximum context length`, `max_tokens`, `token limit`), and generic patterns (`too many tokens`, `input too long`, `exceeds the model`, `context window`). Case-insensitive matching.
- Task 2: Modified `Err(e)` branch in `run_session()` chat loop to intercept context limit errors BEFORE retry logic. Pops the failed user message, calls `context_limit_recovery()` with `recovery_depth: 0`, writes decisions, and returns the outcome (terminal action).
- Task 3: Implemented `context_limit_recovery()` method with depth check (`MAX_RECOVERY_DEPTH = 3`), history extraction, summarization, and provider-matched agent build. Factored BMAD activation + state compression into `drive_activation_and_recover<A: Chat>()` to avoid duplicating activation logic across provider match arms. Used `Box::pin()` to break async recursion cycle (`run_session` → `context_limit_recovery` → `drive_activation_and_recover` → `run_session`).
- Task 4: Implemented `extract_last_exchanges()` (rounds odd counts down to even for clean pairs), `format_exchanges_for_message()` (truncates > 2000 chars), `summarize_history()` (with 50% fallback on context limit), `build_recovery_message()` (SESSION RECOVERY + summary + exchanges + story pointer).
- Task 5: Compressed `SessionState` built with activation turns (4 messages) + recovery message as last user message (total 5 messages). Metadata preserved from original, `last_activity` refreshed. `run_session(Some(state))` re-sends recovery message via Story 6.3's "last msg = user" path.
- Task 6: 30 new unit tests covering: `is_context_limit_error` (8 positive + 3 negative + 4 edge cases), `extract_last_exchanges` (7 tests incl. odd count, empty, exact N), `format_exchanges_for_message` (4 tests incl. truncation), `build_recovery_message` (4 tests), compressed state (5 tests).
- Task 7: `cargo check` 0 errors, `cargo test` 574 passed / 0 failed, `cargo clippy` 0 new warnings, `cargo fmt` clean.
- Design decision: Added `drive_activation_and_recover<A: Chat>()` helper method (not in original story spec) to DRY up the BMAD activation + compressed state + `run_session()` delegation logic that would otherwise be duplicated across all three provider match arms in `context_limit_recovery()`. Used `Box::pin()` on the recursive `run_session()` call inside this helper to satisfy Rust's async recursion sizing requirements.
- Design decision: Used a boxed closure for `summarize_with_prompt` in `summarize_history()` instead of a plain async closure because async closures with captures require explicit `Pin<Box<dyn Future>>` return types in stable Rust. Added explicit `anthropic::Client` / `openai::Client` type annotations to resolve rig 0.30's generic `Client<Ext, H>` type inference ambiguity.
- Design decision: Tracked activation history as both `Vec<Message>` (for rig API calls) and `Vec<ChatMessage>` (for compressed state) in parallel, rather than pattern-matching `Message` enum variants, because rig 0.30's `Message::User` and `Message::Assistant` are struct variants with complex content types (`OneOrMany<UserContent>`) that don't support simple text extraction.

### Change Log

- 2026-02-08: Story 6.4 implementation complete — context window limit recovery with BMAD activation pattern, recursive depth guard, and comprehensive tests.

### File List

- `src/session/runner.rs` (MODIFIED — add `is_context_limit_error()`, `context_limit_recovery()`, `drive_activation_and_recover()`, `summarize_history()`, `build_recovery_message()`, `extract_last_exchanges()`, `format_exchanges_for_message()`, constants `RECOVERY_KEEP_LAST_EXCHANGES`/`MAX_RECOVERY_DEPTH`, import `ChatMessage`, modify `run_session()` error handling to intercept context limit errors before retry logic, 30 new unit tests)